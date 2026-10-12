use std::sync::Mutex;

use crate::github::ApiError;
use crate::ports::{Checks, Cost, Finding, ForgeError, Severity};
use crate::runner::{CHECKS_SETTLE, Runner, StepReport, step};
use crate::test::{Rig, Scripted, ScriptedRound};

const ASKS: &str = "I added the flag.\n\n<kelpie-question>\nShould it be `--dry-run` or \
                    `--check`?\n</kelpie-question>\n";
const QUESTION: &str = "Should it be `--dry-run` or `--check`?";

// A question that lists no options, as its comment reads up to who answers it
const OPEN_QUESTION: &str = "### Should it be `--dry-run` or `--check`?\n\n\
     | | Option | If you pick it |\n|---|---|---|\n| | Other | Reply with your own answer |\n\n";

// ... and how it is answered when someone is mentioned
const REPLY: &str =
    "- Reply with your answer, and start it with `1` if other rulings wait on this thread";

// ... and when no one is mentioned
const HOW_UNMENTIONED: &str = "This ruling mentions no one, so a reaction, a tick or a reply \
     here answers nothing. Answer it at the terminal: `shep kelpie rule 1 <your answer>`.";

// A running project with issue 7 in flight, whose worker's first turn asks
// before any pull request is open
fn asking_before_a_pull_request(rig: &Rig) -> (Mutex<Runner>, StepReport) {
    let runner = rig.open().unwrap();
    rig.ask(&runner, "add", Some("7"));
    rig.claude.script([Scripted::Say(ASKS)]);
    let asked = step(&runner)
        .unwrap()
        .expect("the question raised a ruling");
    (runner, asked)
}

// Pull request 71 is open and a qwen finding went to the worker, whose fix
// turn asks a question: ruling 1 is on the pull request.
fn asking_on_a_pull_request(rig: &Rig) -> (Mutex<Runner>, StepReport) {
    let runner = rig.open().unwrap();
    rig.ask(&runner, "add", Some("7"));
    rig.forge.open_pull_request(71, "kelpie/7", &[7]);
    rig.claude.script([Scripted::Push("work.txt", "work\n")]);
    step(&runner).unwrap(); // opens the pull request, enters round 1 (qwen)
    rig.reviewer.script([ScriptedRound::Findings(vec![Finding {
        severity: Severity::Medium,
        file: "src/lib.rs".into(),
        line: 3,
        what: "unused variable".into(),
        why: "dead code".into(),
        test: None,
    }])]);
    step(&runner).unwrap(); // round 1's qwen call
    step(&runner).unwrap(); // the finding goes to the worker
    rig.claude.script([Scripted::Say(ASKS)]);
    let asked = step(&runner)
        .unwrap()
        .expect("the fix turn's question raised a ruling");
    (runner, asked)
}

// A repo the App covers has no use for the maintainer's keychain: a worker's
// token is all its push needs. A repo it does not cover pushes with the login there.
#[test]
fn a_call_blocks_the_keychain_on_a_repo_the_app_covers_and_on_no_other() {
    for covered in [true, false] {
        let rig = Rig::new("shep");
        if covered {
            rig.with_app();
        }
        let runner = rig.open().unwrap();
        rig.ask(&runner, "add", Some("7"));
        rig.claude.script([Scripted::Say("done")]);
        step(&runner).unwrap();
        let calls = rig.claude.calls();
        assert!(!calls.is_empty());
        for call in calls {
            assert_eq!(call.reach.block_keychain, covered, "{:?}", call.role);
        }
    }
}

