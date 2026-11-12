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
}
