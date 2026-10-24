//! A pass's rotation, with the runner's lock taken once for each step
//!
//! A step and the call it starts are one lock scope. A step that needs a read returns it as a
//! job: the rotation lets the lock go, runs the job, takes the lock again and steps the work item
//! again, which finds the answer in the pass's store. State is read again after the gap, never
//! carried across it, and a work item that changed meanwhile is left for the next pass.

use std::sync::Mutex;

use super::Pass;
use crate::ports::Timestamp;
use crate::runner::Runner;
use crate::runner::off_lock::with_item;
use crate::runner::report::{Begin, StepReport};
use crate::runner::trigger::lock;
use crate::runner::turn::Slot;
use crate::runner::turn::reads::{Done, Job};
use crate::runner::worktree_jobs::Changed;
use crate::state::StateError;
use crate::ticket::TicketId;

/// Begins what is due next, starting the call it names
///
/// Steps each open work item with no call in flight, and the board while a slot is free, from
/// the one after the last to act, until one acts. The snapshot of the pass ends however it
/// does.
pub(super) fn begin_rotation(runner: &Mutex<Runner>) -> Result<Pass, StateError> {
    let pass = rotate(runner);
    lock(runner).end_pass();
    pass
}

/// Begins `ticket`'s turn again, as its session died unborn, and starts it
pub(super) fn start_over(
    runner: &Mutex<Runner>,
    ticket: TicketId,
) -> Result<Option<StepReport>, StateError> {
    let stepped = begin_over(runner, ticket);
    lock(runner).end_pass();
    Ok(match stepped? {
        Stepped::Started(Pass::Report(report)) => Some(report),
        Stepped::Started(Pass::Idle | Pass::Started) | Stepped::Idle | Stepped::Waits(_) => None,
    })
}

fn rotate(runner: &Mutex<Runner>) -> Result<Pass, StateError> {
    let (rotation, now) = {
        let mut held = lock(runner);
        held.open_pass();
        held.plan_rotation()?
    };
    let mut waiting = None;
    for slot in rotation {
        match step(runner, Step::Slot(slot), now)? {
            Stepped::Idle => {}
            Stepped::Waits(report) => {
                waiting.get_or_insert(report);
            }
            Stepped::Started(pass) => return Ok(pass),
        }
    }
    Ok(waiting.map_or(Pass::Idle, Pass::Report))
}

fn begin_over(runner: &Mutex<Runner>, ticket: TicketId) -> Result<Stepped, StateError> {
    let now = {
        let mut held = lock(runner);
        held.open_pass();
        held.ports.clock.now()
    };
    step(runner, Step::StartOver(ticket), now)
}

#[derive(Clone, Copy)]
pub(super) enum Step {
    Slot(Slot),
    StartOver(TicketId),
}

// What a step came to
pub(super) enum Stepped {
    // Nothing to do, or nothing due, or the work item changed during a read
    Idle,
    // A report that waits, which the rotation returns only if no other step did anything
    Waits(StepReport),
    // The step acted, and any call it names is in flight
    Started(Pass),
}

// What one lock scope came to
enum Next {
    Reads(Box<Job>),
    Done(Stepped),
}

// Steps `step` in one lock scope, then in one more for each read it asks for, which runs with the
// lock let go
pub(super) fn step(
    runner: &Mutex<Runner>,
    step: Step,
    now: Timestamp,
) -> Result<Stepped, StateError> {
    let mut next = scope(&mut lock(runner), step, now, Resume::Fresh)?;
    loop {
        let job = match next {
            Next::Done(done) => return Ok(done),
            Next::Reads(job) => job,
        };
        let key = job.key().clone();
        let done = job.run();
        let again = with_item(runner, &key, |runner| {
            let dropped = runner.busy.end();
            // A removal has done what a drop asks, and a drop the job's work item has outlived
            // is refused as any other is.
            if let Some(dropped) =
                dropped.filter(|_| !matches!(done, Done::Changed(Changed::Removed(_))))
            {
                // A rebase has pushed or merged already, and the drop may be refused: it is saved
                // first, and the step's end is the drop's to replace.
                if let Done::Changed(rebased @ Changed::Rebased(_)) = done {
                    runner.changed(rebased)?;
                }
                return runner
                    .drop_after_job(dropped)
                    .map(|()| Next::Done(Stepped::Idle));
            }
            let resume = match runner.apply_job(done)? {
                Some(begin) => Resume::Applied(Box::new(begin)),
                None => Resume::Read,
            };
            scope(runner, step, now, resume)
        });
        next = match again {
            Some(next) => next?,
            None => {
                let mut held = lock(runner);
                if let Some(dropped) = held.busy.end() {
                    held.drop_after_job(dropped)?;
                }
                held.held_up.forget_pending();
                // The next slot's step must not take this one's answers
                held.reads.clear();
                return Ok(Stepped::Idle);
            }
        };
    }
}

// How a scope's step begins
enum Resume {
    // The step is made, if it is due
    Fresh,
    // The step is made again over the reads it asked for, and is already known to be due
    Read,
    // A change to the worktree finished the step, which came to this
    Applied(Box<Begin>),
}

// The step and the call it starts, under the lock
fn scope(
    runner: &mut Runner,
    step: Step,
    now: Timestamp,
    resume: Resume,
) -> Result<Next, StateError> {
    let begin = match (step, resume) {
        (Step::Slot(slot), Resume::Applied(begin)) => {
            runner.slot_finished(slot, &begin, now);
            *begin
        }
        (_, Resume::Applied(begin)) => *begin,
        (Step::Slot(slot), Resume::Fresh) if !runner.looks.due(slot, now) => {
            return Ok(Next::Done(Stepped::Idle));
        }
        (Step::Slot(slot), _) => runner.step_slot(slot, now)?,
        (Step::StartOver(ticket), _) => runner.step_over(ticket)?,
    };
    // What the step read is kept for its steps made again, and no longer
    if !matches!(begin, Begin::Reads(_)) {
        runner.reads.clear();
    }
    let (slot, again) = match step {
        Step::Slot(slot) => (Some(slot), false),
        Step::StartOver(_) => (None, true),
    };
    let stepped = match begin {
        Begin::Reads(job) => {
            runner.busy.begin(&job);
            return Ok(Next::Reads(job));
        }
        Begin::Idle if slot.is_some() => Stepped::Idle,
        Begin::Report(report) if slot.is_some() && report.waits() => Stepped::Waits(report),
        begin => {
            runner.last_acted = slot.or(runner.last_acted);
            // Ended before the call starts, as it always was
            runner.end_pass();
            Stepped::Started(runner.start(begin, again)?)
        }
    };
    Ok(Next::Done(stepped))
}
