//! Which issues other people opened are due for triage
//!
//! While `triage.pick` is not `off`, the runner lists the open issues once a
//! [`BOARD_POLL`](super::BOARD_POLL), and `status` shows the untouched ones
//! under `triage.due`, oldest first. An issue is untouched when it carries
//! none of [`TRIAGE_LABELS`], no bot opened it, it was opened after
//! `triage.since`, and, under `maintainer`, the maintainer opened it.
//!
//! [`Runner::triage_due`], asked by the board's dispatch after a contribution's
//! call and before the board's pick, starts the research on the oldest of
//! them, or on the issue `shep kelpie triage` queued ahead of them: one call at a
//! time, in flight like the security review ([`flight::triage`]), whose
//! reply [`comment`] posts as one comment under the representative's words and
//! [`research`] reads. Each issue ends at `needs-triage`, except a maintainer's
//! issue the pass is sure of, which [`ready`] rewrites and makes `ready-for-agent`.

use std::collections::BTreeMap;

use serde::Serialize;

use super::Runner;
use super::report::Begin;
use super::review::calls::build_call;
use crate::board::TRIAGE;
use crate::ports::{AgentCall, AuthorAssociation, OpenIssue, Role, Timestamp, Tools};
use crate::settings::{ExpertAgent, Settings, TriagePick};
use crate::state::{ProjectState, StateError, TriageState, TriageStep, Triaged};
use research::{Listed, ListedPull};

use ask::Purpose;

pub(super) mod addressed;
pub(super) mod ask;
pub(super) mod comment;
pub(super) mod heard;
pub(super) mod limits;
pub(super) mod local;
pub(super) mod ready;
pub(super) mod research;
pub(super) mod spam;

use super::contributions::post::forge_is_busy;
use super::flight::triage::{Job, Research};
use spam::POSSIBLE_SPAM;

/// The labels that say an issue has been looked at, so triage leaves it alone.
/// `possible-spam` is one of them: an issue its classifier read as spam is left
/// alone until the maintainer says otherwise.
pub(super) const TRIAGE_LABELS: [&str; 6] = [
    "needs-triage",
    "needs-info",
    "ready-for-agent",
    "ready-for-human",
    "wontfix",
    POSSIBLE_SPAM,
];

/// What one runner's triage holds for its own run only: the account ages it
/// read from the forge, and whether the spam check could not be asked. The next
/// start reads the forge and the gateway again.
#[derive(Debug, Default)]
pub(super) struct Run {
    /// When each triage author's account was created, keyed by the login lowered.
    /// A `None` is an age the forge could not say, kept so a poll does not ask again
    pub(super) accounts: BTreeMap<String, Option<Timestamp>>,
    /// Whether the spam check is off for the rest of the run, told once
    pub(super) spam_off: bool,
}

/// An untouched issue, as `status` lists it
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DueStatus {
    /// Its number
    pub issue: u64,
    /// Its title
    pub title: String,
    /// The login of whoever opened it, empty when their account is gone
    pub author: String,
    /// How the forge counts whoever opened it to the repo, which is what the
    /// new-account limit reads, or none when the listing does not say
    pub author_association: Option<AuthorAssociation>,
    /// When it was opened
    pub created: Timestamp,
}

/// What the daily triage cap says, as `status` shows it
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DailyStatus {
    /// The triage calls the records place in the last 24 hours
    pub used: u32,
    /// `triage.per_day`
    pub cap: u32,
    /// When the next place opens, while the day's are all taken
    #[serde(skip_serializing_if = "Option::is_none")]
    pub opens: Option<Timestamp>,
    /// How long from now that is, in seconds, which is the same fact for the
    /// plain block, and has no clock of its own to work it out from
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wait: Option<u64>,
}

