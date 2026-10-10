//! A worker posts no comment: its `gh` is the maintainer's login, whose comment
//! on its own pull request would read as the maintainer's answer to a ruling.
//! `close` and `reopen` post one with `--comment`, so only `create` and `edit`
//! may carry text.

use super::*;

const NO_COMMENTS: &str = "a worker posts no comment or review text";

fn anywhere(command: &str) -> Verdict {
    bash_in(Path::new("/x"), command)
}

fn all_refused(commands: &[&str]) {
    for command in commands {
        let why = refusal(anywhere(command));
        assert!(why.contains(NO_COMMENTS), "{command}: {why}");
    }
}

#[test]
fn a_comment_or_a_review_with_a_body_is_refused_in_every_shape() {
    all_refused(&[
        "gh pr comment 3 --body yes",
        "gh pr comment 3 -b yes",
        "gh pr comment 3 --body-file note.md",
        "gh pr comment 3",
        "gh pr comment --body yes",
        "gh pr -R acme/koji comment 3 --body yes",
        "gh --repo acme/koji pr comment 3 --body yes",
        "gh issue comment 7 --body yes",
        "gh issue comment 7 -F -",
        "gh pr review 3 --body yes",
        "gh pr review 3 --approve --body yes",
        "gh pr review 3 -c -b yes",
        "gh pr review 3 --request-changes --body-file note.md",
        "gh pr review 3 -F - <<'EOF'\nyes\nEOF",
        "bash -c 'gh pr comment 3 --body yes'",
        "env gh pr comment 3 --body yes",
    ]);
}

#[test]
fn closing_or_reopening_with_a_comment_is_refused_in_every_spelling() {
    all_refused(&[
        "gh pr close 418 --comment '100 yes'",
        "gh pr close 418 --comment='100 yes'",
        "gh pr close 418 -c '100 yes'",
        "gh pr close 418 -c'100 yes'",
        "gh pr close 418 -c100",
        "gh pr close 418 -dc '100 yes'",
        "gh pr close 418 --delete-branch --comment x",
        "gh pr close -R acme/koji 418 --comment x",
        "gh pr close --comment x 418",
        "gh pr reopen 418 --comment x",
        "gh pr reopen 418 --comment=x",
        "gh pr reopen 418 -c x",
        "gh pr reopen 418 -cx",
        "gh issue close 7 --comment x",
        "gh issue close 7 --comment=x",
        "gh issue close 7 -c x",
        "gh issue close 7 -cx",
        "gh issue close 7 --reason completed --comment x",
        "gh issue reopen 7 --comment x",
        "gh issue reopen 7 --comment=x",
        "gh issue reopen 7 -c x",
        "gh issue reopen 7 -cx",
        "bash -c 'gh issue close 7 -c x'",
    ]);
}

#[test]
fn any_verb_but_create_and_edit_that_carries_a_body_or_comment_is_refused() {
    // The verbs gh has today, and one it may add: the rule names no verb.
    for verb in [
        "close",
        "reopen",
        "merge",
        "lock",
        "unlock",
        "pin",
        "transfer",
        "frobnicate",
    ] {
        for group in ["pr", "issue"] {
            for flag in [
                "--comment x",
                "--comment=x",
                "-c x",
                "-cx",
                "--body x",
                "--body=x",
                "-b x",
                "-bx",
                "--body-file note.md",
                "--body-file=note.md",
                "-F note.md",
                "-Fnote.md",
            ] {
                let command = format!("gh {group} {verb} 3 {flag}");
                let found = judge_all(&command);
                assert!(!found, "{command} was allowed");
            }
        }
    }
}

// Whether the command is allowed.
fn judge_all(command: &str) -> bool {
    anywhere(command) == Verdict::Allow
}

#[test]
fn what_a_worker_reads_and_a_close_or_review_without_text_stay_allowed() {
    for command in [
        "gh pr view 3 --comments",
        "gh pr view 3 -c",
        "gh issue view 7 --comments",
        "gh issue view 7 -c",
        "gh pr list --search 'is:open'",
        "gh pr checks 3",
        "gh pr diff 3",
        "gh pr close 3",
        "gh pr close 3 --delete-branch",
        "gh issue close 7 --reason completed",
        "gh issue reopen 7",
        "gh pr review 3 --approve",
        "gh pr edit 3 --title 'fix: x'",
        "gh pr edit 3 --body 'a description'",
        "gh pr create --title 'fix: x' --body 'a description'",
        "gh issue create --title 'fix: x' --body 'a description'",
    ] {
        assert_eq!(anywhere(command), Verdict::Allow, "{command}");
    }
}
