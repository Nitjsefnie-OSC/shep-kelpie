//! The board, through its stand-ins
use super::*;
use crate::ports::Checks;
use crate::test::ticket;

fn ready(ticket: TicketId) -> ReadyIssue {
    ReadyIssue {
        ticket,
        title: String::new(),
        body: String::new(),
        assigned: false,
        labels: vec![READY.into()],
        blocked_by: vec![],
        unlisted_blockers: 0,
        parent: None,
        sub_issues: SubIssues::default(),
    }
}

fn blocked(ticket: TicketId, by: &[(TicketId, bool)]) -> ReadyIssue {
    let blocked_by = by.iter().map(|&(blocker, open)| Blocker {
        ticket: blocker,
        open,
    });
    ReadyIssue {
        blocked_by: blocked_by.collect(),
        ..ready(ticket)
    }
}

fn labels(names: &[&str]) -> Vec<String> {
    names.iter().map(|&n| n.to_owned()).collect()
}

// The implementers kelpie can run a work item on: `sonnet-high` and
// `opus-high`, each with an agent file of `role: implementer`.
fn listed() -> Vec<AgentName> {
    ["sonnet-high", "opus-high"]
        .map(|n| AgentName::try_from(n.to_owned()).unwrap())
        .to_vec()
}

// A ticket of a project's own store, which no GitHub issue number names.
fn local(counter: u64) -> TicketId {
    format!("k{counter}").parse().unwrap()
}

#[test]
fn the_oldest_ready_issue_is_picked_whatever_order_they_are_listed_in() {
    let pick = pick(
        &[ready(ticket(16)), ready(ticket(12)), ready(ticket(14))],
        &[],
        &[],
        &[],
        &listed(),
        &[],
    );
    assert_eq!(pick.issue, Some(ticket(12)));
    assert_eq!(pick.skipped, []);
}

fn prioritised(ticket: TicketId, priority: &str) -> ReadyIssue {
    let mut issue = ready(ticket);
    issue.labels.push(priority.into());
    issue
}

#[test]
fn pick_follows_the_order_over_the_priority_labels() {
    let issues = [
        prioritised(ticket(9), "priority: P0"),
        ready(ticket(2)),
        ready(ticket(5)),
    ];
    let pick = pick(
        &issues,
        &[],
        &[],
        &[],
        &listed(),
        &[ticket(5), ticket(9), ticket(2)],
    );
    assert_eq!(pick.issue, Some(ticket(5)));
    assert_eq!(pick.skipped, []);
}

#[test]
fn pick_passes_a_skipped_issue_over_to_the_next_in_the_order() {
    let issues = [
        ready(ticket(3)),
        blocked(ticket(8), &[(ticket(32), true)]),
        prioritised(ticket(9), "priority: P3"),
    ];
    let pick = pick(
        &issues,
        &[],
        &[],
        &[],
        &listed(),
        &[ticket(8), ticket(9), ticket(3)],
    );
    assert_eq!(pick.issue, Some(ticket(9)));
    assert_eq!(pick.skipped.len(), 1);
}

#[test]
fn rule_order_follows_the_order_and_puts_an_issue_it_omits_last_by_the_rule() {
    let mut issues = vec![
        ready(ticket(1)),
        prioritised(ticket(2), "priority: P0"),
        prioritised(ticket(3), "priority: P0"),
        ready(ticket(4)),
    ];
    rule_order(&mut issues, &[ticket(4), ticket(3)]);
    let ids: Vec<TicketId> = issues.iter().map(|i| i.ticket).collect();
    assert_eq!(ids, [ticket(4), ticket(3), ticket(2), ticket(1)]);
}

#[test]
fn with_no_order_kept_the_order_is_the_board_rule() {
    let issues = [
        ready(ticket(1)),
        prioritised(ticket(2), "priority: P3"),
        prioritised(ticket(3), "priority: P1"),
        ready(ticket(4)),
        prioritised(ticket(5), "priority: P0"),
        prioritised(ticket(6), "priority: P2"),
        prioritised(ticket(7), "priority: P1"),
        prioritised(ticket(8), "priority: P0"),
        prioritised(ticket(9), "priority: P4"),
    ];
    assert_eq!(
        kept(&[], &issues),
        [
            ticket(5),
            ticket(8),
            ticket(3),
            ticket(7),
            ticket(6),
            ticket(2),
            ticket(9),
            ticket(1),
            ticket(4)
        ]
    );
}

