//! The triager's research call on an untouched issue, in flight
//!
//! One call at a time, on a thread of its own, so the runner's lock is not
//! held for the minutes a model takes and a call past its ceiling is ended
//! like any other. The thread makes the view of `origin/main` the call reads,
//! runs the call, reads its reply against that view and removes the view; what
//! it came to comes back as news, and only then is anything posted.

use std::fs;
use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};
use std::path::PathBuf;
use std::sync::Arc;
use std::thread;

use super::super::Runner;
use super::super::claim::Notes;
use super::super::code_graph::{indexed, remove};
use super::super::triage::ask::Purpose;
use super::super::triage::research::{Found, Listed, read_reply};
use super::{End, News};
use crate::metrics::Recorder;
use crate::ports::{AgentCall, AgentError, Agents, Ending, Timestamp};
use crate::state::StateError;
use crate::usage::{CallKind, Draft};
use crate::worktree;

/// How long the triager gets to research one issue, in seconds
pub(in crate::runner) const CEILING: u64 = 15 * 60;

/// The key of the research call's open line in the ledger: no work item's, since
/// the issue may be one with a turn in flight
pub(in crate::runner) const KEY: Option<u64> = Some(u64::MAX);

/// One issue to research, with everything its call needs, taken while the
/// runner's lock was held so that the call itself runs without it
pub(in crate::runner) struct Job {
    pub(in crate::runner) issue: u64,
    pub(in crate::runner) author: String,
    pub(in crate::runner) said: String,
    pub(in crate::runner) agent: String,
    /// The numbers the call was shown, which is all an `addressed_by` may name
    pub(in crate::runner) shown: Vec<u64>,
    /// Which pass of the author's round this call is
    pub(in crate::runner) purpose: Purpose,
    pub(in crate::runner) research: Research,
}

/// What the thread of a research call needs
pub(in crate::runner) struct Research {
    pub(in crate::runner) call: AgentCall,
    pub(in crate::runner) checkout: PathBuf,
    pub(in crate::runner) view: PathBuf,
    pub(in crate::runner) open: Vec<Listed>,
    pub(in crate::runner) most: usize,
}

/// The triager's call in flight for an issue
#[derive(Debug)]
pub(in crate::runner) struct Triaging {
    issue: u64,
    author: String,
    said: String,
    shown: Vec<u64>,
    purpose: Purpose,
    deadline: Timestamp,
    ending: Ending,
}

impl Triaging {
    // Ends the call once past its ceiling.
    pub(in crate::runner) fn end_overdue(&self, now: Timestamp) {
        if self.deadline <= now {
            self.ending.end();
        }
    }

    // When it is due to be ended, while it has not been.
    pub(in crate::runner) fn ceiling(&self) -> Option<Timestamp> {
        (!self.ending.asked()).then_some(self.deadline)
    }
}

impl super::Flights {
    /// The issue the triager is researching, while it is
    pub(in crate::runner) fn triaging(&self) -> Option<u64> {
        self.triaging.as_ref().map(|t| t.issue)
    }

    /// Whether `issue` was looked at since the last poll
    pub(in crate::runner) fn triage_tried(&self, issue: u64) -> bool {
        self.triage_tried.contains(&issue)
    }

    /// Notes that `issue` was looked at, so it is not again before the next poll
    pub(in crate::runner) fn triage_try(&mut self, issue: u64) {
        self.triage_tried.push(issue);
    }

    /// Lets `issue` be looked at again before the next poll
    pub(in crate::runner) fn triage_forget(&mut self, issue: u64) {
        self.triage_tried.retain(|&n| n != issue);
    }

    /// Lets every issue be looked at again, as a poll does
    pub(in crate::runner) fn triage_retry(&mut self) {
        self.triage_tried.clear();
    }
}

