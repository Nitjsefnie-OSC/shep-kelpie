use crate::test::ticket;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Mutex;

use serde_json::json;

use super::*;
use crate::ports::{Cost, Usage};
use crate::runner::flight::{Pass, advance};
use crate::runner::gate::CHECKS_SETTLE;
use crate::runner::step;
use crate::test::{Rig, Scripted, git};

pub(super) fn finished(report: Option<StepReport>) -> bool {
    matches!(
       report,
       Some(StepReport::Finished {
           issue,
           pull_request: Some(71),
           merged: true,
           ..
       }) if issue == ticket(7)
    )
}

fn marked_ready() -> Option<StepReport> {
    Some(StepReport::MarkedReady {
        issue: ticket(7),
        pull_request: 71,
    })
}

// After a yes: the pass that marks the draft ready, then the pass that
// merges once CI has had time to settle
pub(super) fn ready_then_merge(rig: &Rig, runner: &Mutex<Runner>) -> Option<StepReport> {
    assert_eq!(step(runner).unwrap(), marked_ready());
    rig.clock.advance(CHECKS_SETTLE);
    step(runner).unwrap()
}

#[test]
fn a_yes_merges_the_ruled_head_and_removes_the_branch_worktree_and_build_folder() {
    let (rig, runner, head) = Rig::parked("koji");
    let status = rig.ask(&runner, "rule", Some("71 yes"));
    assert_eq!(status["rulings"], json!([]));
    assert_eq!(
        status["work_item"]["phase"],
        json!({ "state": "merge", "head": head, "readied": null })
    );

    assert!(finished(ready_then_merge(&rig, &runner)));
    assert_eq!(rig.forge.readied(), [71]);
    assert_eq!(rig.forge.merges(), [(71, head)]);
    assert_eq!(rig.forge.head_of("kelpie/7"), None);
    assert_eq!(git(&rig.repo(), &["branch", "--list", "kelpie/7"]), "");
    assert!(!rig.worktree_7().exists());
    assert!(!rig.home.path().join("shep/kelpie/koji/builds/7").exists());
    let status = rig.ask(&runner, "status", None);
    assert_eq!(
        (&status["work_item"], &status["rulings"]),
        (&json!(null), &json!([]))
    );
}

#[test]
fn a_trigger_is_answered_while_a_draft_is_marked_ready() {
    let (rig, runner, _) = Rig::parked("koji");
    rig.ask(&runner, "rule", Some("71 yes"));
    let call = rig.forge.hold_next_mark_ready();

    let (reply, pass) = rig.asked_while_held(&runner, call, "status", None);

    assert!(reply.is_some(), "the trigger waited for the marking");
    assert!(matches!(
        pass,
        Ok(Pass::Report(StepReport::MarkedReady { .. }))
    ));
    assert_eq!(rig.forge.readied(), [71]);
}

#[test]
fn a_trigger_is_answered_while_a_yes_is_merged() {
    let (rig, runner, head) = Rig::parked("koji");
    rig.ask(&runner, "rule", Some("71 yes"));
    assert_eq!(step(&runner).unwrap(), marked_ready());
    rig.clock.advance(CHECKS_SETTLE);
    let call = rig.forge.hold_next_merge();

    let (reply, pass) = rig.asked_while_held(&runner, call, "status", None);

    assert!(reply.is_some(), "the trigger waited for the merge");
    let Ok(Pass::Report(report)) = pass else {
        panic!("the item did not end: {pass:?}");
    };
    assert!(finished(Some(report)));
    assert_eq!(rig.forge.merges(), [(71, head)]);
}

