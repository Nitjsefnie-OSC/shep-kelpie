//! The nudge for a turn that ended with no pull request names what the worker
//! holds a credential for

use super::*;
use crate::runner::pull_request_file;

// Issue 7 in flight on a repo the App covers, so its worker hands its pull
// request over
fn covered_issue_7() -> (Rig, Mutex<Runner>) {
    let rig = Rig::new("acme");
    rig.with_app();
    let runner = rig.open().unwrap();
    assert_eq!(
        rig.ask(&runner, "add", Some("7"))["work_item"]["issue"],
        "7"
    );
    fs::create_dir_all(rig.build_7()).unwrap();
    (rig, runner)
}

#[test]
fn a_hand_over_workers_nudge_asks_for_the_file_and_never_to_open_a_pull_request() {
    let (rig, runner) = covered_issue_7();
    rig.claude.script([
        Scripted::Say("cargo test failed in the sandbox, so I stopped."),
        Scripted::Reply(usage(1), Cost(1)),
    ]);

    step(&runner).unwrap();
    step(&runner).unwrap();

    let [_, again] = rig.claude.calls().try_into().unwrap();
    assert!(again.prompt.contains("pull-request.md"), "{}", again.prompt);
    assert!(
        again.prompt.contains("git push origin HEAD"),
        "{}",
        again.prompt
    );
    assert!(!again.prompt.contains("open the draft"), "{}", again.prompt);
    assert!(!again.prompt.contains("gh pr create"), "{}", again.prompt);
}

#[test]
fn a_pushed_branch_with_its_pull_request_file_written_gets_no_nudge() {
    let (rig, runner) = covered_issue_7();
    rig.claude.script([Scripted::Push("work.txt", "work\n")]);
    step(&runner).unwrap();
    // The file lands after the turn's end looked for it.
    fs::write(
        pull_request_file::path(&rig.build_7()),
        "feat: x\n\nWhy.\n\nResolves #7\n",
    )
    .unwrap();

    step(&runner).unwrap();

    let item = &rig.ask(&runner, "status", None)["work_item"];
    assert_eq!(item["pull_request"], 500);
    assert_eq!(item["phase"]["state"], "review");
    assert_eq!(rig.claude.calls().len(), 1, "the worker was sent back");
    assert_eq!(
        rig.forge.text_of(500),
        Some(("feat: x".to_owned(), "Why.\n\nResolves #7".to_owned()))
    );
}

#[test]
fn a_worker_that_opens_its_own_pull_request_keeps_the_open_it_nudge() {
    let (rig, runner) = with_issue_7("acme");
    rig.claude.script([
        Scripted::Say("cargo test failed in the sandbox, so I stopped."),
        Scripted::Reply(usage(1), Cost(1)),
    ]);

    step(&runner).unwrap();
    step(&runner).unwrap();

    let [_, again] = rig.claude.calls().try_into().unwrap();
    assert!(
        again.prompt.contains("open the draft pull request"),
        "{}",
        again.prompt
    );
    assert!(
        !again.prompt.contains("pull-request.md"),
        "{}",
        again.prompt
    );
}
