use crate::test::ticket;
use crate::ticket::TicketId;
use serde_json::json;

use super::*;
use crate::ports::{CLEAN, Cost, Session, Usage};
use crate::runner::step;
use crate::test::{Rig, Scripted, git};

pub(super) fn ruling_report(report: Option<StepReport>) -> (TicketId, String) {
    match report {
        Some(StepReport::Ruling { id, question, .. }) => (id, question),
        other => panic!("no ruling was raised: {other:?}"),
    }
}

#[test]
fn a_pending_run_waits_and_a_green_one_raises_the_merge_ruling() {
    let (rig, runner, head) = Rig::with_pull_request("shep");
    assert_eq!(step(&runner).unwrap(), None);
    assert_eq!(rig.ask(&runner, "status", None)["rulings"], json!([]));

    rig.forge.set_checks(&head, Checks::Passed);
    let (id, question) = ruling_report(rig.verdict(&runner));
    assert_eq!(id, ticket(71));
    assert_eq!(
        question,
        format!(
            "Merge pull request #71 at {} into main? `shep kelpie rule 71 yes` \
             merges it, `shep kelpie rule 71 no <note>` sends the worker your note for \
             a fix that goes to CI and back to you, and `shep kelpie rule 71 rework \
             <note>` sends it your note for a change the whole review reads again.",
            &head[..7]
        )
    );
    assert_eq!(rig.forge.comments(), [], "a merge ruling is not posted");
    let status = rig.ask(&runner, "status", None);
    assert_eq!(
        status["rulings"],
        json!([{
            "id": "71",
            "issue": "7",
            "question": question,
            "pull_request": 71,
            "kind": { "kind": "merge", "head": head },
            "alerted": false,
            "raised": Rig::EPOCH + 120,
            "effects": [
                "Merges it into main and closes #7: Title of #7. CI passed. \
                 2 review rounds read it.",
                "Sends the worker back for a fix that goes to CI and back to you, \
                 with no new review round.",
                "Sends the worker back for a change the whole review reads again.",
            ],
        }])
    );
    assert_eq!(
        status["work_item"]["phase"],
        json!({ "state": "ruling", "id": "71" })
    );
    assert_eq!(
        step(&runner).unwrap(),
        Some(StepReport::Alerted { id: ticket(71) })
    );
    assert_eq!(step(&runner).unwrap(), None, "a parked worker waits");
}

// The worker pushed a head past the red one, and green CI on it raises
// the first merge ruling, which names it.
fn assert_fix_landed(rig: &Rig, runner: &std::sync::Mutex<Runner>, red: &str) {
    let fixed = rig.forge.head_of("kelpie/7").unwrap();
    assert_ne!(fixed, red);
    rig.forge.set_checks(&fixed, Checks::Passed);
    let (id, question) = ruling_report(rig.verdict(runner));
    assert_eq!(id, ticket(71));
    assert!(question.contains(&fixed[..7]), "{question}");
}

#[test]
fn a_red_run_is_the_workers_next_turn_naming_the_failed_checks() {
    let (rig, runner, head) = Rig::with_pull_request("koji");
    let failed = vec!["lint".to_owned(), "test".to_owned()];
    rig.forge.set_checks(&head, Checks::Failed(failed.clone()));
    assert_eq!(
        rig.verdict(&runner),
        Some(StepReport::CiFailed {
            issue: ticket(7),
            pull_request: 71,
            head: head.clone(),
            checks: failed,
        })
    );

    rig.claude.script([Scripted::Push("fix.txt", "fixed\n")]);
    step(&runner).unwrap();
    let [first, fix] = rig.claude.calls().try_into().unwrap();
    assert_eq!(fix.session, Session::Resume(first.session.id().clone()));
    let named = format!(
        "\nCI failed on your pull request #71 at {}: lint, test. ",
        &head[..7]
    );
    assert!(fix.prompt.starts_with("/mattpocock:diagnosing-bugs "));
    assert!(fix.prompt.contains(&named), "{}", fix.prompt);

    assert_fix_landed(&rig, &runner, &head);
}

