//! A ruling's comment once someone other than the maintainer, replying in its
//! thread, has settled it

use super::*;
use crate::github::ApiError;
use crate::runner::StepReport;

const NOTHING_WAITS: &str = "Nothing waits on the maintainer.";

// The App numbers its comments from 100, and the ruling's is the last it posted.
fn ruling_comment(rig: &Rig) -> u64 {
    99 + rig.github.comments().len() as u64
}

#[test]
fn a_ruling_answered_at_the_terminal_has_its_comment_edited_to_say_so() {
    let (rig, runner, _) = a_merge_ruling_on_71();

    rig.ask(&runner, "rule", Some("1 no not yet"));

    let [(comment, body)] = rig.github.edits().try_into().unwrap();
    assert_eq!(comment, ruling_comment(&rig));
    let first = format!(
        "**Settled.** Answered at the terminal with `shep kelpie rule`: no, not yet. {NOTHING_WAITS}"
    );
    assert_eq!(body.lines().next(), Some(first.as_str()));
    // The question stays.
    assert!(body.contains("### Merge this pull request at"), "{body}");
    // Nothing invites a reply.
    for gone in ["@octocat", "- [ ]", "👍", "Reply", "| Option |"] {
        assert!(!body.contains(gone), "{gone} is still in:\n{body}");
    }
}

#[test]
fn a_ruling_answered_in_its_own_thread_is_left_as_it_is() {
    let (rig, runner, _) = a_merge_ruling_on_71();
    rig.writes_on(71, "octocat", "1 no not yet");
    rig.claude.script([Scripted::Say("Fixed.")]);

    rig.next_look();
    step(&runner).unwrap();

    assert_eq!(pending(&rig, &runner), none());
    assert_eq!(rig.github.edits(), []);
}

#[test]
fn a_ruling_answered_by_a_reaction_on_its_comment_is_left_as_it_is() {
    let (rig, runner, _) = a_merge_ruling_on_71();
    rig.forge
        .threads
        .thumbs(ruling_comment(&rig), "octocat", false);
    rig.claude.script([Scripted::Push("fix.txt", "fixed\n")]);

    rig.next_look();
    step(&runner).unwrap();

    assert_eq!(pending(&rig, &runner), none());
    assert_eq!(rig.github.edits(), []);
}

#[test]
fn an_edit_that_failed_is_tried_again_at_the_next_poll() {
    let (rig, runner, _) = a_merge_ruling_on_71();
    let comment = ruling_comment(&rig);
    rig.github.fail_next(ApiError::Refused(502));

    rig.ask(&runner, "rule", Some("1 no not yet"));
    assert_eq!(rig.github.edits().len(), 1, "the first try");

    rig.next_look();
    step(&runner).unwrap();

    let [(first, said), (again, body)] = rig.github.edits().try_into().unwrap();
    assert_eq!((first, again), (comment, comment));
    assert_eq!(said, body);
    assert!(
        body.starts_with("**Settled.** Answered at the terminal"),
        "{body}"
    );

    // Once it went through, it is not sent again.
    rig.next_look();
    step(&runner).unwrap();
    assert_eq!(rig.github.edits().len(), 2);
}

#[test]
fn a_terminal_answer_to_a_head_the_branch_moved_past_edits_the_old_comment_to_say_withdrawn() {
    let (rig, runner, _) = Rig::with_pull_request_set("shep", |rig| {
        rig.with_app();
        rig.maintainer("octocat");
    });
    rig.push_by_hand("kelpie/7", "first.txt");
    assert!(matches!(
        rig.verdict(&runner),
        Some(StepReport::Ruling { id: 1, .. })
    ));
    let comment = ruling_comment(&rig);
    rig.push_by_hand("kelpie/7", "second.txt");

    rig.ask(&runner, "rule", Some("1 yes"));

    assert_eq!(pending(&rig, &runner), [2]);
    let [(edited, body)] = rig.github.edits().try_into().unwrap();
    assert_eq!(edited, comment);
    assert!(
        body.starts_with("**Withdrawn:** the branch moved"),
        "{body}"
    );
}

#[test]
fn an_answer_in_the_thread_to_a_head_the_branch_moved_past_leaves_the_comment_as_it_is() {
    let (rig, runner, _) = Rig::with_pull_request_set("shep", |rig| {
        rig.with_app();
        rig.maintainer("octocat");
    });
    rig.forge.threads.grant_write("octocat");
    rig.push_by_hand("kelpie/7", "first.txt");
    assert!(matches!(
        rig.verdict(&runner),
        Some(StepReport::Ruling { id: 1, .. })
    ));
    rig.push_by_hand("kelpie/7", "second.txt");
    rig.clock.advance(5);
    rig.writes_on(71, "octocat", "1 yes");

    rig.next_look();
    step(&runner).unwrap();

    assert_eq!(pending(&rig, &runner), [2]);
    assert_eq!(rig.github.edits(), []);
}
