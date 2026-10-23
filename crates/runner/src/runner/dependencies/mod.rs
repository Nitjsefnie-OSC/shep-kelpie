//! Merging the bumps bots open, and closing the ones that break
//!
//! Under `dependencies.pick = "on"`, once a [`BOARD_POLL`](super::BOARD_POLL)
//! the runner records each open pull request whose author is one of
//! `dependencies.bots`, with the head it first saw, and drops the record of one
//! that left the board. A bump behind `main`, or conflicting with it, gets
//! one `@dependabot rebase` for each head, and nothing is proved on a head
//! that is not on the latest `main`.
//!
//! A head on the latest `main` is proved by its checks, trusted once they have
//! had [`CHECKS_SETTLE`](super::CHECKS_SETTLE) to register as at the gate,
//! and, when it changes a `dependencies.web` path, by an e2e run through the
//! dog ([`flight::proving`](super::flight)). A proved bump is merged with a
//! merge commit. No review, ruling or slot is involved, since a bot's change
//! to a lockfile holds nothing to read that CI does not.
//!
//! A bump is red when a check failed, when the e2e run failed, or when its
//! checks are still pending or never started after `dependencies.wait_hours`.
//! Under `on_red = "close"` the comment naming what failed, the close and the
//! issue are three steps, each saved before the next, so a restart repeats
//! none. Under `"leave"` it is logged and left open until the bot pushes
//! again.

use super::Runner;
use super::dev_server::Browsed;
use super::flight::Verdict;
use super::follow_up::TRIAGE_LABEL;
use super::gate::{settled, short};
use crate::board::{OpenPullRequest, TRIAGE, is_bump};
use crate::ports::{Checks, E2eOutcome, ForgeError, NewLabel, PullRequestState, Timestamp};
use crate::settings::{Dependencies, OnRed};
use crate::state::{Bump, E2eResult, E2eState, Red, RedStep, StateError};
use crate::worktree::Base;
use read::Reads;

/// Made on a repo that lacks it before the first issue for a red bump
const DEPENDENCIES_LABEL: NewLabel<'static> = NewLabel {
    name: "dependencies",
    color: "0366d6",
    description: "A bump a bot opened that kelpie closed because it broke",
};

// What `status` says of a bump whose poll did not read what it needed, which the next one does
const UNREAD: &str = "waiting for the next poll to read it";

// What `status` says of a web bump the dog has no host for
const NO_HOST: &str = "a web bump, and the project has no e2e host, so it is not merged";

// Where one poll left a bump
enum Next {
    // Not yet: this is why
    Waiting(String),
    // Proved
    Merge,
    // Broken: this is what failed
    Red(Vec<String>),
}

impl Runner {
    // The bumps as `status` lists them, each with where it stands
    pub(super) fn bumps_status(&self) -> Vec<BumpStatus> {
        (self.state.bumps.iter())
            .map(|bump| BumpStatus {
                pull_request: bump.pull_request,
                author: bump.author.clone(),
                head: bump.head.clone(),
                standing: standing_of(bump),
            })
            .collect()
    }

    fn forget_bumps(&mut self) -> Result<(), StateError> {
        if self.state.bumps.is_empty() {
            return Ok(());
        }
        let mut next = self.state.clone();
        next.follow_bumps(Vec::new());
        self.save(next)
    }

    fn record_bumps(&mut self, open: &[OpenPullRequest], now: Timestamp) -> Result<(), StateError> {
        let mut followed: Vec<Bump> = Vec::new();
        for pr in open
            .iter()
            .filter(|pr| is_bump(&self.settings.dependencies, pr))
        {
            let known = (self.state.bumps.iter()).find(|b| b.pull_request == pr.number);
            followed.push(match known {
                Some(known) if known.head == pr.commit => known.clone(),
                Some(known) => known.pushed(pr.commit.clone(), now),
                None => Bump::new(
                    pr.number,
                    pr.author.clone(),
                    pr.head.clone(),
                    pr.commit.clone(),
                    now,
                ),
            });
        }
        // A bump kelpie decided to close is owed its issue wherever it went: off
        // the board, a draft, or no longer a listed bot's.
        let owed: Vec<Bump> = (self.state.bumps.iter())
            .filter(|b| b.issue_owed() && followed.iter().all(|f| f.pull_request != b.pull_request))
            .cloned()
            .collect();
        followed.extend(owed);
        let place = |b: &Bump| {
            let held = (self.state.bumps.iter()).position(|k| k.pull_request == b.pull_request);
            (held.unwrap_or(usize::MAX), b.pull_request)
        };
        followed.sort_by_key(place);
        if followed == self.state.bumps {
            return Ok(());
        }
        let mut next = self.state.clone();
        next.follow_bumps(followed);
        self.save(next)
    }

