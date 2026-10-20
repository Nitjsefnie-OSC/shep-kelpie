//! The calls in flight, as the drain reports them

use super::super::drain::{CallRole, CallRunning};
use crate::ticket::TicketId;

impl super::Flights {
    /// Whether no call is in flight, the project manager's included
    #[cfg(any(test, feature = "test-support"))]
    pub(super) fn idle(&self) -> bool {
        self.flying.is_empty()
            && self.pm.is_none()
            && self.labelling.is_none()
            && self.securing.is_none()
            && self.triaging.is_none()
            && !self.proof_running()
            && self.panel.is_none()
            && self.retro.is_none()
    }

    /// Every call in flight: each work item's by its issue, then the
    /// project manager's
    pub(in crate::runner) fn running(&self) -> Vec<CallRunning> {
        let items = self.flying.iter().map(|(&ticket, flight)| CallRunning {
            issue: Some(ticket),
            role: match flight.deadline {
                Some(_) => CallRole::Worker,
                None => CallRole::Reviewer,
            },
        });
        let pm = self.pm.iter().map(|_| CallRunning {
            issue: None,
            role: CallRole::Pm,
        });
        let labelling = self.labelling.iter().map(|l| CallRunning {
            issue: Some(l.issue),
            role: CallRole::IssueWriter,
        });
        let securing = self.securing.iter().map(|_| CallRunning {
            issue: None,
            role: CallRole::Security,
        });
        let triaging = self.triaging().into_iter().map(|issue| CallRunning {
            issue: TicketId::from_issue(issue),
            role: CallRole::Triager,
        });
        let panel = self.panel.iter().map(|_| CallRunning {
            issue: None,
            role: CallRole::Panel,
        });
        let retro = self.retro.iter().map(|_| CallRunning {
            issue: None,
            role: CallRole::Retro,
        });
        let proof = self.proof_running().then_some(CallRunning {
            issue: None,
            role: CallRole::E2e,
        });
        (items.chain(pm).chain(labelling).chain(securing))
            .chain(triaging)
            .chain(panel)
            .chain(retro)
            .chain(proof)
            .collect()
    }
}
