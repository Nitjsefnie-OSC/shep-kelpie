//! A worker's question lands on its draft pull request once it has one, and on
//! its issue before

use super::*;
use crate::runner::pull_request_file;
use crate::state::{Resume, RulingKind};

const ASKS: &str =
    "Pushed.\n\n<kelpie-question>\nShould it be `--dry-run` or `--check`?\n</kelpie-question>\n";

// A worker on a repo the App does not cover opens its own draft, so the
// forge already lists it when the turn that pushed and asked ends.
#[test]
fn a_question_after_the_branch_is_pushed_and_the_draft_opened_is_posted_on_the_pull_request() {
    let (rig, runner) = with_issue_7("acme");
    rig.forge.open_pull_request(71, "kelpie/7", &[7]);
    rig.claude
        .script([Scripted::PushSay("work.txt", "work\n", ASKS)]);

    let report = step(&runner).unwrap();

    assert!(
        matches!(
            report,
            Some(StepReport::Asked {
                pull_request: Some(71),
                ..
            })
        ),
        "{report:?}"
    );
    let [(thread, body)] = rig.forge.comments().try_into().unwrap();
    assert_eq!(thread, 71);
    assert!(body.contains("`--dry-run` or `--check`"), "{body}");
    let kind = runner.lock().unwrap().state.rulings[0].kind.clone();
    assert!(
        matches!(
            kind,
            RulingKind::Question {
                resume: Resume::ReviewFirst,
                ..
            }
        ),
        "{kind:?}"
    );

    rig.ask(&runner, "rule", Some("71 answer use --dry-run"));
    rig.claude
        .script([Scripted::Push("flag.txt", "--dry-run\n")]);
    step(&runner).unwrap();
    let status = rig.ask(&runner, "status", None);
    let item = &status["work_item"];
    assert_eq!(item["pull_request"], 71);
    assert_eq!(item["phase"]["state"], "review");
}

// Nothing to push means no branch to open a draft from, so the issue is
// where the maintainer reads it.
#[test]
fn a_question_with_nothing_pushed_is_posted_on_the_issue() {
    let rig = Rig::new("acme");
    rig.with_app();
    let runner = rig.open().unwrap();
    rig.ask(&runner, "add", Some("7"));
    rig.claude.script([Scripted::Say(ASKS)]);

    let report = step(&runner).unwrap();

    assert!(
        matches!(
            report,
            Some(StepReport::Asked {
                pull_request: None,
                ..
            })
        ),
        "{report:?}"
    );
    let [(thread, body)] = rig.github.comments().try_into().unwrap();
    assert_eq!(thread, 7);
    assert!(body.contains("`--dry-run` or `--check`"), "{body}");
}

// A worker whose credential opens no pull request wrote its text for kelpie
// to open, and kelpie opens it at the turn's end, before the ruling.
#[test]
fn a_question_from_a_worker_that_hands_over_opens_the_draft_from_its_file_and_posts_there() {
    let rig = Rig::new("acme");
    rig.with_app();
    let runner = rig.open().unwrap();
    rig.ask(&runner, "add", Some("7"));
    fs::create_dir_all(rig.build_7()).unwrap();
    fs::write(
        pull_request_file::path(&rig.build_7()),
        "feat: x\n\nWhy.\n\nResolves #7\n",
    )
    .unwrap();
    rig.claude
        .script([Scripted::PushSay("work.txt", "work\n", ASKS)]);

    let report = step(&runner).unwrap();

    assert!(
        matches!(
            report,
            Some(StepReport::Asked {
                pull_request: Some(500),
                ..
            })
        ),
        "{report:?}"
    );
    assert_eq!(
        rig.forge.text_of(500),
        Some(("feat: x".to_owned(), "Why.\n\nResolves #7".to_owned()))
    );
    let [(thread, body)] = rig.github.comments().try_into().unwrap();
    assert_eq!(thread, 500);
    assert!(body.contains("`--dry-run` or `--check`"), "{body}");
}
