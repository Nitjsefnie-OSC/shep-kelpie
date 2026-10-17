//! Reading a contribution's head for security before anything of theirs runs
//!
//! A contributor's code does not run on the maintainer's machine until
//! something has read it. Once a poll, one followed contribution whose
//! current head no review has read gets one, in flight on a thread of its own
//! like every other call, so the runner's lock is not held while it runs and
//! it is ended past its ceiling: the head is fetched into the
//! project's checkout, which moves objects and checks nothing out, and the
//! change since the last passed head, or the whole change from `origin/main`
//! with none or after a force push, is read by the `contributions`
//! `security_reviewer` in a scratch folder under the project's folder, never
//! in the checkout.
//!
//! Kelpie flags in code what a model cannot read, and never passes it: a
//! binary change, a symlink, a submodule change, and a diff over
//! [`MAX_DIFF`] bytes. A clean reply records the head as passed, findings
//! record it as flagged, and a call that fails or answers in no shape
//! leaves the head unpassed, to be read again at the next poll.

use std::fs;
use std::path::{Path, PathBuf};

use super::super::Runner;
use super::super::report::Begin;
use super::super::review::calls::build_call;
use super::super::untrusted;
use super::{Job as Call, ruling};
use crate::dep_sources::{grant, machine_sources};
use crate::ports::{Agents, Cost, Ending, Finding, Role, Severity, Tools, read_review};
use crate::settings::RoleModel;
use crate::state::{Security, SecurityVerdict, StateError};
use crate::worktree::{self, Change};

/// The most a diff may hold, in bytes, for a model to read it: a larger one
/// is flagged, never passed
pub(super) const MAX_DIFF: usize = 200_000;

/// The folder under the project's folder where each contribution's diff is
/// written for its review, one folder each, removed when the review ends
const FOLDER: &str = "security";

/// The file the diff is written to in that folder
const DIFF_FILE: &str = "change.diff";

/// What a finding kelpie makes of a whole diff names as its file
const WHOLE_DIFF: &str = "(the whole diff)";

/// One head to read, with everything its read needs, taken while the runner's
/// lock was held so that the read itself runs without it
pub(in crate::runner) struct Job {
    pub(in crate::runner) pull_request: u64,
    pub(in crate::runner) head: String,
    passed: Option<String>,
    checkout: PathBuf,
    folder: PathBuf,
    settings: PathBuf,
    model: RoleModel,
    instructions: String,
    // The maintainer's home, whose installed dependency and toolchain sources
    // the read judges the change against
    home: PathBuf,
}

/// What reading one head came to
#[derive(Debug, Default)]
pub(in crate::runner) struct Reading {
    /// The verdict, or `None` when the head could not be read, which leaves it
    /// unread for the next poll
    pub(in crate::runner) verdict: Option<SecurityVerdict>,
    /// What the call cost, where it came back priced
    pub(in crate::runner) cost: Cost,
}

impl Runner {
    // The read of the current head of one contribution no review has read, as
    // the call to start, unless a read is in flight or the runner drains. One
    // is tried once a poll, so a head that cannot be read does not hold up the
    // others. A contribution over its cap starts no call: a
    // ruling asks the maintainer, and the next one is tried.
    //
    // The caller holds a free slot for it, which the call keeps until it ends.
    pub(in crate::runner) fn security_due(&mut self) -> Result<Option<Begin>, StateError> {
        if self.draining() || self.flights.securing() {
            return Ok(None);
        }
        let unread: Vec<_> = (self.state.contributions.iter())
            .filter(|c| !c.is_read() && !self.flights.security_tried(c.pull_request))
            .map(|c| c.pull_request)
            .collect();
        for number in unread {
            if self.over_budget(number)? {
                continue;
            }
            return Ok(self
                .security_job(number)
                .map(|job| Begin::Secure(Call::Security(job))));
        }
        Ok(None)
    }

    // Everything the read of `number`'s current head needs, or `None` when
    // the security reviewer cannot run it.
    fn security_job(&self, number: u64) -> Option<Job> {
        let contribution = (self.state.contributions.iter()).find(|c| c.pull_request == number)?;
        let reviewer = match self.settings.security_reviewer(&self.book) {
            Ok(reviewer) => reviewer,
            Err(e) => {
                eprintln!("contribution #{number}: the security review left it unread: {e}");
                return None;
            }
        };
        let (model, _) = reviewer.runs.session()?;
        Some(Job {
            pull_request: number,
            head: contribution.head.clone(),
            passed: contribution.passed.clone(),
            checkout: self.settings.git.checkout.clone(),
            folder: self.paths.folder.join(FOLDER).join(number.to_string()),
            settings: (self.paths.worker).join(format!("security-settings-{number}.json")),
            model: model.clone(),
            instructions: reviewer.prompt.clone().unwrap_or_default(),
            home: self.home.clone(),
        })
    }

    // Records what the call on `pull_request` came to: its cost goes to the
    // contribution's spend whatever head it read, and the verdict is recorded
    // for `head` unless the contribution moved on to another head or is
    // gone. A passed head becomes the one the next is read since.
    pub(in crate::runner) fn record_security(
        &mut self,
        pull_request: u64,
        head: &str,
        reading: Reading,
    ) -> Result<(), StateError> {
        let mut next = self.state.clone();
        let Some(held) = (next.contributions.iter_mut()).find(|c| c.pull_request == pull_request)
        else {
            return Ok(());
        };
        held.spend = Cost(held.spend.0.saturating_add(reading.cost.0));
        let Some(verdict) = reading.verdict.filter(|_| held.head == head) else {
            return match reading.cost.is_free() {
                true => Ok(()),
                false => self.save(next),
            };
        };
        if verdict == SecurityVerdict::Passed {
            held.passed = Some(head.to_owned());
        }
        let flagged = match &verdict {
            SecurityVerdict::Flagged(lines) => Some(lines.clone()),
            SecurityVerdict::Passed => None,
        };
        held.security = Some(Security {
            head: head.to_owned(),
            verdict,
        });
        if let Some(lines) = flagged {
            let now = self.ports.clock.now();
            ruling::raise(&mut next, pull_request, head, lines, now);
        }
        self.save(next)
    }
}