#[test]
fn a_merge_ruling_is_posted_through_the_app_on_its_pull_request_and_mentions_the_maintainer() {
    let (rig, _runner, head) = Rig::parked_set("shep", |rig| {
        rig.with_app();
        rig.maintainer("octocat");
    });

    let short = &head[..7];
    let body = format!(
        "### Merge this pull request at {short} into main?\n\n\
         | | Option | If you pick it |\n\
         |---|---|---|\n\
         | **A** | Merge | Merges it into main and closes #7: Title of #7. CI passed. 2 review rounds read it. |\n\
         | **B** | Send back | Sends the worker back for a fix that goes to CI and back to you, with no new review round. |\n\
         | **C** | Rework | Sends the worker back for a change the whole review reads again. |\n\n\
         @octocat\n\
         - 👍 or 👎 answers, or tick one:\n\
         - [ ] A\n\
         - [ ] B\n\
         - [ ] C\n\
         - Or reply `1 no <note>` or `1 rework <note>`"
    );
    // The two review rounds that came first are on the thread too.
    let comments = rig.github.comments();
    let [qwen, claude, ruling] = comments.as_slice() else {
        panic!("two rounds and the ruling: {comments:?}");
    };
    assert_eq!((qwen.0, claude.0), (71, 71));
    assert_eq!(ruling, &(71, body));
    let tokens: Vec<_> = rig.github.writes().into_iter().map(|(t, _)| t).collect();
    assert!(
        tokens.iter().all(|t| t == "ghs_test1"),
        "through the App's token: {tokens:?}"
    );
    assert_eq!(
        rig.forge.comments(),
        [],
        "nothing posted as the maintainer's login"
    );
}

// The merge ruling's comment on pull request 71, the last of its thread
fn merge_card(rig: &Rig) -> String {
    let (_, body) = rig.github.comments().pop().expect("the ruling's comment");
    body
}

#[test]
fn a_merge_card_says_what_waits_behind_it_and_who_read_the_pull_request() {
    let (rig, _runner, _) = Rig::parked_set("shep", |rig| {
        rig.with_app();
        rig.maintainer("octocat");
        rig.edit_settings(|s| s.replace("pending_rulings = 2", "pending_rulings = 1"));
    });

    let card = merge_card(&rig);

    assert!(
        card.contains(
            "| **A** | Merge | Merges it into main and closes #7: Title of #7. CI passed. \
             2 review rounds read it. 1 ruling is waiting, and `concurrency.pending_rulings` \
             is 1, so no new work item opens until one is answered. |"
        ),
        "{card}"
    );
}

#[test]
fn a_merge_card_says_roughly_what_another_review_round_costs() {
    let rig = Rig::new("shep");
    rig.with_app();
    rig.maintainer("octocat");
    let runner = rig.open().unwrap();
    rig.ask(&runner, "add", Some("7"));
    rig.forge.open_pull_request(71, "kelpie/7", &[7]);
    rig.claude.script([
        Scripted::Push("work.txt", "work\n"),
        Scripted::Billed("CLEAN", Cost(420_000_000)),
    ]);
    step(&runner).unwrap(); // the worker's first turn: opens the pull request
    step(&runner).unwrap(); // review round 1, qwen: clean by default
    step(&runner).unwrap(); // review round 2, claude: billed
    let head = rig.forge.head_of("kelpie/7").expect("the worker pushed");
    rig.ci_on_now(&runner);
    rig.forge.set_checks(&head, Checks::Passed);
    assert!(matches!(
        rig.verdict(&runner),
        Some(StepReport::Ruling { id: 1, .. })
    ));

    let card = merge_card(&rig);

    assert!(
        card.contains(
            "| **C** | Rework | Sends the worker back for a change the whole review reads \
             again. Another review round costs about $0.21. |"
        ),
        "{card}"
    );
}

#[test]
fn a_ruling_before_any_pull_request_is_posted_on_the_issue() {
    let rig = Rig::new("shep");
    rig.with_app();
    rig.maintainer("octocat");

    let (_runner, asked) = asking_before_a_pull_request(&rig);

    assert!(matches!(asked, StepReport::Asked { .. }), "{asked:?}");
    assert_eq!(
        rig.github.comments(),
        [(7, format!("{OPEN_QUESTION}@octocat\n{REPLY}"))]
    );
    assert_eq!(rig.forge.comments(), []);
}

#[test]
fn with_no_maintainer_set_a_ruling_mentions_the_login_gh_acts_as_asked_once_a_run() {
    let (rig, runner, _) = Rig::parked_set("shep", |rig| rig.with_app());
    rig.ask(&runner, "rule", Some("1 no not yet"));
    rig.claude.script([Scripted::Say(ASKS)]);
    step(&runner).unwrap(); // the fix turn asks a question: a second ruling

    let mentions = (rig.github.comments().into_iter())
        .filter(|(_, body)| body.contains("\n@the-maintainer\n"))
        .count();
    assert_eq!(mentions, 2, "the merge ruling and the question");
    assert_eq!(rig.forge.viewer_reads(), 1);
    let notes = runner.lock().unwrap().take_notes();
    assert!(
        !notes.iter().any(|n| n.contains("without a mention")),
        "{notes:?}"
    );
}

