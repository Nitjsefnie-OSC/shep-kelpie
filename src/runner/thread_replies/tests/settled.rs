//! A ruling's comment once someone other than the maintainer, replying in its
//! thread, has settled it

use super::*;

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