#[test]
fn a_trigger_is_answered_while_a_merged_items_worktree_is_removed() {
    let (rig, runner, _) = Rig::parked("koji");
    rig.ask(&runner, "rule", Some("71 yes"));
    assert_eq!(step(&runner).unwrap(), marked_ready());
    rig.clock.advance(CHECKS_SETTLE);
    let read = rig.hold_next_git_push();

    let (reply, pass) = rig.asked_while_held(&runner, read, "status", None);

    let reply = reply.expect("the trigger waited for the lock the removal held");
    assert_eq!(reply["work_items"].as_array().unwrap().len(), 1);
    let Ok(Pass::Report(report)) = pass else {
        panic!("the item did not end: {pass:?}");
    };
    assert!(finished(Some(report)), "the item ended");
    assert!(!rig.worktree_7().exists());
}

#[cfg(unix)]
#[test]
fn a_build_folder_holding_a_read_only_module_cache_is_removed_with_the_item() {
    use std::os::unix::fs::PermissionsExt;
    let (rig, runner, _) = Rig::parked("koji");
    rig.ask(&runner, "rule", Some("71 yes"));
    let build = rig.home.path().join("shep/kelpie/koji/builds/7");
    let cache = build.join("gomod/cache/download");
    std::fs::create_dir_all(&cache).unwrap();
    std::fs::write(cache.join("list"), "v1.0.0\n").unwrap();
    for dir in [&cache, &build.join("gomod")] {
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o555)).unwrap();
    }

    assert!(finished(ready_then_merge(&rig, &runner)));
    assert!(!build.exists());
    let status = rig.ask(&runner, "status", None);
    assert_eq!(status["work_item"], json!(null));
}

// A hook on the bare origin that refuses the worker branch's delete,
// after the forge's own delete has taken the branch first when `forge_won`
fn refuse_delete(rig: &Rig, forge_won: bool) {
    let hook = rig.home.path().join("origin.git/hooks/pre-receive");
    let script = if forge_won {
        "#!/bin/sh\ngit update-ref -d refs/heads/kelpie/7\nexit 1\n"
    } else {
        "#!/bin/sh\nexit 1\n"
    };
    crate::test::write_script(&hook, script);
}

#[test]
fn a_branch_the_forge_deleted_first_still_counts_as_cleaned_up() {
    let (rig, runner, head) = Rig::parked("koji");
    rig.ask(&runner, "rule", Some("71 yes"));
    refuse_delete(&rig, true);

    assert!(finished(ready_then_merge(&rig, &runner)));
    assert_eq!(rig.forge.merges(), [(71, head)]);
    assert_eq!(rig.forge.head_of("kelpie/7"), None);
    assert!(!rig.worktree_7().exists());
    let status = rig.ask(&runner, "status", None);
    assert_eq!(status["work_item"], json!(null));
}

#[test]
fn a_branch_that_cannot_be_deleted_for_another_reason_fails_the_cleanup() {
    let (rig, runner, _) = Rig::parked("koji");
    rig.ask(&runner, "rule", Some("71 yes"));
    refuse_delete(&rig, false);

    let Some(StepReport::GateFailed { reason, .. }) = ready_then_merge(&rig, &runner) else {
        panic!("the cleanup did not fail");
    };
    assert!(reason.starts_with("cannot clean up"), "{reason}");
    assert!(rig.forge.head_of("kelpie/7").is_some());
    let status = rig.ask(&runner, "status", None);
    assert_eq!(status["work_item"]["issue"], json!("7"));
}

#[test]
fn a_yes_on_a_head_that_moved_since_the_question_is_withdrawn() {
    let (rig, runner, _) = Rig::parked("golbat");
    let worktree = rig.worktree_7();
    std::fs::write(worktree.join("late.txt"), "late\n").unwrap();
    git(&worktree, &["add", "late.txt"]);
    git(&worktree, &["commit", "--quiet", "-m", "late"]);
    git(&worktree, &["push", "--quiet", "origin", "HEAD"]);
    let moved = rig.forge.head_of("kelpie/7").unwrap();

    rig.ask(&runner, "rule", Some("71 yes"));
    assert_eq!(
        step(&runner).unwrap(),
        Some(StepReport::YesWithdrawn {
            issue: ticket(7),
            pull_request: 71,
            reason: format!("#71 moved to {}", &moved[..7]),
        })
    );
    assert_eq!(rig.forge.merges(), []);
    rig.forge.set_checks(&moved, Checks::Passed);
    assert!(matches!(
        rig.verdict(&runner),
        Some(StepReport::Ruling { id, .. })
     if id == ticket(71)));
}

