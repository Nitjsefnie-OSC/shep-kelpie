//! Where kelpie's numbers go: shep's metrics dog, through the shepherd
//! channel (ADR 0012)

use std::fmt;

/// Whether a series can go down
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SeriesKind {
    /// A value that goes up and down
    Gauge,
    /// A running total, which only goes up while its runner runs
    Counter,
}

/// One sample of one series, labels sorted by name
#[derive(Debug, Clone, PartialEq)]
pub struct Sample {
    /// The series' name, `kelpie_` first
    pub name: &'static str,
    /// Whether it can go down
    pub kind: SeriesKind,
    /// What it counts, for the endpoint's `# HELP` line
    pub help: &'static str,
    /// Its labels, each from a bounded set
    pub labels: Vec<(&'static str, String)>,
    /// Its value now: a counter's whole total, not the change
    pub value: f64,
}

/// Where a runner's samples go
pub trait Sink: fmt::Debug + Send + Sync {
    /// Sends one sample. Never blocks and never fails: a sample that cannot
    /// go is dropped, and the series' next sample carries its value.
    fn send(&self, sample: &Sample);
}