#[test]
fn a_maintainer_that_is_set_wins_over_the_login_gh_acts_as() {
    let (rig, _runner, _) = Rig::parked_set("shep", |rig| {
        rig.with_app();
        rig.maintainer("octocat");
    });

    let comments = rig.github.comments();
    assert!(comments.last().unwrap().1.contains("\n@octocat\n"));
    assert_eq!(rig.forge.viewer_reads(), 0, "gh is not asked");
}

#[test]
fn a_ruling_posted_when_gh_names_no_login_goes_up_unmentioned_and_says_so() {
    let rig = Rig::new("shep");
    rig.with_app();
    rig.forge
        .set_viewer_down(ForgeError::Failed("not logged in".into()));

    let (runner, asked) = asking_before_a_pull_request(&rig);

    let StepReport::Asked { comment_failed, .. } = asked else {
        panic!("not a question: {asked:?}");
    };
    let why = comment_failed.expect("the step says the mention was lost");
    assert!(
        why.starts_with("posted without mentioning the maintainer"),
        "{why}"
    );
    assert!(why.contains("not logged in"), "{why}");
    assert_eq!(
        rig.github.comments(),
        [(7, format!("{OPEN_QUESTION}{HOW_UNMENTIONED}"))]
    );
    assert_eq!(rig.ask(&runner, "status", None)["rulings"][0]["id"], 1);

    // Asked again at the next ruling, once gh answers, it mentions them.
    rig.forge.set_viewer_up();
    rig.ask(&runner, "rule", Some("1 answer use --dry-run"));
    rig.claude.script([Scripted::Say(ASKS)]);
    step(&runner).unwrap();
    let comments = rig.github.comments();
    assert!(
        comments.last().unwrap().1.contains("\n@the-maintainer\n"),
        "{comments:?}"
    );
}

#[test]
fn an_unmentioned_ruling_answered_at_the_terminal_has_its_comment_edited_to_say_so() {
    let rig = Rig::new("shep");
    rig.with_app();
    rig.forge
        .set_viewer_down(ForgeError::Failed("not logged in".into()));
    let (runner, _) = asking_before_a_pull_request(&rig);

    rig.ask(&runner, "rule", Some("1 answer use --dry-run"));

    let [(comment, body)] = rig.github.edits().try_into().unwrap();
    assert_eq!(comment, 100);
    assert_eq!(
        body.lines().next(),
        Some(
            "**Settled.** Answered at the terminal with `shep kelpie rule`: use --dry-run. \
             Nothing waits on the maintainer."
        )
    );
}

#[test]
fn an_unmentioned_ruling_is_noted_once_and_only_once_it_went_up() {
    let rig = Rig::new("shep");
    rig.with_app();
    rig.forge
        .set_viewer_down(ForgeError::Failed("not logged in".into()));
    rig.github.refuse_comments(ApiError::Refused(502));

    // The first ruling never went up, so there is nothing to say it lacked a mention.
    let (runner, _) = asking_before_a_pull_request(&rig);
    let told = |runner: &Mutex<Runner>| {
        let notes = runner.lock().unwrap().take_notes();
        (notes.iter())
            .filter(|n| n.contains("without a mention"))
            .count()
    };
    assert_eq!(told(&runner), 0);

    // The next two do, and the note is made for the first of them only.
    rig.github.accept_comments();
    for id in 1..=2 {
        rig.ask(&runner, "rule", Some(&format!("{id} answer use --dry-run")));
        rig.claude.script([Scripted::Say(ASKS)]);
        step(&runner).unwrap();
    }
    let notes = runner.lock().unwrap().take_notes();
    let told: Vec<_> = (notes.iter())
        .filter(|n| n.contains("without a mention"))
        .collect();
    assert_eq!(told.len(), 1, "{notes:?}");
    assert!(told[0].starts_with("ruling 2 is posted on #7"), "{told:?}");
    assert!(told[0].contains("set `git.maintainer`"), "{told:?}");
}