impl Job {
    // Reads the head: its verdict, or `None` when it could not be had (a head
    // that cannot be fetched or diffed, a call that fails or is ended, a
    // reply in no shape), which leaves it unread for the next poll.
    pub(in crate::runner) fn read(self, agents: &dyn Agents, ending: &Ending) -> Reading {
        let number = self.pull_request;
        let change = worktree::contribution_change(
            &self.checkout,
            number,
            &self.head,
            self.passed.as_deref(),
            MAX_DIFF,
        );
        let change = match change {
            Ok(change) => change,
            Err(e) => {
                eprintln!(
                    "contribution #{number}: cannot fetch {} to read it: {e}",
                    self.head
                );
                return Reading::unread(Cost::default());
            }
        };
        let flagged = flagged_in_code(&change);
        if !flagged.is_empty() {
            return Reading::read(SecurityVerdict::Flagged(flagged), Cost::default());
        }
        self.ask(agents, ending, &change.diff)
    }

    // The security reviewer's verdict on `diff`, the change the head makes,
    // and what the call cost.
    fn ask(&self, agents: &dyn Agents, ending: &Ending, diff: &str) -> Reading {
        let number = self.pull_request;
        let fail = |why: String, cost: Cost| {
            eprintln!("contribution #{number}: the security review left it unread: {why}");
            Reading::unread(cost)
        };
        if let Err(e) = write_diff(&self.folder, diff) {
            return fail(e, Cost::default());
        }
        let prompt = prompt(&self.instructions, diff);
        let mut call = match build_call(Role::Reviewer, number, &self.folder, &self.model, prompt) {
            Ok(call) => call,
            Err(e) => return fail(e, Cost::default()),
        };
        call.settings.clone_from(&self.settings);
        call.tools = Tools::Review;
        // It judges the change against the crates it calls, so it reads the
        // installed sources of this machine's dependencies.
        grant(&mut call, &machine_sources(&self.home));
        let ran = agents
            .prepare(&call)
            .and_then(|()| agents.run(&call, ending));
        // The contributor's text and the call's settings are of no use after it.
        let _ = fs::remove_dir_all(&self.folder);
        let _ = fs::remove_file(&call.settings);
        let reply = match ran {
            Ok(reply) => reply,
            Err(e) => return fail(e.to_string(), Cost::default()),
        };
        // A fresh session, so what it has cost is what this call did.
        let cost = reply.session_cost.unwrap_or_default();
        match read_review(&reply.text) {
            Ok(findings) if findings.is_empty() => Reading::read(SecurityVerdict::Passed, cost),
            Ok(findings) => Reading::read(SecurityVerdict::Flagged(findings), cost),
            Err(said) => fail(format!("its reply is no verdict: {said:?}"), cost),
        }
    }
}

impl Reading {
    // A head read to `verdict`, by a call that cost `cost`
    fn read(verdict: SecurityVerdict, cost: Cost) -> Self {
        Self {
            verdict: Some(verdict),
            cost,
        }
    }

    // A head left unread, by a call that cost `cost`
    fn unread(cost: Cost) -> Self {
        Self {
            verdict: None,
            cost,
        }
    }
}

// What kelpie flags without asking a model: a path a model cannot read, and a
// diff too long for one to read whole.
fn flagged_in_code(change: &Change) -> Vec<Finding> {
    let finding =
        |file: &str, what: String, why: String| Finding::new(Severity::High, file, 0, what, why);
    let mut found: Vec<Finding> = (change.opaque.iter())
        .map(|touched| {
            finding(
                &touched.path,
                touched.kind.as_str().to_owned(),
                "no model can read it, so it is flagged for the maintainer, never passed"
                    .to_owned(),
            )
        })
        .collect();
    if change.cut {
        found.push(finding(
            WHOLE_DIFF,
            format!("a diff over {MAX_DIFF} bytes"),
            format!("it is over {MAX_DIFF} bytes, more than a model reads whole, so it is flagged for the maintainer, never passed"),
        ));
    }
    found
}

// Writes `diff` to the contribution's scratch `folder`, which holds nothing
// else, and nothing of the contributor's but this text.
fn write_diff(folder: &Path, diff: &str) -> Result<(), String> {
    fs::create_dir_all(folder).map_err(|e| format!("cannot create {}: {e}", folder.display()))?;
    let file = folder.join(DIFF_FILE);
    fs::write(&file, diff).map_err(|e| format!("cannot write {}: {e}", file.display()))
}

// The reviewer's prompt: its file's `body`, with the diff fenced as data
// where the body puts `{{DIFF}}`, or after it.
fn prompt(body: &str, diff: &str) -> String {
    let fenced = untrusted::fenced("The diff", diff);
    match body.contains("{{DIFF}}") {
        true => body.replacen("{{DIFF}}", &fenced, 1),
        false => format!("{body}\n\n{fenced}"),
    }
}