    fn work_bump(
        &mut self,
        number: u64,
        open: &[OpenPullRequest],
        now: Timestamp,
        reads: &mut Reads,
    ) -> Result<(), StateError> {
        let Some(bump) = self.bump(number) else {
            return Ok(());
        };
        if bump.red.is_some() {
            let on_board = open.iter().any(|pr| pr.number == number);
            if !on_board && bump.issue_owed() && self.settle_left_red(&bump, reads)? {
                return Ok(());
            }
            return self.finish_red(number);
        }
        let Some(pr) = open.iter().find(|pr| pr.number == number) else {
            return Ok(());
        };
        match self.prove(&bump, pr, now, reads) {
            Next::Waiting(why) => self.stand(number, why, false),
            Next::Red(why) => self.go_red(number, &bump.head, why),
            Next::Merge => self.merge_bump(&bump),
        }
    }

    // A red bump that left the board before kelpie closed it: closed by
    // someone else, its issue is still owed; merged, there is nothing to
    // file; unreadable, it waits. Says whether that is all for this poll.
    fn settle_left_red(&mut self, bump: &Bump, reads: &mut Reads) -> Result<bool, StateError> {
        let number = bump.pull_request;
        if bump
            .red
            .as_ref()
            .is_none_or(|red| red.step == RedStep::File)
        {
            return Ok(false);
        }
        let Some(read) = reads.left.remove(&number) else {
            return Ok(true);
        };
        match read {
            Ok(PullRequestState::Merged) => {
                self.notes.push(format!(
                    "bump #{number}: merged by someone else while red, so no issue"
                ));
                self.drop_bump(number)?;
                Ok(true)
            }
            Ok(PullRequestState::Closed) => {
                self.amend(number, |b| {
                    if let Some(red) = b.red.as_mut() {
                        red.step = RedStep::File;
                    }
                })?;
                Ok(false)
            }
            Ok(_) => Ok(false),
            Err(e) => {
                let why = format!("red, and cannot tell whether it was merged: {e}");
                self.stand(number, why, true)?;
                Ok(true)
            }
        }
    }

    fn bump(&self, number: u64) -> Option<Bump> {
        (self.state.bumps.iter())
            .find(|b| b.pull_request == number)
            .cloned()
    }

    // Changes bump `number`'s record with `change`, saving it when it changed.
    fn amend(&mut self, number: u64, change: impl FnOnce(&mut Bump)) -> Result<(), StateError> {
        let mut next = self.state.clone();
        if let Some(bump) = next.bumps.iter_mut().find(|b| b.pull_request == number) {
            change(bump);
        }
        match next == self.state {
            true => Ok(()),
            false => self.save(next),
        }
    }

    // Says why bump `number` is not merged yet, in the log too when `noted`.
    fn stand(&mut self, number: u64, why: String, noted: bool) -> Result<(), StateError> {
        if noted && self.bump(number).is_some_and(|b| b.standing != why) {
            self.notes.push(format!("bump #{number}: {why}"));
        }
        self.amend(number, |bump| bump.standing = why)
    }

