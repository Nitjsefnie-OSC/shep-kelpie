//! A runner's samples, sent on its shepherd channel to shep's metrics dog

use shep_channel::{MetricOptions, MetricType, Shepherd};

use crate::ports::{Sample, SeriesKind, Sink};

impl Sink for Shepherd {
    fn send(&self, sample: &Sample) {
        let kind = match sample.kind {
            SeriesKind::Gauge => MetricType::Gauge,
            SeriesKind::Counter => MetricType::Counter,
        };
        let options = (sample.labels.iter()).fold(
            MetricOptions::new().kind(kind).help(sample.help),
            |options, (key, value)| options.label(*key, value.clone()),
        );
        self.metric_with(sample.name, sample.value, options);
    }
}