#[test]
fn a_newly_ready_p0_lands_after_the_last_p0_and_before_the_first_p1() {
    let issues = [
        prioritised(ticket(1), "priority: P0"),
        prioritised(ticket(2), "priority: P0"),
        prioritised(ticket(3), "priority: P1"),
        prioritised(ticket(7), "priority: P0"),
    ];
    assert_eq!(
        kept(&[ticket(1), ticket(2), ticket(3)], &issues),
        [ticket(1), ticket(2), ticket(7), ticket(3)]
    );
}

#[test]
fn a_newly_ready_issue_goes_after_the_last_of_its_priority_wherever_the_order_put_it() {
    let issues = [
        prioritised(ticket(1), "priority: P0"),
        prioritised(ticket(2), "priority: P1"),
        prioritised(ticket(3), "priority: P0"),
        ready(ticket(4)),
        prioritised(ticket(9), "priority: P0"),
    ];
    // #1 sits below #2, so it is the last P0, and #3 and #9 follow it.
    assert_eq!(
        kept(&[ticket(2), ticket(1), ticket(4)], &issues),
        [ticket(2), ticket(1), ticket(3), ticket(9), ticket(4)]
    );
}

#[test]
fn a_closed_or_unlabelled_issue_leaves_the_order() {
    // The forge lists only open issues labelled `ready-for-agent`, so #2
    // is missing from `issues` once it closes or loses the label.
    let issues = [ready(ticket(1)), ready(ticket(3))];
    assert_eq!(
        kept(&[ticket(1), ticket(2), ticket(3)], &issues),
        [ticket(1), ticket(3)]
    );
}

// The order after a read, when no issue has a priority on record yet.
fn kept(order: &[TicketId], issues: &[ReadyIssue]) -> Vec<TicketId> {
    keep_order(order, &Ranked::new(), issues).0
}

// The order and the priorities on record after a first read of `issues`.
fn first_read(issues: &[ReadyIssue]) -> (Vec<TicketId>, Ranked) {
    keep_order(&[], &Ranked::new(), issues)
}

#[test]
fn a_relabelled_issue_moves_to_after_the_last_issue_of_its_new_priority() {
    let mut issues = vec![
        prioritised(ticket(1), "priority: P0"),
        prioritised(ticket(2), "priority: P0"),
        prioritised(ticket(3), "priority: P1"),
        prioritised(ticket(4), "priority: P1"),
    ];
    let (order, ranked) = first_read(&issues);
    assert_eq!(order, [ticket(1), ticket(2), ticket(3), ticket(4)]);

    // #4 is bumped to P0 and #1 dropped to P2.
    issues[3].labels = labels(&[READY, "priority: P0"]);
    issues[0].labels = labels(&[READY, "priority: P2"]);
    let (order, ranked) = keep_order(&order, &ranked, &issues);
    assert_eq!(order, [ticket(2), ticket(4), ticket(3), ticket(1)]);

    // Read again with nothing changed, the order stands.
    assert_eq!(
        keep_order(&order, &ranked, &issues).0,
        [ticket(2), ticket(4), ticket(3), ticket(1)]
    );
}

#[test]
fn an_issue_whose_priority_is_not_on_record_keeps_its_place() {
    let issues = [
        prioritised(ticket(1), "priority: P3"),
        prioritised(ticket(2), "priority: P0"),
    ];
    let (order, ranked) = keep_order(&[ticket(1), ticket(2)], &Ranked::new(), &issues);
    assert_eq!(order, [ticket(1), ticket(2)]);
    assert_eq!(ranked.len(), 2);
}

#[test]
fn a_p0_issue_is_picked_before_an_older_unlabelled_one() {
    let pick = pick(
        &[ready(ticket(3)), prioritised(ticket(9), "priority: P0")],
        &[],
        &[],
        &[],
        &listed(),
        &[],
    );
    assert_eq!(pick.issue, Some(ticket(9)));
    assert_eq!(pick.skipped, []);
}

