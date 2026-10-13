//! The board's list as the maintainer reads and orders it
//!
//! `board` shows the ready issues in the order the board takes them,
//! numbered from 1. `board move` and `board top` change that order, which the
//! state keeps in `order`, so the runner follows the new one on its next pick
//! and across a restart.

use std::fmt;

use serde::Serialize;

use super::Runner;
use crate::board::priority;
use crate::state::StateError;

/// What `board` answers
#[derive(Debug, Serialize)]
pub struct Board {
    /// The project
    pub project: String,
    /// The ready issues in the order the board takes them, first to go first,
    /// none of them open
    pub board: Vec<Place>,
}

/// An issue's place in the list
#[derive(Debug, Serialize)]
pub struct Place {
    /// Its place in the list, from 1
    pub position: usize,
    /// The issue
    pub issue: u64,
    /// Its title
    pub title: String,
    /// Its priority, from `P0` to `P4`, or null with none
    pub priority: Option<&'static str>,
    /// Whether an open issue blocks it, so the board passes it over
    pub blocked: bool,
    /// Whether the project manager holds it back until an open work item closes
    pub held: bool,
}

/// Why `board move` or `board top` was refused
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MoveError {
    /// The issue is not in the list
    NotReady(u64),
    /// Positions start at 1
    FromOne,
    /// The position is past the last place
    PastTheEnd {
        /// The position asked for
        position: u64,
        /// How many issues the list holds
        len: usize,
    },
    /// The change could not be saved
    State(StateError),
}

impl fmt::Display for MoveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotReady(issue) => write!(f, "#{issue} is not ready, so it is not in the list"),
            Self::FromOne => f.write_str("positions start at 1"),
            Self::PastTheEnd { position, len } => write!(
                f,
                "position {position} is past the end of the list, which holds {len} issues"
            ),
            Self::State(e) => e.fmt(f),
        }
    }
}

impl core::error::Error for MoveError {}

impl Runner {
    /// The list, as the board last read it
    pub fn board(&self) -> Board {
        let ready = self.ready_in_order();
        let held = self.pm_held();
        let board = ready
            .iter()
            .enumerate()
            .map(|(at, issue)| Place {
                position: at + 1,
                issue: issue.number,
                title: issue.title.clone(),
                priority: priority(&issue.labels),
                blocked: issue.blocked_by.iter().any(|b| b.open) || issue.unlisted_blockers > 0,
                held: held.contains(&issue.number),
            })
            .collect();
        Board {
            project: self.project.as_str().to_owned(),
            board,
        }
    }

    /// Puts `issue` at `position` in the list, counted from 1, and moves the
    /// issues between its old place and the new one by one
    ///
    /// # Errors
    ///
    /// [`MoveError`] when the issue is not in the list or the position is not
    /// a place in it. Nothing changes then.
    pub fn move_on_board(&mut self, issue: u64, position: u64) -> Result<(), MoveError> {
        let mut listed: Vec<u64> = self.ready_in_order().iter().map(|i| i.number).collect();
        if !listed.contains(&issue) {
            return Err(MoveError::NotReady(issue));
        }
        let place = match usize::try_from(position) {
            Ok(0) => return Err(MoveError::FromOne),
            Ok(place) if place <= listed.len() => place,
            _ => {
                return Err(MoveError::PastTheEnd {
                    position,
                    len: listed.len(),
                });
            }
        };
        listed.retain(|&n| n != issue);
        listed.insert(place - 1, issue);
        let order = refill(&self.state.order, &listed);
        if order == self.state.order {
            return Ok(());
        }
        let mut next = self.state.clone();
        next.order = order;
        self.save(next).map_err(MoveError::State)?;
        self.board_changed();
        Ok(())
    }
}

// `order` with `listed`'s issues in `listed`'s sequence, each in the places the
// listed ones held, so an issue the list leaves out, one a work item holds,
// keeps its own place between them. When `order` holds fewer places than
// `listed` has issues, the sequence's last issues are appended.
fn refill(order: &[u64], listed: &[u64]) -> Vec<u64> {
    let mut sequence = listed.iter().copied();
    let mut refilled: Vec<u64> = order
        .iter()
        .map(|n| {
            if listed.contains(n) {
                sequence.next().expect("a listed issue for each place")
            } else {
                *n
            }
        })
        .collect();
    refilled.extend(sequence);
    refilled
}