#[test]
fn a_yes_after_ci_stopped_being_green_is_withdrawn() {
    let (rig, runner, head) = Rig::parked("chelone");
    rig.forge.set_checks(&head, Checks::Pending(Vec::new()));
    rig.ask(&runner, "rule", Some("71 yes"));
    assert!(matches!(
        step(&runner).unwrap(),
        Some(StepReport::YesWithdrawn { reason, .. }) if reason == "CI on #71 is no longer green"
    ));
    assert_eq!(rig.forge.merges(), []);
}

#[test]
fn a_yes_after_main_moved_is_withdrawn_and_the_branch_rebased() {
    let (rig, runner, _) = Rig::parked("rotom");
    rig.land_on_origin("landed.txt");
    rig.ask(&runner, "rule", Some("71 yes"));
    assert!(matches!(
        step(&runner).unwrap(),
        Some(StepReport::YesWithdrawn { reason, .. }) if reason == "main moved since the question"
    ));
    assert!(matches!(
        step(&runner).unwrap(),
        Some(StepReport::Rebased { .. })
    ));
    assert_eq!(rig.forge.merges(), []);
}

#[test]
fn a_pull_request_closed_after_the_yes_is_not_merged_and_parks_the_worker() {
    let (rig, runner, _) = Rig::parked("koji");
    rig.ask(&runner, "rule", Some("71 yes"));
    rig.forge.set_state(71, PullRequestState::Closed);
    let Some(StepReport::Ruling { id, question, .. }) = step(&runner).unwrap() else {
        panic!("no ruling was raised");
    };
    assert_eq!(id, ticket(71));
    assert!(
        question.starts_with("Pull request #71 was closed"),
        "{question}"
    );
    assert_eq!(rig.forge.merges(), []);
}

#[test]
fn a_draft_marked_ready_merges_only_on_a_later_pass_once_ci_settles() {
    let (rig, runner, head) = Rig::parked("golbat");
    rig.ask(&runner, "rule", Some("71 yes"));
    assert_eq!(step(&runner).unwrap(), marked_ready());
    assert_eq!(rig.forge.merges(), []);
    assert_eq!(step(&runner).unwrap(), None);
    rig.clock.advance(CHECKS_SETTLE - 1);
    assert_eq!(step(&runner).unwrap(), None);
    assert_eq!(rig.forge.merges(), []);
    rig.clock.advance(1);
    assert!(finished(step(&runner).unwrap()));
    assert_eq!(rig.forge.merges(), [(71, head)]);
}

#[test]
fn a_run_that_marking_ready_starts_is_waited_for() {
    let (rig, runner, head) = Rig::parked("xilriws");
    rig.ask(&runner, "rule", Some("71 yes"));
    assert_eq!(step(&runner).unwrap(), marked_ready());
    rig.forge.set_checks(&head, Checks::Pending(Vec::new()));
    rig.clock.advance(3 * CHECKS_SETTLE);
    assert_eq!(step(&runner).unwrap(), None);
    assert_eq!(rig.forge.merges(), []);
    rig.forge.set_checks(&head, Checks::Passed);
    assert!(finished(step(&runner).unwrap()));
}

#[test]
fn a_red_run_after_marking_ready_withdraws_the_yes() {
    let (rig, runner, head) = Rig::parked("chelone");
    rig.ask(&runner, "rule", Some("71 yes"));
    assert_eq!(step(&runner).unwrap(), marked_ready());
    rig.forge
        .set_checks(&head, Checks::Failed(vec!["test".into()]));
    rig.clock.advance(CHECKS_SETTLE);
    assert!(matches!(
        step(&runner).unwrap(),
        Some(StepReport::YesWithdrawn { reason, .. }) if reason == "CI on #71 is no longer green"
    ));
    assert_eq!(rig.forge.merges(), []);
}