/// What `status` says of triage
#[derive(Debug, Serialize)]
pub struct TriageStatus<'a> {
    /// The untouched issues, oldest first
    pub due: &'a [DueStatus],
    /// The issue the research call is on, while it runs
    #[serde(skip_serializing_if = "Option::is_none")]
    pub running: Option<u64>,
    /// The issues kelpie has commented on, and how far each has got
    #[serde(skip_serializing_if = "<[Triaged]>::is_empty")]
    pub issues: &'a [Triaged],
    /// The day's cap and how many of its places are taken, while triage is on
    /// and the cap is
    #[serde(skip_serializing_if = "Option::is_none")]
    pub daily: Option<DailyStatus>,
}

/// Sets `state.triage` for a start under `settings`: stamped `now` when
/// `triage.pick` is not `off` and none is saved, cleared when it is `off`.
/// Returns whether it changed.
pub(super) fn settle_since(state: &mut ProjectState, settings: &Settings, now: Timestamp) -> bool {
    match (settings.triage.is_off(), &state.triage) {
        (true, Some(_)) => state.triage = None,
        (false, None) => {
            state.triage = Some(TriageState {
                since: now,
                issues: Vec::new(),
                unrecorded: Vec::new(),
            })
        }
        _ => return false,
    }
    true
}

impl Runner {
    // Lists the untouched issues once a poll while `triage.pick` is not `off`.
    // A listing that cannot be read leaves the last one as it was.
    //
    // The same poll lets an issue that failed be tried again, and finishes the
    // triage a restart cut short.
    pub(super) fn follow_triage(&mut self) -> Result<(), StateError> {
        let Some(since) = self.state.triage.as_ref().map(|t| t.since) else {
            return Ok(());
        };
        let now = self.ports.clock.now();
        if !self.looks.triage_due(now) {
            return Ok(());
        }
        self.flights.triage_retry();
        let Ok(open) = self.ports.forge.open_issues(&self.remote) else {
            return Ok(());
        };
        // A record that names no author, which is what a file saved before the
        // caps existed leaves, gets it from the listing: the author is what the
        // author's cap counts by and what tells the maintainer's own issues from
        // everyone else's.
        self.note_triage_authors(&open)?;
        // A wait on the author ends when the forge says the issue is closed or
        // carries a triage label the maintainer put on by hand, before anything
        // the record owes is written.
        self.settle_triage_waits(&open)?;
        // An author who took `local` answers by rewriting the issue body, which is
        // what the next pass is made on.
        self.watch_local_waits()?;
        self.resume_triage()?;
        let only = match self.settings.triage.pick {
            TriagePick::Maintainer => match self.maintainer() {
                Ok(login) => Some(login),
                Err(_) => return Ok(()),
            },
            TriagePick::All | TriagePick::Off => None,
        };
        let mut due: Vec<&OpenIssue> = (open.iter())
            .filter(|i| {
                untouched(i, since, only.as_deref()) && self.triage_record(i.number).is_none()
            })
            .collect();
        due.sort_by_key(|issue| (issue.created, issue.number));
        self.triage_due = due
            .into_iter()
            .map(|issue| DueStatus {
                issue: issue.number,
                title: issue.title.clone(),
                author: issue.author.clone(),
                author_association: issue.author_association,
                created: issue.created,
            })
            .collect();
        Ok(())
    }

    // What the daily cap says, or none while triage is off or the cap is
    fn triage_daily(&self) -> Option<DailyStatus> {
        let cap = self.settings.triage.per_day;
        if self.state.triage.is_none() || cap == 0 {
            return None;
        }
        let now = self.ports.clock.now();
        let (used, opens) = self.triage_day(now);
        let wait = (opens).map(|at| at.0.saturating_sub(now.0));
        Some(DailyStatus {
            used,
            cap,
            opens,
            wait,
        })
    }

