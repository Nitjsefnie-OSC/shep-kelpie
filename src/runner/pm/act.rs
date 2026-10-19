//! Checking the project manager's answer against the board, and acting on
//! what holds
//!
//! A pick or a hold must name a ready issue the board shows, and an unstick
//! a work item that is stuck now: parked on a ruling a stuck item raises,
//! or with a call the board reads as idle. Anything else is dropped, and
//! named in the log. Each unstick in the answer runs on its own, so one wake
//! that carried several stuck items can settle each, and one naming the same
//! item twice acts once.

use super::answer::{Action, Answer, Unstick};
use super::{Pick, StuckState, stuck_on};
use crate::runner::thread_replies::Settler;
use crate::runner::{Answer as Ruled, RuleError, Runner};
use crate::state::{RulingKind, StateError};
use crate::ticket::TicketId;
use crate::work_item::Phase;

/// The most of the project manager's words a ruling or a note carries, in
/// characters
pub(super) const WORDS: usize = 300;

impl Runner {
    /// Acts on `answer` as far as the board allows, and returns what it
    /// did and what it dropped, each in words. `picking` says the wake was
    /// for a free slot.
    ///
    /// # Errors
    ///
    /// [`StateError`] when what it acted on cannot be saved.
    pub(super) fn act(
        &mut self,
        answer: &Answer,
        picking: bool,
    ) -> Result<(Vec<String>, Vec<String>), StateError> {
        let ready = self.board_ready();
        // Starting nothing lasts until its next wake, which says again.
        if matches!(self.pm.pick, Some(Pick::Nothing(_))) {
            self.pm.pick = None;
        }
        let mut held: Vec<u64> = Vec::new();
        let mut dropped = answer.unread.clone();
        for &n in &answer.hold {
            match ready.contains(&n) {
                true if !held.contains(&n) => held.push(n),
                true => {}
                false => dropped.push(format!("hold #{n}: not a ready issue on the board")),
            }
        }
        let mut acted = Vec::new();
        let now = self.ports.clock.now();
        match answer.pick {
            Some(n) if held.contains(&n) => {
                dropped.push(format!("pick #{n}: it holds #{n} too"));
                if picking {
                    self.pm.pick = Some(Pick::Rule);
                }
            }
            Some(n) if ready.contains(&n) => {
                self.pm.pick = Some(Pick::Issue(n));
                acted.push(format!("picked #{n}"));
            }
            Some(n) => {
                dropped.push(format!("pick #{n}: not a ready issue on the board"));
                if picking {
                    self.pm.pick = Some(Pick::Rule);
                }
            }
            None if picking => {
                self.pm.pick = Some(Pick::Nothing(now));
                acted.push("starts nothing now".to_owned());
            }
            None => {}
        }
        if picking {
            self.pm.asked_pick = false;
        }
        if !held.is_empty() {
            let named: Vec<String> = held.iter().map(|n| format!("#{n}")).collect();
            acted.push(format!("holds {}", named.join(", ")));
        }
        self.pm.held = held;
        let mut unstuck: Vec<u64> = Vec::new();
        for unstick in &answer.unstick {
            if unstuck.contains(&unstick.item) {
                dropped.push(format!(
                    "{} #{}: the answer names it twice",
                    unstick.action.as_str(),
                    unstick.item
                ));
                continue;
            }
            unstuck.push(unstick.item);
            match self.unstick(unstick)? {
                Ok(done) => acted.push(done),
                Err(why) => dropped.push(why),
            }
        }
        if let Some(settle) = &answer.answer {
            match self.settle_question(settle)? {
                Ok(done) => acted.push(done),
                Err(why) => dropped.push(why),
            }
        }
        if let Some(decide) = &answer.follow_up {
            match self.settle_follow_up(decide)? {
                Ok(done) => acted.push(done),
                Err(why) => dropped.push(why),
            }
        }
        if let Some(standing) = &answer.standing {
            match self.pm_stand(standing) {
                Ok(done) => acted.push(done),
                Err(why) => dropped.push(why),
            }
        }
        if let Some(reply) = &answer.reply {
            self.pm.reply = Some(reply.clone());
        }
        Ok((acted, dropped))
    }