#[test]
fn marking_ready_is_kelpies_own_change_so_a_withdrawn_yes_is_not_parked_on_it() {
    let (rig, runner, head) = Rig::parked("koji");
    rig.ask(&runner, "rule", Some("71 yes"));
    assert_eq!(step(&runner).unwrap(), marked_ready());
    rig.forge
        .set_checks(&head, Checks::Failed(vec!["test".into()]));
    rig.clock.advance(CHECKS_SETTLE);
    assert!(matches!(
        step(&runner).unwrap(),
        Some(StepReport::YesWithdrawn { .. })
    ));
    // The gate's next look must treat kelpie's own mark-ready as known,
    // not raise a foreign-change ruling that recurs forever.
    assert_eq!(
        rig.verdict(&runner),
        Some(StepReport::CiFailed {
            issue: ticket(7),
            pull_request: 71,
            head: head.clone(),
            checks: vec!["test".into()],
        })
    );
}

#[test]
fn a_runner_restarted_between_ready_and_merge_still_waits_then_merges() {
    let (rig, runner, head) = Rig::parked("acme");
    rig.ask(&runner, "rule", Some("71 yes"));
    assert_eq!(step(&runner).unwrap(), marked_ready());
    drop(runner);
    let runner = rig.open().unwrap();
    assert_eq!(
        step(&runner).unwrap(),
        None,
        "the settling survived the restart"
    );
    rig.clock.advance(CHECKS_SETTLE);
    assert!(finished(step(&runner).unwrap()));
    assert_eq!(rig.forge.merges(), [(71, head)]);
    assert_eq!(rig.forge.readied(), [71]);
}

#[test]
fn a_merge_the_forge_refuses_is_tried_again_later() {
    let (rig, runner, head) = Rig::parked("reactmap");
    rig.ask(&runner, "rule", Some("71 yes"));
    assert_eq!(step(&runner).unwrap(), marked_ready());
    rig.clock.advance(CHECKS_SETTLE);
    rig.forge.set_merges_down(true);
    let report = step(&runner).unwrap().expect("a report");
    assert_eq!(
        report,
        StepReport::GateFailed {
            issue: ticket(7),
            reason: "cannot merge #71: gh failed: merges are down".into()
        }
    );
    assert!(report.waits());
    assert!(rig.worktree_7().exists());

    rig.forge.set_merges_down(false);
    rig.next_look();
    assert!(finished(step(&runner).unwrap()));
    assert_eq!(rig.forge.merges(), [(71, head)]);
}

// The work item the playground's board wrongly took: a turn that ended
// with no pull request, in a runner held from starting another.
#[test]
fn drop_clears_a_work_item_in_a_draining_runner_and_the_board_never_retakes_it() {
    let rig = Rig::new("webapp");
    let runner = rig.open().unwrap();
    rig.ask(&runner, "add", Some("7"));
    rig.claude
        .script([Scripted::Reply(Usage::default(), Cost(1))]);
    step(&runner).unwrap();
    rig.ask(&runner, "drain", None);
    let build = rig.home.path().join("shep/kelpie/webapp/builds/7");
    assert!(rig.worktree_7().exists() && build.exists());

    let status = rig.ask(&runner, "drop", None);
    assert_eq!(
        (&status["work_item"], &status["draining"]["calls"]),
        (&json!(null), &json!([]))
    );
    assert!(!rig.worktree_7().exists());
    assert!(!build.exists());
    assert_eq!(git(&rig.repo(), &["branch", "--list", "kelpie/7"]), "");

    rig.forge.list_ready(7, false);
    assert_eq!(step(&runner).unwrap(), None);
    assert_eq!(rig.claude.calls().len(), 1);
}

