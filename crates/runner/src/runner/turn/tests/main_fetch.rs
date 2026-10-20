//! A turn whose fetch of `origin/main` failed, and the warning it leaves
//!
//! The worker runs no fetch of its own, so a main kelpie could not fetch is
//! one the worker merges without knowing. The runner stands a warning `status`
//! lists, and ends it when a fetch lands or the work item does.

use super::*;

#[test]
fn a_turn_that_cannot_fetch_main_says_so_once_and_stops_when_main_comes_back() {
    let (rig, runner) = with_issue_7("shep");
    rig.claude
        .script([Scripted::Reply(usage(1), Cost(20_085_300))]);
    step(&runner).unwrap();
    let item = runner.lock().unwrap().current().expect("an item").clone();
    let main = git(&rig.repo(), &["rev-parse", "origin/main"]);
    let origin = rig.home.path().join("origin.git");
    // origin's `main` goes away, which is what a fetch of it fails against.
    // The worktree is there, and its `origin/main` is the one kelpie last got.
    git(&origin, &["update-ref", "-d", "refs/heads/main"]);

    for _ in 0..2 {
        runner
            .lock()
            .unwrap()
            .worker_reach(&item, Start::Main, None)
            .unwrap();
    }

    let notes = runner.lock().unwrap().take_notes();
    let said: Vec<_> = (notes.iter())
        .filter(|n| n.contains("kelpie last fetched it"))
        .collect();
    assert_eq!(said.len(), 1, "{notes:#?}");
    assert!(said[0].contains("issue #7's worker"), "{}", said[0]);
    assert!(said[0].contains("git fetch"), "{}", said[0]);
    let status = rig.ask(&runner, "status", None);
    assert_eq!(status["standing"], json!([said[0]]));

    git(&origin, &["update-ref", "refs/heads/main", &main]);
    runner
        .lock()
        .unwrap()
        .worker_reach(&item, Start::Main, None)
        .unwrap();

    assert_eq!(runner.lock().unwrap().take_notes(), Vec::<String>::new());
    let status = rig.ask(&runner, "status", None);
    assert_eq!(status["standing"], serde_json::Value::Null, "none stands");
}

// An item that ends takes its stale main with it, so `status` is not left
// warning about an item that no longer runs.
#[test]
fn a_dropped_work_item_leaves_no_standing_main_warning() {
    let (rig, runner) = with_issue_7("shep");
    rig.claude
        .script([Scripted::Reply(usage(1), Cost(20_085_300))]);
    step(&runner).unwrap();
    let item = runner.lock().unwrap().current().expect("an item").clone();
    git(
        &rig.home.path().join("origin.git"),
        &["update-ref", "-d", "refs/heads/main"],
    );
    runner
        .lock()
        .unwrap()
        .worker_reach(&item, Start::Main, None)
        .unwrap();

    let standing = rig.ask(&runner, "status", None)["standing"]
        .as_array()
        .expect("a standing warning")
        .clone();
    assert_eq!(standing.len(), 1, "{standing:#?}");

    rig.ask(&runner, "drop", Some("7"));

    assert_eq!(
        rig.ask(&runner, "status", None)["standing"],
        serde_json::Value::Null,
        "none stands"
    );
}