#[test]
fn with_no_app_a_ruling_posts_as_it_always_did() {
    let rig = Rig::new("shep");
    let (_runner, asked) = asking_on_a_pull_request(&rig);
    assert!(matches!(asked, StepReport::Asked { .. }), "{asked:?}");

    assert_eq!(
        rig.forge.comments(),
        [(71, format!("{QUESTION}\n\nWaiting on the maintainer."))]
    );
    assert_eq!(rig.github.writes(), []);
    let (rig, _runner, _) = Rig::parked("koji");
    assert_eq!(rig.forge.comments(), [], "a merge ruling has no comment");
    assert_eq!(rig.github.writes(), []);
}

#[test]
fn an_app_that_is_registered_for_another_owner_leaves_a_ruling_as_it_was() {
    let rig = Rig::new("shep");
    let elsewhere = crate::test::FakeGithub::new(rig.clock.clone(), "someone-else");
    elsewhere.registered(&rig.paths().kelpie_home, "someone-else/koji");

    asking_on_a_pull_request(&rig);

    assert_eq!(rig.forge.comments().len(), 1);
    assert_eq!(rig.github.writes(), []);
}

#[test]
fn a_ruling_whose_post_fails_is_still_raised_and_says_why() {
    let rig = Rig::new("shep");
    rig.with_app();
    // GitHub takes no comment: the ruling is raised and saved, the step says
    // why the comment failed, and nothing is posted as the maintainer instead.
    rig.github.refuse_comments(ApiError::Refused(403));

    let (runner, asked) = asking_before_a_pull_request(&rig);

    let StepReport::Asked {
        id, comment_failed, ..
    } = asked
    else {
        panic!("not a question: {asked:?}");
    };
    let why = "the GitHub App could not post: GitHub answered HTTP 403";
    assert_eq!(comment_failed.as_deref(), Some(why));
    assert_eq!(rig.forge.comments(), []);
    assert_eq!(rig.ask(&runner, "status", None)["rulings"][0]["id"], id);
}

#[test]
fn an_automatic_merge_s_notice_is_a_comment_on_its_pull_request_as_well_as_the_webhook_post() {
    let (rig, runner, head) = Rig::with_pull_request("shep");
    drop(runner);
    rig.with_app();
    rig.merge_auto();
    let runner = rig.open().unwrap();
    rig.forge.set_checks(&head, Checks::Passed);
    rig.verdict(&runner);
    rig.clock.advance(CHECKS_SETTLE);

    let merged = step(&runner).unwrap();

    assert!(
        matches!(merged, Some(StepReport::Finished { merged: true, .. })),
        "{merged:?}"
    );
    let notice = format!(
        "Pull request #71 for issue #7 merged into main at {}, every gate passed. \
         Nothing to answer.",
        &head[..7]
    );
    assert_eq!(rig.github.comments(), [(71, notice)]);
    assert_eq!(rig.forge.comments(), []);
    assert!(matches!(
        step(&runner).unwrap(),
        Some(StepReport::Noticed { .. })
    ));
    assert_eq!(
        rig.alerts.posts().len(),
        1,
        "the webhook still gets its notice"
    );
}

