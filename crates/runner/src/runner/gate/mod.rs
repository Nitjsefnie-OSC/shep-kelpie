//! The gate between the worker's pull request and the merge ruling
//!
//! Each step looks once at the pull request's head. A label, ready or head
//! change kelpie did not make parks the worker before anything else. A
//! branch that changes agents' own files parks it next. A
//! branch without the latest `main` is rebased and pushed, or merged when
//! it is adopted or holds a merge already, and a conflict is
//! the worker's next turn, naming the files. A conflict the worker left
//! standing parks it on a ruling. A pending run, or none yet, waits for the
//! next step. A red run is the worker's next turn, naming the checks that
//! failed. A green run raises the merge ruling, or under `auto` merges. A
//! project without CI skips the checks.

use super::Runner;
use super::claude_files::Unchecked;
use super::report::{Begin, StepReport};
use super::talk::At;
use super::turn::KEEP_BODY_TRUE;
use crate::board::{HUMAN, READY};
use crate::ports::{Checks, PullRequestState, Timestamp};
use crate::settings::Merging;
use crate::skills::Step;
use crate::state::{RulingKind, StateError, Stuck};
use crate::work_item::{Phase, Review, Turn, WorkItem, foreign_change};
use crate::worktree::{self, Base};

// GitHub registers the checks a push or a ready pull request starts within
// seconds, one at a time. Kelpie trusts a rollup once two minutes have passed.
pub(crate) const CHECKS_SETTLE: u64 = 120;

mod catch_up;
#[cfg(test)]
mod fix_attempts;
pub(super) mod queued;
mod reads;
mod rerun;

impl Runner {
    pub(super) fn check_ci(&mut self) -> Result<Begin, StateError> {
        let Some(item) = self.current() else {
            return Ok(Begin::Idle);
        };
        let Some(number) = item.pull_request else {
            return Ok(Begin::Idle);
        };
        let Phase::Ci { head: seen, since } = item.phase.clone() else {
            return Ok(Begin::Idle);
        };
        // A fork worker only commits, so what it left is pushed before the
        // fork's head is read.
        if let Some(parked) = self.send_commits(number)? {
            return Ok(parked);
        }
        // Read with the runner's lock let go, and the step made again with the result
        let pr = match self.gate_view(number) {
            Ok(pr) => pr,
            Err(begin) => return Ok(*begin),
        };
        let Some(item) = self.current() else {
            return Ok(Begin::Idle);
        };
        match pr.state {
            PullRequestState::Open => {}
            // The maintainer merged it by hand, which is their own ruling,
            // unless it is kelpie's `auto` merge whose answer was an error.
            PullRequestState::Merged => {
                let notice = item.merge_tried.as_deref() == Some(pr.head.as_str());
                return self.merged(item.issue, number, pr.head, notice);
            }
            PullRequestState::Closed => return self.raise(number, Stuck::Closed.into()),
        }
        let known = item.known.clone();
        let now = self.ports.clock.now();
        let since = if seen.as_deref() == Some(pr.head.as_str()) {
            since
        } else {
            let head = Some(pr.head.clone());
            self.update(|item| item.phase = Phase::Ci { head, since: now })?;
            now
        };
        self.note_checks(&pr.checks);
        let base = match self.base_of(&pr.head) {
            Ok(Base::Lagging) => return Ok(Begin::Idle),
            Ok(base) => base,
            Err(reason) => return Ok(self.gate_failed(reason)),
        };
        // A late round that left the head it read pushed no fix, so a
        // catch-up of that head is not one.
        let head = Some(pr.head.as_str());
        if self
            .current()
            .is_some_and(|i| i.late_from.as_deref() == head)
        {
            self.update(|item| item.late_from = None)?;
        }
        // Past the lag, the forge's head is the branch on `origin`, not a
        // worker's push still arriving. A rework asked for mid-flight waits.
        let mut labels = pr.labels;
        labels.retain(|l| l != READY);
        if let Some((seen, description)) = foreign_change(&known, &labels, !pr.draft, &pr.head) {
            let kind = RulingKind::ForeignChange {
                description,
                known: seen,
            };
            return self.raise(number, kind);
        }
        if let Some(parked) = self.claude_files_changed(Unchecked::Stop)? {
            return Ok(parked);
        }
        if known.head.is_none() {
            if self.settings.git.merging == Merging::Auto {
                return self.regate_unknown(number, &pr.head);
            }
            let head = Some(pr.head.clone());
            self.update(|item| item.known.head = head)?;
        }
        if base == Base::Behind {
            return self.rebase(number, &pr.head, None);
        }
        if !self.settings.ci.block {
            return self.passed(number, pr.head);
        }
        // A check set registers a check at a time, so a verdict waits until
        // it has had time to register whole.
        if !settled(since, now) {
            return Ok(Begin::Idle);
        }
        let Some(issue) = self.current().map(|item| item.issue) else {
            return Ok(Begin::Idle);
        };
        match pr.checks {
            Checks::None => Ok(Begin::Idle),
            Checks::Pending(queued) => {
                let rerun = self.check_queued(number, &pr.head, since, &queued, None)?;
                Ok(rerun.unwrap_or(Begin::Idle))
            }
            Checks::Passed | Checks::Skipped => {
                let conclusion = if pr.checks == Checks::Skipped {
                    "skipped"
                } else {
                    "passed"
                };
                self.ci_settled(issue, number, &pr.head, conclusion, &[]);
                self.passed(number, pr.head)
            }
            Checks::Failed(checks) => {
                self.ci_settled(issue, number, &pr.head, "failed", &checks);
                self.ci_failed(number, pr.head, checks, None)
            }
        }
    }

