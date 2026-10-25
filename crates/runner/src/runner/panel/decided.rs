//! A ruling the panel decides: applied at once, and posted headed "Decided
//! by the panel" so the maintainer can read it later
//!
//! The decision is a comment of its own, since the ruling's was held back for
//! the votes and never posted. A `no <note>` reply to it reopens the ruling
//! (see `thread_replies`).

use crate::panel::{Advice, Voter};
use crate::runner::Runner;
use crate::runner::thread_replies::Settler;
use crate::runner::voice::{
    answer_for, asks_the_maintainer, is_the_maintainers, named, settled_comment,
};
use crate::state::Ruling;
use crate::ticket::TicketId;

impl Runner {
    // Applies the option `advice` stands behind to ruling `id` as the panel's
    // decision, and posts what was decided. A ruling that cannot be applied
    // stays pending, to be posted to the maintainer once its hold ends.
    pub(super) fn panel_decides(&mut self, id: TicketId, advice: Advice) {
        let Some(ruling) = self.state.rulings.iter().find(|r| r.id == id).cloned() else {
            return;
        };
        // A candidate the maintainer carries out is no answer for the worker.
        let picked = named(&ruling.kind, advice.choice);
        if picked.as_deref().is_some_and(asks_the_maintainer) {
            self.notes.push(format!(
                "ruling {id}: the panel's pick is to ask the maintainer, so the maintainer decides"
            ));
            return;
        }
        if picked.as_deref().is_some_and(is_the_maintainers) {
            self.notes.push(format!(
                "ruling {id}: the panel's pick is for the maintainer to carry out, so the \
                 maintainer decides"
            ));
            return;
        }
        let note = format!("Decided by the panel: {}", agreeing_why(&ruling, advice));
        let Some(answer) = answer_for(&ruling.kind, advice.choice, &note) else {
            self.notes.push(format!(
                "ruling {id}: option {} is not one it lists, so the maintainer decides",
                advice.choice
            ));
            return;
        };
        if let Err(e) = self.rule_by(id, answer, Settler::Panel) {
            self.notes.push(format!(
                "cannot apply the panel's decision on ruling {id}, so the maintainer decides: {e}"
            ));
            return;
        }
        // An answer about a head that is gone withdraws the ruling instead.
        let withdrawn = (self.threads_told(id)).is_some_and(|t| t.starts_with("is withdrawn"));
        if withdrawn {
            return;
        }
        self.notes.push(format!(
            "ruling {id}: decided by the panel on {} ({} of {})",
            advice.choice, advice.agree, advice.of
        ));
        self.post_decided(&ruling, advice);
    }

    // Posts the decision on the ruling's thread, and, where the maintainer
    // is known, watches it for a `no <note>`.
    fn post_decided(&mut self, ruling: &Ruling, advice: Advice) {
        let id = ruling.id;
        let key = advice.choice;
        let name = named(&ruling.kind, key).unwrap_or_default();
        let line = format!(
            "**Decided by the panel: {key}, {name}** ({} of {}). Nothing waits on the \
             maintainer. Reply `{id} no <note>` to reopen it.",
            advice.agree, advice.of
        );
        let body = settled_comment(ruling, &line);
        let Some(thread) = ruling
            .pull_request
            .or_else(|| ruling.issue.and_then(|t| t.github()))
        else {
            return;
        };
        if !self.app_covers() {
            if let Err(e) = self.comment_as_gh(thread, &body) {
                self.notes.push(format!(
                    "cannot post the panel's decision on #{thread}: {e}"
                ));
            }
            return;
        }
        let posted = match self.ports.voice.post(&self.remote, thread, &body) {
            Ok(posted) => posted,
            Err(e) => {
                self.notes.push(format!(
                    "cannot post the panel's decision on #{thread}: {e}"
                ));
                return;
            }
        };
        match self.maintainer() {
            Ok(login) => self.watch_decided(ruling.clone(), thread, &login, posted),
            Err(e) => self.notes.push(format!(
                "ruling {id}'s decision cannot be reopened from its thread, since the \
                 maintainer's login is not known: {e}"
            )),
        }
    }
}

// The first agreeing expert's reason, in the order the comment lists them.
fn agreeing_why(ruling: &Ruling, advice: Advice) -> String {
    let ballots = ruling.panel.as_deref().unwrap_or_default();
    Voter::ALL
        .into_iter()
        .filter_map(|voter| ballots.iter().find(|b| b.voter == voter))
        .filter_map(|b| b.vote.as_ref())
        .find(|v| v.choice == advice.choice)
        .map_or_else(String::new, |v| v.why.clone())
}