#[test]
fn a_ci_fix_prompt_asks_for_the_body_to_be_kept_true() {
    let (rig, runner, head) = Rig::with_pull_request("koji");
    rig.forge
        .set_checks(&head, Checks::Failed(vec!["test".into()]));
    rig.verdict(&runner);
    rig.claude.script([Scripted::Push("fix.txt", "fixed\n")]);
    step(&runner).unwrap();
    let fix = rig.claude.calls().pop().unwrap();
    assert!(
        fix.prompt
            .contains("Update the pull request's body so it matches the head you push"),
        "{}",
        fix.prompt
    );
}

#[test]
fn a_fix_left_uncommitted_is_sent_back_naming_its_files_before_the_worker_is_parked() {
    let (rig, runner, head) = Rig::with_pull_request("koji");
    rig.forge
        .set_checks(&head, Checks::Failed(vec!["test".into()]));
    rig.verdict(&runner);
    rig.claude.script([
        Scripted::Plant("fix.txt", "fixed\n"),
        Scripted::Push("fix.txt", "fixed\n"),
    ]);
    step(&runner).unwrap();
    assert!(matches!(
       step(&runner).unwrap(),
       Some(StepReport::Ended { issue, .. }) if issue == ticket(7)
    ));
    let [first, _, again] = rig.claude.calls().try_into().unwrap();
    assert_eq!(again.session, Session::Resume(first.session.id().clone()));
    assert!(
        again
            .prompt
            .starts_with("Your last turn ended with uncommitted changes in your worktree"),
        "{}",
        again.prompt
    );
    assert!(again.prompt.contains("fix.txt"), "{}", again.prompt);
    assert_eq!(rig.ask(&runner, "status", None)["rulings"], json!([]));
    assert_fix_landed(&rig, &runner, &head);
}

#[test]
fn a_second_red_run_on_a_head_the_worker_left_alone_parks_it() {
    let (rig, runner, head) = Rig::with_pull_request("rotom");
    rig.forge
        .set_checks(&head, Checks::Failed(vec!["lint".into()]));
    rig.verdict(&runner);
    rig.claude
        .script([Scripted::Reply(Usage::default(), Cost(1))]);
    step(&runner).unwrap();

    let (_, question) = ruling_report(rig.verdict(&runner));
    let parked = format!(
        "CI failed again on pull request #71 at {}, and the worker pushed no fix: lint.",
        &head[..7]
    );
    assert!(question.starts_with(&parked), "{question}");
    assert_eq!(rig.claude.calls().len(), 2, "the red run went out once");
    assert!(matches!(
        step(&runner).unwrap(),
        Some(StepReport::Alerted { .. })
    ));
    assert_eq!(step(&runner).unwrap(), None);

    // The maintainer's fix and yes vouch for the head: no foreign-change ruling.
    let fixed = rig.push_by_hand("kelpie/7", "lint.txt");
    rig.ask(&runner, "rule", Some("71 yes"));
    rig.forge.set_checks(&fixed, Checks::Passed);
    let (_, question) = ruling_report(rig.verdict(&runner));
    assert!(question.starts_with("Merge pull request #71"), "{question}");
}

#[test]
fn a_branch_behind_main_is_rebased_and_asked_about_only_after_ci_on_the_new_head() {
    let (rig, runner, head) = Rig::with_pull_request("shep");
    let landed = rig.land_on_origin("landed.txt");
    rig.forge.set_checks(&head, Checks::Passed);
    let Some(StepReport::Rebased { head: rebased, .. }) = step(&runner).unwrap() else {
        panic!("the branch was not rebased");
    };
    assert_eq!(rig.forge.head_of("kelpie/7").as_ref(), Some(&rebased));
    let worktree = rig.worktree_7();
    assert_eq!(git(&worktree, &["rev-parse", "HEAD"]), rebased);
    assert_eq!(git(&worktree, &["rev-parse", "HEAD~1"]), landed);
    assert_eq!(git(&worktree, &["show", "HEAD:work.txt"]), "work");

    assert_eq!(
        step(&runner).unwrap(),
        None,
        "CI on the rebased head is pending"
    );
    assert_eq!(rig.forge.comments(), []);
    rig.forge.set_checks(&rebased, Checks::Passed);
    ruling_report(rig.verdict(&runner));
    assert_eq!(
        rig.ask(&runner, "status", None)["rulings"][0]["kind"],
        json!({ "kind": "merge", "head": rebased })
    );
}