#[test]
fn a_priority_label_is_the_boards_name_or_the_bare_one_and_nothing_else() {
    for label in ["priority: P2", "P2", "p4", "Priority: P0"] {
        assert!(is_priority_label(label), "{label}");
    }
    for label in ["priority: P5", "P", "priority:P2", "bug", "ready-for-agent"] {
        assert!(!is_priority_label(label), "{label}");
    }
}

#[test]
fn priorities_order_the_board_p0_to_p3_then_unlabelled_oldest_first_within_each() {
    let mut order = Vec::new();
    let mut issues = vec![
        ready(ticket(1)),
        prioritised(ticket(2), "priority: P3"),
        prioritised(ticket(3), "priority: P1"),
        ready(ticket(4)),
        prioritised(ticket(5), "priority: P0"),
        prioritised(ticket(6), "priority: P2"),
        prioritised(ticket(7), "priority: P1"),
        prioritised(ticket(8), "priority: P0"),
    ];
    while let Some(next) = pick(&issues, &[], &[], &[], &listed(), &[]).issue {
        order.push(next);
        issues.retain(|i| i.ticket != next);
    }
    assert_eq!(
        order,
        [
            ticket(5),
            ticket(8),
            ticket(3),
            ticket(7),
            ticket(6),
            ticket(2),
            ticket(1),
            ticket(4)
        ]
    );
}

#[test]
fn an_issue_with_several_priority_labels_ranks_by_the_highest() {
    let mut both = prioritised(ticket(9), "priority: P3");
    both.labels.push("priority: P1".into());
    let pick = pick(
        &[prioritised(ticket(2), "priority: P2"), both],
        &[],
        &[],
        &[],
        &listed(),
        &[],
    );
    assert_eq!(pick.issue, Some(ticket(9)));
}

#[test]
fn a_blocked_p0_issue_waits_and_the_next_priority_is_picked() {
    let mut p0 = blocked(ticket(9), &[(ticket(4), true)]);
    p0.labels.push("priority: P0".into());
    let pick = pick(&[p0, ready(ticket(1))], &[], &[], &[], &listed(), &[]);
    assert_eq!(pick.issue, Some(ticket(1)));
    assert_eq!(pick.skipped.len(), 1);
}

#[test]
fn an_issue_with_sub_issues_is_never_picked_while_its_sub_issues_are() {
    let mut parent = ready(ticket(4));
    parent.sub_issues = SubIssues {
        total: 3,
        closed: 1,
    };
    let mut piece = ready(ticket(5));
    piece.parent = Some(ticket(4));
    let pick = pick(&[parent, piece], &[], &[], &[], &listed(), &[]);
    assert_eq!(pick.issue, Some(ticket(5)));
    assert_eq!(
        pick.skipped,
        [Skip::Split {
            issue: ticket(4),
            open: 2
        }]
    );
}

#[test]
fn nothing_ready_picks_nothing() {
    assert_eq!(
        pick(&[], &[], &[], &[], &listed(), &[]),
        Pick {
            issue: None,
            skipped: vec![]
        }
    );
}

#[test]
fn an_issue_with_an_open_pull_request_or_an_assignee_is_passed_over() {
    let mut taken = ready(ticket(3));
    taken.assigned = true;
    let open = [OpenPullRequest {
        number: 40,
        head: "feat/2".into(),
        closes: vec![2],
        labels: vec![],
        draft: false,
        commit: "c0ffee".into(),
        checks: Checks::None,
        from_fork: false,
        author: "someone".into(),
        author_is_bot: false,
        marks: ReviewMarks::default(),
    }];
    let pick = pick(
        &[ready(ticket(2)), taken, ready(ticket(5))],
        &open,
        &[],
        &[],
        &listed(),
        &[],
    );
    assert_eq!(pick.issue, Some(ticket(5)));
    assert_eq!(
        pick.skipped,
        [
            Skip::PullRequest {
                issue: ticket(2),
                pull_request: 40
            },
            Skip::Assigned { issue: ticket(3) },
        ]
    );
}

