//! Reading a contribution's head for security before anything of theirs runs
//!
//! A contributor's code does not run on the maintainer's machine until
//! something has read it. Once a poll, each followed contribution whose
//! current head no review has read gets one: the head is fetched into the
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
use std::path::Path;

use super::super::Runner;
use super::super::review::calls::build_call;
use super::super::untrusted;
use crate::ports::{Ending, Finding, Role, Severity, Tools, read_review};
use crate::state::{Contribution, Security, SecurityVerdict, StateError};
use crate::worktree::{self, Change};

/// The most a diff may hold, in bytes, for a model to read it: a larger one
/// is flagged, never passed
pub(super) const MAX_DIFF: usize = 200_000;

/// The folder under the project's folder where each contribution's diff is
/// written for its review, one folder each
const FOLDER: &str = "security";

/// The file the diff is written to in that folder
const DIFF_FILE: &str = "change.diff";

/// What a finding kelpie makes of a whole diff names as its file
const WHOLE_DIFF: &str = "(the whole diff)";

impl Runner {
    // Reads the current head of each contribution no review has read yet.
    pub(super) fn read_securities(&mut self) -> Result<(), StateError> {
        let unread: Vec<u64> = (self.state.contributions.iter())
            .filter(|c| !c.is_read())
            .map(|c| c.pull_request)
            .collect();
        for pull_request in unread {
            self.read_security(pull_request)?;
        }
        Ok(())
    }

    // Reads contribution `pull_request`'s current head, unless it was read, and
    // records the verdict. A head that cannot be fetched or diffed, or a call
    // that fails or answers in no shape, records nothing.
    pub(super) fn read_security(&mut self, pull_request: u64) -> Result<(), StateError> {
        let Some(contribution) = (self.state.contributions.iter())
            .find(|c| c.pull_request == pull_request)
            .cloned()
        else {
            return Ok(());
        };
        if contribution.is_read() || self.draining {
            return Ok(());
        }
        let checkout = &self.settings.git.checkout;
        let (head, passed) = (&contribution.head, contribution.passed.as_deref());
        let change = match worktree::contribution_change(checkout, pull_request, head, passed) {
            Ok(change) => change,
            Err(e) => {
                eprintln!("contribution #{pull_request}: cannot fetch {head} to read it: {e}");
                return Ok(());
            }
        };
        let flagged = flagged_in_code(&change);
        let verdict = match flagged.is_empty() {
            false => SecurityVerdict::Flagged(flagged),
            true => match self.ask_security(&contribution, &change.diff) {
                Some(verdict) => verdict,
                None => return Ok(()),
            },
        };
        self.record_security(&contribution, verdict)
    }

    // The security reviewer's verdict on `diff`, the change `contribution`'s
    // head makes, or `None` when the call could not give one.
    fn ask_security(&self, contribution: &Contribution, diff: &str) -> Option<SecurityVerdict> {
        let number = contribution.pull_request;
        let fail = |why: String| {
            eprintln!("contribution #{number}: the security review left it unread: {why}");
            None
        };
        let reviewer = match self.settings.security_reviewer(&self.book) {
            Ok(reviewer) => reviewer,
            Err(e) => return fail(e.to_string()),
        };
        let (model, _) = reviewer.runs.session()?;
        let folder = self.paths.folder.join(FOLDER).join(number.to_string());
        if let Err(e) = write_diff(&folder, diff) {
            return fail(e);
        }
        let prompt = prompt(reviewer.prompt.as_deref().unwrap_or_default(), diff);
        let mut call = match build_call(Role::Reviewer, number, &folder, model, prompt) {
            Ok(call) => call,
            Err(e) => return fail(e),
        };
        call.settings = self
            .paths
            .worker
            .join(format!("security-settings-{number}.json"));
        call.tools = Tools::Review;
        let ran = (self.ports.agents.prepare(&call))
            .and_then(|()| self.ports.agents.run(&call, &Ending::default()));
        let reply = match ran {
            Ok(reply) => reply,
            Err(e) => return fail(e.to_string()),
        };
        match read_review(&reply.text) {
            Ok(findings) if findings.is_empty() => Some(SecurityVerdict::Passed),
            Ok(findings) => Some(SecurityVerdict::Flagged(findings)),
            Err(said) => fail(format!("its reply is no verdict: {said:?}")),
        }
    }

    // Records that `contribution`'s head was read, and what came of it. A
    // passed head becomes the one the next is read since.
    fn record_security(
        &mut self,
        contribution: &Contribution,
        verdict: SecurityVerdict,
    ) -> Result<(), StateError> {
        let mut next = self.state.clone();
        let Some(held) = (next.contributions.iter_mut())
            .find(|c| c.pull_request == contribution.pull_request && c.head == contribution.head)
        else {
            return Ok(());
        };
        if verdict == SecurityVerdict::Passed {
            held.passed = Some(contribution.head.clone());
        }
        held.security = Some(Security {
            head: contribution.head.clone(),
            verdict,
        });
        self.save(next)
    }
}

// What kelpie flags without asking a model: a path a model cannot read, and a
// diff too long for one to read whole.
fn flagged_in_code(change: &Change) -> Vec<Finding> {
    let finding = |file: &str, what: String, why: String| Finding {
        severity: Severity::High,
        file: file.to_owned(),
        line: 0,
        what,
        why,
        test: None,
    };
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
    if change.diff.len() > MAX_DIFF {
        found.push(finding(
            WHOLE_DIFF,
            format!("a diff of {} bytes", change.diff.len()),
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
