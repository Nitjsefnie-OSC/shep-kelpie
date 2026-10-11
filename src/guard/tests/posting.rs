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

const FLAG_FIRST: &str = "reads a gh command's group and verb by position";
const BUILT: &str = "whose words a shell builds or expands";

fn all_refused_for(part: &str, commands: &[&str]) {
    for command in commands {
        let why = refusal(anywhere(command));
        assert!(why.contains(part), "{command}: {why}");
    }
}

// gh takes an unknown flag before the subcommand with the next word as its
// value, so these ran `close` and `comment` while the guard read another verb.
#[test]
fn a_flag_before_the_group_or_the_verb_is_refused_but_the_repo() {
    all_refused_for(
        FLAG_FIRST,
        &[
            r#"gh pr --comment="101 yes" close 418"#,
            "gh pr --comment x close 418",
            "gh pr -c x close 418",
            "gh pr -b x comment 418",
            "gh pr --body x comment 418",
            "gh pr -b close 418",
            "gh --comment x pr close 418",
            "gh -b x pr comment 418",
            "gh -R acme/koji -b x pr comment 418",
            "gh pr -R acme/koji --comment x close 418",
            "gh pr --repo=acme/koji -b x comment 418",
            "gh issue --comment=x close 7",
            "gh pr -R acme/koji -F note.md comment 418",
            "gh --help pr",
            "gh pr --web view 3",
            "gh pr -- close 3",
        ],
    );
}

#[test]
fn the_repo_may_come_before_the_group_or_the_verb() {
    for command in [
        "gh pr -R acme/koji view 3",
        "gh pr --repo acme/koji view 3",
        "gh pr --repo=acme/koji view 3",
        "gh pr -Racme/koji view 3",
        "gh -R acme/koji pr view 3",
        "gh --repo acme/koji pr view 3",
        "gh --repo=acme/koji pr view 3",
        "gh -R acme/koji pr -R acme/koji view 3",
        "gh pr view 3 --repo acme/koji",
    ] {
        assert_eq!(anywhere(command), Verdict::Allow, "{command}");
    }
}

// What a shell builds is not what the guard read: the words reach gh expanded.
#[test]
fn a_gh_word_a_shell_builds_or_expands_is_refused_in_every_shape() {
    all_refused_for(
        BUILT,
        &[
            r#"gh pr close 3 $'--comment' x"#,
            "gh pr close 3 --comment$IFS x",
            "gh pr close 3 --comment${IFS}x",
            "F=--comment; gh pr close 3 $F x",
            "gh pr close 3 $F",
            r#"gh pr $'merge' 3"#,
            "gh pr $(echo merge) 3",
            "gh pr `echo merge` 3",
            "gh pr close 3 `echo --comment` x",
            "gh pr close 3 {--comment,x}",
            "gh pr close 3 ~/x",
            "gh pr close 3 ~ x",
            "gh pr clos* 3",
            "gh pr clo?e 3",
            "gh pr clos[e] 3",
            "gh pr close 3 --comme?t x",
            "gh pr close 3 --commen[t] x",
            "gh pr close 3 --comm* x",
            "gh pr view $N",
            "gh pr view 3 --json $FIELDS",
            "gh pr list --search 'is:open [WIP]'",
            "gh pr view 3 --json number --jq '.[0]'",
            "gh issue close 7 --reason $R",
            "gh pr edit 3 --add-label $L",
        ],
    );
}

// A shell splits an unquoted `$X` into words, so an unquoted value can hold
// flags, and a glob can build a flag from a file named for it.
#[test]
fn an_unquoted_body_or_title_value_is_a_word_like_any_other() {
    all_refused_for(
        BUILT,
        &[
            "gh pr edit 3 --body x$IFS--add-label${IFS}CodeRabbit",
            "gh pr edit 3 --body=x$IFS--add-label${IFS}CodeRabbit",
            "gh pr edit 3 -bx$IFS--add-label${IFS}CodeRabbit",
            "gh pr edit 3 --title $T",
            "gh pr edit 3 --title=$T",
            "gh pr edit 3 -t $T",
            "gh pr edit 3 --body $HOME",
            "gh pr edit 3 --body=$HOME",
            "gh pr edit 3 --body *",
            "gh pr edit 3 --body $'x'",
        ],
    );
}

#[test]
fn a_glob_a_shell_expands_to_a_flag_is_refused() {
    all_refused_for(
        BUILT,
        &[
            "gh pr close 3 --comment!(x) hello",
            "gh pr close 3 --comment?(x) hello",
            "gh pr close 3 --comment@(|x) hello",
            "gh pr close 3 --comment+(x) hello",
            "gh pr close 3 --comment*(x) hello",
            "gh pr close 3 --comment^x hello",
            "gh pr close 3 --comment<1-> hello",
            "gh pr close 3 --commen# hello",
        ],
    );
}

#[test]
fn a_word_outside_the_safe_characters_is_refused_even_when_quoted() {
    all_refused_for(
        BUILT,
        &[
            "gh pr view 3 --json 'number title'",
            "gh pr view 3 --json 'number!'",
            "gh pr edit 3 --add-label 'good first issue'",
            r#"gh pr edit 3 --milestone "v 1""#,
            r"gh pr view 3 --json a\ b",
            "gh pr view 3 --json 'a'$X",
        ],
    );
}

#[test]
fn a_quoted_body_or_title_value_may_hold_text_in_every_spelling() {
    for command in [
        "gh pr create --title 'fix: x' --body 'costs $5'",
        "gh pr create --title 'fix: x' --body \"costs \\$5\"",
        "gh pr create --title='fix: x' --body='costs $5'",
        "gh pr create --title 'fix: x' --body=\"costs 5\"",
        "gh pr create -t 'fix: x' -b 'costs $5'",
        "gh pr create -t'fix: x' -b'a $5'",
        "gh pr create --title 'fix: x' --body 'a (b) | c; d > e'",
        "gh pr create --title 'fix: x' --body-file - <<'EOF'\ncosts $5 (really) *\nEOF",
        "gh pr create --title 'fix: x' --body \"$(cat <<'EOF'\ncosts $5 (really) *\nEOF\n)\"",
    ] {
        assert_eq!(anywhere(command), Verdict::Allow, "{command}");
    }
}

// The one place text goes: a quoted `--body` or `--title` value.
#[test]
fn a_body_or_a_title_may_hold_what_a_shell_would_expand() {
    for command in [
        "gh pr create --title 'fix: {x} ~ * ? [y] $z' --body 'costs $5, see ~/a {b} *.rs `c`'",
        "gh pr create --title='fix: $x' --body='a $b'",
        "gh pr create -t 'fix: $x' -b 'a $b'",
        "gh pr create -t'fix: $x' -b'a $b'",
        "gh pr edit 3 --body \"$(cat <<'EOF'\nanother ~ $ { *\nEOF\n)\"",
        "gh issue create --title 'fix: x' --body \"a $HOME b\"",
    ] {
        let verdict = anywhere(command);
        // A leak check may still refuse a path in the text; the words are not why.
        if let Verdict::Refuse(why) = &verdict {
            assert!(!why.contains(BUILT), "{command}: {why}");
        }
    }
}

#[test]
fn what_a_worker_reads_and_a_close_or_review_without_text_stay_allowed() {
    for command in [
        "gh pr view 3 --json number,title,headRefName 2>/dev/null",
        "gh pr list --search is:open --limit 5",
        "gh pr view 3 -R acme/koji",
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