// A ticket of a project's own store is a ticket like any other: the board
// picks it, and a pull request that resolves no issue of its own cannot
// pass it over.
#[test]
fn a_ticket_from_another_source_is_picked_and_no_open_pull_request_closes_it() {
    let open = [OpenPullRequest {
        number: 40,
        head: "feat/20".into(),
        closes: vec![20],
        labels: vec![],
        draft: false,
        commit: "c0ffee".into(),
        checks: Checks::None,
        from_fork: false,
        author: "someone".into(),
        author_is_bot: false,
        marks: ReviewMarks::default(),
    }];
    let pick = pick(
        &[ready(ticket(20)), ready(local(12))],
        &open,
        &[],
        &[],
        &listed(),
        &[],
    );
    assert_eq!(pick.issue, Some(local(12)));
    assert_eq!(
        pick.skipped,
        [Skip::PullRequest {
            issue: ticket(20),
            pull_request: 40
        }]
    );
}

#[test]
fn an_issue_an_earlier_worker_labelled_in_progress_is_passed_over_for_the_next_oldest() {
    let mut held = ready(ticket(2));
    held.labels.push(IN_PROGRESS.into());
    let pick = pick(&[held, ready(ticket(5))], &[], &[], &[], &listed(), &[]);
    assert_eq!(pick.issue, Some(ticket(5)));
    assert_eq!(pick.skipped, [Skip::InProgress { issue: ticket(2) }]);
}

#[test]
fn an_issue_kelpie_finished_is_passed_over_while_the_forge_still_lists_it() {
    let pick = pick(
        &[ready(ticket(22)), ready(ticket(23))],
        &[],
        &[ticket(22)],
        &[],
        &listed(),
        &[],
    );
    assert_eq!(pick.issue, Some(ticket(23)));
    assert_eq!(pick.skipped, [Skip::Finished { issue: ticket(22) }]);
}

#[test]
fn an_issue_kelpie_dropped_is_passed_over_as_dropped_and_not_as_finished() {
    let pick = pick(
        &[ready(ticket(22)), ready(ticket(23))],
        &[],
        &[],
        &[ticket(22)],
        &listed(),
        &[],
    );
    assert_eq!(pick.issue, Some(ticket(23)));
    assert_eq!(pick.skipped, [Skip::Dropped { issue: ticket(22) }]);
}

#[test]
fn a_board_where_every_issue_is_taken_picks_nothing_and_says_why() {
    let mut taken = ready(ticket(3));
    taken.assigned = true;
    assert_eq!(
        pick(&[taken], &[], &[], &[], &listed(), &[]),
        Pick {
            issue: None,
            skipped: vec![Skip::Assigned { issue: ticket(3) }]
        }
    );
}

#[test]
fn an_issue_labelled_for_an_agent_kelpie_has_no_file_for_is_passed_over() {
    let mut odd = ready(ticket(1));
    odd.labels.push("agent:haiku-low".into());
    let pick = pick(&[odd, ready(ticket(2))], &[], &[], &[], &listed(), &[]);
    assert_eq!(pick.issue, Some(ticket(2)));
    assert_eq!(
        pick.skipped,
        [Skip::Label {
            issue: ticket(1),
            error: LabelError::NoAgent {
                label: "agent:haiku-low".into(),
                agents: vec!["sonnet-high".into(), "opus-high".into()],
            }
        }]
    );
    let Skip::Label { error, .. } = &pick.skipped[0] else {
        panic!("{:?}", pick.skipped);
    };
    assert_eq!(
        error.to_string(),
        "label `agent:haiku-low` names no implementer: kelpie's agent files with \
         `role: implementer` are sonnet-high, opus-high"
    );
}

#[test]
fn an_old_worker_label_is_no_label_error() {
    let mut old = ready(ticket(1));
    old.labels.push("worker:gpt-high".into());
    assert_eq!(
        pick(&[old, ready(ticket(2))], &[], &[], &[], &listed(), &[]).issue,
        Some(ticket(1))
    );
}

#[test]
fn an_issue_with_an_open_blocker_is_passed_over_for_the_next_oldest() {
    let pick = pick(
        &[blocked(ticket(8), &[(ticket(32), true)]), ready(ticket(9))],
        &[],
        &[],
        &[],
        &listed(),
        &[],
    );
    assert_eq!(pick.issue, Some(ticket(9)));
    assert_eq!(
        pick.skipped,
        [Skip::Blocked {
            issue: ticket(8),
            by: vec![ticket(32)],
            unlisted: 0
        }]
    );
}

