use super::*;
use crate::test::{short_tempdir, ticket};

fn id(text: &str) -> TicketId {
    text.parse().unwrap()
}

fn repo() -> ForgeSlug {
    "shep-pm/shep".to_owned().try_into().unwrap()
}

fn store() -> (tempfile::TempDir, LocalTickets) {
    let home = short_tempdir();
    let tickets = LocalTickets::new(home.path().join("tickets"));
    (home, tickets)
}

fn file(tickets: &LocalTickets, title: &str, labels: &[&str]) -> TicketId {
    tickets
        .create_issue(&repo(), title, "Body.", labels)
        .unwrap()
}

#[test]
fn tickets_take_the_next_counter_from_k1() {
    let (_home, tickets) = store();
    assert_eq!(file(&tickets, "One", &[]), id("k1"));
    assert_eq!(file(&tickets, "Two", &[]), id("k2"));
    let read = tickets.issue(&repo(), id("k2")).unwrap();
    assert_eq!((read.title.as_str(), read.body.as_str()), ("Two", "Body."));
    assert!(read.open);
}

#[test]
fn a_counter_comes_after_the_highest_on_disk() {
    let (_home, tickets) = store();
    file(&tickets, "One", &[]);
    std::fs::copy(
        tickets.folder().join("k1.md"),
        tickets.folder().join("k7.md"),
    )
    .unwrap();
    assert_eq!(file(&tickets, "Next", &[]), id("k8"));
}

#[test]
fn a_ticket_that_is_not_there_is_no_issue() {
    let (_home, tickets) = store();
    assert!(matches!(
        tickets.issue(&repo(), id("k1")),
        Err(ForgeError::NoIssue { .. })
    ));
    file(&tickets, "One", &[]);
    assert!(matches!(
        tickets.issue(&repo(), ticket(1)),
        Err(ForgeError::NoIssue { .. })
    ));
}

#[test]
fn only_an_open_ticket_labelled_ready_is_on_the_board() {
    let (_home, tickets) = store();
    let ready = file(&tickets, "Ready", &[READY, "priority: P1"]);
    file(&tickets, "Idea", &[]);
    let done = file(&tickets, "Done", &[READY]);
    tickets.close_issue(&repo(), done).unwrap();
    let board = tickets.ready_issues(&repo()).unwrap();
    assert_eq!(board.len(), 1);
    assert_eq!(board[0].ticket, ready);
    assert_eq!(board[0].labels, [READY, "priority: P1"]);
    assert!(!board[0].assigned);
}

#[test]
fn a_blocker_is_open_until_it_closes() {
    let (_home, tickets) = store();
    let first = file(&tickets, "First", &[]);
    let second = tickets
        .create(
            "Second",
            "",
            &[READY],
            Links {
                parent: None,
                blocked_by: vec![first],
            },
        )
        .unwrap();
    let blockers = |tickets: &LocalTickets| {
        let board = tickets.ready_issues(&repo()).unwrap();
        board
            .into_iter()
            .find(|i| i.ticket == second)
            .unwrap()
            .blocked_by
    };
    assert_eq!(
        blockers(&tickets),
        [Blocker {
            ticket: first,
            open: true
        }]
    );
    tickets.close_issue(&repo(), first).unwrap();
    assert_eq!(
        blockers(&tickets),
        [Blocker {
            ticket: first,
            open: false
        }]
    );
}

#[test]
fn a_link_to_a_ticket_the_store_lacks_is_refused() {
    let (_home, tickets) = store();
    let links = Links {
        parent: None,
        blocked_by: vec![id("k9")],
    };
    assert!(matches!(
        tickets.create("T", "", &[], links),
        Err(LocalError::Unknown(_))
    ));
    let links = Links {
        parent: Some(ticket(5)),
        blocked_by: vec![],
    };
    assert!(matches!(
        tickets.create("T", "", &[], links),
        Err(LocalError::Unknown(_))
    ));
    assert!(tickets.all().unwrap().is_empty(), "nothing was filed");
}

