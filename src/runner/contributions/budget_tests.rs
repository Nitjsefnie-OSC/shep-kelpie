//! A contribution's calls and the slots and budget they share, through the runner's stand-ins

use std::path::Path;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use super::security_tests::{commit_on, contribute, contribution, reviews};
use super::tests::{picking, poll, posts};
use crate::board::READY;
use crate::ports::{CLEAN, Cost, MaintainerReview};
use crate::runner::rework::HUMAN;
use crate::runner::{Pass, Runner, StepReport, advance, step};
use crate::test::ticket;
use crate::test::{CALL_PATIENCE, Hold, Rig, Scripted};
use crate::ticket::TicketId;

fn write(folder: &Path, file: &str, text: &str) {
    std::fs::write(folder.join(file), text).unwrap();
}

// What `usd` dollars cost, the way a call is priced
fn dollars(usd: u64) -> Cost {
    Cost(usd * 100 * Cost::PER_CENT)
}

// A runner that follows every pull request of another person, whose
// contributions may spend 1 dollar before their calls stop
fn capped() -> (Rig, Mutex<Runner>) {
    let rig = Rig::new("shep");
    rig.with_app();
    rig.maintainer("Boss");
    rig.edit_settings(|s| {
        let s = s.replace("per_item_usd = 10", "per_item_usd = 1");
        format!("{s}\n[app.dogs.kelpie.contributions]\npick = \"all\"\n")
    });
    let runner = rig.open().unwrap();
    (rig, runner)
}

fn rulings(rig: &Rig, runner: &Mutex<Runner>) -> Vec<Value> {
    let status = rig.ask(runner, "status", None);
    status["rulings"].as_array().cloned().unwrap_or_default()
}

// Steps until no security review is in flight
pub(in crate::runner) fn settle(runner: &Mutex<Runner>) {
    let by = Instant::now() + CALL_PATIENCE;
    while runner.lock().unwrap().flights.securing() {
        assert!(Instant::now() < by, "the call never ended");
        std::thread::sleep(Duration::from_millis(10));
        step(runner).unwrap();
    }
}

// #30, whose first head cost a nanodollar more than its cap, and a new head
// of it that no review has read, with the ruling for its cap alerted
fn over_its_cap() -> (Rig, Mutex<Runner>) {
    let (rig, runner) = capped();
    rig.claude
        .script([Scripted::Billed(CLEAN, Cost(dollars(1).0 + 1))]);
    contribute(&rig, 30, |h| write(h, "one.txt", "one\n"));
    assert_eq!(step(&runner).unwrap(), None);
    commit_on(&rig, 30, "topic/30", |h| write(h, "two.txt", "two\n"));
    poll(&rig, &runner);
    assert_eq!(
        step(&runner).unwrap(),
        Some(StepReport::Alerted { id: ticket(30) })
    );
    (rig, runner)
}

#[test]
fn a_contribution_holds_no_slot_while_it_waits_on_the_contributor() {
    let (rig, runner) = picking("all");
    let hold = Hold::default();
    rig.claude
        .script([Scripted::HoldThenSay(hold.clone(), CLEAN)]);
    contribute(&rig, 30, |h| write(h, "one.txt", "one\n"));

    // A model call on it takes the one slot while it runs.
    assert!(matches!(advance(&runner).unwrap(), Pass::Started));
    assert!(hold.entered(CALL_PATIENCE), "the review never began");
    assert_eq!(runner.lock().unwrap().slot_issues(), [ticket(30)]);
    assert!(!runner.lock().unwrap().slot_free());
    assert!(!runner.lock().unwrap().board_open());

    // It gives the slot back after.
    hold.release();
    assert!(hold.answered(CALL_PATIENCE), "the review never answered");
    settle(&runner);
    assert_eq!(runner.lock().unwrap().slot_issues(), Vec::<TicketId>::new());
    assert!(runner.lock().unwrap().slot_free());
    assert!(runner.lock().unwrap().board_open());

    // Approved and green, its head is read by the reviewers, whose call takes the slot.
    let kept = contribution(&rig, &runner);
    assert_eq!(kept["passed"], kept["head"]);
    poll(&rig, &runner);
    settle(&runner);
    assert_eq!(contribution(&rig, &runner)["pass"]["answered"], "qwen");

    // Waiting on its contributor, it holds none, and a work item opens.
    assert!(runner.lock().unwrap().slot_free());
    rig.forge.list_ready(8, false);
    rig.next_look();
    assert!(matches!(
       step(&runner).unwrap(),
       Some(StepReport::Dispatched { issue, .. }) if issue == ticket(8)
    ));
}

