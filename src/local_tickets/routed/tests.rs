use std::sync::Mutex;

use super::*;
use crate::local_tickets::Links;
use crate::test::short_tempdir;

fn id(text: &str) -> TicketId {
    text.parse().unwrap()
}

fn repo() -> ForgeSlug {
    "shep-pm/shep".to_owned().try_into().unwrap()
}

// GitHub as the tests see it: it says which calls reached it, and files issue 900
#[derive(Default)]
struct Github {
    calls: Mutex<Vec<String>>,
}

impl Github {
    fn called(&self, what: String) {
        self.calls.lock().unwrap().push(what);
    }
}

fn issue(title: &str) -> Issue {
    Issue {
        title: title.to_owned(),
        body: String::new(),
        labels: Vec::new(),
        open: true,
        parent: None,
        blocked_by: Vec::new(),
    }
}

impl Tickets for Github {
    fn issue(&self, _repo: &ForgeSlug, ticket: TicketId) -> Result<Issue, ForgeError> {
        self.called(format!("issue {ticket}"));
        Ok(issue("on GitHub"))
    }

    fn sub_issues(&self, _: &ForgeSlug, parent: TicketId) -> Result<Vec<SubIssue>, ForgeError> {
        self.called(format!("sub_issues {parent}"));
        Ok(Vec::new())
    }

    fn finished_parents(&self, _: &ForgeSlug) -> Result<Vec<FinishedParent>, ForgeError> {
        self.called("finished_parents".into());
        Ok(Vec::new())
    }

    fn ready_issues(&self, _: &ForgeSlug) -> Result<Vec<ReadyIssue>, ForgeError> {
        self.called("ready_issues".into());
        Ok(Vec::new())
    }

    fn open_issues(&self, _: &ForgeSlug) -> Result<Vec<OpenIssue>, ForgeError> {
        self.called("open_issues".into());
        Ok(Vec::new())
    }

    fn issue_comments(
        &self,
        _: &ForgeSlug,
        ticket: TicketId,
    ) -> Result<Vec<ThreadComment>, ForgeError> {
        self.called(format!("issue_comments {ticket}"));
        Ok(Vec::new())
    }

    fn create_issue(
        &self,
        _: &ForgeSlug,
        title: &str,
        _: &str,
        _: &[&str],
    ) -> Result<TicketId, ForgeError> {
        self.called(format!("create_issue {title}"));
        Ok(id("900"))
    }

    fn close_issue(&self, _: &ForgeSlug, ticket: TicketId) -> Result<(), ForgeError> {
        self.called(format!("close_issue {ticket}"));
        Ok(())
    }

    fn close_issue_as(
        &self,
        _: &ForgeSlug,
        ticket: TicketId,
        _: IssueCloseReason,
    ) -> Result<(), ForgeError> {
        self.called(format!("close_issue_as {ticket}"));
        Ok(())
    }

    fn set_issue_label(
        &self,
        _: &ForgeSlug,
        ticket: TicketId,
        label: &str,
        _: bool,
    ) -> Result<(), ForgeError> {
        self.called(format!("set_issue_label {ticket} {label}"));
        Ok(())
    }
}

fn stand_up(source: TicketSource) -> (tempfile::TempDir, Arc<Github>, Routed, LocalTickets) {
    let home = short_tempdir();
    let local = LocalTickets::new(home.path().join("tickets"));
    let github = Arc::new(Github::default());
    let routed = Routed::new(github.clone(), local.clone(), source);
    (home, github, routed, local)
}

#[test]
fn each_ticket_is_read_from_the_source_its_id_names() {
    for source in [TicketSource::Local, TicketSource::Github] {
        let (_home, github, routed, local) = stand_up(source);
        local
            .create("In the store", "", &[], Links::default())
            .unwrap();
        assert_eq!(routed.issue(&repo(), id("12")).unwrap().title, "on GitHub");
        assert_eq!(
            routed.issue(&repo(), id("k1")).unwrap().title,
            "In the store"
        );
        assert_eq!(*github.calls.lock().unwrap(), ["issue 12"]);
    }
}

#[test]
fn a_ticket_is_closed_and_labelled_in_the_source_its_id_names() {
    let (_home, github, routed, local) = stand_up(TicketSource::Local);
    local
        .create("In the store", "", &[], Links::default())
        .unwrap();
    routed.close_issue(&repo(), id("k1")).unwrap();
    routed.close_issue(&repo(), id("12")).unwrap();
    routed
        .set_issue_label(&repo(), id("12"), "wip", true)
        .unwrap();
    assert!(!routed.issue(&repo(), id("k1")).unwrap().open);
    assert_eq!(
        *github.calls.lock().unwrap(),
        ["close_issue 12", "set_issue_label 12 wip"]
    );
}

#[test]
fn the_board_reads_the_projects_source() {
    let (_home, github, routed, local) = stand_up(TicketSource::Local);
    local
        .create("Ready", "", &[crate::board::READY], Links::default())
        .unwrap();
    assert_eq!(routed.ready_issues(&repo()).unwrap().len(), 1);
    assert!(github.calls.lock().unwrap().is_empty());

    let (_home, github, on_github, _local) = stand_up(TicketSource::Github);
    on_github.ready_issues(&repo()).unwrap();
    assert_eq!(*github.calls.lock().unwrap(), ["ready_issues"]);
}

#[test]
fn a_follow_up_is_filed_on_github_even_for_a_local_project() {
    let (_home, github, routed, local) = stand_up(TicketSource::Local);
    let filed = routed.create_issue(&repo(), "Follow up", "", &[]).unwrap();
    routed.open_issues(&repo()).unwrap();
    assert_eq!(filed, id("900"));
    assert_eq!(
        *github.calls.lock().unwrap(),
        ["create_issue Follow up", "open_issues"]
    );
    assert!(local.all().unwrap().is_empty());
}