    // What `status` says of triage, or none while there is nothing to say
    pub(super) fn triage_status(&self) -> Option<TriageStatus<'_>> {
        let issues = self.state.triage.as_ref().map_or(&[][..], |t| &t.issues);
        let running = self.flights.triaging();
        let daily = self.triage_daily();
        let silent = self.triage_due.is_empty() && issues.is_empty() && running.is_none();
        (!silent).then_some(TriageStatus {
            due: &self.triage_due,
            running,
            issues,
            daily,
        })
    }

    // Finishes the triage of each issue whose record stopped short of `done`,
    // as a restart leaves it
    fn resume_triage(&mut self) -> Result<(), StateError> {
        let held = self
            .state
            .triage
            .as_ref()
            .map_or_else(Vec::new, |t| t.issues.clone());
        for record in held
            .iter()
            .filter(|t| !matches!(t.step, TriageStep::Done | TriageStep::Ready))
        {
            if self.triage_given_up(record.issue) || self.flights.triaging() == Some(record.issue) {
                continue;
            }
            self.finish_triage(record.issue)?;
        }
        Ok(())
    }

    // The research call to start, if one is due: the issue the verb queued, then
    // the oldest untouched one, unless a call is in flight or the runner drains.
    // The caller holds a free slot for it, which the call keeps until it ends.
    //
    // An issue is tried once a poll, so one that fails does not hold up the
    // others. Where no App covers the repo nothing is started, since the comment
    // could not be posted as one.
    pub(super) fn triage_due(&mut self) -> Result<Option<Begin>, StateError> {
        let idle = self.state.triage.is_some() && !self.draining() && self.app_covers();
        if !idle || self.flights.triaging().is_some() {
            return Ok(None);
        }
        let queued = self.triage_queue.iter().map(|&issue| (issue, true));
        let untouched = self.triage_due.iter().map(|due| (due.issue, false));
        let candidates: Vec<(u64, bool)> = queued.chain(untouched).collect();
        for (issue, asked) in candidates {
            if self.flights.triage_tried(issue) || self.triage_given_up(issue) {
                continue;
            }
            // A record is the issue's, to finish or to leave, unless the verb asked again.
            if !asked && self.triage_record(issue).is_some() {
                continue;
            }
            self.flights.triage_try(issue);
            if self.triage_settled(issue)? {
                continue;
            }
            // A research that read but could not be posted is posted again, not redone.
            if self
                .triage_tries
                .get(&issue)
                .is_some_and(|t| t.waiting.is_some())
            {
                self.retry_triage(issue)?;
                continue;
            }
            // The limits, read from the records and the listing, before the call
            // is made.
            let waiting = (!asked)
                .then(|| self.triage_due.iter().find(|d| d.issue == issue).cloned())
                .flatten();
            let limited = match &waiting {
                Some(due) => self.triage_limited(&due.author, asked),
                None => false,
            };
            if let Some(due) = &waiting
                && limited
            {
                match self.triage_held(due) {
                    Some(limits::Held::Day) => continue,
                    Some(limits::Held::Final(limit)) => {
                        self.triage_hold(issue, &due.author, limit)?;
                        continue;
                    }
                    None => {}
                }
            }
            if let Some(job) = self.triage_job(issue, Purpose::First, limited) {
                return Ok(Some(Begin::Triage(job)));
            }
        }
        // The author's round: the second triager call, once their answers are
        // in, the body their own session rewrote, and a questions comment or a
        // prompt a restart left owed.
        self.triage_round_due()
    }

    // Reads `issue`'s comments, once, for a triage comment of the App's own: one
    // there records the issue as triaged with no call, and closes it again when
    // its marker says it was closing a duplicate. Whether the issue needs no
    // call now: it was recorded, or its comments could not be read.
    fn triage_settled(&mut self, issue: u64) -> Result<bool, StateError> {
        if self.triage_tries.get(&issue).is_some_and(|t| t.read) {
            return Ok(false);
        }
        let before = self.triage_record(issue).map(|t| t.comment);
        match self.marked_by_app(issue, before) {
            Ok(Some(marked)) => {
                self.record_triage(issue, marked.comment, marked.duplicate_of)?;
                match marked.duplicate_of {
                    Some(of) => self.close_duplicate(issue, of)?,
                    None => self.finish_triage(issue)?,
                }
                Ok(true)
            }
            Ok(None) => {
                self.triage_tries.entry(issue).or_default().read = true;
                Ok(false)
            }
            Err(e) => {
                let why = format!("cannot read its comments: {e}");
                self.triage_failed(issue, &why, !forge_is_busy(&e));
                Ok(true)
            }
        }
    }

    // Everything the research on `issue` needs, or `None` when it cannot be
    // had: the issue is no longer open, or what it needs cannot be read or built.
    fn triage_job(&mut self, issue: u64, purpose: Purpose, limited: bool) -> Option<Job> {
        let open = match self.ports.forge.open_issues(&self.remote) {
            Ok(open) => open,
            Err(e) => {
                let why = format!("cannot list the open issues: {e}");
                self.triage_failed(issue, &why, !forge_is_busy(&e));
                return None;
            }
        };
        let Some(this) = open.iter().find(|i| i.number == issue) else {
            self.triage_queue.retain(|&n| n != issue);
            return None;
        };
        // The labels the issue carries when its pass begins, kept once so a
        // triage label that comes by later is the maintainer's own.
        let tries = self.triage_tries.entry(issue).or_default();
        if tries.seen.is_none() {
            tries.seen = Some(this.labels.clone());
        }
        // The author's login comes from the same listing that made the call, so
        // a call that never posted is still counted against their cap.
        if tries.author.is_none() {
            tries.author = Some(this.author.clone());
        }
        // The classifier, after the limits and before the call. The maintainer's
        // own issues and one the verb asked for skip it, as they skip the limits.
        if matches!(purpose, Purpose::First) && limited {
            match self.triage_spam(this) {
                Ok(true) => return None,
                Ok(false) => {}
                Err(e) => {
                    let why = format!("cannot record its spam check: {e}");
                    self.triage_failed(issue, &why, true);
                    return None;
                }
            }
        }
        let listed: Vec<Listed> = (open.iter())
            .filter(|i| i.number != issue)
            .map(|i| (i.number, i.title.clone()))
            .collect();
        let pulls = match self.ports.forge.open_pulls(&self.remote) {
            Ok(pulls) => pulls,
            Err(e) => {
                let why = format!("cannot list the open pull requests: {e}");
                self.triage_failed(issue, &why, !forge_is_busy(&e));
                return None;
            }
        };
        let listed_pulls: Vec<ListedPull> = (pulls.iter())
            .filter(|pull| pull.number != issue)
            .map(|pull| ListedPull {
                number: pull.number,
                title: pull.title.clone(),
                closes: pull.closes.clone(),
            })
            .collect();
        let shown: Vec<u64> = (listed.iter())
            .map(|(n, _)| *n)
            .chain(listed_pulls.iter().map(|pull| pull.number))
            .collect();
        let agent = match self.settings.triager(&self.book) {
            Ok(agent) => agent,
            Err(e) => {
                self.triage_failed(issue, &e.to_string(), true);
                return None;
            }
        };
        let most = usize::from(self.settings.triage.max_questions.get());
        // The second pass reads the questions it asked and the author's answers
        // to them, each fenced like everything else that comes from outside. It
        // is not made without them: a triager shown neither the questions nor
        // the answers answers from the repo alone, and its `body` would be
        // written into the issue as if it came from the author.
        let settling = match purpose {
            Purpose::Answers => self.answers_round(issue),
            Purpose::First | Purpose::Questions | Purpose::LocalPrompt | Purpose::LocalRead => None,
        };
        if matches!(purpose, Purpose::Answers) && settling.is_none() {
            return None;
        }
        let subject = research::Subject {
            number: issue,
            title: &this.title,
            body: &this.body,
            // The body limit is one of the four, so it reaches no issue the
            // limits do not.
            max_body_kb: if limited {
                self.settings.triage.max_body_kb
            } else {
                0
            },
        };
        let prompt = research::prompt(&subject, &listed, &listed_pulls, most, settling.as_ref());
        let view = self.paths.triage(issue);
        let call = match self.triage_call(issue, &agent, &view, prompt) {
            Ok(call) => call,
            Err(why) => {
                self.triage_failed(issue, &why, true);
                return None;
            }
        };
        Some(Job {
            issue,
            author: this.author.clone(),
            said: format!("{}\n\n{}", this.title.trim(), this.body.trim()),
            agent: agent.name.as_str().to_owned(),
            shown,
            purpose,
            research: Research {
                call,
                checkout: self.settings.git.checkout.clone(),
                view,
                open: listed,
                most,
            },
        })
    }

    // The triager's call on `issue`: a fresh session in `view`, reading the repo
    // and changing nothing. It gets none of the repo's plugins, as the security
    // reviewer gets none, so it is not `prepared`.
    fn triage_call(
        &self,
        issue: u64,
        agent: &ExpertAgent,
        view: &std::path::Path,
        prompt: String,
    ) -> Result<AgentCall, String> {
        let folder = &self.paths.worker;
        let mut call = build_call(Role::Triager, issue, view, &agent.model, prompt)?;
        let instructions = folder.join(format!("triage-instructions-{issue}.md"));
        super::turn::write(
            folder,
            &instructions,
            agent.prompt.as_deref().unwrap_or_default(),
        )?;
        call.settings = folder.join(format!("triage-settings-{issue}.json"));
        call.instructions = Some(instructions);
        call.tools = Tools::Review;
        self.with_code_graph(&mut call);
        Ok(call)
    }

    // Queues issue `issue` ahead of the untouched ones, for `shep kelpie triage`
    //
    // The pick rule's `since` and author tests do not apply. A recorded issue is
    // triaged again, and its record is replaced once the new comment posts.
    //
    // # Errors
    //
    // Why it is refused: `triage.pick` is `off`, the issue is not open or
    // carries a triage label, or its call is in flight.
    pub(super) fn triage_ask(&mut self, issue: u64) -> Result<(), String> {
        if self.state.triage.is_none() {
            return Err("`triage.pick` is `off`, so no issue is triaged".to_owned());
        }
        if self.flights.triaging() == Some(issue) {
            return Err(format!("the triage call on #{issue} is in flight"));
        }
        let open = (self.ports.forge.open_issues(&self.remote))
            .map_err(|e| format!("cannot read the open issues: {e}"))?;
        let Some(found) = open.iter().find(|i| i.number == issue) else {
            return Err(format!("#{issue} is not an open issue"));
        };
        // The `needs-triage` its own record put on lets it through: that is the
        // issue to triage again.
        // Kelpie's own labels let the verb through: the `needs-triage` its record
        // put on, which is the issue to triage again, and the `possible-spam` its
        // classifier put on, which the maintainer overrules by asking for it.
        let recorded = self.triage_record(issue).is_some();
        let held = (found.labels.iter()).find(|l| {
            TRIAGE_LABELS.contains(&l.as_str())
                && *l != POSSIBLE_SPAM
                && !(recorded && *l == TRIAGE)
        });
        if let Some(label) = held {
            return Err(format!(
                "#{issue} carries `{label}`, so it is not untouched"
            ));
        }
        if !self.triage_queue.contains(&issue) {
            self.triage_queue.push(issue);
        }
        // Asked again by hand, it is tried now, and whatever kept it gives way. Its
        // comments are read only once a post may have gone up, since the one
        // asked for is the first of the new triage.
        self.triage_tries.insert(issue, comment::Tries::asked());
        self.flights.triage_forget(issue);
        self.looks.board_moved();
        Ok(())
    }
}

// Whether `issue` is untouched, opened after `since`, and by `only` when one is named
fn untouched(issue: &OpenIssue, since: Timestamp, only: Option<&str>) -> bool {
    let labelled = (issue.labels.iter()).any(|l| TRIAGE_LABELS.contains(&l.as_str()));
    !labelled
        && !issue.author_is_bot
        && issue.created > since
        && only.is_none_or(|login| issue.author.eq_ignore_ascii_case(login))
}

#[cfg(test)]
mod addressed_tests;
#[cfg(test)]
mod answers_tests;
#[cfg(test)]
mod ask_tests;
#[cfg(test)]
mod limits_tests;
#[cfg(test)]
mod local_tests;
#[cfg(test)]
mod pick_tests;
#[cfg(test)]
mod ready_tests;
#[cfg(test)]
mod research_tests;
#[cfg(test)]
mod spam_tests;
#[cfg(test)]
mod verb_tests;