#[test]
fn a_due_contribution_call_starts_before_a_new_issue() {
    let (rig, runner) = picking("all");
    rig.claude.script([Scripted::Text(CLEAN)]);
    contribute(&rig, 30, |h| write(h, "one.txt", "one\n"));
    rig.forge.list_ready(8, false);

    assert_eq!(step(&runner).unwrap(), None, "the review took the slot");
    assert_eq!(reviews(&rig).len(), 1);
    assert_eq!(rig.ask(&runner, "status", None)["work_items"], json!([]));

    assert!(matches!(
       step(&runner).unwrap(),
       Some(StepReport::Dispatched { issue, .. }) if issue == ticket(8)
    ));
}

#[test]
fn an_adopted_pull_request_and_a_rework_still_go_before_a_contribution() {
    // An adopted pull request goes first.
    let (rig, runner) = picking("all");
    rig.push_by_hand("fix/timeline", "work.txt");
    rig.forge.open_pull_request(80, "fix/timeline", &[5]);
    rig.forge.label_pull_request(80, READY);
    contribute(&rig, 30, |h| write(h, "one.txt", "one\n"));
    assert!(matches!(
        step(&runner).unwrap(),
        Some(StepReport::Adopted {
            pull_request: 80,
            ..
        })
    ));
    assert!(reviews(&rig).is_empty(), "the contribution waits");

    // So does a pull request that asks for a rework.
    let (rig, runner) = picking("all");
    rig.push_by_hand("kelpie/7", "work.txt");
    rig.forge.open_pull_request(71, "kelpie/7", &[7]);
    rig.forge.ready_pull_request(71);
    rig.forge.label_pull_request(71, READY);
    rig.forge.label_pull_request(71, HUMAN);
    rig.forge.review(
        71,
        MaintainerReview {
            id: "PRR_71".into(),
            changes_requested: false,
            body: "Keep the dates, lose the cards.".into(),
            comments: vec![],
        },
    );
    contribute(&rig, 30, |h| write(h, "one.txt", "one\n"));
    assert!(matches!(
        step(&runner).unwrap(),
        Some(StepReport::Reworked {
            pull_request: 71,
            ..
        })
    ));
    assert!(reviews(&rig).is_empty(), "the contribution waits");
}

#[test]
fn a_calls_cost_is_added_to_the_contributions_spend() {
    let (rig, runner) = picking("all");
    rig.claude.script([
        Scripted::Billed(CLEAN, Cost(3 * Cost::PER_CENT)),
        Scripted::Billed(CLEAN, Cost(4 * Cost::PER_CENT)),
    ]);
    contribute(&rig, 30, |h| write(h, "one.txt", "one\n"));
    assert_eq!(step(&runner).unwrap(), None);
    assert_eq!(contribution(&rig, &runner)["spend"], 3 * Cost::PER_CENT);

    // The spend is the contribution's, over every head it has.
    commit_on(&rig, 30, "topic/30", |h| write(h, "two.txt", "two\n"));
    poll(&rig, &runner);
    assert_eq!(reviews(&rig).len(), 2);
    assert_eq!(contribution(&rig, &runner)["spend"], 7 * Cost::PER_CENT);
}

#[test]
fn no_call_starts_on_a_contribution_over_per_item_usd_and_status_says_so() {
    let (rig, runner) = over_its_cap();

    assert_eq!(reviews(&rig).len(), 1, "the new head is not read");
    let [ruling] = rulings(&rig, &runner).try_into().unwrap();
    assert_eq!(ruling["id"], "30");
    assert_eq!(ruling["issue"], Value::Null);
    assert_eq!(ruling["kind"]["kind"], "contribution");
    assert_eq!(ruling["kind"]["reason"], "over-budget");
    assert_eq!(ruling["kind"]["cap"], dollars(1).0);
    assert_eq!(contribution(&rig, &runner)["spend"], dollars(1).0 + 1);

    // Later polls raise no second ruling and start no call.
    poll(&rig, &runner);
    poll(&rig, &runner);
    assert_eq!(reviews(&rig).len(), 1);
    assert_eq!(rulings(&rig, &runner).len(), 1);
    assert_eq!(posts(&rig), Vec::<String>::new());
}

