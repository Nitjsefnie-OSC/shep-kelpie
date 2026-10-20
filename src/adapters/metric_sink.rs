//! A runner's samples, sent on its shepherd channel to shep's metrics dog

use shep_channel::{MetricOptions, MetricType, Shepherd};

use crate::ports::{Sample, SeriesKind, Sink};

/// A runner's shepherd channel, as the sink its samples go to
///
/// A newtype: `Sink` and `Shepherd` both live in other crates.
#[derive(Debug, Clone)]
pub struct ShepherdSink(Shepherd);

impl ShepherdSink {
    /// The sink that sends on `shepherd`
    pub fn new(shepherd: Shepherd) -> Self {
        Self(shepherd)
    }
}

impl Sink for ShepherdSink {
    fn send(&self, sample: &Sample) {
        let kind = match sample.kind {
            SeriesKind::Gauge => MetricType::Gauge,
            SeriesKind::Counter => MetricType::Counter,
        };
        let options = (sample.labels.iter()).fold(
            MetricOptions::new().kind(kind).help(sample.help),
            |options, (key, value)| options.label(*key, value.clone()),
        );
        self.0.metric_with(sample.name, sample.value, options);
    }
}