// Seen live on the playground: the step after a rebase read the head
// from before the push, and parked the worker on a false ruling.
#[test]
fn a_forge_still_showing_the_head_from_before_a_push_is_waited_out() {
    let (rig, runner, head) = Rig::with_pull_request("webapp");
    rig.land_on_origin("landed.txt");
    let Some(StepReport::Rebased { head: rebased, .. }) = step(&runner).unwrap() else {
        panic!("the branch was not rebased");
    };
    rig.forge.set_lagging(71, Some(&head));
    rig.forge.set_checks(&head, Checks::Passed);
    rig.clock.advance(CHECKS_SETTLE);
    assert_eq!(step(&runner).unwrap(), None);
    assert_eq!(rig.ask(&runner, "status", None)["rulings"], json!([]));
    assert_eq!(rig.forge.head_of("kelpie/7"), Some(rebased.clone()));

    rig.forge.set_lagging(71, None);
    rig.forge.set_checks(&rebased, Checks::Passed);
    ruling_report(rig.verdict(&runner));
    assert_eq!(
        rig.ask(&runner, "status", None)["rulings"][0]["kind"],
        json!({ "kind": "merge", "head": rebased })
    );
}

#[test]
fn with_ci_on_a_head_without_checks_waits_however_long_it_takes() {
    let (rig, runner, head) = Rig::with_pull_request("chelone");
    rig.forge.set_checks(&head, Checks::None);
    for _ in 0..5 {
        rig.clock.advance(3600);
        assert_eq!(step(&runner).unwrap(), None);
    }
    assert_eq!(rig.forge.comments(), []);
}

#[test]
fn a_verdict_waits_for_the_check_set_to_settle() {
    let (rig, runner, head) = Rig::with_pull_request("acme");
    rig.forge.set_checks(&head, Checks::Passed);
    assert_eq!(step(&runner).unwrap(), None);
    rig.clock.advance(CHECKS_SETTLE - 1);
    assert_eq!(step(&runner).unwrap(), None);
    rig.clock.advance(1);
    ruling_report(step(&runner).unwrap());
}

#[test]
fn with_ci_off_the_checks_are_never_read() {
    let rig = Rig::new("webapp");
    rig.edit_settings(|s| s.replace("block = true", "block = false"));
    let runner = rig.open().unwrap();
    rig.ask(&runner, "add", Some("7"));
    rig.forge.open_pull_request(71, "kelpie/7", &[7]);
    rig.claude
        .script([Scripted::Push("work.txt", "work\n"), Scripted::Text(CLEAN)]);
    step(&runner).unwrap(); // the worker's first turn: opens the pull request
    step(&runner).unwrap(); // review round 1, qwen: clean by default
    step(&runner).unwrap(); // review round 2, claude: scripted clean above
    let head = rig.forge.head_of("kelpie/7").unwrap();
    rig.forge
        .set_checks(&head, Checks::Failed(vec!["lint".into()]));

    let (id, question) = ruling_report(step(&runner).unwrap());
    assert!(question.starts_with("Merge pull request #71"), "{question}");
    rig.ask(&runner, "rule", Some(&format!("{id} yes")));
    assert!(matches!(
        step(&runner).unwrap(),
        Some(StepReport::MarkedReady { .. })
    ));
    assert!(matches!(
        step(&runner).unwrap(),
        Some(StepReport::Finished {
            issue,
            pull_request: Some(71),
            merged: true,
            ..
        }) if issue == ticket(7)
    ));
    assert_eq!(rig.forge.merges(), [(71, head)]);
}

#[test]
fn a_ruling_whose_comment_fails_still_stands_in_status_and_the_log() {
    let (rig, runner, _) = Rig::with_pull_request("golbat");
    rig.forge.set_state(71, PullRequestState::Closed);
    rig.forge.set_comments_down(true);
    let Some(StepReport::Ruling { comment_failed, .. }) = rig.verdict(&runner) else {
        panic!("no ruling was raised");
    };
    assert_eq!(
        comment_failed.as_deref(),
        Some("gh failed: comments are down")
    );
    assert_eq!(rig.ask(&runner, "status", None)["rulings"][0]["id"], "71");
}

