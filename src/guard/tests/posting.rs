//! A worker posts no comment: its `gh` is the maintainer's login, whose comment
//! on its own pull request would read as the maintainer's answer to a ruling

use super::*;

const NO_COMMENTS: &str = "a worker posts no comment or review text";

fn anywhere(command: &str) -> Verdict {
    bash_in(Path::new("/x"), command)
}

#[test]
fn a_comment_or_a_review_with_a_body_is_refused_in_every_shape() {
    for command in [
        "gh pr comment 3 --body yes",
        "gh pr comment 3 -b yes",
        "gh pr comment 3 --body-file note.md",
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
    ] {
        let why = refusal(anywhere(command));
        assert!(why.contains(NO_COMMENTS), "{command}: {why}");
    }
}

#[test]
fn what_a_worker_reads_and_a_review_without_a_body_stay_allowed() {
    for command in [
        "gh pr view 3 --comments",
        "gh issue view 7 --comments",
        "gh pr checks 3",
        "gh pr diff 3",
        "gh pr review 3 --approve",
        "gh pr edit 3 --title 'fix: x'",
    ] {
        assert_eq!(anywhere(command), Verdict::Allow, "{command}");
    }
}