#[test]
fn each_local_round_is_posted_on_the_pull_request_as_it_ends() {
    let rig = Rig::new("shep");
    rig.with_app();
    let runner = rig.open().unwrap();
    rig.ask(&runner, "add", Some("7"));
    rig.forge.open_pull_request(71, "kelpie/7", &[7]);
    rig.claude.script([Scripted::Push("work.txt", "work\n")]);
    step(&runner).unwrap(); // opens the pull request, enters round 1 (qwen)
    let head = rig.forge.head_of("kelpie/7").unwrap();
    rig.reviewer.script([ScriptedRound::Findings(vec![
        Finding {
            severity: Severity::High,
            file: "src/lib.rs".into(),
            line: 3,
            what: "unused variable".into(),
            why: "dead code".into(),
            test: None,
        },
        Finding {
            severity: Severity::Low,
            file: "README.md".into(),
            line: 0,
            what: "typo".into(),
            why: "reads badly".into(),
            test: None,
        },
    ])]);

    step(&runner).unwrap(); // round 1's qwen call

    let short = &head[..7];
    let found = format!(
        "qwen read this pull request at {short} in review round 1 and found 2 things:\n\n\
         - HIGH `src/lib.rs:3`: unused variable. dead code\n\
         - LOW `README.md`: typo. reads badly"
    );
    assert_eq!(rig.github.comments(), [(71, found)]);

    step(&runner).unwrap(); // the findings go to the worker
    rig.claude.script([
        Scripted::Push("fixed.txt", "fixed\n"),
        Scripted::Text("CLEAN"),
    ]);
    step(&runner).unwrap(); // the fix turn pushes
    step(&runner).unwrap(); // the head moved, so round 1 ends
    step(&runner).unwrap(); // round 2, the Claude reviewer, reads the fix
    let comments = rig.github.comments();
    let [_, (thread, clean)] = comments.as_slice() else {
        panic!("one comment for each round: {comments:?}");
    };
    let head = rig.forge.head_of("kelpie/7").unwrap();
    let want = format!(
        "claude read this pull request at {} in review round 2 and found nothing.",
        &head[..7]
    );
    assert_eq!((*thread, clean.as_str()), (71, want.as_str()));
    assert_eq!(rig.forge.comments(), []);
}

#[test]
fn a_round_whose_post_fails_still_ends_and_the_failure_is_told_once() {
    let rig = Rig::new("shep");
    rig.with_app();
    rig.github.refuse_comments(ApiError::Refused(403));
    let runner = rig.open().unwrap();
    rig.ask(&runner, "add", Some("7"));
    rig.forge.open_pull_request(71, "kelpie/7", &[7]);
    rig.claude.script([Scripted::Push("work.txt", "work\n")]);
    step(&runner).unwrap(); // opens the pull request, enters round 1 (qwen)

    let round = step(&runner).unwrap(); // round 1's qwen call: clean

    assert!(
        matches!(round, Some(StepReport::ReviewFindingsSent { held: 0, .. })),
        "{round:?}"
    );
    let notes = runner.lock().unwrap().take_notes();
    let told: Vec<_> = (notes.iter())
        .filter(|n| n.contains("cannot post qwen's round 1 on #71"))
        .collect();
    assert_eq!(told.len(), 1, "{notes:?}");
    assert!(told[0].contains("HTTP 403"), "{told:?}");
}

#[test]
fn with_no_app_a_round_posts_nothing() {
    let (rig, _runner, _) = Rig::with_pull_request("shep");

    assert_eq!(rig.github.writes(), []);
    assert_eq!(rig.forge.comments(), []);
}

#[test]
fn the_label_that_holds_an_issue_is_put_on_through_the_app() {
    let rig = Rig::new("shep");
    rig.with_app();
    let runner = rig.open().unwrap();

    rig.ask(&runner, "add", Some("7"));
    step(&runner).unwrap();

    let held = (rig.github.writes().into_iter())
        .find(|(_, call)| call.path == "/repos/shep-pm/shep/issues/7/labels")
        .expect("the issue was labelled through the App");
    assert_eq!(held.0, "ghs_test1");
    assert_eq!(
        held.1.body.as_deref(),
        Some(r#"{"labels":["in-progress"]}"#)
    );
}

#[test]
fn an_issue_closed_with_no_change_is_noticed_on_the_issue() {
    let rig = Rig::new("shep");
    rig.with_app();
    let runner = rig.open().unwrap();
    rig.ask(&runner, "add", Some("7"));
    rig.claude.script([Scripted::Say(ASKS)]);
    step(&runner).unwrap(); // asks, parked with no pull request
    rig.forge.close_issue(7);
    rig.clock.advance(3600);

    step(&runner).unwrap(); // reads the parked issue and finds it closed

    let comments = rig.github.comments();
    let (thread, notice) = comments.last().unwrap();
    assert_eq!(*thread, 7);
    assert!(
        notice.starts_with("Issue #7 was closed with no change"),
        "{notice}"
    );
}