#[test]
fn drop_refuses_with_nothing_in_flight_a_running_turn_or_a_merge() {
    let rig = Rig::new("koji");
    let runner = rig.open().unwrap();
    assert_eq!(
        rig.ask(&runner, "drop", None),
        json!({ "error": "no work item is in flight" })
    );
    rig.ask(&runner, "add", Some("7"));
    rig.claude.script([Scripted::Kill]);
    let _ = catch_unwind(AssertUnwindSafe(|| step(&runner)));
    drop(runner);
    let runner = rig.open().unwrap();
    assert_eq!(
        rig.ask(&runner, "drop", None),
        json!({ "error": "the worker's turn on #7 is running" })
    );

    let (rig, runner, _) = Rig::parked("rotom");
    rig.ask(&runner, "rule", Some("71 yes"));
    assert_eq!(
        rig.ask(&runner, "drop", None),
        json!({ "error": "the work item for #7 is merging" })
    );
    assert!(rig.worktree_7().exists());
}

// A review call runs outside the runner's lock, the
// same as a worker's turn. Dropping the work item out from under one
// used to clear it while the call was still in flight, so the result
// panicked the runner (`end_review`'s `.expect` on a work item that
// was no longer there). Recording it in state the way a running turn
// is, and refusing `drop` while it is set, keeps the call from ever
// outliving its own work item. Unlike a turn, nothing ever reruns a
// review call on restart, so the marker cannot be left running: `open`
// clears it, rather than resuming it, before this drop can even ask.
#[test]
fn a_restart_clears_a_stale_review_call_so_drop_no_longer_refuses() {
    let (rig, runner, _) = Rig::with_pull_request("shep");
    drop(runner);
    let state = rig.paths().state;
    let text = std::fs::read_to_string(&state).unwrap();
    let mut saved: serde_json::Value = serde_json::from_str(&text).unwrap();
    saved["work_items"][0]["review_call"] = json!({ "state": "running", "since": Rig::EPOCH });
    std::fs::write(&state, saved.to_string()).unwrap();

    let runner = rig.open().unwrap();
    assert_eq!(
        rig.ask(&runner, "drop", None)["work_item"],
        json!(null),
        "the restart cleared the stale marker, so the drop goes through"
    );
    assert!(!rig.worktree_7().exists());
}

// Seen live on the playground: the board polled a second after the
// merge, before GitHub closed the issue, and dispatched it again.
#[test]
fn an_issue_whose_work_item_finished_is_never_dispatched_again() {
    let (rig, runner, _) = Rig::parked("webapp");
    rig.forge.list_ready(7, false);
    rig.forge.list_ready(8, true);
    rig.ask(&runner, "rule", Some("71 yes"));
    assert!(finished(ready_then_merge(&rig, &runner)));

    let calls = rig.claude.calls().len();
    assert_eq!(step(&runner).unwrap(), None);
    drop(runner);
    let runner = rig.open().unwrap();
    assert_eq!(step(&runner).unwrap(), None, "and not after a restart");
    let status = rig.ask(&runner, "status", None);
    assert_eq!(status["work_item"], json!(null));
    assert_eq!(rig.claude.calls().len(), calls);
}

fn finished_on_disk(rig: &Rig) -> serde_json::Value {
    let text = std::fs::read_to_string(rig.paths().state).unwrap();
    serde_json::from_str::<serde_json::Value>(&text).unwrap()["finished"].clone()
}

#[test]
fn a_finished_issue_leaves_the_state_file_once_its_issue_is_closed() {
    let (rig, runner, _) = Rig::parked("webapp");
    // A close the forge refuses leaves the issue open.
    rig.forge.set_closes(71, &[]);
    rig.forge.set_closes_down(true);
    rig.forge.list_ready(7, false);
    rig.ask(&runner, "rule", Some("71 yes"));
    // The close is refused in each of three steps before the item finishes.
    assert!(matches!(
       ready_then_merge(&rig, &runner),
       Some(StepReport::GateFailed { issue, .. }) if issue == ticket(7)
    ));
    for wait in [15, 30] {
        rig.clock.advance(wait);
        let report = step(&runner).unwrap();
        assert_eq!(finished(report.clone()), wait == 30, "{report:?}");
    }
    assert_eq!(finished_on_disk(&rig), json!(["7"]));

    assert_eq!(step(&runner).unwrap(), None);
    assert_eq!(finished_on_disk(&rig), json!(["7"]), "the issue is open");

    rig.forge.close_issue(7);
    assert_eq!(step(&runner).unwrap(), None);
    assert_eq!(finished_on_disk(&rig), json!([]));
}

