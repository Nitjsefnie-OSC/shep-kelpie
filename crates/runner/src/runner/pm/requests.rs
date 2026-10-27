//! Which ruling a request of the project manager is about
//!
//! A work item can raise a new ruling under the id of one the maintainer
//! asked about while the wake runs. A request remembers when its ruling
//! was raised, and an answer that arrives for another is dropped. The
//! ruling keeps what was asked until it is answered, so a runner that
//! restarts asks again.

use super::Wake;
use super::asked::Asked;
use crate::ports::Timestamp;
use crate::runner::Runner;
use crate::runner::voice::brief;
use crate::state::Ruling;
use crate::ticket::TicketId;

/// What the maintainer asked of the project manager, about the ruling
/// raised at `raised`
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::runner) struct Request {
    /// What was asked
    pub(in crate::runner) asked: Asked,
    /// When the ruling asked about was raised
    pub(in crate::runner) raised: Option<Timestamp>,
}

/// The rulings `wakes` ask about: those the project leaves to the project
/// manager, and those the maintainer asked about, with what they asked
pub(super) fn split(wakes: &[Wake]) -> (Vec<TicketId>, Vec<(TicketId, Request)>) {
    let mut left = Vec::new();
    let mut requested = Vec::new();
    for wake in wakes {
        if let Wake::Answer { id, request, .. } = wake {
            match request {
                None => left.push(*id),
                Some(request) => requested.push((*id, *request)),
            }
        }
    }
    (left, requested)
}

impl Request {
    /// `asked` about `ruling`
    pub(super) fn of(asked: Asked, ruling: &Ruling) -> Self {
        Self {
            asked,
            raised: ruling.raised,
        }
    }
}

impl Runner {
    /// The pending ruling `id` that `request` was about, or what became of
    /// it, for the log; a ruling raised since under its id is told in its
    /// thread that the answer was dropped
    ///
    /// # Errors
    ///
    /// What was done instead, when the ruling is gone or is another.
    pub(super) fn ruling_asked(
        &mut self,
        id: TicketId,
        request: Request,
    ) -> Result<Ruling, String> {
        let Some(ruling) = self.state.rulings.iter().find(|r| r.id == id).cloned() else {
            return Err(format!("left ruling {id}: it was answered meanwhile"));
        };
        if ruling.raised != request.raised {
            let number = ruling.number();
            let said = format!(
                "The project manager's answer about ruling #{number} was dropped: the ruling was \
                 raised again while it worked. Ask again if you still want it."
            );
            self.pm_says(&ruling, &said);
            return Err(format!(
                "dropped its answer on ruling {id}: it was raised again"
            ));
        }
        self.mark_asked(id, None);
        Ok(Ruling {
            asked: None,
            ..ruling
        })
    }

    /// Says in its thread that the project manager did not answer
    /// `request` about ruling `id`, where that ruling still waits
    pub(super) fn pm_request_lost(&mut self, id: TicketId, request: Request) {
        let ruling = self.state.rulings.iter().find(|r| r.id == id).cloned();
        if let Some(ruling) = ruling.filter(|r| r.raised == request.raised) {
            self.mark_asked(id, None);
            self.pm_did_not_answer(&ruling);
        }
    }

    /// Asks again what each ruling holds a request for that no wake
    /// carries, as after a restart; with no project manager, says it was
    /// not answered
    pub(super) fn pm_requests_kept(&mut self) {
        let kept: Vec<Ruling> = (self.state.rulings.iter())
            .filter(|r| r.asked.is_some() && !self.pm.asking.contains_key(&r.id))
            .cloned()
            .collect();
        for ruling in kept {
            let (id, Some(asked)) = (ruling.id, ruling.asked) else {
                continue;
            };
            if self.agents.pm.is_none() {
                self.mark_asked(id, None);
                self.pm_did_not_answer(&ruling);
                continue;
            }
            self.pm.asking.insert(id, self.ports.clock.now());
            self.pm_due(Wake::Answer {
                id,
                asked: brief(&ruling).0,
                request: Some(Request::of(asked, &ruling)),
            });
        }
    }

    /// Keeps on ruling `id` what the maintainer asked of the project
    /// manager, or that nothing is owed
    pub(super) fn mark_asked(&mut self, id: TicketId, asked: Option<Asked>) {
        let mut next = self.state.clone();
        let Some(ruling) = next.rulings.iter_mut().find(|r| r.id == id) else {
            return;
        };
        if ruling.asked == asked {
            return;
        }
        ruling.asked = asked;
        if let Err(e) = self.save(next) {
            self.notes.push(format!(
                "cannot save what was asked of the project manager on ruling {id}: {e}"
            ));
        }
    }
}