#[test]
fn a_pull_request_merged_by_hand_ends_the_work_item_with_no_merge_by_kelpie() {
    let (rig, runner, _) = Rig::with_pull_request("acme");
    rig.forge.set_state(71, PullRequestState::Merged);
    assert!(matches!(
        step(&runner).unwrap(),
        Some(StepReport::Finished {
            issue,
            pull_request: Some(71),
            merged: true,
            ..
        }) if issue == ticket(7)
    ));
    assert_eq!(rig.forge.merges(), []);
    assert!(!rig.worktree_7().exists());
    assert_eq!(rig.ask(&runner, "status", None)["work_item"], json!(null));
}

#[test]
fn a_closed_pull_request_parks_the_worker_and_a_yes_keeps_its_branch() {
    let (rig, runner, head) = Rig::with_pull_request("reactmap");
    rig.forge.set_state(71, PullRequestState::Closed);
    let (id, question) = ruling_report(step(&runner).unwrap());
    assert!(
        question.starts_with("Pull request #71 was closed without merging."),
        "{question}"
    );
    assert_eq!(
        rig.forge.comments(),
        [(
            71,
            "This pull request was closed without merging.\n\nWaiting on the maintainer."
                .to_owned()
        )]
    );
    rig.ask(&runner, "rule", Some(&format!("{id} yes")));
    assert!(matches!(
        step(&runner).unwrap(),
        Some(StepReport::Finished {
            issue,
            pull_request: Some(71),
            merged: false,
            ..
        }) if issue == ticket(7)
    ));
    assert_eq!(rig.forge.head_of("kelpie/7"), Some(head));
    assert!(!rig.worktree_7().exists());
    assert_eq!(git(&rig.repo(), &["branch", "--list", "kelpie/7"]), "");
}

#[test]
fn a_label_added_outside_kelpie_parks_the_worker_naming_it() {
    let (rig, runner, head) = Rig::with_pull_request("shep");
    rig.forge.label_pull_request(71, "bug");
    let (id, question) = ruling_report(step(&runner).unwrap());
    assert_eq!(id, ticket(71));
    assert_eq!(
        question,
        "Pull request #71 changed outside kelpie: the `bug` label was added. \
         `shep kelpie rule 71 yes` accepts it and kelpie carries on, and \
         `shep kelpie rule 71 no <note>` sends the worker your note."
    );
    assert_eq!(
        rig.ask(&runner, "status", None)["rulings"][0]["kind"],
        json!({
            "kind": "foreign-change",
            "description": "the `bug` label was added",
            "known": { "labels": ["bug"], "ready": false, "head": head },
        })
    );

    // A yes accepts it, and kelpie carries on watching the same pull request.
    rig.ask(&runner, "rule", Some("71 yes"));
    rig.forge.set_checks(&head, Checks::Passed);
    let (next, merge_question) = ruling_report(rig.verdict(&runner));
    assert_eq!(next, ticket(71));
    assert!(merge_question.starts_with("Merge pull request #71"));
    // Kelpie's own record now matches, so the accepted label is not asked again.
    assert_eq!(rig.forge.merges(), []);
}

#[test]
fn marking_a_pull_request_ready_outside_kelpie_parks_the_worker() {
    let (rig, runner, _) = Rig::with_pull_request("koji");
    rig.forge.ready_pull_request(71);
    let (_, question) = ruling_report(step(&runner).unwrap());
    assert!(
        question.starts_with(
            "Pull request #71 changed outside kelpie: it was marked ready for review."
        ),
        "{question}"
    );
}

