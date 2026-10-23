//! How long each hold of the runner's lock lasts, by the line that took it

use std::collections::BTreeMap;
use std::ops::{Deref, DerefMut};
use std::panic::Location;
use std::sync::MutexGuard;
use std::time::{Duration, Instant};

use super::Runner;
use crate::metrics::Recorder;

// The longest hold `kelpie_runner_lock_holds_total{le="1"}` counts
const SHORT: Duration = Duration::from_secs(1);

// A source file and line. The sites are the lock calls the code has, so the map stays that size.
type Site = (&'static str, u32);

/// The holds of the runner's lock so far: the longest of each site, and the ones not yet sent as
/// metrics
#[derive(Debug, Default)]
pub(super) struct Holds {
    longest: BTreeMap<Site, Duration>,
    unsent: Unsent,
}

#[derive(Debug, Default)]
struct Unsent {
    total: Duration,
    short: u32,
    all: u32,
}

impl Holds {
    /// Notes that the lock was held `elapsed` since `site` took it
    pub(super) fn record(&mut self, site: &'static Location<'static>, elapsed: Duration) {
        let longest = (self.longest)
            .entry((site.file(), site.line()))
            .or_default();
        *longest = (*longest).max(elapsed);
        self.unsent.total += elapsed;
        self.unsent.short += u32::from(elapsed <= SHORT);
        self.unsent.all += 1;
    }

    /// The longest hold of each site, in seconds, by `file:line`
    pub(super) fn longest(&self) -> BTreeMap<String, f64> {
        (self.longest.iter())
            .map(|((file, line), held)| (format!("{file}:{line}"), held.as_secs_f64()))
            .collect()
    }

    /// Sends the holds recorded since the last flush
    pub(super) fn flush(&mut self, metrics: &Recorder) {
        let Unsent { total, short, all } = std::mem::take(&mut self.unsent);
        if all > 0 {
            metrics.lock_holds(total.as_secs_f64(), short, all);
        }
    }
}

/// The runner's lock, held: records how long once it is dropped
///
/// Keyed by the line that took the lock.
pub(super) struct Held<'a> {
    guard: MutexGuard<'a, Runner>,
    site: &'static Location<'static>,
    since: Instant,
}

impl<'a> Held<'a> {
    #[track_caller]
    pub(super) fn new(guard: MutexGuard<'a, Runner>) -> Self {
        Self {
            guard,
            site: Location::caller(),
            since: Instant::now(),
        }
    }
}

impl Deref for Held<'_> {
    type Target = Runner;

    fn deref(&self) -> &Runner {
        &self.guard
    }
}

impl DerefMut for Held<'_> {
    fn deref_mut(&mut self) -> &mut Runner {
        &mut self.guard
    }
}

impl Drop for Held<'_> {
    fn drop(&mut self) {
        let elapsed = self.since.elapsed();
        self.guard.holds.record(self.site, elapsed);
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::runner::trigger::lock;
    use crate::test::{RecordedSink, Rig};

    #[track_caller]
    fn here() -> &'static Location<'static> {
        Location::caller()
    }

    #[test]
    fn a_hold_is_recorded_against_the_line_that_took_the_lock() {
        let rig = Rig::new("koji");
        let runner = rig.open().unwrap();
        let line = line!() + 1;
        drop(lock(&runner));

        let held = lock(&runner).holds.longest();
        assert!(
            held.contains_key(&format!("{}:{line}", file!())),
            "no hold for the line that took the lock: {held:?}"
        );
    }

    #[test]
    fn a_hold_over_one_second_counts_in_inf_and_not_in_le_one() {
        let sink = RecordedSink::default();
        let metrics = Recorder::new(Arc::new(sink.clone()));
        let mut holds = Holds::default();
        holds.record(here(), Duration::from_millis(250));
        holds.record(here(), Duration::from_secs(1));
        holds.record(here(), Duration::from_millis(1_500));
        holds.flush(&metrics);

        assert_eq!(
            sink.value("kelpie_runner_lock_holds_total", &[("le", "1")]),
            Some(2.0)
        );
        assert_eq!(
            sink.value("kelpie_runner_lock_holds_total", &[("le", "+Inf")]),
            Some(3.0)
        );
        assert_eq!(
            sink.value("kelpie_runner_lock_held_seconds_total", &[]),
            Some(2.75)
        );

        holds.record(here(), Duration::from_secs(2));
        holds.flush(&metrics);
        assert_eq!(
            sink.value("kelpie_runner_lock_holds_total", &[("le", "1")]),
            Some(2.0)
        );
        assert_eq!(
            sink.value("kelpie_runner_lock_holds_total", &[("le", "+Inf")]),
            Some(4.0)
        );
        assert_eq!(
            sink.value("kelpie_runner_lock_held_seconds_total", &[]),
            Some(4.75)
        );
    }

    #[test]
    fn the_longest_hold_per_site_is_kept_and_the_sites_stay_bounded() {
        let mut holds = Holds::default();
        let first = here();
        let second = here();
        for millis in [30, 900, 40] {
            holds.record(first, Duration::from_millis(millis));
        }
        holds.record(second, Duration::from_millis(5));
        for _ in 0..10_000 {
            holds.record(first, Duration::from_millis(1));
            holds.record(second, Duration::from_millis(1));
        }

        let longest = holds.longest();
        let mut seconds: Vec<f64> = longest.values().copied().collect();
        seconds.sort_by(f64::total_cmp);
        assert_eq!(seconds, [0.005, 0.9], "{longest:?}");
    }
}
