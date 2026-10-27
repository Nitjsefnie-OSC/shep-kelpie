//! Which ruling a request of the project manager is about
//!
//! A work item can raise a new ruling under the id of one the maintainer
//! asked about while the wake runs. A request remembers when its ruling
//! was raised, and an answer that arrives for another is dropped.

use super::asked::Asked;
use crate::ports::Timestamp;
use crate::runner::Runner;
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
        Ok(ruling)
    }

    /// Says in its thread that the project manager did not answer
    /// `request` about ruling `id`, where that ruling still waits
    pub(super) fn pm_request_lost(&mut self, id: TicketId, request: Request) {
        let ruling = self.state.rulings.iter().find(|r| r.id == id).cloned();
        if let Some(ruling) = ruling.filter(|r| r.raised == request.raised) {
            self.pm_did_not_answer(&ruling);
        }
    }
}