#[test]
fn a_no_on_a_foreign_change_sends_the_worker_a_note_and_asks_again_once_it_reaches_ci() {
    let (rig, runner, head) = Rig::with_pull_request("rotom");
    rig.forge.label_pull_request(71, "bug");
    ruling_report(step(&runner).unwrap());
    rig.ask(&runner, "rule", Some("71 no  remove that label yourself "));
    rig.claude.script([
        Scripted::Push("rename.txt", "renamed\n"),
        Scripted::Text(CLEAN),
    ]);
    step(&runner).unwrap(); // the noted turn: pushes, enters round 1
    let [first, noted] = rig.claude.calls().try_into().unwrap();
    assert_eq!(noted.session, Session::Resume(first.session.id().clone()));
    assert_eq!(
        noted.prompt,
        "The maintainer answered no on pull request #71, with this note:\n\n\
         remove that label yourself\n\nUpdate the pull request's body so it matches \
         the head you push: what changed, the files touched, the test evidence, and \
         the decisions you made.\n"
    );
    let fixed = rig.forge.head_of("kelpie/7");
    assert_ne!(fixed, Some(head));

    rig.forge.set_checks(&fixed.unwrap(), Checks::Passed);
    rig.verdict(&runner); // review round 1, qwen: clean by default, once its head is green
    step(&runner).unwrap(); // review round 2, claude: scripted clean above
    // The label is still there, and unaccepted, so the next look parks again.
    let (id, _) = ruling_report(rig.verdict(&runner));
    assert_eq!(id, ticket(71));
}

#[test]
fn a_label_removed_after_a_yes_accepted_it_parks_the_worker_again() {
    let (rig, runner, _) = Rig::with_pull_request("chelone");
    rig.forge.label_pull_request(71, "bug");
    ruling_report(step(&runner).unwrap());
    rig.ask(&runner, "rule", Some("71 yes"));
    rig.forge.unlabel_pull_request(71, "bug");
    let (id, question) = ruling_report(step(&runner).unwrap());
    assert_eq!(id, ticket(71));
    assert!(
        question
            .starts_with("Pull request #71 changed outside kelpie: the `bug` label was removed."),
        "{question}"
    );
}

#[test]
fn a_normal_gate_run_with_nothing_changed_outside_kelpie_asks_nothing_about_it() {
    let (rig, runner, head) = Rig::with_pull_request("acme");
    rig.forge.set_checks(&head, Checks::Passed);
    let (_, question) = ruling_report(rig.verdict(&runner));
    assert!(question.starts_with("Merge pull request #71"), "{question}");
}

#[test]
fn a_work_item_saved_before_heads_were_known_parks_nothing_and_learns_its_head() {
    let (rig, runner, head) = Rig::with_pull_request("chelone");
    drop(runner);
    let state = rig.paths().state;
    let text = std::fs::read_to_string(&state).unwrap();
    let mut saved: serde_json::Value = serde_json::from_str(&text).unwrap();
    let known = saved["work_items"][0]["known"].as_object_mut().unwrap();
    assert!(known.remove("head").is_some());
    std::fs::write(&state, saved.to_string()).unwrap();

    let runner = rig.open().unwrap();
    rig.forge.set_checks(&head, Checks::Passed);
    let (_, question) = ruling_report(rig.verdict(&runner));
    assert!(question.starts_with("Merge pull request #71"), "{question}");
    let text = std::fs::read_to_string(&state).unwrap();
    let saved: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(saved["work_items"][0]["known"]["head"], json!(head));
}

#[test]
fn a_work_item_saved_before_the_gate_existed_is_left_alone() {
    let (rig, runner, head) = Rig::with_pull_request("xilriws");
    drop(runner);
    let state = rig.paths().state;
    let text = std::fs::read_to_string(&state).unwrap();
    let mut saved: serde_json::Value = serde_json::from_str(&text).unwrap();
    let item = saved["work_items"][0].as_object_mut().unwrap();
    item.remove("phase");
    item.remove("red_head");
    std::fs::write(&state, saved.to_string()).unwrap();

    let runner = rig.open().unwrap();
    rig.forge.set_checks(&head, Checks::Passed);
    assert_eq!(step(&runner).unwrap(), None);
    assert_eq!(rig.forge.comments(), []);
    assert_eq!(
        rig.ask(&runner, "status", None)["work_item"]["phase"],
        json!({ "state": "implement" })
    );
}
