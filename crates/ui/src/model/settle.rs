//! Settling watch bumps: refetch after `wait` ms of quiet, or at the latest
//! `max_wait` ms after the first pending bump. While paused, bumps wait;
//! on resume they fire at once.

/// Sessions: about one reload per 1.5 s of quiet, and at least every 4 s.
pub const SESSIONS_WAIT_MS: f64 = 1500.0;
pub const SESSIONS_MAX_WAIT_MS: f64 = 4000.0;

/// Delay until pending bumps should fire, never negative.
pub fn settle_delay(
    now: f64,
    first_pending_at: f64,
    last_bump_at: f64,
    wait: f64,
    max_wait: f64,
) -> f64 {
    ((last_bump_at + wait).min(first_pending_at + max_wait) - now).max(0.0)
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Settle {
    Idle,
    Pending { first: f64, last: f64 },
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SettleEvent {
    /// A watched version went up at `now`.
    Bump { now: f64 },
    /// The clock reached `now`.
    Tick { now: f64 },
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SettleEffect {
    /// Wake again at this time.
    WakeAt(f64),
    /// Refetch now.
    Fire,
    None,
}

/// One step of the settle machine, with sessions timing.
pub fn step(state: &mut Settle, event: SettleEvent, paused: bool) -> SettleEffect {
    let due =
        |first: f64, last: f64| last + SESSIONS_WAIT_MS.min(first + SESSIONS_MAX_WAIT_MS - last);
    match (event, *state) {
        (SettleEvent::Bump { now }, Settle::Idle) => {
            *state = Settle::Pending {
                first: now,
                last: now,
            };
            if paused {
                SettleEffect::None
            } else {
                SettleEffect::WakeAt(due(now, now))
            }
        }
        (SettleEvent::Bump { now }, Settle::Pending { first, .. }) => {
            *state = Settle::Pending { first, last: now };
            if paused {
                SettleEffect::None
            } else {
                SettleEffect::WakeAt(due(first, now))
            }
        }
        (SettleEvent::Tick { now }, Settle::Pending { first, last }) if !paused => {
            let delay = settle_delay(now, first, last, SESSIONS_WAIT_MS, SESSIONS_MAX_WAIT_MS);
            if delay <= 0.0 {
                *state = Settle::Idle;
                SettleEffect::Fire
            } else {
                SettleEffect::WakeAt(now + delay)
            }
        }
        _ => SettleEffect::None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delay_is_quiet_time_capped_by_max_wait() {
        assert!((settle_delay(0.0, 0.0, 0.0, 1500.0, 4000.0) - 1500.0).abs() < 1e-9);
        assert!((settle_delay(3000.0, 0.0, 3000.0, 1500.0, 4000.0) - 1000.0).abs() < 1e-9);
        assert!(settle_delay(5000.0, 0.0, 1000.0, 1500.0, 4000.0).abs() < 1e-9);
    }

    #[test]
    fn steady_bumps_fire_once_at_max_wait() {
        let mut s = Settle::Idle;
        let mut fired = Vec::new();
        for t in 0..=60 {
            let now = f64::from(t) * 100.0;
            if t % 10 == 0 && t <= 30 {
                step(&mut s, SettleEvent::Bump { now }, false);
            }
            if step(&mut s, SettleEvent::Tick { now }, false) == SettleEffect::Fire {
                fired.push(now);
            }
        }
        assert_eq!(fired, [4000.0]);
    }

    #[test]
    fn quiet_fires_after_wait_and_pause_holds() {
        let mut s = Settle::Idle;
        assert_eq!(
            step(&mut s, SettleEvent::Bump { now: 0.0 }, false),
            SettleEffect::WakeAt(1500.0)
        );
        assert_eq!(
            step(&mut s, SettleEvent::Tick { now: 1499.0 }, false),
            SettleEffect::WakeAt(1500.0)
        );
        assert_eq!(
            step(&mut s, SettleEvent::Tick { now: 1500.0 }, false),
            SettleEffect::Fire
        );
        assert_eq!(s, Settle::Idle);
        assert_eq!(
            step(&mut s, SettleEvent::Bump { now: 2000.0 }, true),
            SettleEffect::None
        );
        assert_eq!(
            step(&mut s, SettleEvent::Tick { now: 9000.0 }, true),
            SettleEffect::None
        );
        assert_eq!(
            step(&mut s, SettleEvent::Tick { now: 9000.0 }, false),
            SettleEffect::Fire
        );
    }
    // Ports of `apps/inspector/test/settle.test.ts`. The atom cases run on the
    // sessions timing (wait 1500, max wait 4000) instead of TS's 20/60 ms.

    fn run(s: &mut Settle, bumps: &[f64], until: f64, paused: bool) -> Vec<f64> {
        let mut fired = Vec::new();
        let mut t = 0.0;
        while t <= until {
            if bumps.contains(&t) {
                step(s, SettleEvent::Bump { now: t }, paused);
            }
            if step(s, SettleEvent::Tick { now: t }, paused) == SettleEffect::Fire {
                fired.push(t);
            }
            t += 50.0;
        }
        fired
    }

    #[test]
    fn delay_is_capped_by_max_wait_under_continuous_bumps() {
        let delays: Vec<f64> = [1000.0, 2000.0, 3000.0]
            .into_iter()
            .map(|now| settle_delay(now, 0.0, now, 1500.0, 4000.0))
            .collect();
        assert_eq!(delays, [1500.0, 1500.0, 1000.0]);
        assert!(settle_delay(5000.0, 0.0, 5000.0, 1500.0, 4000.0).abs() < 1e-9);
    }

    #[test]
    fn a_burst_of_bumps_fires_once() {
        let mut s = Settle::Idle;
        for _ in 0..3 {
            step(&mut s, SettleEvent::Bump { now: 0.0 }, false);
        }
        assert_eq!(run(&mut s, &[], 8000.0, false), [1500.0]);
    }

    #[test]
    fn continuous_bumps_still_fire_by_max_wait() {
        let bumps: Vec<f64> = (0..10).map(|i| f64::from(i) * 500.0).collect();
        let fired = run(&mut Settle::Idle, &bumps, 8000.0, false);
        assert!(fired.len() > 1, "{fired:?}");
        assert!(fired[0] < 4500.0, "fired before the last bump: {fired:?}");
        assert_eq!(fired.last().copied(), Some(6000.0));
    }

    #[test]
    fn pause_holds_bumps_and_resume_fires_at_once() {
        let mut s = Settle::Idle;
        assert_eq!(run(&mut s, &[0.0], 8000.0, true), Vec::<f64>::new());
        assert_eq!(
            step(&mut s, SettleEvent::Tick { now: 8000.0 }, false),
            SettleEffect::Fire
        );
    }

    #[test]
    fn resume_with_nothing_pending_fires_nothing() {
        let mut s = Settle::Idle;
        assert_eq!(run(&mut s, &[], 1000.0, true), Vec::<f64>::new());
        assert_eq!(run(&mut s, &[], 8000.0, false), Vec::<f64>::new());
    }
}
