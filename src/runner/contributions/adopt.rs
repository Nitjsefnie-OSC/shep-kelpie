//! Taking a released contribution over
//!
//! A release makes the pull request kelpie's (ADR 0010), and kelpie finishes it
//! as it finishes an adopted pull request: a work item on its branch, in the
//! next free slot, after an adopted pull request and a rework and ahead of any
//! new issue ([`adopt_waiting`](super::super::adopt)). A release, and a
//! `rework` answer to the merge ruling, queue it here ([`follow_adoption`]);
//! the board's step starts it.
//!
//! A work item hangs on an issue, and a contributor's pull request often
//! closes none. Where it closes one, that one is the work item's. Where it
//! closes none, kelpie files one from the pull request's title and body,
//! labelled for the project's default implementer so the work item has an
//! agent, and adds `Resolves #<issue>` to the pull request's body through the
//! forge's edit, so the merge closes it. The record keeps the issue it filed,
//! so a poll that filed it and could not edit the body files no second one, and
//! that issue is what the work item hangs on even where the pull request closes
//! another one later: the filed issue is the one its body names, so it closes
//! with the merge and none is left open beside the work.
//!
//! A fork kelpie cannot push to is the maintainer's to decide on, not
//! kelpie's: a quiet one gets a ruling ([`awaits_edits`]) and nothing is
//! released. A filing or an edit the forge refuses is told in the log and
//! tried again at the next poll, and the pull request is queued for adoption
//! only once both went through.

use super::super::Runner;
use super::ruling::raise_no_edits;
use crate::issues::AgentLabel;
use crate::ports::Reviewed;
use crate::state::{Adopting, How, StateError, Waiting};

impl Runner {
    // Whether the release stage stops for the pull request this poll: the
    // contribution is on a fork kelpie cannot push to, and its ruling is
    // raised now, or the forge could not say, so nothing is concluded. The
    // next poll looks again.
    pub(super) fn awaits_edits(&mut self, number: u64) -> Result<bool, StateError> {
        let Ok(pr) = self.ports.forge.reviewed(&self.remote, number) else {
            return Ok(true);
        };
        if pr.pushable() {
            return Ok(false);
        }
        let now = self.ports.clock.now();
        let mut next = self.state.clone();
        raise_no_edits(&mut next, number, now);
        self.save(next)?;
        Ok(true)
    }

    // Queues each released contribution for adoption, filing the issue for one
    // whose pull request closes none. The work item itself starts on the
    // board's step, which takes adopted pull requests before its own issues.
    pub(super) fn follow_adoption(&mut self) -> Result<(), StateError> {
        let numbers: Vec<u64> = (self.state.contributions.iter())
            .filter(|c| c.released.is_some())
            .map(|c| c.pull_request)
            .collect();
        for number in numbers {
            self.queue_adoption(number)?;
        }
        Ok(())
    }

    // Queues released pull request `number` for adoption, once it has the issue
    // its work item hangs on. One already queued, or whose work item is open,
    // is left as it is.
    fn queue_adoption(&mut self, number: u64) -> Result<(), StateError> {
        let Some(held) = (self.state.contributions.iter())
            .find(|c| c.pull_request == number)
            .cloned()
        else {
            return Ok(());
        };
        let queued = (self.state.adopted.iter()).any(|w| w.pull_request == number);
        let open = (self.state.work_items.iter()).any(|i| i.pull_request == Some(number));
        if queued || open {
            return Ok(());
        }
        let pr = match self.ports.forge.reviewed(&self.remote, number) {
            Ok(pr) => pr,
            Err(e) => {
                self.adopt_told(number, "read the pull request", &e);
                return Ok(());
            }
        };
        // A fork kelpie cannot push to is the maintainer's to decide on, whether
        // it was quiet or said yes: the ruling asks, and nothing is posted on
        // the pull request.
        if !pr.pushable() {
            if self.ruling_waits(number) {
                return Ok(());
            }
            let now = self.ports.clock.now();
            let mut next = self.state.clone();
            raise_no_edits(&mut next, number, now);
            self.save(next)?;
            return Ok(());
        }
        // The issue its work item hangs on: the one an earlier poll filed for
        // it, or the lowest the pull request closes. A pull request that closes
        // an issue only after kelpie filed one keeps kelpie's: the filed issue
        // is the one named on the pull request's body, so it is the one the
        // merge closes, and none is left open beside the work.
        let mut filed = held.adopting.map(|a| (a.issue, a.resolves));
        let issue = match (
            filed.map(|(issue, _)| issue),
            pr.closes.iter().min().copied(),
        ) {
            (Some(issue), _) => issue,
            (None, Some(issue)) => issue,
            (None, None) => {
                let how = held.released.as_ref().map(|r| r.how);
                match self.file_issue(number, &pr, how)? {
                    Some(issue) => {
                        filed = Some((issue, false));
                        issue
                    }
                    None => return Ok(()),
                }
            }
        };
        // Only the issue kelpie filed needs naming on the pull request: one it
        // already closes is named there.
        let to_link = filed.is_some_and(|(_, resolves)| !resolves);
        if to_link && !self.link_issue(number, &pr, issue)? {
            return Ok(());
        }
        let mut next = self.state.clone();
        next.adopted.push(Waiting {
            pull_request: number,
            by_label: false,
        });
        self.save(next)
    }

