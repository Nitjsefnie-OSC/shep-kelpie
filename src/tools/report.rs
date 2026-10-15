//! What `tools install` says as it goes: a line as each step starts and ends,
//! and a table of every tool and its result at the close

use std::io::Write;

/// How one tool's install ended
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Outcome {
    NotRun,
    Ok,
    Skipped(String),
    Failed,
}

impl Outcome {
    fn text(&self) -> String {
        match self {
            Self::NotRun => "not run".to_owned(),
            Self::Ok => "ok".to_owned(),
            Self::Skipped(why) => format!("skipped, {why}"),
            Self::Failed => "failed".to_owned(),
        }
    }
}

struct Row {
    step: usize,
    tool: String,
    version: String,
    outcome: Outcome,
}

/// The lines of one install, written to `out`
pub(super) struct Report<'a> {
    out: &'a mut dyn Write,
    steps: usize,
    rows: Vec<Row>,
}

const INDENT: &str = "      ";

impl<'a> Report<'a> {
    /// A report of an install of `steps` steps, the last of which is the summary
    pub(super) fn new(out: &'a mut dyn Write, steps: usize) -> Self {
        Self {
            out,
            steps,
            rows: Vec::new(),
        }
    }

    /// Lists a tool that `step` installs, not yet run
    pub(super) fn expect(&mut self, step: usize, tool: &str, version: &str) {
        self.rows.push(Row {
            step,
            tool: tool.to_owned(),
            version: version.to_owned(),
            outcome: Outcome::NotRun,
        });
    }

    pub(super) fn header(&mut self, dir: &std::path::Path) {
        self.line(&format!("kelpie tools -> {}", dir.display()));
    }

    pub(super) fn start(&mut self, step: usize, what: &str) {
        let total = self.steps;
        self.line(&format!("[{step}/{total}] {what}"));
    }

    pub(super) fn note(&mut self, text: &str) {
        self.line(&format!("{INDENT}{text}"));
    }

    /// Records how every tool of `step` ended
    pub(super) fn finish(&mut self, step: usize, outcome: &Outcome) {
        for row in self.rows.iter_mut().filter(|r| r.step == step) {
            row.outcome = outcome.clone();
        }
    }

    /// Sets the version of the tool `tool` of `step`, when only the install learns it
    pub(super) fn learned(&mut self, step: usize, tool: &str, version: &str) {
        for row in self
            .rows
            .iter_mut()
            .filter(|r| r.step == step && r.tool == tool)
        {
            row.version = version.to_owned();
        }
    }

    /// The closing table
    pub(super) fn summary(&mut self) {
        let step = self.steps;
        self.start(step, "summary");
        let tool_width = self.rows.iter().map(|r| r.tool.len()).max().unwrap_or(0);
        let version_width = self.rows.iter().map(|r| r.version.len()).max().unwrap_or(0);
        let lines: Vec<String> = self
            .rows
            .iter()
            .map(|r| {
                format!(
                    "{INDENT}{:tool_width$}  {:version_width$}  {}",
                    r.tool,
                    r.version,
                    r.outcome.text()
                )
            })
            .collect();
        for line in lines {
            self.line(&line);
        }
    }

    // A reader that has gone away is not a reason to stop installing.
    fn line(&mut self, text: &str) {
        let _ = writeln!(self.out, "{text}");
    }
}