    // Green CI: `auto` merges by the path a yes takes, and `ask` raises the
    // merge ruling with the pull request handed back `ready-for-human`. A
    // pull request no reviewer read, one with a listed bot's threads
    // unaddressed, or a head no round read and no fix turn of kelpie's
    // pushed, gets the ruling under `auto` too, naming why; open nits are
    // named and hold nothing. One an older state file left owing the listed
    // bots a pass gets it first, and a bot the pass went on without that
    // has since reviewed the head gets its late round first.
    fn passed(&mut self, number: u64, head: String) -> Result<Begin, StateError> {
        if self.bots_before_merge(&head)? {
            return self.review_step();
        }
        if let Some(begin) = self.late_review()? {
            return Ok(begin);
        }
        if let Some(begin) = self.talk_turn(&At::Ci(head.clone()))? {
            return Ok(begin);
        }
        // Read before the disputes are posted: a read to make sends the step
        // round again, and the post's count must not be lost to that.
        let open = match self.threads_open_read(number) {
            Ok(open) => open,
            Err(begin) => return Ok(*begin),
        };
        let disputes = match self.post_disputes(number)? {
            Ok(disputes) => disputes,
            Err(reason) => return Ok(self.gate_failed(reason)),
        };
        let unreviewed = self.current().and_then(|item| item.unreviewed.clone());
        let unread_head = self.current().is_some_and(|item| !item.vouches_for(&head));
        // A merge ruling's `no`, or a bot's late round, sent its fix here
        // unread, so the maintainer decides on it again, even under `auto`,
        // and is told so. A head kelpie did not send unread itself, such as
        // someone else's push, is not the worker's fix.
        let Some(item) = self.current() else {
            return Ok(Begin::Idle);
        };
        let fix_from = |from: &Option<String>| {
            from.as_ref().is_some_and(|from| *from != head)
                && !item.reviewed_heads.contains(&head)
                && item.sent_unread.contains(&head)
        };
        let (note_fix, late_fix) = (fix_from(&item.noted_from), fix_from(&item.late_from));
        let vouched = unreviewed.is_none()
            && open.holding.is_none()
            && !unread_head
            && !note_fix
            && !late_fix;
        if self.settings.git.merging == Merging::Auto && vouched {
            self.update(|item| {
                item.phase = Phase::Merge {
                    head,
                    readied: None,
                    auto: true,
                };
            })?;
            return self.merge();
        }
        if let Err(reason) = self.hand_back(number) {
            return Ok(self.gate_failed(reason));
        }
        self.update(|item| {
            item.known.labels.retain(|l| l != READY && l != HUMAN);
            item.known.labels.push(HUMAN.to_owned());
        })?;
        let kind = RulingKind::Merge {
            head,
            unreviewed,
            open_threads: open.holding,
            held_by: open.held_by,
            unread_head,
            note_fix,
            late_fix,
            nits: open.nits,
            disputes,
        };
        self.late_read_now();
        self.raise(number, kind)
    }