    // Where `bump`, whose listing is `pr`, stands, and what to do about it.
    fn prove(
        &mut self,
        bump: &Bump,
        pr: &OpenPullRequest,
        now: Timestamp,
        reads: &mut Reads,
    ) -> Next {
        let Some(read) = reads.proofs.remove(&bump.pull_request) else {
            return Next::Waiting(UNREAD.into());
        };
        match read.base {
            Err(e) => return Next::Waiting(format!("cannot tell where it stands on main: {e}")),
            Ok(Base::Lagging) => {
                return Next::Waiting("waiting for its branch to match the forge's head".into());
            }
            Ok(Base::Behind) => return self.ask_rebase(bump),
            Ok(Base::Current) => {}
        }
        if !settled(bump.seen, now) {
            return Next::Waiting("waiting for its checks to register".into());
        }
        let hours = self.settings.dependencies.wait_hours.get();
        let overdue = now.0.saturating_sub(bump.seen.0) >= u64::from(hours) * 3600;
        match &pr.checks {
            Checks::Failed(names) => {
                return Next::Red(
                    names
                        .iter()
                        .map(|n| format!("check `{n}` failed"))
                        .collect(),
                );
            }
            Checks::Pending(_) if overdue => {
                return Next::Red(vec![format!(
                    "its checks were still pending after {hours} hours"
                )]);
            }
            Checks::None if overdue => {
                return Next::Red(vec![format!("no check reported on it after {hours} hours")]);
            }
            Checks::None | Checks::Pending(_) => {
                return Next::Waiting("waiting for its checks to finish".into());
            }
            Checks::Passed | Checks::Skipped => {}
        }
        self.prove_web(bump, read.web)
    }

    // A bump behind `main` is asked to rebase once for each head.
    fn ask_rebase(&mut self, bump: &Bump) -> Next {
        if bump.rebase_asked.as_deref() == Some(&bump.head) {
            return Next::Waiting("behind main: asked to rebase, and has not pushed yet".into());
        }
        let ask = format!("@{} rebase", Dependencies::mention(&bump.author));
        if let Err(e) = self.comment_as_gh(bump.pull_request, &ask) {
            return Next::Waiting(format!("behind main, and cannot ask for a rebase: {e}"));
        }
        self.notes.push(format!(
            "bump #{}: asked {} to rebase it onto main, at {}",
            bump.pull_request,
            bump.author,
            short(&bump.head),
        ));
        let head = bump.head.clone();
        match self.amend(bump.pull_request, |b| b.rebase_asked = Some(head)) {
            Ok(()) => Next::Waiting("behind main: asked to rebase".into()),
            Err(e) => Next::Waiting(format!("asked to rebase, but cannot save that: {e}")),
        }
    }

    // A web bump also needs its proof on this head: its routes opened in the local
    // browser where the project has one, and a passing e2e run where it has a host.
    fn prove_web(&mut self, bump: &Bump, web: Option<Result<bool, String>>) -> Next {
        let number = bump.pull_request;
        if bump.e2e_failure().is_some() {
            return Next::Red(vec!["its e2e run failed".into()]);
        }
        if bump.e2e_passed() {
            return Next::Merge;
        }
        if self.flights.proving(number, &bump.head) {
            return Next::Waiting("proving it: running the browser and e2e tests".into());
        }
        if let Some(why) = self.flights.declined(number, &bump.head) {
            return Next::Waiting(why.to_owned());
        }
        if self.settings.dependencies.web.is_empty() {
            return Next::Merge;
        }
        let web = match web {
            Some(Ok(web)) => web,
            Some(Err(e)) => return Next::Waiting(format!("cannot list the files it changes: {e}")),
            None => return Next::Waiting(UNREAD.into()),
        };
        if !web {
            return Next::Merge;
        }
        if self.draining() {
            return Next::Waiting("a web bump, waiting for the runner to stop draining".into());
        }
        if self.flights.proof_running() {
            return Next::Waiting("a web bump, waiting for the e2e host".into());
        }
        let browser = match self.browser_proof(number) {
            Ok(browser) => browser,
            Err(why) => return Next::Waiting(why),
        };
        if !self.launch_proof(number, &bump.head, browser) {
            self.release_proof_port(number);
            return Next::Waiting("a web bump, waiting for the e2e host".into());
        }
        Next::Waiting("proving it: running the browser and e2e tests".into())
    }