#[test]
fn an_issue_whose_blockers_are_all_closed_is_picked() {
    let pick = pick(
        &[blocked(
            ticket(8),
            &[(ticket(32), false), (ticket(33), false)],
        )],
        &[],
        &[],
        &[],
        &listed(),
        &[],
    );
    assert_eq!(pick.issue, Some(ticket(8)));
    assert_eq!(pick.skipped, []);
}

#[test]
fn a_skip_for_blockers_names_only_the_open_ones_lowest_first() {
    let mixed = blocked(
        ticket(27),
        &[
            (ticket(40), true),
            (ticket(24), false),
            (ticket(19), true),
            (ticket(37), true),
        ],
    );
    assert_eq!(
        pick(&[mixed], &[], &[], &[], &listed(), &[]),
        Pick {
            issue: None,
            skipped: vec![Skip::Blocked {
                issue: ticket(27),
                by: vec![ticket(19), ticket(37), ticket(40)],
                unlisted: 0
            }]
        }
    );
}

#[test]
fn blockers_the_forge_did_not_list_count_as_open() {
    let mut long = blocked(ticket(8), &[(ticket(12), false)]);
    long.unlisted_blockers = 3;
    let pick = pick(&[long, ready(ticket(9))], &[], &[], &[], &listed(), &[]);
    assert_eq!(pick.issue, Some(ticket(9)));
    assert_eq!(
        pick.skipped,
        [Skip::Blocked {
            issue: ticket(8),
            by: vec![],
            unlisted: 3
        }]
    );
    let json = serde_json::to_value(&pick.skipped[0]).unwrap();
    assert_eq!(
        json,
        serde_json::json!({ "reason": "blocked", "issue": "8", "by": [], "unlisted": 3 })
    );
}

#[test]
fn an_agent_label_names_an_implementer_kelpie_has_a_file_for() {
    let opus = AgentName::try_from("opus-high".to_owned()).unwrap();
    assert_eq!(
        agent_label(&labels(&[READY, "agent:opus-high"]), &listed()),
        Ok(Some(opus))
    );
    assert_eq!(agent_label(&labels(&[READY, "bug"]), &listed()), Ok(None));
    assert_eq!(
        agent_label(&labels(&["worker:opus-high"]), &listed()),
        Ok(None)
    );
    let pin = labels(&[READY, "Agent:opus-high!"]);
    assert_eq!(
        agent_label(&pin, &listed()).unwrap().unwrap().as_str(),
        "opus-high"
    );
    assert!(pinned(&pin));
    assert!(!pinned(&labels(&[READY, "agent:opus-high", "bug!"])));
    for bad in [
        "agent:",
        "agent:Opus-high",
        "agent:opus-low",
        "agent:!",
        "agent:opus-high!!",
    ] {
        assert!(
            matches!(
                agent_label(&labels(&[bad]), &listed()),
                Err(LabelError::NoAgent { label, .. }) if label == bad
            ),
            "{bad}"
        );
    }
    assert_eq!(
        agent_label(
            &labels(&["agent:opus-high", "agent:sonnet-high"]),
            &listed()
        ),
        Err(LabelError::Several)
    );
}

#[test]
fn an_old_worker_label_is_found_to_be_told_about() {
    let found = labels(&[READY, "worker:opus-medium"]);
    assert_eq!(old_worker_label(&found), Some("worker:opus-medium"));
    assert_eq!(old_worker_label(&labels(&[READY, "agent:x"])), None);
}

#[test]
fn criteria_need_a_heading_then_a_list_item() {
    for body in [
        "## Acceptance criteria\n- [ ] it works",
        "Why.\n\n## Acceptance criteria\n\n* it works",
        "### acceptance criteria\n  - it works",
    ] {
        assert!(has_criteria(body), "{body}");
    }
    for body in [
        "## Acceptance criteria\nIt works.",
        "- it works\n\nAcceptance criteria",
        "- it works",
        "- it works\n\n## Acceptance criteria\nIt works.",
        "",
    ] {
        assert!(!has_criteria(body), "{body}");
    }
}