    // A worker that pushed nothing after its last red run would get the
    // same run again and again, so the second time the maintainer decides.
    // A red run once the work item's fix turns reach `ci.fix_attempts`
    // raises that ruling too, in place of a fix turn. A red run before a
    // reviewer read the head sends the worker's fix back to that `review`.
    // Each head read red is counted once, by whether its worker could have
    // caught it (`super::ci_failure`).
    pub(super) fn ci_failed(
        &mut self,
        number: u64,
        head: String,
        checks: Vec<String>,
        review: Option<Review>,
    ) -> Result<Begin, StateError> {
        // The failed runs' logs are read once here: they are what the failure is
        // counted by, what a rerun is judged on, and what the fix turn is told.
        let logs = match self.read_logs(&head, false) {
            Ok(logs) => logs,
            Err(begin) => return Ok(*begin),
        };
        self.count_ci_failure(&head, &logs)?;
        let Some(item) = self.current() else {
            return Ok(Begin::Idle);
        };
        if item.red_head.as_deref() == Some(head.as_str()) {
            let fix_turns = None;
            let rerun = self.rerun_offered(&head, &logs);
            return self.raise(
                number,
                Stuck::StillRed {
                    head,
                    checks,
                    fix_turns,
                    review,
                    rerun,
                }
                .into(),
            );
        }
        if let Some(begin) = self.rerun_if_unrelated(number, &head, &checks, &logs)? {
            return Ok(begin);
        }
        let Some(item) = self.current() else {
            return Ok(Begin::Idle);
        };
        let turns = item.counts.ci_fix_turns;
        if self
            .settings
            .ci
            .fix_attempts
            .cap()
            .is_some_and(|cap| turns >= cap)
        {
            let fix_turns = Some(turns);
            let rerun = self.rerun_offered(&head, &logs);
            return self.raise(
                number,
                Stuck::StillRed {
                    head,
                    checks,
                    fix_turns,
                    review,
                    rerun,
                }
                .into(),
            );
        }
        let help = format!("{}{}", self.red_help(number, &logs), self.flake_note());
        let repeated = self.current().and_then(|i| i.reran.as_deref()) == Some(head.as_str());
        let prompt = self.skills.invoke(
            Step::CiFix,
            &red_prompt(number, &head, &checks, &help, repeated),
        );
        // The fix turn is saved before the contributor is told, so a restart
        // does not tell them twice.
        let begin =
            self.back_to_worker(number, head.clone(), checks.clone(), prompt, true, review)?;
        self.tell_of_fix(number, &head, &checks);
        Ok(begin)
    }

