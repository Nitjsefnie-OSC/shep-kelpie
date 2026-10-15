//! The rig's metrics dog: every sample the runner sends, kept

use std::sync::{Arc, Mutex};

use crate::metrics::Recorder;
use crate::ports::{Sample, Sink};

/// A sink that keeps every sample sent to it, oldest first
#[derive(Debug, Clone, Default)]
pub(crate) struct RecordedSink(Arc<Mutex<Vec<Sample>>>);

impl Sink for RecordedSink {
    fn send(&self, sample: &Sample) {
        self.0.lock().unwrap().push(sample.clone());
    }
}

impl RecordedSink {
    /// A recorder whose samples all come here
    pub(crate) fn recorder(&self) -> Arc<Recorder> {
        Arc::new(Recorder::new(Arc::new(self.clone())))
    }

    /// Every sample sent, oldest first
    pub(crate) fn sent(&self) -> Vec<Sample> {
        self.0.lock().unwrap().clone()
    }

    /// What the metrics dog would serve for series `name` with exactly
    /// `labels`: its last sample's value, or none if it was never sent
    pub(crate) fn value(&self, name: &str, labels: &[(&str, &str)]) -> Option<f64> {
        let mut wanted: Vec<_> = labels.to_vec();
        wanted.sort_unstable();
        let matches = |sample: &&Sample| {
            let held: Vec<_> = (sample.labels.iter())
                .map(|(k, v)| (*k, v.as_str()))
                .collect();
            sample.name == name && held == wanted
        };
        self.sent()
            .iter()
            .rev()
            .find(matches)
            .map(|sample| sample.value)
    }

    /// The sum of every series named `name`, as `sum(name)` in Prometheus
    pub(crate) fn total(&self, name: &str) -> f64 {
        let mut last = std::collections::BTreeMap::new();
        for sample in self.sent().into_iter().filter(|s| s.name == name) {
            last.insert(sample.labels.clone(), sample.value);
        }
        last.values().sum()
    }
}
