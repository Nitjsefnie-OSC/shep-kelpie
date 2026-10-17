//! The spam check an issue goes through after the limits and before its call
//!
//! One yes/no question to a text classifier the project's gateway serves: is
//! this issue spam, an advertisement, or text unrelated to the project? The
//! answer is the probability that it is. At or above `triage.spam.threshold` the
//! issue gets `possible-spam` and nothing else: no comment, no research call, no
//! close, no lock and no hide. The maintainer decides. Below it the pass goes on
//! as it did.
//!
//! The check is off while `triage.spam.gateway` is absent. It sits after the
//! four limits, so an issue a limit held costs no classifier call, and the
//! maintainer's own issues and one `shep kelpie triage` asked for skip it as they
//! skip the limits.
//!
//! A gateway that cannot be reached, or one that refuses the request, is told
//! once and turns the check off for the rest of the run: an outage that left a
//! question on every issue would spend a connection timeout on each. The pass
//! goes on without the check, and the limits still apply, so it opens no flood.
//!
//! Each check that got an answer is a ledger line of its own, role `triager` and
//! kind `spam-check`, with the issue, no tokens and no cost. It is a call the
//! project made, and it is not one of the triager calls the caps count, which
//! count research calls.

use crate::ports::{NewLabel, OpenIssue, Role, SpamState, Timestamp, Usage};
use crate::settings::ModelHost;
use crate::state::{StateError, TriageStep, Triaged};
use crate::usage::{CallKind, CallLine, Ended, Line};

use super::super::contributions::post::forge_is_busy;
use super::Runner;
use super::limits::{DAY, cut_body};

/// The name of the label an issue the classifier read as spam gets, and the only
/// thing it gets
pub(in crate::runner) const POSSIBLE_SPAM: &str = "possible-spam";

// Made on a repo that lacks it before an issue is labelled with it
fn possible_spam() -> NewLabel<'static> {
    NewLabel {
        name: POSSIBLE_SPAM,
        color: "fbca04",
        description: "Kelpie's classifier read this issue as spam; the maintainer decides",
    }
}

impl Runner {
    // Whether the classifier reads `issue` as spam, in which case it has its
    // label and its record and gets no comment and no research call. `false`
    // sends the pass on to the call, which is what a check that is off, a gateway
    // that could not be asked, and an answer under the threshold each do.
    pub(super) fn triage_spam(&mut self, issue: &OpenIssue) -> Result<bool, StateError> {
        let spam = self.settings.triage.spam.clone();
        if spam.is_off() || self.triage_run.spam_off {
            return Ok(false);
        }
        let (Some(gateway), Some(path)) = (spam.gateway.clone(), spam.path.clone()) else {
            return Ok(false);
        };
        let upstream = match self.gateways.upstream(&ModelHost::Gateway(gateway)) {
            Ok(upstream) => upstream,
            Err(why) => return Ok(self.triage_spam_gone(&why)),
        };
        let now = self.ports.clock.now();
        let age =
            (self.triage_account_created(&issue.author)).map(|at| now.0.saturating_sub(at.0) / DAY);
        // The same cut the research call is given, so the classifier and the
        // triager read the same issue.
        let (body, _) = cut_body(&issue.body, self.settings.triage.max_body_kb);
        let asked = SpamState {
            title: &issue.title,
            body: &body,
            author_association: issue.author_association,
            account_age_days: age,
        };
        let probability = match (self.ports.classifier).spam(&upstream, path.as_str(), &asked) {
            Ok(p) => p,
            Err(e) => return Ok(self.triage_spam_gone(&e.to_string())),
        };
        self.ledger
            .append(&Line::Call(self.spam_line(issue.number, now)));
        if !spam.is_spam(probability) {
            return Ok(false);
        }
        if let Err(why) = self.make_missing_labels([possible_spam()]) {
            self.triage_failed(
                issue.number,
                &format!("cannot make `{POSSIBLE_SPAM}`: {why}"),
                true,
            );
            return Ok(true);
        }
        if let Err(e) = self.label_issue(issue.number, POSSIBLE_SPAM, true) {
            let why = format!("cannot label it `{POSSIBLE_SPAM}`: {e}");
            self.triage_failed(issue.number, &why, !forge_is_busy(&e));
            return Ok(true);
        }
        let mut next = self.state.clone();
        if let Some(triage) = next.triage.as_mut() {
            triage.issues.retain(|t| t.issue != issue.number);
            triage.issues.push(Triaged {
                issue: issue.number,
                comment: 0,
                step: TriageStep::Done,
                at: now,
                author: issue.author.clone(),
                held: None,
                calls: Vec::new(),
                labels: Vec::new(),
                duplicate_of: None,
                ready_body: None,
                seen: Vec::new(),
                asked: None,
                questions: None,
                local: None,
                spam: Some(probability),
            });
            triage.prune();
        }
        self.save(next)?;
        self.triage_due.retain(|d| d.issue != issue.number);
        self.triage_queue.retain(|&n| n != issue.number);
        self.triage_tries.remove(&issue.number);
        self.notes.push(format!(
            "triage #{}: its classifier read it as spam ({probability}), so it is labelled \
             `{POSSIBLE_SPAM}` with no comment and no call; the maintainer decides",
            issue.number
        ));
        Ok(true)
    }

    // A check that could not be asked: told once, and off for the rest of the
    // run, so an outage costs one attempt and not one per issue. `false`, which
    // sends the pass on to the research call.
    fn triage_spam_gone(&mut self, why: &str) -> bool {
        if self.triage_run.spam_off {
            return false;
        }
        self.triage_run.spam_off = true;
        self.notes.push(format!(
            "triage's spam check is off for the rest of this run: {why}. Issues go on to their \
             research call, and the triage limits still apply"
        ));
        false
    }

    // A check's ledger line: the issue, no tokens and no cost. It has no session,
    // no harness and no model of kelpie's naming, and it spent nothing.
    fn spam_line(&self, issue: u64, at: Timestamp) -> CallLine {
        CallLine {
            at,
            issue: Some(issue),
            pull_request: None,
            role: Role::Triager,
            kind: CallKind::SpamCheck,
            agent: None,
            harness: None,
            model: None,
            effort: None,
            session: None,
            usage: Usage::default(),
            compactions: 0,
            units: 0,
            cost_usd: None,
            unpriced: false,
            session_cost_usd: None,
            seconds: None,
            ended: Ended::Answered,
            gpu_seconds: None,
            pacer: Default::default(),
            imported: false,
            votes: Vec::new(),
        }
    }
}
