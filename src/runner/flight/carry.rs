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
use super::super::briefing::Watched;
use super::super::in_flight::Open;
use super::{End, Flight, News, Tell, unstarted, waiting};
use crate::ports::{Adopted, Ending, Timestamp};
use crate::settings::Harness;

/// A worker's turn as it crosses a handover
// wire format: changing this is a change to the handover's state format
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CarriedTurn {
    issue: u64,
    adopted: Adopted,
    started: Timestamp,
    open: Open,
}

impl CarriedTurn {
    /// The issue of the work item the turn is for
    pub fn issue(&self) -> u64 {
        self.issue
    }
}

impl Runner {
    /// The turns in flight, as the next image takes them, or why they
    /// cannot be taken
    ///
    /// # Errors
    ///
    /// Why the handover waits: a call in flight that is not a worker's turn,
    /// a turn whose process has not started or has just ended, or one a
    /// handover already carried.
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
        flights
            .flying
            .iter()
            .map(|(&issue, flight)| {
                let Some(call) = &flight.watched.call else {
                    return Err(format!(
                        "the call of #{issue} is a review or one the last handover carried, \
                         and only a worker's turn from this image is carried"
                    ));
                };
                let adopted = (self.ports.agents.adoption(call))
                    .filter(|adopted| adopted.flying.is_some())
                    .ok_or_else(|| {
                        format!("the turn of #{issue} has no process running, or has just ended")
                    })?;
                let open = (flights.ledger.peek(Some(issue))).ok_or_else(|| {
                    format!("the turn of #{issue} has no line in the usage ledger")
                })?;
                Ok(CarriedTurn {
                    issue,
                    adopted,
                    started: flight.watched.started,
                    open,
                })
            })
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
            started,
            open,
        } = turn;
        let now = self.ports.clock.now();
        let flying = adopted.flying.unwrap_or(crate::ports::Flying {
            left: 0,
            began: now,
            again: false,
            first: false,
            compact: false,
        });
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
        let run = move || {
            let ran = catch_unwind(AssertUnwindSafe(|| {
                End::Turn(agents.adopt(&adopted, &held))
            }));
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
            let end = unstarted(Some(Harness::ClaudeCode), &e);
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
            },
        );
        self.board_changed();
    }
}