impl Runner {
    /// Starts the research `job` is on a thread of its own, ended past its
    /// ceiling, whose end comes back as news
    pub(in crate::runner) fn launch_triage(&mut self, job: Job) {
        let Job {
            issue,
            author,
            said,
            agent,
            shown,
            purpose,
            mut research,
        } = job;
        let now = self.ports.clock.now();
        let mut draft = Draft::of(&research.call, &agent, CallKind::Triage, now);
        draft.issue = Some(issue);
        let key = KEY;
        self.flights.ledger.open(key, draft, self.pacer_lines());
        self.call_started(&research.call, &agent);
        let ledger = self.flights.ledger.clone();
        let (news, wake) = (self.flights.send.clone(), self.flights.wake.clone());
        let ending = Ending::default();
        let held = ending.clone();
        let agents = self.counted(Arc::clone(&self.ports.agents));
        let (notes, metrics) = (self.notes.clone(), Arc::clone(&self.metrics));
        let graph = research
            .call
            .reach
            .code_graph
            .as_ref()
            .map(|g| [g.index.clone(), g.runtime.clone()]);
        let run = move || {
            let ran = catch_unwind(AssertUnwindSafe(|| {
                researched(
                    issue,
                    agents.as_ref(),
                    &mut research,
                    &notes,
                    &metrics,
                    &held,
                )
            }));
            if let Err(e) = worktree::remove_view(&research.checkout, &research.view) {
                eprintln!("triage #{issue}: cannot remove its view of main: {e}");
            }
            let _ = fs::remove_file(&research.call.settings);
            if let Some(graph) = graph
                && let Err(e) = remove(&graph)
            {
                eprintln!("triage #{issue}: {e}");
            }
            let (end, found) = ran.unwrap_or_else(|panic| (End::Panicked(panic), None));
            if end.reached_no_model() {
                ledger.no_model(key);
            }
            let _ = news.send(News::Triaged {
                issue,
                end,
                found: found.map(Box::new),
            });
            if let Some(wake) = &wake {
                let _ = wake.send(());
            }
        };
        let name = format!("#{issue} triage");
        if let Err(e) = thread::Builder::new().name(name).spawn(run) {
            let reason = format!("cannot start a thread for the call: {e}");
            let end = End::Turn(Err(AgentError::Setup(reason)));
            let heard = News::Triaged {
                issue,
                end,
                found: None,
            };
            let _ = self.flights.send.send(heard);
        }
        self.flights.triaging = Some(Triaging {
            issue,
            author,
            said,
            shown,
            purpose,
            deadline: Timestamp(now.0.saturating_add(CEILING)),
            ending,
        });
    }

    // Records the end of the call in flight, and gives its slot back: a reply
    // that reads is posted, and anything else counts as a failure to try
    // again at the next poll.
    pub(super) fn triage_heard(
        &mut self,
        issue: u64,
        end: End,
        found: Option<Result<Found, String>>,
    ) -> Result<(), StateError> {
        let Some(flight) = self.flights.triaging.take() else {
            return Ok(());
        };
        let open = self.flights.ledger.take(KEY);
        // Whether the call reached a model, which is what a cap counts: a call
        // whose reply could not be parsed cost one as surely as a call whose
        // reply read, and it is tried again at the next poll.
        let spent = !end.reached_no_model();
        let result = match end {
            End::Turn(result) => result,
            End::Review(_) => return Ok(()),
            End::Panicked(panic) => {
                if let Some(open) = &open {
                    self.panicked_line(open);
                }
                resume_unwind(panic)
            }
        };
        if let Some(open) = &open {
            self.plain_line(open, &result);
        }
        // A call that reached a model spent a place of the day's cap and of its
        // author's share of it, whether or not its reply read. One that never
        // got that far, whose settings could not be written or whose harness
        // could not start, spends nothing.
        if spent {
            self.triage_call_made(issue)?;
        }
        match (result, found) {
            // Stopped with the runner: the next poll asks again.
            (Err(AgentError::Stopped), _) => Ok(()),
            (Err(e), _) => {
                self.triage_failed(issue, &format!("its call failed: {e}"), true);
                Ok(())
            }
            (Ok(_), Some(Ok(found))) => {
                match flight.purpose {
                    // The questions or the prompt a restart lost are researched
                    // again, and what its reply gives is what goes up.
                    Purpose::Questions => {
                        if let Some(tries) = self.triage_tries.get_mut(&issue) {
                            tries.open = found.open.clone();
                        }
                        self.post_questions(issue, &flight.said)
                    }
                    Purpose::LocalPrompt => {
                        if let Some(tries) = self.triage_tries.get_mut(&issue) {
                            tries.open = found.open.clone();
                            tries.files = found.files.clone();
                        }
                        self.post_local_prompt(issue, &flight.said)
                    }
                    Purpose::First | Purpose::Answers | Purpose::LocalRead => self.post_triage(
                        issue,
                        &flight.author,
                        &flight.said,
                        &flight.shown,
                        &found,
                        flight.purpose,
                    ),
                }
            }
            (Ok(_), Some(Err(why))) => {
                self.triage_failed(issue, &why, true);
                Ok(())
            }
            (Ok(_), None) => {
                self.triage_failed(issue, "its reply was not read", true);
                Ok(())
            }
        }
    }
}

// Runs the call in a view of `origin/main` made for it, and reads its reply
// against that view.
fn researched(
    issue: u64,
    agents: &dyn Agents,
    research: &mut Research,
    notes: &Notes,
    metrics: &Recorder,
    ending: &Ending,
) -> (End, Option<Result<Found, String>>) {
    let Research {
        call,
        checkout,
        view,
        open,
        most,
    } = research;
    // A view a crash left behind would stop the new one.
    let _ = worktree::remove_view(checkout, view);
    if let Err(e) = worktree::view(checkout, view) {
        let reason = format!("cannot make a view of origin/main: {e}");
        return (End::Turn(Err(AgentError::Setup(reason))), None);
    }
    indexed(call, notes, metrics, ending);
    let ran = agents.prepare(call).and_then(|()| agents.run(call, ending));
    let found =
        (ran.as_ref().ok()).map(|reply| read_reply(&reply.text, issue, open, view, *most, notes));
    (End::Turn(ran), found)
}
