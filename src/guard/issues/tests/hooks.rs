//! The project's `publish` hooks, run on each issue the issue writer files

use super::*;
use crate::settings::{HookPoint, NonBlank, ProjectHook};

fn hooks(command: &str) -> Hooks {
    let hook = ProjectHook {
        point: HookPoint::Publish,
        command: NonBlank::try_from(command.to_owned()).unwrap(),
    };
    Hooks::new("koji", "acme/koji", Path::new("/"), &[hook])
}

fn file(session: &Session, hooks: &Hooks, body: &str) -> Verdict {
    let labels = format!("--label {HUMAN} --label agent:sonnet-high");
    let call = json!({
        "tool_name": "Bash",
        "cwd": "/k/acme/issues/one",
        "tool_input": { "command": create(&labels, body) },
    });
    let home = Some(Path::new(HOME));
    judge_issues_with(
        call.to_string().as_bytes(),
        home,
        LocalPaths::new(home),
        hooks,
        &session.rules,
    )
}

#[test]
fn an_issue_the_publish_hook_refuses_is_not_filed() {
    let session = Session::new(HUMAN);
    let hooks = hooks("if grep -q Acme; then echo 'a private word' >&2; exit 2; fi");

    let body = format!("{BODY}\nFor Acme.\n");

    refused(
        file(&session, &hooks, &body),
        "the issue's body: the `publish` hook `if` refused: a private word",
    );
    assert_eq!(file(&session, &hooks, BODY), Verdict::Allow);
}

#[test]
fn the_hook_reads_the_issue_writer_as_the_role() {
    let session = Session::new(HUMAN);
    let dir = tempfile::tempdir().unwrap();
    let seen = dir.path().join("seen.json");
    let hooks = hooks(&format!("cat > {}", seen.display()));

    assert_eq!(file(&session, &hooks, BODY), Verdict::Allow);

    let payload: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&seen).unwrap()).unwrap();
    assert_eq!(payload["role"], "issue-writer");
    assert_eq!(payload["point"], "publish");
}

// A `command` hook sees each command the issue writer runs, under its own role.
#[test]
fn a_command_hook_judges_the_issue_writer_s_commands_too() {
    let session = Session::new(HUMAN);
    let dir = tempfile::tempdir().unwrap();
    let seen = dir.path().join("seen.json");
    let record = format!("cat > {}; exit 2", seen.display());
    let hook = ProjectHook {
        point: HookPoint::Command,
        command: NonBlank::try_from(record).unwrap(),
    };
    let hooks = Hooks::new("koji", "acme/koji", Path::new("/"), &[hook]);

    let verdict = file(&session, &hooks, BODY);

    refused(
        verdict,
        "this command: the `command` hook `cat` refused: it gave no reason",
    );
    let payload: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&seen).unwrap()).unwrap();
    assert_eq!(payload["point"], "command");
    assert_eq!(payload["role"], "issue-writer");
    assert_eq!(payload["cwd"], "/k/acme/issues/one");
    assert!(
        payload["command"]
            .as_str()
            .unwrap()
            .starts_with("gh issue create")
    );
}