#[test]
fn a_yes_or_a_no_with_a_note_doubles_the_cap() {
    let (rig, runner) = over_its_cap();
    rig.claude.script([
        Scripted::Billed(CLEAN, dollars(1)),
        Scripted::Billed(CLEAN, dollars(1)),
    ]);

    // A yes lifts the cap to 2 dollars, and the new head is read.
    rig.ask(&runner, "rule", Some("30 yes"));
    assert_eq!(rulings(&rig, &runner).len(), 0);
    poll(&rig, &runner);
    assert_eq!(reviews(&rig).len(), 2);
    assert_eq!(contribution(&rig, &runner)["budget_raises"], 1);

    // It has spent over 2 dollars now, so another head is over the cap. A no
    // with a note lifts the cap to 4, and the head is read.
    commit_on(&rig, 30, "topic/30", |h| write(h, "three.txt", "three\n"));
    poll(&rig, &runner);
    assert_eq!(reviews(&rig).len(), 2, "over the doubled cap");
    let [ruling] = rulings(&rig, &runner).try_into().unwrap();
    assert_eq!(ruling["kind"]["cap"], dollars(2).0);
    assert_eq!(
        step(&runner).unwrap(),
        Some(StepReport::Alerted { id: ticket(30) })
    );
    rig.ask(&runner, "rule", Some("30 no it is worth the cost"));
    poll(&rig, &runner);
    assert_eq!(reviews(&rig).len(), 3);
    assert_eq!(contribution(&rig, &runner)["budget_raises"], 2);
    assert_eq!(rulings(&rig, &runner).len(), 0);
}

#[test]
fn a_no_without_a_note_does_not_raise_the_cap() {
    let (rig, runner) = over_its_cap();

    let refused = rig.ask(&runner, "rule", Some("30 no"));

    assert!(refused.get("error").is_some(), "{refused}");
    assert_eq!(rulings(&rig, &runner).len(), 1, "the ruling still waits");
    assert_eq!(contribution(&rig, &runner).get("budget_raises"), None);
    poll(&rig, &runner);
    assert_eq!(reviews(&rig).len(), 1);
}

#[test]
fn a_contribution_taken_off_the_board_and_back_keeps_its_spend_and_its_cap() {
    let (rig, runner) = over_its_cap();

    // A draft is not a contribution, and the pull request is dropped.
    rig.forge.set_draft(30, true);
    poll(&rig, &runner);
    let status = rig.ask(&runner, "status", None);
    assert_eq!(status["contributions"], Value::Null);
    assert_eq!(rulings(&rig, &runner).len(), 0);

    // Marked ready again, it is over its cap still, and asks again.
    rig.forge.set_draft(30, false);
    poll(&rig, &runner);
    assert_eq!(contribution(&rig, &runner)["spend"], dollars(1).0 + 1);
    assert_eq!(reviews(&rig).len(), 1, "the new head is not read");
    assert_eq!(
        step(&runner).unwrap(),
        Some(StepReport::Alerted { id: ticket(30) })
    );
    let [ruling] = rulings(&rig, &runner).try_into().unwrap();
    assert_eq!(ruling["kind"]["reason"], "over-budget");
    assert_eq!(ruling["kind"]["cap"], dollars(1).0);

    // An answer's doubling of the cap is kept the same way.
    rig.claude.script([Scripted::Billed(CLEAN, dollars(0))]);
    rig.ask(&runner, "rule", Some("30 yes"));
    rig.forge.set_draft(30, true);
    poll(&rig, &runner);
    rig.forge.set_draft(30, false);
    poll(&rig, &runner);
    assert_eq!(contribution(&rig, &runner)["budget_raises"], 1);
    assert_eq!(reviews(&rig).len(), 2, "read under the doubled cap");
}

#[test]
fn an_over_budget_ruling_holds_the_calls_until_it_is_answered() {
    let (rig, runner) = over_its_cap();

    // A larger cap alone does not lift it.
    rig.edit_settings(|s| s.replace("per_item_usd = 1", "per_item_usd = 5"));
    runner
        .lock()
        .unwrap()
        .reread(rig.settings(), rig.kelpie_settings())
        .unwrap();
    poll(&rig, &runner);
    assert_eq!(reviews(&rig).len(), 1, "the ruling still holds the review");
    assert_eq!(rulings(&rig, &runner).len(), 1);

    // The answer lifts it, doubling the cap for the head that waits.
    rig.claude.script([Scripted::Billed(CLEAN, dollars(1))]);
    rig.ask(&runner, "rule", Some("30 yes"));
    poll(&rig, &runner);
    assert_eq!(reviews(&rig).len(), 2);
}
