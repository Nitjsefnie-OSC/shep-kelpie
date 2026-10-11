use super::*;
use crate::settings::NonBlank;

fn hook(point: HookPoint, command: &str) -> ProjectHook {
    ProjectHook {
        point,
        command: NonBlank::try_from(command.to_owned()).unwrap(),
    }
}

fn hooks(list: &[ProjectHook]) -> Hooks {
    Hooks::new("koji", "acme/koji", Path::new("/"), list)
}

// Refuses, with exit 2, a payload that holds the word.
const NO_ACME: &str = "grep -q Acme && { echo 'names Acme' >&2; exit 2; }; exit 0";

fn publish(hooks: &Hooks, text: &str) -> Result<(), Refusal> {
    hooks.publish(Role::Kelpie, "the comment", text)
}

#[test]
fn a_hook_that_exits_zero_passes() {
    let hooks = hooks(&[hook(HookPoint::Publish, "exit 0")]);
    assert_eq!(publish(&hooks, "anything"), Ok(()));
}

#[test]
fn a_hook_that_exits_two_refuses_with_its_stderr_as_the_reason() {
    let hooks = hooks(&[hook(HookPoint::Publish, NO_ACME)]);

    let refusal = publish(&hooks, "built for Acme").unwrap_err();

    assert!(!refusal.failed);
    assert_eq!(
        refusal.to_string(),
        "the `publish` hook `grep` refused: names Acme"
    );
    assert_eq!(publish(&hooks, "built for a customer"), Ok(()));
}

#[test]
fn a_hook_that_exits_two_with_nothing_to_say_still_gives_a_reason() {
    let hooks = hooks(&[hook(HookPoint::Publish, "exit 2")]);

    let refusal = publish(&hooks, "x").unwrap_err();

    assert_eq!(
        refusal.to_string(),
        "the `publish` hook `exit` refused: it gave no reason"
    );
}

#[test]
fn a_hook_that_exits_otherwise_refuses_and_says_it_failed() {
    for (command, how) in [
        ("echo boom >&2; exit 1", "it exited with 1: boom"),
        ("exit 127", "it exited with 127"),
        ("kill -9 $$", "it was stopped by a signal"),
    ] {
        let hooks = hooks(&[hook(HookPoint::Publish, command)]);

        let refusal = publish(&hooks, "x").unwrap_err();

        assert!(refusal.failed, "{command}");
        assert!(
            refusal.to_string().contains("failed"),
            "{command}: {refusal}"
        );
        assert!(refusal.to_string().ends_with(how), "{command}: {refusal}");
    }
}

#[test]
fn a_hook_whose_program_is_missing_refuses_as_one_that_failed() {
    let hooks = hooks(&[hook(HookPoint::Publish, "kelpie-no-such-hook --strict")]);

    let refusal = publish(&hooks, "x").unwrap_err();

    assert!(refusal.failed);
    assert!(
        refusal.to_string().contains("`kelpie-no-such-hook` failed"),
        "{refusal}"
    );
}

#[test]
fn a_slow_hook_is_stopped_and_refuses_as_one_that_failed() {
    let hooks =
        hooks(&[hook(HookPoint::Publish, "sleep 30")]).with_timeout(Duration::from_millis(100));
    let started = Instant::now();

    let refusal = publish(&hooks, "x").unwrap_err();

    assert!(refusal.failed);
    assert!(refusal.to_string().contains("ran past 0s"), "{refusal}");
    assert!(started.elapsed() < Duration::from_secs(10));
}

#[test]
fn a_hook_reads_the_point_the_project_the_role_and_the_text() {
    let folder = tempfile::tempdir().unwrap();
    let seen = folder.path().join("seen.json");
    let command = format!("cat > {}", seen.display());
    let hooks = hooks(&[hook(HookPoint::Publish, &command)]);

    hooks
        .publish(Role::Worker, "the pull request's body", "Why.")
        .unwrap();

    let payload: Value = serde_json::from_slice(&std::fs::read(seen).unwrap()).unwrap();
    assert_eq!(
        payload,
        json!({
            "point": "publish",
            "project": "koji",
            "role": "worker",
            "repo": "acme/koji",
            "field": "the pull request's body",
            "text": "Why.",
        })
    );
}

#[test]
fn a_payload_larger_than_a_pipe_holds_does_not_hang_a_hook_that_reads_it_or_not() {
    let big = "y".repeat(1 << 20);

    for command in ["cat >/dev/null", "exit 0"] {
        let hooks = hooks(&[hook(HookPoint::Publish, command)]);
        assert_eq!(publish(&hooks, &big), Ok(()), "{command}");
    }
}

#[test]
fn a_hook_runs_only_at_its_own_point() {
    let hooks = hooks(&[hook(HookPoint::Commit, "exit 2")]);

    assert_eq!(publish(&hooks, "x"), Ok(()));
    assert!(
        hooks
            .check(HookPoint::Commit, Role::Worker, None, json!({}))
            .is_err()
    );
}

#[test]
fn the_first_hook_to_refuse_stops_the_rest() {
    let folder = tempfile::tempdir().unwrap();
    let ran = folder.path().join("ran");
    let hooks = hooks(&[
        hook(HookPoint::Publish, "exit 2"),
        hook(HookPoint::Publish, &format!("touch {}", ran.display())),
    ]);

    assert!(publish(&hooks, "x").is_err());
    assert!(!ran.exists());
}

#[test]
fn a_hook_runs_in_the_folder_it_is_given() {
    let folder = tempfile::tempdir().unwrap();
    std::fs::write(folder.path().join("marker"), "").unwrap();
    let hooks = hooks(&[hook(HookPoint::Commit, "test -f marker")]);

    let at =
        |folder: Option<&Path>| hooks.check(HookPoint::Commit, Role::Worker, folder, json!({}));

    assert!(at(Some(folder.path())).is_ok());
    assert!(at(None).is_err());
}

#[test]
fn the_hooks_cross_a_command_line_and_come_back() {
    let hooks = hooks(&[
        hook(HookPoint::Commit, NO_ACME),
        hook(HookPoint::Push, "exit 0"),
    ]);
    let flag = hooks.flag().unwrap();

    let (back, rest) = Hooks::take(&["--folder=/x".to_owned(), flag]).unwrap();

    assert_eq!(rest, ["--folder=/x"]);
    assert_eq!(back.list, hooks.list);
    assert!(back.has(HookPoint::Commit));
    assert!(!back.has(HookPoint::Handover));
}

#[test]
fn no_hooks_add_no_flag_and_an_unreadable_one_is_an_error() {
    assert_eq!(Hooks::default().flag(), None);
    let (none, _) = Hooks::take(&[]).unwrap();
    assert!(none.is_empty());
    let err = Hooks::take(&[format!("{FLAG}not json")]).unwrap_err();
    assert!(err.contains("cannot read its hooks"), "{err}");
}

#[test]
fn a_hook_is_named_by_its_program_and_never_by_a_path() {
    assert_eq!(program_of("~/bin/check-words --strict"), "check-words");
    assert_eq!(program_of("MODE=strict /opt/acme/check.sh"), "check.sh");
    assert_eq!(program_of("sh ~/hooks/guard.sh"), "sh");
}