    // Files the issue `number`'s work item hangs on, from the pull request's
    // title and body, under the default implementer's `agent:` label. It is
    // saved as filed before the body is touched, so a body the forge will not
    // let kelpie edit costs no second issue.
    fn file_issue(
        &mut self,
        number: u64,
        pr: &Reviewed,
        how: Option<How>,
    ) -> Result<Option<u64>, StateError> {
        let label = AgentLabel::of(&self.agents.default_implementer.name);
        if let Err(why) = self.make_missing_labels([label.new_label()]) {
            self.adopt_told(number, "make the label it needs", &why);
            return Ok(None);
        }
        let name = label.name().to_owned();
        let issue = match self.open_issue(&pr.title, &filed_body(number, pr, how), &[&name]) {
            Ok(issue) => issue,
            Err(e) => {
                self.adopt_told(number, "file its issue", &e);
                return Ok(None);
            }
        };
        self.change(number, |held| {
            held.adopting = Some(Adopting {
                issue,
                resolves: false,
            });
        })?;
        Ok(Some(issue))
    }

    // Adds `Resolves #<issue>` to the pull request's body, so the merge closes
    // the issue kelpie filed. Whether the forge took the edit, and the next
    // poll's retry when it did not.
    fn link_issue(&mut self, number: u64, pr: &Reviewed, issue: u64) -> Result<bool, StateError> {
        let body = format!("{}\n\nResolves #{issue}", pr.body.trim_end());
        match self
            .ports
            .forge
            .edit_pull_request(&self.remote, number, &pr.title, &body)
        {
            Ok(()) => {
                self.change(number, |held| {
                    if let Some(adopting) = held.adopting.as_mut() {
                        adopting.resolves = true;
                    }
                })?;
                Ok(true)
            }
            Err(e) => {
                let what = format!("add `Resolves #{issue}` to its body");
                self.adopt_told(number, &what, &e);
                Ok(false)
            }
        }
    }

    // Says that the forge would not do what adopting this contribution needs,
    // and that the next poll tries again.
    fn adopt_told(&self, number: u64, what: &str, why: &dyn std::fmt::Display) {
        self.notes.push(format!(
            "contribution #{number}: cannot {what}: {why}; it is tried at the next poll"
        ));
    }
}

// What the filed issue says: where it came from and who handed the pull
// request over, then its body as its contributor wrote it. A release by the
// maintainer's ruling says so, and does not claim the contributor released it.
fn filed_body(number: u64, pr: &Reviewed, how: Option<How>) -> String {
    let taken = match how {
        Some(How::Yes | How::ThumbsUp | How::Silence) => {
            "which its contributor released and kelpie took over"
        }
        Some(How::Maintainer) | None => "which the maintainer's ruling had kelpie take over",
    };
    format!(
        "Kelpie filed this issue from pull request #{number}, {taken}.\n\n{}\n",
        pr.body.trim_end()
    )
}