#[test]
fn a_finished_issue_the_forge_cannot_answer_for_stays() {
    let (rig, runner, _) = Rig::parked("webapp");
    rig.forge.list_ready(7, false);
    rig.ask(&runner, "rule", Some("71 yes"));
    assert!(finished(ready_then_merge(&rig, &runner)));

    rig.forge.remove_issue(7);
    assert_eq!(step(&runner).unwrap(), None);
    assert_eq!(finished_on_disk(&rig), json!(["7"]));
}

// The merge's reads are made with the runner's lock let go: a trigger is answered while each is
// held, and the write that follows is still the forge's to refuse.
// After a yes and the pass that marks the draft ready, with CI settled: the next pass reads
// before it merges
fn about_to_merge(project: &str) -> (Rig, Mutex<Runner>) {
    let (rig, runner, _) = Rig::parked(project);
    rig.ask(&runner, "rule", Some("71 yes"));
    assert_eq!(step(&runner).unwrap(), marked_ready());
    rig.clock.advance(CHECKS_SETTLE);
    (rig, runner)
}

#[test]
fn a_trigger_is_answered_while_the_pull_request_is_read_for_the_merge() {
    let (rig, runner) = about_to_merge("koji");
    // The board's read would take the listing the pass makes first, and a finishing runner picks
    // nothing.
    rig.ask(&runner, "finish", None);
    let read = rig.forge.hold_next_open_pull_requests_read();

    let (reply, pass) = rig.asked_while_held(&runner, read, "status", None);

    let reply = reply.expect("the trigger waited for the lock the read held");
    assert_eq!(reply["work_items"].as_array().unwrap().len(), 1);
    pass.unwrap();
    assert_eq!(rig.forge.merges().len(), 1, "the merge still went out");
}

#[test]
fn a_trigger_is_answered_while_the_merge_queue_is_read() {
    let (rig, runner) = about_to_merge("koji");
    let read = rig.forge.hold_next_merge_queue_read();

    let (reply, pass) = rig.asked_while_held(&runner, read, "status", None);

    let reply = reply.expect("the trigger waited for the lock the read held");
    assert_eq!(reply["work_items"].as_array().unwrap().len(), 1);
    pass.unwrap();
    assert_eq!(rig.forge.merges().len(), 1, "the merge still went out");
}

#[test]
fn a_head_that_moved_during_the_read_is_still_refused_by_the_forge() {
    let (rig, runner) = about_to_merge("koji");
    let read = rig.forge.hold_next_merge_queue_read();

    let (_, pass) = rig.during_read(
        read,
        || advance(&runner),
        // Git still shows the head the step read, so only the forge's own check catches it.
        || rig.forge.set_lagging(71, Some("0123456789abcdef")),
    );

    let Ok(Pass::Report(StepReport::GateFailed { reason, .. })) = pass else {
        panic!("the merge of a moved head was not refused: {pass:?}");
    };
    assert!(reason.starts_with("cannot merge #71:"), "{reason}");
    assert_eq!(rig.forge.merges(), [], "the forge took the old head");
}

#[test]
fn a_pull_request_that_closed_during_the_queue_read_is_not_merged() {
    let (rig, runner) = about_to_merge("koji");
    let read = rig.forge.hold_next_merge_queue_read();

    let (_, pass) = rig.during_read(
        read,
        || advance(&runner),
        || rig.forge.set_state(71, PullRequestState::Closed),
    );

    assert!(matches!(pass, Ok(Pass::Report(_))), "{pass:?}");
    assert_eq!(rig.forge.merges(), []);
}
