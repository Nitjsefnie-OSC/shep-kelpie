//! Worker turns in flight across a handover
//!
//! A runner that hands over to a new build keeps its pid and its children,
//! so a turn's `claude` goes on running. The outgoing image carries what the
//! incoming one needs to finish the turn as it would have: the call's record
//! on disk, its line in the usage ledger as it started, and when it started.
//! Anything else in flight is not carried, and the handover is refused while
//! it runs.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;
use std::thread;

use serde::{Deserialize, Serialize};

use super::super::Runner;
use super::super::attach::live;
use super::super::briefing::Watched;
use super::super::in_flight::Open;
use super::super::transcript_count::Mark;
use super::{End, Flight, News, Tell, unstarted, waiting};
use crate::lease::wire::AskerCarry;
use crate::ports::{Adopted, Ending, Flying, Timestamp};
use crate::settings::Harness;
use crate::work_item::WorkItem;

/// A worker's turn as it crosses a handover
// wire format: changing this is a change to the handover's state format
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CarriedTurn {
    issue: u64,
    adopted: Adopted,
    flying: Flying,
    started: Timestamp,
    open: Open,
    // Where the turn's transcript stood as it began, so the next image counts its requests
    #[serde(default, skip_serializing_if = "Option::is_none")]
    requests: Option<Mark>,
}

impl CarriedTurn {
    /// The issue of the work item the turn is for
    pub fn issue(&self) -> u64 {
        self.issue
    }

    /// The process the turn's call runs as
    pub fn pid(&self) -> u32 {
        self.adopted.pid
    }
}

impl Runner {
    /// The turns in flight, as the next image takes them, or why they
    /// cannot be taken
    ///
    /// # Errors
    ///
    /// Why the handover waits: a call in flight that is not a worker's turn,
    /// a turn whose process has not started or has just ended, one that holds
    /// a browser, or one a handover already carried.
    pub fn carry_calls(&self) -> Result<Vec<CarriedTurn>, String> {
        let flights = &self.flights;
        let others = [
            (flights.pm.is_some(), "the project manager's call"),
            (flights.labelling.is_some(), "the issue writer's call"),
            (flights.panel.is_some(), "a ruling's panel"),
            (flights.retro.is_some(), "the batch retro"),
        ];
        if let Some((_, what)) = others.iter().find(|(flying, _)| *flying) {
            return Err(format!(
                "{what} is in flight, and only a worker's turn is carried"
            ));
        }
        for (harness, name) in [(Harness::Pi, "pi"), (Harness::Goose, "goose")] {
            if let Some(issue) = self.sessions_attached_on(harness).first() {
                return Err(format!(
                    "#{issue} is attached, and the {name} session in the maintainer's terminal \
                     dials a forwarder this image holds, which the next image would not"
                ));
            }
        }
        flights
            .flying
            .iter()
            .map(|(&issue, flight)| {
                if flight.browser.is_some() {
                    return Err(format!(
                        "#{issue}'s turn drives a browser, whose socket this image holds, \
                         which the next image would not"
                    ));
                }
                let Some(call) = &flight.watched.call else {
                    return Err(format!(
                        "the call of #{issue} is a review or one the last handover carried, \
                         and only a worker's turn from this image is carried"
                    ));
                };
                let adopted = (self.ports.agents.adoption(call))
                    .map_err(|why| format!("the turn of #{issue} cannot be carried: {why}"))?;
                let flying = adopted.flying.ok_or_else(|| {
                    format!("the record of #{issue}'s turn notes no ceiling, so it cannot be timed")
                })?;
                let open = (flights.ledger.peek(Some(issue))).ok_or_else(|| {
                    format!("the turn of #{issue} has no line in the usage ledger")
                })?;
                Ok(CarriedTurn {
                    issue,
                    adopted,
                    flying,
                    started: flight.watched.started,
                    open,
                    requests: flight.requests.clone(),
                })
            })
            .collect()
    }

    /// The lease totals as the next image takes them up, if this runner's
    /// leases keep any
    pub fn carry_leases(&self) -> Option<AskerCarry> {
        self.ports.leases.keep()
    }

    // The work items whose hold stands on a worker of `harness`, whose session in the
    // maintainer's terminal dials a forwarder only this image holds.
    pub(in crate::runner) fn sessions_attached_on(&self, harness: Harness) -> Vec<u64> {
        let holds = |item: &&WorkItem| item.attached.as_ref().is_some_and(live);
        let on_it = |item: &&WorkItem| {
            // parity: harness.attach-ends
            (self.worker_agent(item)).is_ok_and(|agent| agent.harness.harness() == harness)
        };
        let items = self.state.work_items.iter();
        items
            .filter(|item| holds(item) && on_it(item))
            .map(|item| item.issue)
            .collect()
    }

    /// Takes on `turns` a previous image carried, each finished on its own
    /// thread as the image that started it would have
    pub fn adopt_calls(&mut self, turns: Vec<CarriedTurn>) {
        for turn in turns {
            self.adopt_turn(turn);
        }
    }

    fn adopt_turn(&mut self, turn: CarriedTurn) {
        let CarriedTurn {
            issue,
            adopted,
            flying,
            started,
            open,
            requests,
        } = turn;
        let harness = adopted.harness;
        let ending = Ending::default();
        ending.carry(flying);
        let tell = Tell {
            issue,
            news: self.flights.send.clone(),
            wake: self.flights.wake.clone(),
        };
        self.flights.ledger.restore(Some(issue), open);
        let (held, ledger) = (ending.clone(), self.flights.ledger.clone());
        let agents = Arc::clone(&self.ports.agents);
        let metrics = Arc::clone(&self.metrics);
        let notes = self.notes.clone();
        let run = move || {
            let ran = catch_unwind(AssertUnwindSafe(|| {
                End::Turn(agents.adopt(&adopted, &held))
            }));
            if let Some(mark) = &requests {
                mark.count(adopted.harness, issue, &metrics, &notes);
            }
            let end = ran.unwrap_or_else(End::Panicked);
            if end.reached_no_model() {
                ledger.no_model(Some(issue));
            }
            tell.send(News::Ended { issue, end });
        };
        let began = thread::Builder::new()
            .name(format!("#{issue} worker"))
            .spawn(run);
        if let Err(e) = began {
            let end = unstarted(Some(harness), &e);
            let _ = self.flights.send.send(News::Ended { issue, end });
        }
        self.flights.flying.insert(
            issue,
            Flight {
                deadline: Some(Timestamp(flying.began.0.saturating_add(flying.left))),
                left: flying.left,
                ending,
                again: flying.again,
                watched: Watched {
                    started,
                    call: None,
                    idle: false,
                },
                first: flying.first,
                compact: flying.compact,
                began_at: Some(flying.began),
                told: None,
                fallback: waiting::Fallback::Stays,
                browser: None,
                requests: None,
            },
        );
        self.board_changed();
    }
}

#[cfg(test)]
mod tests;
