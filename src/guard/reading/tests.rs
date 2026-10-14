use std::path::Path;

use serde_json::json;

use super::*;
use crate::settings::{HookPoint, NonBlank, ProjectHook};

fn judge(command: &str) -> Verdict {
    judge_with_hooks(command, &Hooks::default())
}

fn judge_with_hooks(command: &str, hooks: &Hooks) -> Verdict {
    let call = json!({
        "tool_name": "Bash",
        "cwd": "/k/acme/wt/one",
        "tool_input": { "command": command },
    });
    judge_reading(call.to_string().as_bytes(), hooks)
}

#[track_caller]
fn refused(command: &str, why: &str) {
    match judge(command) {
        Verdict::Refuse(message) => {
            assert!(message.contains(why), "{command}: {message}\nwanted: {why}");
        }
        Verdict::Allow => panic!("{command} was allowed, wanted a refusal naming: {why}"),
    }
}

#[track_caller]
fn allowed(command: &str) {
    assert_eq!(judge(command), Verdict::Allow, "{command}");
}

#[test]
fn cat_is_allowed_as_plain_words() {
    allowed("cat src/lib.rs");
}

#[test]
fn head_is_allowed_as_plain_words() {
    allowed("head -n 40 src/lib.rs");
}

#[test]
fn tail_is_allowed_as_plain_words() {
    allowed("tail -n 40 src/lib.rs");
}

#[test]
fn ls_is_allowed_as_plain_words() {
    allowed("ls -la src");
}

#[test]
fn tree_is_allowed_as_plain_words() {
    allowed("tree -L 2 src");
}

#[test]
fn grep_is_allowed_as_plain_words() {
    allowed("grep -rn 'fn main$' src");
}

#[test]
fn rg_is_allowed_as_plain_words() {
    allowed("rg -n \"fn judge\" src");
}

#[test]
fn find_is_allowed_as_plain_words() {
    allowed("find src -name '*.rs' -print");
}

#[test]
fn wc_is_allowed_as_plain_words() {
    allowed("wc -l src/lib.rs");
}

#[test]
fn file_is_allowed_as_plain_words() {
    allowed("file src/lib.rs");
}

#[test]
fn stat_is_allowed_as_plain_words() {
    allowed("stat src/lib.rs");
}

#[test]
fn nl_is_allowed_as_plain_words() {
    allowed("nl -ba src/lib.rs");
}

#[test]
fn a_pipe_between_listed_commands_is_allowed() {
    allowed("grep -rn judge src | head -n 20 | wc -l");
    allowed("ls src && cat src/lib.rs");
}

#[test]
fn a_command_outside_the_list_is_refused_with_the_list() {
    for command in [
        "git log",
        "python -c 'print(1)'",
        "sh -c 'cat x'",
        "curl https://example.com",
        "rm -rf src",
        "find src -name '*.rs' | xargs cat",
        "FOO=1 cat src/lib.rs",
        "/bin/cat src/lib.rs",
        "grep x src | sort",
    ] {
        refused(
            command,
            "`cat`, `head`, `tail`, `ls`, `tree`, `grep`, `rg`, `find`, `wc`, `file`, `stat`, `nl`",
        );
    }
}

#[test]
fn redirects_substitution_and_exec_flags_are_refused() {
    refused("cat src/lib.rs > out.txt", "redirect");
    refused("cat src/lib.rs >> out.txt", "redirect");
    refused("cat <<< hello", "redirect");
    refused("cat < src/lib.rs", "redirect");
    refused("cat <(ls)", "redirect");
    refused("ls 2>&1", "redirect");
    refused("cat $(ls)", "`$`");
    refused("cat \"$HOME/x\"", "`$`");
    refused("cat `ls`", "`$`");
    refused("find . -exec cat {} ;", "`find -exec`");
    refused("find . -execdir cat {} ;", "`find -execdir`");
    refused("find . -ok cat {} ;", "`find -ok`");
    refused("find . -delete", "`find -delete`");
    refused("find . -fprint out.txt", "`find -fprint`");
    refused("find . -fprintf out.txt x", "`find -fprintf`");
    refused("find . -fls out.txt", "`find -fls`");
    allowed("rg --pre-glob '*.gz' x");
    refused("file --comp magic", "`file --compile`");
    refused("file --compile magic", "`file --compile`");
    refused("rg --pr=cat x", "`rg --pre`");
    refused("rg --pre ./script x", "`rg --pre`");
    refused("rg --pre=./script x", "`rg --pre`");
    refused("rg --hostname-bin ./script x", "`rg --hostname-bin`");
    refused("tree -o out.txt", "`tree -o`");
    refused("file -C -m magic", "`file -C`");
}

#[test]
fn a_function_definition_is_refused() {
    refused("f() { cat x; }; f", "function");
}

#[test]
fn a_dollar_or_redirect_inside_single_quotes_is_a_pattern() {
    allowed("grep 'a>b' src");
    allowed("rg '\\$HOME' src");
}

#[test]
fn a_call_that_is_not_a_shell_command_is_left_alone() {
    let call = json!({ "tool_name": "apply_patch", "tool_input": {} });
    assert_eq!(
        judge_reading(call.to_string().as_bytes(), &Hooks::default()),
        Verdict::Allow
    );
}

#[test]
fn a_call_kelpie_cannot_read_is_refused() {
    assert!(matches!(
        judge_reading(&b"not json"[..], &Hooks::default()),
        Verdict::Refuse(_)
    ));
}

#[test]
fn the_projects_command_hooks_still_judge_an_allowed_command() {
    let dir = tempfile::tempdir().unwrap();
    let seen = dir.path().join("seen.json");
    let record = format!("cat > {}; echo 'no secrets' >&2; exit 2", seen.display());
    let hook = ProjectHook {
        point: HookPoint::Command,
        command: NonBlank::try_from(record).unwrap(),
    };
    let hooks = Hooks::new("koji", "acme/koji", Path::new("/"), &[hook]);

    let verdict = judge_with_hooks("cat .env", &hooks);

    match verdict {
        Verdict::Refuse(why) => assert!(why.contains("no secrets"), "{why}"),
        Verdict::Allow => panic!("the hook's refusal was dropped"),
    }
    let payload: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&seen).unwrap()).unwrap();
    assert_eq!(payload["point"], "command");
    assert_eq!(payload["cwd"], "/k/acme/wt/one");
    assert_eq!(payload["command"], "cat .env");

    // A command outside the list never reaches the hooks.
    std::fs::remove_file(&seen).unwrap();
    assert!(matches!(
        judge_with_hooks("git log", &hooks),
        Verdict::Refuse(_)
    ));
    assert!(!seen.exists());
}

#[test]
fn the_flag_is_taken_from_the_hooks_command_line() {
    let args = vec![READ_ONLY_FLAG.to_owned(), "--folder=/k".to_owned()];
    assert_eq!(reading_flag(&args), (true, vec!["--folder=/k".to_owned()]));
    assert_eq!(reading_flag(&[]), (false, vec![]));
}
