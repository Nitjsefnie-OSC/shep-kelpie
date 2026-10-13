//! The heads a work item's merge may take: ones a review round read, ones
//! kelpie sends to CI unread on purpose, and an adopted pull request's head
//! as it arrived, and the one whose CI rerun is awaited

use super::{Phase, WorkItem};

impl WorkItem {
    /// Records that a review round read `head`
    pub fn reviewed(&mut self, head: String) {
        if !self.reviewed_heads.contains(&head) {
            self.reviewed_heads.push(head);
        }
    }

    /// Records `head` as one kelpie sends to CI unread on purpose: a fix
    /// turn's push, a pass the project's own list gives nobody to read, or
    /// kelpie's own catch-up of a head it vouches for
    pub fn send_unread(&mut self, head: String) {
        if !self.sent_unread.contains(&head) {
            self.sent_unread.push(head);
        }
    }

    /// Records `head` as one a fix turn sent only nits pushed
    pub fn nit_fixed(&mut self, head: String) {
        if !self.nit_fix_heads.contains(&head) {
            self.nit_fix_heads.push(head);
        }
    }

    /// Whether the merge gate may take `head`: a review round read it, kelpie
    /// sent it to CI unread on purpose, or the pull request arrived at it
    /// when adopted
    pub fn vouches_for(&self, head: &str) -> bool {
        let is = |known: &String| known == head;
        self.reviewed_heads.iter().any(is)
            || self.sent_unread.iter().any(is)
            || self.arrived.as_ref().is_some_and(is)
    }

    /// The head whose failed CI jobs kelpie reran, while the item still
    /// waits on that rerun: in CI, or in a review waiting for CI, on that
    /// head, with no fix turn sent for it
    pub fn rerunning(&self) -> Option<&str> {
        let reran = self.reran.as_deref()?;
        let waiting = match &self.phase {
            Phase::Ci { head, .. } => head.as_deref() == Some(reran),
            Phase::Review(review) => review.ci.as_ref().is_some_and(|ci| ci.head == reran),
            _ => false,
        };
        (waiting && self.red_head.as_deref() != Some(reran)).then_some(reran)
    }
}
