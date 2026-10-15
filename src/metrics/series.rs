//! What each series counts, and where in a ledger line, the state or the
//! settings it comes from

use serde::Serialize;

use super::{Family, Label, Recorder};
use crate::ports::SeriesKind::{Counter, Gauge};
use crate::settings::Settings;
use crate::state::ProjectState;
use crate::usage::{CallLine, FinishedLine, Line};
use crate::work_item::Phase;

const CALLS: Family = Family {
    name: "kelpie_calls_total",
    kind: Counter,
    help: "Model calls that ended, by role, call kind, harness and how they ended.",
};
const TOKENS: Family = Family {
    name: "kelpie_tokens_total",
    kind: Counter,
    help: "Tokens the calls used as their harness reported them, by type.",
};
const COST: Family = Family {
    name: "kelpie_cost_usd_total",
    kind: Counter,
    help: "What the calls whose harness reported a cost cost, in US dollars.",
};
const UNPRICED: Family = Family {
    name: "kelpie_unpriced_calls_total",
    kind: Counter,
    help: "Calls whose harness reported no cost.",
};
const CALL_SECONDS: Family = Family {
    name: "kelpie_call_seconds_total",
    kind: Counter,
    help: "Seconds the calls ran, start to end; over kelpie_calls_total for a mean.",
};
const WORK_ITEMS: Family = Family {
    name: "kelpie_work_items",
    kind: Gauge,
    help: "Open work items, by phase.",
};
const FINISHED: Family = Family {
    name: "kelpie_work_items_finished_total",
    kind: Counter,
    help: "Work items finished: merged, closed with no change, or dropped.",
};
const ITEM_SECONDS: Family = Family {
    name: "kelpie_work_item_seconds_total",
    kind: Counter,
    help: "Wall seconds of the finished work items, from creation to finish.",
};
const REVIEW_ROUNDS: Family = Family {
    name: "kelpie_review_rounds_total",
    kind: Counter,
    help: "Reviewers' rounds that came back with an answer, on finished work items.",
};
const FIX_TURNS: Family = Family {
    name: "kelpie_fix_turns_total",
    kind: Counter,
    help: "Worker turns sent to fix a round's held findings, on finished work items.",
};
const RULINGS: Family = Family {
    name: "kelpie_rulings_total",
    kind: Counter,
    help: "Rulings raised, on finished work items.",
};
const FEATURE_ENABLED: Family = Family {
    name: "kelpie_feature_enabled",
    kind: Gauge,
    help: "1 when the project's settings turn the feature on, 0 when not.",
};
/// How often a feature ran, by how it went
pub const FEATURE_RUNS: Family = Family {
    name: "kelpie_feature_runs_total",
    kind: Counter,
    help: "Times a feature ran for a call, by outcome. An enabled feature with no ok run is broken.",
};

// Every phase, so one with no work item reads 0 rather than vanishing
const PHASES: [&str; 6] = ["implement", "review", "ci", "ruling", "merge", "done"];

/// A feature a project turns on, as its series name it
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Feature {
    /// `code_graph`: codebase-memory-mcp for workers and reviewers
    CodeGraph,
    /// `browser`: the Playwright MCP server behind kelpie's bridge
    Browser,
    /// `dev_server`: the project's server from `main`'s launch file, started for a turn's browser
    DevServer,
    /// `hooks`: the project's own commands at kelpie's points
    Hooks,
}

impl Feature {
    const ALL: [Self; 4] = [Self::CodeGraph, Self::Browser, Self::DevServer, Self::Hooks];

    fn as_str(self) -> &'static str {
        match self {
            Self::CodeGraph => "code_graph",
            Self::Browser => "browser",
            Self::DevServer => "dev_server",
            Self::Hooks => "hooks",
        }
    }

    // `launch_file`: whether `main` has one
    fn enabled(self, settings: &Settings, launch_file: bool) -> bool {
        match self {
            Self::CodeGraph => settings.code_graph.enabled,
            Self::Browser => settings.browser.enabled,
            Self::DevServer => settings.browser.enabled && launch_file,
            Self::Hooks => !settings.hooks.is_empty(),
        }
    }
}

impl Recorder {
    /// Counts what a ledger line records, as it is written
    pub fn line(&self, line: &Line) {
        match line {
            Line::Call(call) => self.call(call),
            Line::Finished(finished) => self.finished(finished),
        }
    }