#[test]
fn a_parent_has_sub_issues_and_is_finished_when_they_all_close() {
    let (_home, tickets) = store();
    let parent = file(&tickets, "Parent", &[]);
    let child = |title: &str| {
        let links = Links {
            parent: Some(parent),
            blocked_by: vec![],
        };
        tickets.create(title, "", &[READY], links).unwrap()
    };
    let (a, b) = (child("A"), child("B"));
    tickets.close_issue(&repo(), a).unwrap();
    let subs = tickets.sub_issues(&repo(), parent).unwrap();
    assert_eq!(
        subs,
        [
            SubIssue {
                ticket: a,
                state: SubIssueState::Completed
            },
            SubIssue {
                ticket: b,
                state: SubIssueState::Open
            },
        ]
    );
    assert!(tickets.finished_parents(&repo()).unwrap().is_empty());
    tickets
        .close_issue_as(&repo(), b, IssueCloseReason::NotPlanned)
        .unwrap();
    assert_eq!(
        tickets.finished_parents(&repo()).unwrap(),
        [FinishedParent {
            ticket: parent,
            sub_issues: 2
        }]
    );
    let board = tickets.ready_issues(&repo()).unwrap();
    assert!(board.is_empty(), "both children are closed");
}

#[test]
fn a_label_goes_on_and_comes_off() {
    let (_home, tickets) = store();
    let one = file(&tickets, "One", &[]);
    tickets.set_issue_label(&repo(), one, READY, true).unwrap();
    tickets.set_issue_label(&repo(), one, READY, true).unwrap();
    assert_eq!(tickets.issue(&repo(), one).unwrap().labels, [READY]);
    tickets.set_issue_label(&repo(), one, READY, false).unwrap();
    assert!(tickets.issue(&repo(), one).unwrap().labels.is_empty());
}

#[test]
fn comments_read_in_the_order_they_were_written() {
    let (_home, tickets) = store();
    let one = file(&tickets, "One", &[]);
    tickets
        .update(one, |s| {
            s.comments.push(Comment {
                author: "rin".into(),
                at: 5,
                body: "First".into(),
            });
            s.comments.push(Comment {
                author: "kelpie".into(),
                at: 9,
                body: "Second".into(),
            });
        })
        .unwrap();
    let read = tickets.issue_comments(&repo(), one).unwrap();
    let said: Vec<_> = read
        .iter()
        .map(|c| (c.id, c.author.as_str(), c.body.as_str()))
        .collect();
    assert_eq!(said, [(1, "rin", "First"), (2, "kelpie", "Second")]);
    assert_eq!(read[1].created, Timestamp(9));
    assert_eq!(read[0].thread, 1);
}

#[test]
fn a_file_that_is_not_a_ticket_fails_the_read_naming_it() {
    let (_home, tickets) = store();
    file(&tickets, "One", &[READY]);
    std::fs::write(tickets.folder().join("k2.md"), "no front matter\n").unwrap();
    let error = tickets.ready_issues(&repo()).unwrap_err();
    assert!(matches!(&error, ForgeError::Unreadable(why) if why.contains("k2.md")));
}

#[test]
fn other_files_in_the_folder_are_not_tickets() {
    let (_home, tickets) = store();
    file(&tickets, "One", &[READY]);
    std::fs::write(tickets.folder().join("notes.md"), "scratch\n").unwrap();
    std::fs::write(tickets.folder().join("k2.md.tmp"), "half\n").unwrap();
    assert_eq!(tickets.all().unwrap().len(), 1);
    assert_eq!(file(&tickets, "Two", &[]), id("k2"));
}

#[test]
fn no_open_issues_are_listed_for_telling_a_finding_by() {
    let (_home, tickets) = store();
    file(&tickets, "One", &[]);
    assert!(tickets.open_issues(&repo()).unwrap().is_empty());
}