    // The failure becomes the worker's next turn, and this head is marked red
    // so a second failure at it goes to the maintainer instead. A red run's
    // turn, `counted`, is one of those `ci.fix_attempts` caps, saved with it.
    // The turn's end goes on to CI, or back to `review` where one was waiting
    // for this head to be green.
    pub(super) fn back_to_worker(
        &mut self,
        number: u64,
        head: String,
        checks: Vec<String>,
        prompt: String,
        counted: bool,
        review: Option<Review>,
    ) -> Result<Begin, StateError> {
        let Some(issue) = self.current().map(|item| item.issue) else {
            return Ok(Begin::Idle);
        };
        let red = head.clone();
        self.update(|item| {
            if counted {
                item.counts.ci_fix_turns = item.counts.ci_fix_turns.saturating_add(1);
            }
            item.red_head = Some(red);
            if let Some(review) = review {
                item.resume = Some(Phase::Review(review));
            }
            item.turn = Turn::Next { prompt };
            item.phase = Phase::Implement;
        })?;
        Ok(Begin::Report(StepReport::CiFailed {
            issue,
            pull_request: number,
            head,
            checks,
        }))
    }

    /// Keeps whether the listing of the current work item's head reported any
    /// check, for `status`, in memory only
    pub(super) fn note_checks(&mut self, checks: &Checks) {
        let Some(ticket) = self.current().map(|item| item.issue) else {
            return;
        };
        if *checks == Checks::None {
            self.no_checks.insert(ticket);
        } else {
            self.no_checks.remove(&ticket);
        }
    }

    /// `"none"` while `item` waits on CI and its last listing reported no check
    pub(super) fn no_checks_of(&self, item: &WorkItem) -> Option<&'static str> {
        let waiting = matches!(item.phase, Phase::Ci { .. } | Phase::Review(_));
        (waiting && self.no_checks.contains(&item.issue)).then_some("none")
    }

    /// Where `head` stands against `origin`, or why git could not say
    pub(super) fn base_of(&self, head: &str) -> Result<Base, String> {
        let item = self.current().expect("a base is of a work item");
        worktree::base_of(
            &self.settings.git.checkout,
            &item.branch,
            head,
            &item.remote,
            self.item_base(item),
        )
        .map_err(|e| format!("cannot fetch main: {e}"))
    }

    // A head no gate vouched for, such as one a yes under `ask` left before
    // a restart onto `auto`, goes through every gate before any merge.
    fn regate_unknown(&mut self, number: u64, head: &str) -> Result<Begin, StateError> {
        let Some(item) = self.current() else {
            return Ok(Begin::Idle);
        };
        let issue = item.issue;
        let at = worktree::head(&self.settings.git.checkout, &item.worktree);
        let regated = at
            .map_err(|e| format!("cannot read the worktree's head: {e}"))
            .and_then(|at| self.regate(&at, head));
        if let Err(reason) = regated {
            return Ok(self.gate_failed(reason));
        }
        Ok(Begin::Report(StepReport::Regated {
            issue,
            pull_request: number,
            head: head.to_owned(),
        }))
    }

    pub(super) fn gate_failed(&self, reason: String) -> Begin {
        let Some(issue) = self.current().map(|item| item.issue) else {
            return Begin::Idle;
        };
        Begin::Report(StepReport::GateFailed { issue, reason })
    }
}

pub(super) fn settled(since: Timestamp, now: Timestamp) -> bool {
    now.0.saturating_sub(since.0) >= CHECKS_SETTLE
}

/// The first seven characters of a commit hash, as git shows it
pub(super) fn short(head: &str) -> &str {
    head.get(..7).unwrap_or(head)
}

fn red_prompt(number: u64, head: &str, checks: &[String], help: &str, repeated: bool) -> String {
    let repeated = match repeated {
        true => {
            " Its log named no file this branch changed, so kelpie reran this head's failed \
             jobs and they failed again: the failure repeats, so do not call it a flake."
        }
        false => "",
    };
    format!(
        "CI failed on your pull request #{number} at {}: {}.{repeated} {help} \
         Fix it on this branch, commit, and push with `git push origin HEAD`. \
         {KEEP_BODY_TRUE}",
        short(head),
        checks.join(", ")
    )
}

#[cfg(test)]
pub(super) mod tests;
