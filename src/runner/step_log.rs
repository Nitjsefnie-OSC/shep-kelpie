//! A runner's two logs of what it does: a plain line per event for the
//! shepherd's out log, and the same step as JSON in `steps.jsonl`
//!
//! The out log is for a person reading `shep logs`. `shep kelpie watch` and
//! `usage --import` read the steps from the file, which is the project's
//! folder beside the usage ledger, appended to and never rewritten.

use std::path::PathBuf;

use serde::Serialize;
use serde_json::Value;

use super::Runner;
use super::report::StepReport;
use crate::home::STEPS_FILE;
use crate::ticket::TicketId;

/// The file a project's runner appends its steps to
#[derive(Debug)]
pub(super) struct StepLog {
    path: PathBuf,
    // Whether the last append failed, so a run of failures is told once
    failing: bool,
}

impl StepLog {
    pub fn in_folder(folder: &std::path::Path) -> Self {
        Self {
            path: folder.join(STEPS_FILE),
            failing: false,
        }
    }

    // A failure is told on stderr, once until an append works again, and
    // goes no further: no step fails on it.
    fn append(&mut self, report: &StepReport) {
        #[derive(Serialize)]
        struct Entry<'a> {
            at: String,
            #[serde(flatten)]
            report: &'a StepReport,
        }
        // The system's clock, not the runner's, which counts whole seconds:
        // `watch` lays these lines among transcript lines stamped to the millisecond.
        let now = jiff::Timestamp::now();
        let at = jiff::Timestamp::from_millisecond(now.as_millisecond())
            .unwrap_or(now)
            .to_string();
        match crate::usage::append_json_line(&self.path, &Entry { at, report }) {
            Ok(()) => self.failing = false,
            Err(e) if !self.failing => {
                self.failing = true;
                eprintln!(
                    "cannot add to the steps file {}, so `watch` misses steps until it can be \
                     written: {e}",
                    self.path.display()
                );
            }
            Err(_) => {}
        }
    }
}

impl Runner {
    /// The line the out log gets as the runner starts
    pub fn up_line(&self) -> String {
        let status = self.status();
        format!(
            "up: {}, {} work items open, {} rulings waiting",
            status.project,
            status.work_items.len(),
            status.rulings.len()
        )
    }

    /// Records `report` in `steps.jsonl`, and returns the plain line the out
    /// log gets for it
    pub fn log_step(&mut self, report: &StepReport) -> String {
        self.steps.append(report);
        let ran_on = |report: &StepReport| match report {
            StepReport::Dispatched { agent, .. } => {
                let implementer = self.agents.implementer(agent)?;
                Some(format!(
                    "{} ({})",
                    implementer.model.model.as_str(),
                    implementer.model.effort.as_str()
                ))
            }
            _ => None,
        };
        plain(report, ran_on(report))
    }
}

// The step as one line of words: its work item, its name, and its fields.
// A dispatch names the model and effort its agent runs on, when the project
// still lists the agent.
fn plain(report: &StepReport, ran_on: Option<String>) -> String {
    let Ok(Value::Object(mut fields)) = serde_json::to_value(report) else {
        return String::new();
    };
    let name = fields
        .remove("step")
        .and_then(|name| name.as_str().map(str::to_owned))
        .unwrap_or_default();
    let mut parts = Vec::new();
    if let Some(ticket) = fields
        .remove("issue")
        .and_then(|issue| serde_json::from_value::<TicketId>(issue).ok())
    {
        parts.push(ticket.shown());
    }
    parts.push(name);
    if let Some(ran_on) = ran_on {
        fields.remove("agent");
        parts.push(format!("to {ran_on}"));
    }
    for (key, value) in &fields {
        if let Some(shown) = show(value) {
            parts.push(format!("{key}={shown}"));
        }
    }
    parts.join(" ")
}

fn show(value: &Value) -> Option<String> {
    match value {
        Value::Null => None,
        Value::String(text) => Some(text.clone()),
        Value::Array(items) => {
            let items: Vec<String> = items.iter().filter_map(show).collect();
            (!items.is_empty()).then(|| format!("[{}]", items.join(", ")))
        }
        Value::Object(fields) => {
            let fields: Vec<String> = (fields.iter())
                .filter_map(|(key, value)| Some(format!("{key}={}", show(value)?)))
                .collect();
            (!fields.is_empty()).then(|| format!("{{{}}}", fields.join(", ")))
        }
        other => Some(other.to_string()),
    }
}