    // Records how the proof of `head` ended, if the bump is still at it.
    pub(super) fn record_proof(
        &mut self,
        number: u64,
        head: &str,
        verdict: Verdict,
    ) -> Result<(), StateError> {
        if self.bump(number).is_none_or(|b| b.head != head) {
            return Ok(());
        }
        let at = short(head);
        let (outcome, browsed) = match verdict {
            Verdict::E2e { outcome, browsed } => (outcome, browsed),
            Verdict::Browser(Browsed::Failed(why)) => {
                self.notes
                    .push(format!("bump #{number}: browser proof failed at {at}"));
                return self.go_red(number, head, why);
            }
            Verdict::Browser(Browsed::Unavailable(why)) => {
                self.flights.decline(number, head, &why);
                self.notes.push(format!(
                    "bump #{number}: cannot be proved in the browser: {why}"
                ));
                return self.stand(number, why, false);
            }
            Verdict::Browser(Browsed::NotRun(reason)) => {
                let why = format!("the browser proof did not happen: {reason}");
                return self.stand(number, why, true);
            }
            // A pass goes on to the e2e run, so the thread never ends on one.
            Verdict::Browser(Browsed::Passed) => return Ok(()),
        };
        if browsed {
            self.notes
                .push(format!("bump #{number}: browser passed at {at}"));
        }
        let state = match outcome {
            E2eOutcome::Passed => {
                self.notes
                    .push(format!("bump #{number}: e2e passed at {at}"));
                E2eState::Passed
            }
            E2eOutcome::Failed { output } => {
                let last = output.lines().last().unwrap_or_default();
                self.notes
                    .push(format!("bump #{number}: e2e failed at {at}: {last}"));
                E2eState::Failed(output)
            }
            // The browser proof stands where the dog has no host to add to it.
            E2eOutcome::NoHost if browsed => E2eState::Passed,
            E2eOutcome::NoHost => {
                self.flights.decline(number, head, NO_HOST);
                self.notes
                    .push(format!("bump #{number}: the project has no e2e host"));
                return self.stand(number, NO_HOST.into(), false);
            }
            E2eOutcome::NotRun { reason } => {
                let why = format!("the e2e run did not happen: {reason}");
                return self.stand(number, why, true);
            }
        };
        let head = head.to_owned();
        self.amend(number, |bump| bump.e2e = Some(E2eResult { head, state }))
    }

    fn merge_bump(&mut self, bump: &Bump) -> Result<(), StateError> {
        let number = bump.pull_request;
        if let Err(e) = self.ports.forge.merge(&self.remote, number, &bump.head) {
            return self.stand(number, format!("cannot merge it: {e}"), true);
        }
        self.notes
            .push(format!("bump #{number}: merged at {}", short(&bump.head)));
        self.drop_bump(number)
    }

    fn drop_bump(&mut self, number: u64) -> Result<(), StateError> {
        let mut next = self.state.clone();
        next.bumps.retain(|b| b.pull_request != number);
        self.save(next)
    }

    // Marks bump `number` red at `head`, with what failed, and goes on to
    // what `on_red` says.
    fn go_red(&mut self, number: u64, head: &str, why: Vec<String>) -> Result<(), StateError> {
        let (step, then) = match self.settings.dependencies.on_red {
            OnRed::Close => (RedStep::Comment, "closing it"),
            OnRed::Leave => (RedStep::Left, "left open"),
        };
        self.notes
            .push(format!("bump #{number}: red: {}; {then}", why.join("; ")));
        let red = Red {
            head: head.to_owned(),
            why,
            step,
        };
        self.amend(number, |bump| bump.red = Some(red))?;
        self.finish_red(number)
    }

