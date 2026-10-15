//! `shep kelpie usage --openmetrics`: the ledger's call and finished lines as the counters the
//! runner sends live, each with the time its line was written, for
//! `promtool tsdb create-blocks-from openmetrics`
//!
//! The lines are replayed through the runner's own [`Recorder`], so a series has the name and
//! labels it has on the endpoint, and each total is the running one a runner started at the
//! ledger's first line would have sent.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, PoisonError};

use super::Line;
use crate::metrics::Recorder;
use crate::ports::{Sample, Sink, Timestamp};

// Every sample sent to it, kept for the line that caused it
#[derive(Debug, Default)]
struct Collect(Mutex<Vec<Sample>>);

impl Sink for Collect {
    fn send(&self, sample: &Sample) {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(sample.clone());
    }
}

struct Family {
    help: &'static str,
    series: BTreeMap<Vec<(&'static str, String)>, BTreeMap<u64, f64>>,
}

/// The OpenMetrics text of each project's ledger `lines`, a project's name with its lines
///
/// A line at the same second as another of its series leaves the later total.
pub fn openmetrics(ledgers: &[(String, Vec<Line>)]) -> Vec<String> {
    let mut families: BTreeMap<&'static str, Family> = BTreeMap::new();
    for (project, lines) in ledgers {
        let sink = Arc::new(Collect::default());
        let recorder = Recorder::uncapped(sink.clone());
        // Written in the order they ended, but for imports, which come last.
        let mut lines: Vec<&Line> = lines.iter().collect();
        lines.sort_by_key(|line| at(line));
        for line in lines {
            recorder.line(line);
            let sent = std::mem::take(&mut *sink.0.lock().unwrap_or_else(PoisonError::into_inner));
            for sample in sent {
                let mut labels = vec![("sheep", project.clone())];
                labels.extend(sample.labels);
                let family = families.entry(sample.name).or_insert_with(|| Family {
                    help: sample.help,
                    series: BTreeMap::new(),
                });
                (family.series.entry(labels).or_default()).insert(at(line).0, sample.value);
            }
        }
    }
    let mut out = Vec::new();
    for (name, family) in &families {
        let metric = name.strip_suffix("_total").unwrap_or(name);
        out.push(format!("# TYPE {metric} counter"));
        out.push(format!("# HELP {metric} {}", family.help));
        for (labels, points) in &family.series {
            for (at, value) in points {
                out.push(format!("{name}{} {value} {at}", label_set(labels)));
            }
        }
    }
    out.push("# EOF".to_owned());
    out
}

fn at(line: &Line) -> Timestamp {
    match line {
        Line::Call(call) => call.at,
        Line::Finished(finished) => finished.at,
    }
}

fn label_set(labels: &[(&'static str, String)]) -> String {
    let each: Vec<String> = (labels.iter())
        .map(|(name, value)| format!("{name}=\"{}\"", escaped(value)))
        .collect();
    format!("{{{}}}", each.join(","))
}

fn escaped(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
}
