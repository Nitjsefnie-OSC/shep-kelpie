//! The maintainer's comments on a work item's issue, in its first prompt

use super::*;
use crate::ports::{Cost, ForgeError};
use crate::runner::turn::first_prompt;
use crate::runner::turn::tests::{issue_of, usage};
use crate::runner::{StepReport, step};
use crate::test::{Rig, Scripted, VIEWER};

const MAINTAINER: &str = "rin";

// A running project whose maintainer is `rin`, with issue 7 taken
fn with_maintainer() -> (Rig, std::sync::Mutex<Runner>) {
    let rig = Rig::new("shep");
    rig.maintainer(MAINTAINER);
    let runner = rig.open().unwrap();
    assert_eq!(
        rig.ask(&runner, "add", Some("7"))["work_item"]["issue"],
        "7"
    );
    (rig, runner)
}

fn at(day: u64) -> Timestamp {
    // 2026-01-01 and a day for each
    Timestamp(1_767_225_600 + day * 86_400)
}

fn first_turn_prompt(rig: &Rig, runner: &std::sync::Mutex<Runner>) -> String {
    rig.claude
        .script([Scripted::Reply(usage(1), Cost(20_085_300))]);
    step(runner).unwrap();
    let [seen] = rig.claude.seen().try_into().unwrap();
    seen.call.prompt
}

#[test]
fn the_maintainer_s_comments_follow_the_issue_oldest_first() {
    let (rig, runner) = with_maintainer();
    rig.forge
        .threads
        .write(7, "rin", "Name the file `a.toml`.", at(0));
    rig.forge
        .threads
        .write(7, "Rin", "Make that `b.toml`.\n\n", at(2));

    let prompt = first_turn_prompt(&rig, &runner);

    assert!(
        prompt.ends_with(
            "\nYour work item is issue #7: Title of #7\n\nBody of #7.\n\
             \nComments on the issue from the maintainer, oldest first:\n\
             \n2026-01-01:\nName the file `a.toml`.\n\
             \n2026-01-03:\nMake that `b.toml`.\n"
        ),
        "{prompt}"
    );
}

#[test]
fn comments_from_anyone_else_a_bot_or_the_app_are_left_out() {
    let (rig, runner) = with_maintainer();
    rig.forge.threads.write(7, "stranger", "Use YAML.", at(0));
    rig.forge
        .threads
        .bot_writes(7, "dependabot[bot]", "Bumped.", at(1));
    // The App's triage note is listed like anyone's, and is a bot's.
    rig.forge
        .threads
        .bot_writes(7, "kelpie-app[bot]", "Triaged: ready.", at(2));
    // Kelpie's own login is not the maintainer, since the setting names one.
    rig.forge.threads.write(7, VIEWER, "Kelpie here.", at(3));
    rig.forge.threads.write(7, "rin", "Use TOML.", at(4));

    let prompt = first_turn_prompt(&rig, &runner);

    assert!(prompt.contains("2026-01-05:\nUse TOML.\n"), "{prompt}");
    for left_out in ["YAML", "Bumped", "Triaged", "Kelpie here"] {
        assert!(!prompt.contains(left_out), "{left_out}: {prompt}");
    }
}

#[test]
fn a_comment_kelpie_posted_under_the_maintainer_s_login_is_left_out() {
    // No `git.maintainer`, so the maintainer is the login `gh` acts as, which
    // is also the login kelpie comments with where no App covers the repo.
    let rig = Rig::new("shep");
    let runner = rig.open().unwrap();
    rig.forge.open_issue(7, "Title of #7", "Body of #7.\n");
    assert_eq!(
        rig.ask(&runner, "add", Some("7"))["work_item"]["issue"],
        "7"
    );
    runner
        .lock()
        .unwrap()
        .comment_as_gh(7, "A review of #3 confirmed this too.")
        .unwrap();
    rig.forge.threads.write(7, VIEWER, "Use TOML.", at(4));

    let prompt = first_turn_prompt(&rig, &runner);

    assert!(prompt.contains("2026-01-05:\nUse TOML.\n"), "{prompt}");
    assert!(!prompt.contains("A review of #3"), "{prompt}");
}

#[test]
fn a_thread_with_no_comment_from_the_maintainer_leaves_the_prompt_as_it_was() {
    let (rig, runner) = with_maintainer();
    rig.forge.threads.write(7, "stranger", "Use YAML.", at(0));

    let prompt = first_turn_prompt(&rig, &runner);

    assert!(
        prompt.ends_with("\nYour work item is issue #7: Title of #7\n\nBody of #7.\n"),
        "{prompt}"
    );
    assert!(!prompt.contains("Comments on"), "{prompt}");
}

