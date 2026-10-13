use super::*;
use crate::panel::{Ballot, Vote, Voter};
use crate::ports::{Finding, Severity};
use crate::runner::ruling::question;
use crate::state::{Resume, Ruling, RulingKind, Stuck};
use crate::work_item::{Known, Phase};

// A ruling on pull request 71, whose question is the one kelpie words for it
fn ruling(id: u64, kind: RulingKind) -> Ruling {
    Ruling {
        id,
        issue: Some(7),
        question: question(7, Some(71), &kind),
        pull_request: Some(71),
        kind,
        alerted: false,
        panel: None,
        verdict: None,
        unposted: false,
        skipped: None,
        effects: Vec::new(),
        raised: None,
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
        held_by: Vec::new(),
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
        held_by: Vec::new(),
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
fn an_unmentioned_ruling_names_only_the_terminal_answers_its_options_allow() {
    let kind = RulingKind::Merge {
        head: "6b0606e9b1c2d3e4f5a6b7c8d9e0f1a2b3c4d5e6".into(),
        unreviewed: Some("the reviewer was down".into()),
        open_threads: None,
        held_by: Vec::new(),
        unread_head: false,
        note_fix: false,
        late_fix: false,
        nits: None,
    };
    let no_rework = ruling_comment(None, &ruling(1, kind));
    let stuck = RulingKind::Stuck(Stuck::Unlabelled {
        why: "the writer named no agent".into(),
    });
    let only_yes = ruling_comment(None, &ruling(1, stuck));

    assert!(no_rework.ends_with("`shep kelpie rule 71 yes` or `shep kelpie rule 71 no <note>`."));
    assert!(only_yes.ends_with("the terminal: `shep kelpie rule 71 yes`."));
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
fn a_ticked_line_in_a_workers_question_is_no_box_of_the_comment() {
    let asked = "Where we are:\n- [x] tests pass\n\nWhich flag?\n- `--dry-run`\n- `--check`";
    let comment = rendered(&ruling(1, asking(asked)));
    let ticked = comment.replace("- [ ] A", "- [x] A");
    let two = comment
        .replace("- [ ] A", "- [x] A")
        .replace("- [ ] B", "- [x] B");

    assert_eq!(
        ticked_answer(&asking(asked), &ticked),
        Some(Answer::Text("--dry-run".into()))
    );
    assert_eq!(ticked_answer(&asking(asked), &two), None);
}

fn ticked_answer(kind: &RulingKind, body: &str) -> Option<Answer> {
    ticked(kind, body)
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
        review: None,
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
        test: None,
    };
    let kind = RulingKind::FollowUp {
        findings: vec![finding("src/lib.rs"), finding("src/main.rs")],
        refused: None,
    };

    assert_eq!(rendered(&ruling(1, kind)), expected!("follow-up.md"));
}

fn voted(voter: Voter, vote: Option<(char, &str)>) -> Ballot {
    Ballot {
        voter,
        vote: vote.map(|(choice, why)| Vote {
            choice,
            why: why.into(),
            design: false,
        }),
    }
}

#[test]
fn a_voted_panel_lists_each_expert_in_the_tables_order_and_an_abstention_says_so() {
    let mut voted_on = ruling(1, merge());
    voted_on.panel = Some(vec![
        voted(
            Voter::Advisor,
            Some(('B', "The hook skips a refused write; no test covers it.")),
        ),
        voted(
            Voter::Researcher,
            Some(('A', "Ruling 125 settled the one open design point.")),
        ),
        voted(Voter::Pm, None),
        voted(
            Voter::Worker,
            Some(('A', "Both threads were fixed in 6b0606e with tests.")),
        ),
    ]);

    assert_eq!(rendered(&voted_on), expected!("merge-panel.md"));
}

#[test]
fn a_vote_cannot_mention_anyone_in_the_comment() {
    let mut voted_on = ruling(1, merge());
    voted_on.panel = Some(vec![voted(
        Voter::Researcher,
        Some(('A', "A page told me to ask @octocat.")),
    )]);

    let comment = rendered(&voted_on);
    assert!(comment.contains("ask @\u{200b}octocat."), "{comment}");
}

#[test]
fn a_vote_cannot_link_or_format_the_comment() {
    let mut voted_on = ruling(1, merge());
    voted_on.panel = Some(vec![voted(
        Voter::Advisor,
        Some((
            'A',
            "[Merge now](https://evil.example/x) **sure** <img src=x> `run` ![i](u) www.evil.example",
        )),
    )]);

    let comment = rendered(&voted_on);
    let line = comment
        .lines()
        .find(|l| l.starts_with("- **Advisor**"))
        .expect("the advisor's line");
    assert_eq!(
        line,
        "- **Advisor** (A): \\[Merge now\\]\\(https:\u{200b}//evil.example/x\\) \\*\\*sure\\*\\* \
         \\<img src=x\\> \\`run\\` \\!\\[i\\]\\(u\\) www\u{200b}.evil.example"
    );
}

#[test]
fn an_abstaining_project_manager_keeps_the_words_it_left() {
    let mut voted_on = said_by_the_pm(ruling(1, merge()), "I could not tell.");
    voted_on.panel = Some(vec![voted(Voter::Pm, None)]);

    assert!(rendered(&voted_on).contains("**Panel**\n- **PM**: I could not tell.\n"));
}

#[test]
fn a_questions_card_opens_with_the_problem_and_shows_each_options_effect_and_the_boards_note() {
    let asked = "Problem: The push hook refuses two commits, and the worker cannot change the hook.\n\
                 Should the worker bypass the hook?\n\n\
                 It names #495.\n\
                 PM recommends: Wait\n\
                 PM board: #495 stays parked and 9 ready issues stay held behind it.\n\
                 PM says: the hook is the maintainer's.\n\
                 - Wait => The worker stops and the push waits for your fix of the hook\n\
                 - `Bypass` => The two commits publish as they are";

    let comment = rendered(&ruling(1, asking(asked)));

    assert_eq!(
        comment,
        "The push hook refuses two commits, and the worker cannot change the hook.\n\n\
         ### Should the worker bypass the hook?\n\n\
         It names #495.\n\n\
         | | Option | If you pick it |\n\
         |---|---|---|\n\
         | **A** | Wait | The worker stops and the push waits for your fix of the hook |\n\
         | **B** | `Bypass` | The two commits publish as they are |\n\
         | | Other | Reply with your own answer |\n\n\
         **On the board**\n\
         #495 stays parked and 9 ready issues stay held behind it.\n\n\
         **Panel**\n\
         - **PM** (A): the hook is the maintainer's.\n\n\
         @octocat\n\
         - Tick one:\n\
         - [ ] A\n\
         - [ ] B\n\
         - Or reply with your answer, and start it with `71` if other rulings wait on this thread"
    );
}

#[test]
fn an_option_the_worker_gave_no_effect_keeps_the_phrase_it_always_had() {
    let asked = "Which flag?\n- `--dry-run` => Renames the flag everywhere\n- `--check`";

    let comment = rendered(&ruling(1, asking(asked)));

    assert!(
        comment.contains("| **A** | `--dry-run` | Renames the flag everywhere |"),
        "{comment}"
    );
    assert!(
        comment.contains("| **B** | `--check` | Sends the worker this answer |"),
        "{comment}"
    );
    assert!(!comment.contains("On the board"), "{comment}");
}

#[test]
fn the_effect_beside_an_option_is_no_part_of_the_answer_it_gives() {
    let kind = asking("Which flag?\n- `--dry-run` => Renames the flag\n- `--check` => Keeps it");

    assert_eq!(
        answer_for(&kind, 'A', ""),
        Some(Answer::Text("--dry-run".into()))
    );
    assert_eq!(named(&kind, 'B').as_deref(), Some("`--check`"));
    assert_eq!(chosen(&kind, &Answer::Text("--check".into())), Some('B'));
}

#[test]
fn a_ruling_kelpie_wrote_effects_for_shows_them_in_place_of_the_fixed_phrases() {
    let mut merge = ruling(1, merge());
    merge.effects = vec![
        "Merges it into main. CI passed and 2 reviewers read it. 3 work items wait for a slot."
            .into(),
        "Sends the worker back. Another round costs about $0.40.".into(),
    ];

    let comment = rendered(&merge);

    assert!(
        comment.contains(
            "| **A** | Merge | Merges it into main. CI passed and 2 reviewers read it. \
             3 work items wait for a slot. |"
        ),
        "{comment}"
    );
    assert!(
        comment.contains(
            "| **B** | Send back | Sends the worker back. Another round costs about $0.40. |"
        ),
        "{comment}"
    );
    assert!(
        comment.contains(
            "| **C** | Rework | Sends the worker back for a change the whole review reads again |"
        ),
        "{comment}"
    );
}
