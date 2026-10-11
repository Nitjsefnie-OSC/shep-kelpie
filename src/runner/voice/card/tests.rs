use super::*;
use crate::ports::{Finding, Severity};
use crate::runner::ruling::question;
use crate::state::{Resume, Ruling, RulingKind, Stuck};
use crate::work_item::{Known, Phase};

// A ruling on pull request 71, whose question is the one kelpie words for it
fn ruling(id: u64, kind: RulingKind) -> Ruling {
    Ruling {
        id,
        issue: Some(7),
        question: question(id, 7, Some(71), &kind),
        pull_request: Some(71),
        kind,
        alerted: false,
    }
}

// ... with the project manager's words added to its question, as it adds them
fn said_by_the_pm(mut ruling: Ruling, words: &str) -> Ruling {
    ruling.question = format!(
        "{}\n\nThe project manager asks you to decide. PM says: {words}",
        ruling.question
    );
    ruling
}

fn merge() -> RulingKind {
    RulingKind::Merge {
        head: "6b0606e9b1c2d3e4f5a6b7c8d9e0f1a2b3c4d5e6".into(),
        unreviewed: None,
        open_threads: None,
        unread_head: false,
        note_fix: false,
        late_fix: false,
        nits: None,
    }
}

fn asking(asked: &str) -> RulingKind {
    RulingKind::Question {
        asked: asked.into(),
        resume: Resume::Nothing,
    }
}

// The expected comment: a file under `fixtures/rulings`, less its last newline
macro_rules! expected {
    ($name:literal) => {
        include_str!(concat!("../../../../fixtures/rulings/", $name)).trim_end()
    };
}

fn rendered(ruling: &Ruling) -> String {
    ruling_comment(Some("octocat"), ruling)
}

#[test]
fn a_merge_ruling_offers_merge_send_back_and_rework() {
    assert_eq!(rendered(&ruling(1, merge())), expected!("merge.md"));
}

#[test]
fn a_merge_ruling_that_warns_says_what_it_warns_of_and_offers_no_separate_rework() {
    let kind = RulingKind::Merge {
        head: "6b0606e9b1c2d3e4f5a6b7c8d9e0f1a2b3c4d5e6".into(),
        unreviewed: Some("the reviewer was down".into()),
        open_threads: Some("coderabbit".into()),
        unread_head: false,
        note_fix: false,
        late_fix: false,
        nits: None,
    };

    assert_eq!(
        rendered(&ruling(1, kind)),
        expected!("merge-open-threads.md")
    );
}

#[test]
fn a_ruling_that_mentions_no_one_says_to_answer_it_at_the_terminal() {
    assert_eq!(
        ruling_comment(None, &ruling(1, merge())),
        expected!("merge-unmentioned.md")
    );
}

#[test]
fn a_question_becomes_the_options_it_lists_plus_an_other_row() {
    let asked = "Should it be `--dry-run` or `--check`?\n\nBoth appear in the docs.\n\
                 PM recommends: `--dry-run`\nPM says: shep uses `--dry-run` everywhere.\n\
                 - `--dry-run`\n- `--check`";

    assert_eq!(
        rendered(&ruling(1, asking(asked))),
        expected!("question.md")
    );
}

#[test]
fn a_question_that_lists_no_options_is_answered_by_reply_alone() {
    let asked = "Which name should the flag have?";

    assert_eq!(
        rendered(&ruling(1, asking(asked))),
        expected!("question-open.md")
    );
}

#[test]
fn a_stuck_ruling_takes_a_yes_or_a_no_and_carries_the_project_managers_words() {
    let kind = RulingKind::Stuck(Stuck::StillRed {
        head: "6b0606e9b1c2d3e4f5a6b7c8d9e0f1a2b3c4d5e6".into(),
        checks: vec!["test".into(), "clippy".into()],
        fix_turns: Some(2),
    });
    let words = "The failure is a flaky network test, so one rerun should clear it.";

    assert_eq!(
        rendered(&said_by_the_pm(ruling(1, kind), words)),
        expected!("stuck-still-red.md")
    );
}

#[test]
fn a_stuck_ruling_that_a_no_cannot_answer_offers_only_a_yes() {
    let kind = RulingKind::Stuck(Stuck::Unlabelled {
        why: "the writer named no agent".into(),
    });

    assert_eq!(rendered(&ruling(1, kind)), expected!("stuck-unlabelled.md"));
}

#[test]
fn an_agent_files_ruling_names_the_files() {
    let kind = RulingKind::AgentFiles {
        head: "6b0606e9b1c2d3e4f5a6b7c8d9e0f1a2b3c4d5e6".into(),
        files: vec![".claude/settings.json".into(), ".mcp.json".into()],
        phase: Phase::Implement,
    };

    assert_eq!(rendered(&ruling(1, kind)), expected!("agent-files.md"));
}

#[test]
fn a_foreign_change_ruling_says_what_changed() {
    let kind = RulingKind::ForeignChange {
        description: "labels added: `ready-for-human`".into(),
        known: Known::default(),
    };

    assert_eq!(rendered(&ruling(1, kind)), expected!("foreign-change.md"));
}

#[test]
fn a_follow_up_ruling_counts_the_findings_it_would_file() {
    let finding = |file: &str| Finding {
        severity: Severity::Medium,
        file: file.into(),
        line: 3,
        what: "unused variable".into(),
        why: "dead code".into(),
    };
    let kind = RulingKind::FollowUp {
        findings: vec![finding("src/lib.rs"), finding("src/main.rs")],
        refused: None,
    };

    assert_eq!(rendered(&ruling(1, kind)), expected!("follow-up.md"));
}