    // Takes a red bump through the steps left, each saved before the next.
    // A step the forge refuses is tried again at the next poll.
    fn finish_red(&mut self, number: u64) -> Result<(), StateError> {
        loop {
            let Some((bump, red)) = self.bump(number).and_then(|b| Some((b.clone(), b.red?)))
            else {
                return Ok(());
            };
            let refused = match red.step {
                RedStep::Left => return Ok(()),
                RedStep::Comment => self.comment_red(&bump, &red).map(|()| RedStep::Close),
                RedStep::Close => self.close_red(&bump).map(|()| RedStep::File),
                RedStep::File => return self.file_red(&bump, &red),
            };
            match refused {
                Ok(step) => self.amend(number, |bump| {
                    if let Some(red) = bump.red.as_mut() {
                        red.step = step;
                    }
                })?,
                Err(e) => {
                    return self.stand(number, format!("red, and the forge refused: {e}"), true);
                }
            }
        }
    }

    fn comment_red(&self, bump: &Bump, red: &Red) -> Result<(), ForgeError> {
        let follows = match self.local_store() {
            true => "A ticket follows.",
            false => "An issue follows.",
        };
        let body = format!(
            "Kelpie is closing this pull request because it broke at `{}`:\n\n{}\n\n{follows}",
            short(&red.head),
            lines_of(&red.why),
        );
        self.comment_as_gh(bump.pull_request, &body)
    }

    fn close_red(&mut self, bump: &Bump) -> Result<(), ForgeError> {
        let number = bump.pull_request;
        (self.ports.forge).close_pull_request(&self.remote, number)?;
        self.notes.push(format!("bump #{number}: closed, red"));
        Ok(())
    }

    // Files the issue for a closed bump, which ends its record.
    // Files the issue for a closed bump, which ends its record. A project on
    // the local store files a ticket in the store instead, and makes no label
    // on its repo.
    fn file_red(&mut self, bump: &Bump, red: &Red) -> Result<(), StateError> {
        let number = bump.pull_request;
        let title = format!("dependencies: #{number}, {}, broke", bump.branch);
        let body = format!(
            "Kelpie closed pull request #{number} from `{}`, branch `{}`, because it broke \
             at `{}`:\n\n{}",
            bump.author,
            bump.branch,
            short(&red.head),
            lines_of(&red.why),
        );
        let labels = [TRIAGE, DEPENDENCIES_LABEL.name];
        let made = match self.local_store() {
            true => (self.file_ticket(&title, &body, &labels))
                .map(|ticket| format!("ticket {ticket}"))
                .map_err(|e| e.to_string()),
            false => {
                (self.make_missing_labels([TRIAGE_LABEL, DEPENDENCIES_LABEL])).and_then(|()| {
                    (self.open_issue(&title, &body, &labels))
                        .map(|issue| format!("issue #{issue}"))
                        .map_err(|e| e.to_string())
                })
            }
        };
        match made {
            Ok(issue) => {
                self.notes
                    .push(format!("bump #{number}: filed {issue} for it"));
                self.drop_bump(number)
            }
            Err(e) => self.stand(
                number,
                format!("closed, and cannot file its issue: {e}"),
                true,
            ),
        }
    }
}

fn lines_of(why: &[String]) -> String {
    (why.iter())
        .map(|line| format!("- {line}"))
        .collect::<Vec<_>>()
        .join("\n")
}

// What `status` says of a bump: the end of a red one, or why it is waiting.
fn standing_of(bump: &Bump) -> String {
    let Some(red) = &bump.red else {
        return match bump.standing.is_empty() {
            true => "not looked at yet".to_owned(),
            false => bump.standing.clone(),
        };
    };
    let then = match red.step {
        RedStep::Left => "left open",
        RedStep::Comment | RedStep::Close => "being closed",
        RedStep::File => "closed, its issue is next",
    };
    format!("red ({}), {then}", red.why.join("; "))
}

/// A bump as `status` lists it
// wire format: changing this is a breaking change to the status answer
#[derive(Debug, Clone, serde::Serialize)]
pub struct BumpStatus {
    /// The pull request
    pub pull_request: u64,
    /// Its author's login
    pub author: String,
    /// Its head commit's hash
    pub head: String,
    /// Where it stands, in a line
    pub standing: String,
}

pub(super) use read::follow_dependencies;
#[cfg(test)]
mod browser_tests;
mod read;
#[cfg(test)]
pub(in crate::runner) mod tests;