    /// Sets the work items by phase, from `state` as saved
    pub fn work_items(&self, state: &ProjectState) {
        for phase in PHASES {
            let open = (state.work_items.iter())
                .filter(|item| phase_name(&item.phase) == phase)
                .count();
            #[expect(clippy::cast_precision_loss, reason = "a count of work items")]
            self.set(&WORK_ITEMS, vec![("phase", phase.to_owned())], open as f64);
        }
    }

    /// Sets which features `settings` turn on, where `launch_file` is whether `main` has a
    /// launch file for the dev server
    pub fn features(&self, settings: &Settings, launch_file: bool) {
        for feature in Feature::ALL {
            let on = f64::from(u8::from(feature.enabled(settings, launch_file)));
            self.set(&FEATURE_ENABLED, vec![feature_label(feature)], on);
        }
    }

    /// Counts one run of `feature` for a call, which went as `outcome`
    pub fn feature_ran(&self, feature: Feature, outcome: &'static str) {
        let labels = vec![feature_label(feature), ("outcome", outcome.to_owned())];
        self.add(&FEATURE_RUNS, labels, 1.0);
    }

    fn call(&self, call: &CallLine) {
        let role = ("role", call.role.as_str().to_owned());
        let harness = ("harness", known(call.harness.as_deref()));
        let model = ("model", known(call.model.as_deref()));
        let kind = ("kind", wire(&call.kind));
        let calls = vec![
            role.clone(),
            kind.clone(),
            harness.clone(),
            ("ended", wire(&call.ended)),
        ];
        self.add(&CALLS, calls, 1.0);
        let usage = call.usage;
        let tokens = [
            ("input", usage.input),
            ("output", usage.output),
            ("cache_read", usage.cache_read),
            ("cache_write", usage.cache_write),
        ];
        for (kind, count) in tokens {
            let labels = vec![
                role.clone(),
                harness.clone(),
                model.clone(),
                ("type", kind.into()),
            ];
            #[expect(
                clippy::cast_precision_loss,
                reason = "a call's tokens stay far below 2^53"
            )]
            self.add(&TOKENS, labels, count as f64);
        }
        match call.cost_usd {
            Some(usd) => self.add(&COST, vec![role.clone(), harness.clone(), model], usd),
            None if call.unpriced => self.add(&UNPRICED, vec![role.clone(), harness], 1.0),
            None => {}
        }
        if let Some(seconds) = call.seconds {
            #[expect(
                clippy::cast_precision_loss,
                reason = "a call's seconds stay far below 2^53"
            )]
            self.add(&CALL_SECONDS, vec![role, kind], seconds as f64);
        }
    }

    fn finished(&self, finished: &FinishedLine) {
        let outcome = match (finished.merged, finished.closed) {
            (true, _) => "merged",
            (false, true) => "closed",
            (false, false) => "dropped",
        };
        let labels = || vec![("outcome", outcome.to_owned())];
        self.add(&FINISHED, labels(), 1.0);
        #[expect(
            clippy::cast_precision_loss,
            reason = "a work item's seconds stay far below 2^53"
        )]
        self.add(&ITEM_SECONDS, labels(), finished.wall as f64);
        self.add(
            &REVIEW_ROUNDS,
            Vec::new(),
            f64::from(finished.review_rounds),
        );
        self.add(&FIX_TURNS, Vec::new(), f64::from(finished.fix_turns));
        self.add(&RULINGS, Vec::new(), f64::from(finished.rulings));
    }
}

fn feature_label(feature: Feature) -> Label {
    ("feature", feature.as_str().to_owned())
}

// The glossary's name of a phase
fn phase_name(phase: &Phase) -> &'static str {
    match phase {
        Phase::Implement => "implement",
        Phase::Review(_) => "review",
        Phase::Ci { .. } => "ci",
        Phase::Ruling { .. } => "ruling",
        Phase::Merge { .. } => "merge",
        Phase::Done { .. } => "done",
    }
}

fn known(value: Option<&str>) -> String {
    value.unwrap_or("unknown").to_owned()
}

// The name an enum has in the ledger, so a series and a ledger line agree
fn wire(value: &impl Serialize) -> String {
    let value = serde_json::to_value(value).ok();
    (value.as_ref().and_then(serde_json::Value::as_str)).map_or_else(String::new, str::to_owned)
}