#[test]
fn a_thread_over_16_kib_keeps_the_newest_comments_and_counts_the_rest() {
    let (rig, runner) = with_maintainer();
    for (day, mark) in ["first", "second", "third", "fourth"]
        .into_iter()
        .enumerate()
    {
        let body = format!("{mark} {}", "x".repeat(6_000));
        rig.forge.threads.write(7, "rin", &body, at(day as u64));
    }

    let prompt = first_turn_prompt(&rig, &runner);

    assert!(!prompt.contains("first x"), "{prompt}");
    assert!(!prompt.contains("second x"), "{prompt}");
    let third = prompt.find("third x").expect("the third is kept");
    let fourth = prompt.find("fourth x").expect("the fourth is kept");
    assert!(third < fourth, "oldest first");
    let note = "[2 older comments left out: this list holds at most 16 KiB.]";
    assert!(prompt.contains(note), "{prompt}");
    assert!(prompt.find(note).unwrap() < third, "the count comes first");
    let block = prompt.split("oldest first:").nth(1).unwrap();
    assert!(block.len() <= 16 * 1024 + 256, "{} bytes", block.len());
}

#[test]
fn one_comment_over_16_kib_is_cut_and_not_left_out() {
    let (rig, runner) = with_maintainer();
    rig.forge
        .threads
        .write(7, "rin", &format!("big {}", "é".repeat(20_000)), at(0));

    let prompt = first_turn_prompt(&rig, &runner);

    assert!(prompt.contains("2026-01-01:\nbig é"), "{prompt}");
    assert!(prompt.contains("Kelpie cut this comment here"), "{prompt}");
    assert!(!prompt.contains("left out"), "{prompt}");
    let block = prompt.split("oldest first:").nth(1).unwrap();
    assert!(block.len() <= 16 * 1024, "{} bytes", block.len());
}

#[test]
fn a_maintainer_who_cannot_be_named_gets_no_comments_and_a_note() {
    let rig = Rig::new("shep");
    rig.forge
        .set_viewer_down(ForgeError::Failed("no login".into()));
    let runner = rig.open().unwrap();
    assert_eq!(
        rig.ask(&runner, "add", Some("7"))["work_item"]["issue"],
        "7"
    );
    rig.forge
        .threads
        .write(7, VIEWER, "Name it `a.toml`.", at(0));
    runner.lock().unwrap().take_notes();

    let prompt = first_turn_prompt(&rig, &runner);

    assert!(
        prompt.ends_with("\nYour work item is issue #7: Title of #7\n\nBody of #7.\n"),
        "{prompt}"
    );
    let notes = runner.lock().unwrap().take_notes();
    let said: Vec<_> = notes.iter().filter(|n| n.contains("issue #7")).collect();
    assert_eq!(said.len(), 1, "{notes:#?}");
    assert!(said[0].contains("maintainer"), "{}", said[0]);
    assert!(said[0].contains("no login"), "{}", said[0]);
}

#[test]
fn a_comment_read_that_fails_fails_the_turn_start_and_a_yes_sends_them() {
    let (rig, runner) = with_maintainer();
    rig.forge
        .threads
        .write(7, "rin", "Name it `a.toml`.", at(0));
    rig.forge.threads.set_unlisted(7, true);

    let Some(StepReport::Failed { question, .. }) = step(&runner).unwrap() else {
        panic!("the turn started without the comments");
    };

    assert!(
        question.contains("cannot read issue #7 comments"),
        "{question}"
    );
    assert_eq!(rig.claude.calls(), []);

    rig.forge.threads.set_unlisted(7, false);
    rig.ask(&runner, "rule", Some("7 yes"));
    rig.claude
        .script([Scripted::Reply(usage(1), Cost(20_085_300))]);
    step(&runner).unwrap();

    let [call] = rig.claude.calls().try_into().unwrap();
    assert!(
        call.prompt.contains("2026-01-01:\nName it `a.toml`.\n"),
        "{}",
        call.prompt
    );
}

#[test]
fn a_ticket_from_another_source_calls_its_comments_the_ticket_s() {
    let local: TicketId = "k12".parse().unwrap();
    let kept = [ThreadComment {
        id: 1,
        thread: 12,
        author: MAINTAINER.to_owned(),
        bot: false,
        body: "Use TOML.".to_owned(),
        created: at(0),
    }];

    assert_eq!(
        first_prompt(local, &issue_of(), &kept),
        "Your work item is ticket k12: Title of #7\n\nBody of #7.\n\
         \nComments on the ticket from the maintainer, oldest first:\n\
         \n2026-01-01:\nUse TOML.\n"
    );
}