    // What one unstick in the answer did, or why it was dropped.
    fn unstick(&mut self, unstick: &Unstick) -> Result<Result<String, String>, StateError> {
        let Unstick { item, action, why } = unstick;
        let n = *item;
        let Some((ticket, stuck)) = self.stuck(n) else {
            return Ok(Err(format!(
                "{} #{n}: not a stuck work item on the board",
                action.as_str()
            )));
        };
        let now = self.ports.clock.now();
        let done = match (stuck, action) {
            (_, Action::Leave) => {
                self.pm_left_alone(n, stuck, now);
                format!("left #{n} to resolve itself")
            }
            (StuckState::Idle(..), action) => {
                self.pm.left_alone.remove(&n);
                // Its end parks it on the ceiling's ruling, which this then settles.
                self.flights.end(ticket);
                self.pm.after_end.insert(n, (*action, why.clone()));
                format!("ended #{n}'s idle call, to {}", action.as_str())
            }
            (StuckState::Ruling { id, .. }, action) => {
                self.pm.left_alone.remove(&n);
                if let Err(e) = self.settle(n, id, *action, why)? {
                    return Ok(Err(e));
                }
                match action {
                    Action::Retry => format!("retried #{n}"),
                    _ => format!("put #{n} to the maintainer on ruling {id}"),
                }
            }
        };
        Ok(Ok(done))
    }

    /// Settles a work item whose idle call the project manager had ended,
    /// once its end has parked it
    ///
    /// # Errors
    ///
    /// [`StateError`] when what it did cannot be saved.
    pub(in crate::runner) fn pm_after_end(&mut self, issue: u64) -> Result<(), StateError> {
        let Some((action, why)) = self.pm.after_end.remove(&issue) else {
            return Ok(());
        };
        let Some((_, StuckState::Ruling { id, .. })) = self.stuck(issue) else {
            return Ok(());
        };
        if let Err(e) = self.settle(issue, id, action, &why)? {
            self.notes.push(format!(
                "the project manager could not act on #{issue}: {e}"
            ));
        }
        Ok(())
    }

    // Retries the item parked on ruling `id` as a yes would, or for CI
    // still red sends the worker back; or puts it to the maintainer with
    // the project manager's words added to the ruling, posted again.
    fn settle(
        &mut self,
        issue: u64,
        id: u64,
        action: Action,
        why: &str,
    ) -> Result<Result<(), String>, StateError> {
        let Some(ruling) = self.state.rulings.iter().find(|r| r.id == id) else {
            return Ok(Err(format!("#{issue}: ruling {id} is answered")));
        };
        let why = plain(&self.shown(why.to_owned()), WORDS);
        if action == Action::Retry {
            let (answer, said) = match ruling.kind {
                RulingKind::Stuck(crate::state::Stuck::StillRed { .. }) => (
                    Ruled::No(format!(
                        "(from the project manager) CI is still red: fix it and push. PM says: {why}"
                    )),
                    "no, CI is still red, so the worker fixes it and pushes",
                ),
                _ => (Ruled::Yes, "yes"),
            };
            let by = Settler::Pm {
                answer: said.to_owned(),
                why: why.clone(),
                unread: false,
            };
            return match self.rule_by(id, answer, by) {
                Ok(()) => Ok(Ok(())),
                Err(RuleError::State(e)) => Err(e),
                Err(e) => Ok(Err(format!("retry #{issue}: {e}"))),
            };
        }
        let lead = match action {
            Action::ReScope => "The project manager proposes re-scoping it",
            _ => "The project manager asks you to decide",
        };
        let mut next = self.state.clone();
        let ruling = (next.rulings.iter_mut().find(|r| r.id == id)).expect("found above");
        ruling.question = match why.is_empty() {
            true => format!("{}\n\n{lead}.", ruling.question),
            false => format!("{}\n\n{lead}. PM says: {why}", ruling.question),
        };
        ruling.alerted = false;
        self.save(next)?;
        Ok(Ok(()))
    }

    // How `issue`'s work item is stuck now, if it is, by its ticket.
    fn stuck(&self, issue: u64) -> Option<(TicketId, StuckState)> {
        let ticket = TicketId::from_issue(issue)?;
        let item = self.state.item_by_ticket(ticket)?;
        if let Phase::Ruling { id } = item.phase {
            let ruling = self.state.rulings.iter().find(|r| r.id == id)?;
            return stuck_on(&ruling.kind).map(|_| {
                (
                    ticket,
                    StuckState::Ruling {
                        id,
                        raised: ruling.raised,
                    },
                )
            });
        }
        let watched = self.flights.watched(ticket)?;
        watched
            .idle
            .then_some((ticket, StuckState::Idle(watched.started)))
    }
}

// `text` on one line with no control characters, cut past `most` characters,
// since it reaches the maintainer's webhook and the state file as it is.
pub(super) fn plain(text: &str, most: usize) -> String {
    let spaced: String = (text.chars())
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let line = spaced.split_whitespace().collect::<Vec<_>>().join(" ");
    match line.char_indices().nth(most) {
        Some((end, _)) => format!("{} …", &line[..end]),
        None => line,
    }
}
