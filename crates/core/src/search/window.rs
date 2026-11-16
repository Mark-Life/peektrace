//! Scan items in order on a few threads, stopping once enough matched.

use std::sync::atomic::{AtomicUsize, Ordering};

use rayon::prelude::*;

/// Most items scanned at once, which caps the transcripts held in memory.
const MAX_IN_FLIGHT: usize = 16;

/// `scan(i)` for `i` in `0..n`, returned in index order. Each thread takes
/// the next unscanned index, so the scan runs newest first with no batch
/// barrier. With `stop_after`, threads take no new index once that many
/// scans matched, and the result is cut after the scan that made it
/// `stop_after`. Every index up to that one was scanned, so the result does
/// not depend on timing.
pub fn scan_in_order<T, F>(n: usize, stop_after: Option<usize>, scan: F) -> Vec<T>
where
    T: Send,
    F: Fn(usize) -> (T, bool) + Sync,
{
    let next = AtomicUsize::new(0);
    let matched = AtomicUsize::new(0);
    let done = || stop_after.is_some_and(|s| matched.load(Ordering::Relaxed) >= s);
    let workers = rayon::current_num_threads().clamp(1, MAX_IN_FLIGHT).min(n);
    let mut scans: Vec<(usize, T, bool)> = (0..workers)
        .into_par_iter()
        .flat_map_iter(|_| {
            std::iter::from_fn(|| {
                if done() {
                    return None;
                }
                let i = next.fetch_add(1, Ordering::Relaxed);
                (i < n).then(|| {
                    let (t, hit) = scan(i);
                    if hit {
                        matched.fetch_add(1, Ordering::Relaxed);
                    }
                    (i, t, hit)
                })
            })
        })
        .collect();
    scans.sort_unstable_by_key(|s| s.0);
    let cut = stop_after
        .and_then(|s| scans.iter().filter(|x| x.2).nth(s.saturating_sub(1)))
        .map_or(scans.len(), |x| x.0 + 1);
    scans.truncate(cut);
    scans.into_iter().map(|(_, t, _)| t).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hits(every: usize) -> impl Fn(usize) -> (usize, bool) + Sync {
        move |i| (i, i % every == 0)
    }

    #[test]
    fn scans_everything_without_a_stop() {
        assert_eq!(
            scan_in_order(50, None, hits(7)),
            (0..50).collect::<Vec<_>>()
        );
        assert_eq!(scan_in_order(0, None, hits(1)), Vec::<usize>::new());
    }

    #[test]
    fn cuts_after_the_match_that_reached_the_stop() {
        for _ in 0..20 {
            assert_eq!(
                scan_in_order(1000, Some(3), hits(10)),
                (0..=20).collect::<Vec<_>>()
            );
        }
        assert_eq!(scan_in_order(5, Some(3), hits(10)), [0, 1, 2, 3, 4]);
    }
}
