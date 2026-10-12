use super::*;
use crate::test::a_work_item;

const DOLLAR: u64 = 1_000_000_000;

fn call(role: Role, cost: u64, unpriced: bool) -> CallRecord {
    CallRecord {
        role,
        at: Timestamp(1),
        session: SessionId("s".into()),
        usage: Usage::default(),
        cost: Cost(cost),
        session_cost: Cost(cost),
        unpriced,
    }
}

#[test]
fn the_workers_and_the_reviewers_priced_calls_add_up() {
    let mut item = a_work_item();
    item.calls = vec![
        call(Role::Worker, 3 * DOLLAR, false),
        call(Role::Reviewer, DOLLAR, false),
        call(Role::Worker, 2 * DOLLAR, false),
    ];
    let priced = item.priced();
    assert_eq!(priced.worker, Cost(5 * DOLLAR));
    assert_eq!(priced.reviewer, Cost(DOLLAR));
    assert_eq!(priced.total(), Cost(6 * DOLLAR));
}

#[test]
fn an_unpriced_call_counts_nothing_toward_the_cap() {
    let mut item = a_work_item();
    item.calls = vec![
        call(Role::Worker, 3 * DOLLAR, false),
        // Its harness reported no dollars, so whatever its record holds is no measure.
        call(Role::Worker, 7 * DOLLAR, true),
        call(Role::Reviewer, 9 * DOLLAR, true),
    ];
    assert_eq!(item.priced().total(), Cost(3 * DOLLAR));
}

#[test]
fn a_call_made_for_a_role_a_work_item_does_not_record_counts_nothing() {
    let mut item = a_work_item();
    item.calls = vec![call(Role::Pm, DOLLAR, false)];
    assert_eq!(item.priced().total(), Cost(0));
}

#[test]
fn an_answer_doubles_the_cap_and_again_until_it_covers_what_was_spent() {
    for (spent, cap) in [(4, 10), (6, 10), (10, 10), (11, 20), (41, 80)] {
        assert_eq!(
            raised_cap(Cost(5 * DOLLAR), Cost(spent * DOLLAR)),
            Cost(cap * DOLLAR)
        );
    }
    let mut item = a_work_item();
    item.calls.push(call(Role::Worker, 11 * DOLLAR, false));
    let five = PerItemUsd::try_from(5.0).unwrap();
    item.raise_budget(five);
    assert_eq!(item.budget_cap(five), Cost(20 * DOLLAR));
    assert_eq!(item.over_budget(five), None);
}
