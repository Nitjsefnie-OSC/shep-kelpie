//! The rig's tickets: the same issues its forge holds (ADR 0014)
//!
//! The rig has one forge, so a ticket read or written through it lands on the
//! same fake a test set up. The impl stands here rather than in `forge` itself,
//! so no call site there has both ports in scope at once.
//!
//! No runner call reaches it. `Runner::open` wraps the forge it is given in the
//! guard and takes both ports from that one guard, so a ticket write lands on
//! the rig's forge impl whichever port it was asked of. Which port a call used
//! is a rule over the source, and `scripts/ticket-port.sh` holds it.

use std::sync::atomic::Ordering;

use super::{CreatedIssue, FakeForge, held};
use crate::board::{READY, ReadyIssue};
use crate::ports::{
    FinishedParent, ForgeError, Issue, IssueCloseReason, OpenIssue, SubIssue, ThreadComment,
    Tickets,
};
use crate::settings::ForgeSlug;

impl Tickets for FakeForge {
    fn issue(&self, _repo: &ForgeSlug, number: u64) -> Result<Issue, ForgeError> {
        self.ask()?;
        self.wait_if_held(held::At::Issue);
        if self.missing.lock().unwrap().contains(&number) {
            return Err(ForgeError::Failed(format!("no issue #{number}")));
        }
        if let Some(allowed) = self.unread.lock().unwrap().get_mut(&number) {
            if *allowed == 0 {
                return Err(ForgeError::Failed(format!(
                    "#{number} cannot be read right now"
                )));
            }
            *allowed -= 1;
        }
        // Each read takes its own lock, so none may be held across another.
        let open = !self.closed.lock().unwrap().contains(&number);
        let filed = (self.issues.lock().unwrap().iter())
            .find(|i| i.number == number)
            .map(|i| (i.title.clone(), i.body.clone()));
        let (title, body) = filed.unwrap_or_else(|| {
            (
                format!("Title of #{number}"),
                format!("Body of #{number}.\n"),
            )
        });
        Ok(Issue {
            title,
            body,
            labels: self.labels_of(number),
            open,
            parent: self.parent_of(number),
            blocked_by: self.blockers(number),
        })
    }

    fn sub_issues(&self, _repo: &ForgeSlug, parent: u64) -> Result<Vec<SubIssue>, ForgeError> {
        self.read_sub_issues(parent)
    }

    fn finished_parents(&self, _repo: &ForgeSlug) -> Result<Vec<FinishedParent>, ForgeError> {
        self.read_finished_parents()
    }

    fn ready_issues(&self, _repo: &ForgeSlug) -> Result<Vec<ReadyIssue>, ForgeError> {
        self.board_reads.fetch_add(1, Ordering::SeqCst);
        self.ask()?;
        self.board()?;
        let ready = self.ready.lock().unwrap().clone();
        let closed = self.closed.lock().unwrap().clone();
        Ok(ready
            .into_iter()
            .filter(|i| !closed.contains(&i.number))
            .map(|i| ReadyIssue {
                labels: self.labels_of(i.number),
                blocked_by: self.blockers_of(i.number),
                parent: self.parent_of(i.number),
                sub_issues: self.count_sub_issues(i.number),
                ..i
            })
            .collect())
    }

    fn open_issues(&self, _repo: &ForgeSlug) -> Result<Vec<OpenIssue>, ForgeError> {
        self.ask()?;
        if let Some(why) = &*self.issues_down.lock().unwrap() {
            return Err(ForgeError::Failed(why.clone()));
        }
        Ok(self.issues.lock().unwrap().clone())
    }

    fn issue_comments(
        &self,
        _repo: &ForgeSlug,
        number: u64,
    ) -> Result<Vec<ThreadComment>, ForgeError> {
        self.ask()?;
        let mut on = self.threads.on(number);
        // An App's own comment is on the issue like anyone's: the forge lists it.
        if let Some(github) = self.app_comments.lock().unwrap().clone() {
            on.extend(github.posted_on(number));
            on.sort_by_key(|c| c.id);
        }
        Ok(on)
    }

    // Numbers start at 900, clear of every issue a test opens by hand.
    fn create_issue(
        &self,
        _repo: &ForgeSlug,
        title: &str,
        body: &str,
        labels: &[&str],
    ) -> Result<u64, ForgeError> {
        self.ask()?;
        self.issues_up()?;
        if let Some(left) = self.creates_left.lock().unwrap().as_mut() {
            if *left == 0 {
                return Err(ForgeError::Failed("issues are down".into()));
            }
            *left -= 1;
        }
        let mut created = self.created.lock().unwrap();
        let number = 900 + created.len() as u64;
        created.push(CreatedIssue {
            number,
            title: title.to_owned(),
            body: body.to_owned(),
            labels: labels.iter().map(|&l| l.to_owned()).collect(),
        });
        self.list_created(number, title, body, labels);
        drop(created);
        if labels.contains(&READY) {
            self.list_ready(number, false);
        }
        for label in labels.iter().filter(|&&l| l != READY) {
            self.label(number, label);
        }
        Ok(number)
    }

    fn close_issue(&self, _repo: &ForgeSlug, number: u64) -> Result<(), ForgeError> {
        self.ask()?;
        self.issues_up()?;
        if self.closes_down.load(Ordering::SeqCst) {
            self.closes_refused.fetch_add(1, Ordering::SeqCst);
            return Err(ForgeError::Failed("closing is refused".into()));
        }
        self.closed.lock().unwrap().insert(number);
        self.closings.lock().unwrap().push(number);
        Ok(())
    }

    fn close_issue_as(
        &self,
        _repo: &ForgeSlug,
        number: u64,
        reason: IssueCloseReason,
    ) -> Result<(), ForgeError> {
        self.ask()?;
        self.issues_up()?;
        if self.closes_down.load(Ordering::SeqCst) {
            self.closes_refused.fetch_add(1, Ordering::SeqCst);
            return Err(ForgeError::Failed("closing is refused".into()));
        }
        self.closed.lock().unwrap().insert(number);
        self.closings.lock().unwrap().push(number);
        self.closed_as.lock().unwrap().insert(number, reason);
        Ok(())
    }

    fn set_issue_label(
        &self,
        _repo: &ForgeSlug,
        number: u64,
        label: &str,
        on: bool,
    ) -> Result<(), ForgeError> {
        self.ask()?;
        if self.labels_down.load(Ordering::SeqCst) {
            return Err(ForgeError::Failed("labels are down".into()));
        }
        let mut labels = self.labels.lock().unwrap();
        let on_issue = labels.entry(number).or_default();
        on_issue.retain(|l| l != label);
        if on {
            on_issue.push(label.to_owned());
        }
        Ok(())
    }
}
