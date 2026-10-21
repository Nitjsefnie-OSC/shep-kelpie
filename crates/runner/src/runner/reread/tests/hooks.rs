//! A changed `hooks` table reaching a running runner

use serde_json::json;

use crate::hooks::Role;
use crate::runner::{Runner, step};
use crate::settings::HookPoint;
use crate::test::{Rig, Scripted};

use super::settings_with;

const WORD: &str = "Acme";

const NOW: &str = "settings changed: hooks now in effect";

// What the project's forge port does with a comment naming the word.
fn comment_naming_the_word(rig: &Rig, runner: &Runner) -> Result<u64, String> {
    let repo = runner.remote.clone();
    let text = format!("built for {WORD}");
    rig.forge.open_pull_request(71, "kelpie/7", &[7]);
    runner
        .ports
        .forge
        .comment(&repo, 71, &text)
        .map_err(|e| e.to_string())
}

#[test]
fn a_publish_hook_added_runs_on_the_next_forge_write() {
    let rig = Rig::new("shep");
    let runner = rig.open().unwrap();
    assert!(comment_naming_the_word(&rig, &runner.lock().unwrap()).is_ok());
    rig.forbid_word(&["publish"], WORD);
    let mut runner = runner.lock().unwrap();

    let line = runner.reread(rig.settings(), rig.kelpie_settings());

    assert_eq!(line.unwrap().as_deref(), Some(NOW));
    assert_eq!(runner.settings().hooks.len(), 1);
    let refused = comment_naming_the_word(&rig, &runner).unwrap_err();
    assert!(refused.contains("`forbid-word.sh` refused"), "{refused}");
}

#[test]
fn a_hook_taken_away_no_longer_runs() {
    let rig = Rig::new("shep");
    rig.forbid_word(&["publish"], WORD);
    let runner = rig.open().unwrap();
    assert!(comment_naming_the_word(&rig, &runner.lock().unwrap()).is_err());
    let next = settings_with(&rig, |s| {
        let at = s.find("\n[[app.dogs.kelpie.hooks]]").unwrap();
        s[..at].to_owned()
    });
    let mut runner = runner.lock().unwrap();

    let line = runner.reread(next, rig.kelpie_settings());

    assert_eq!(line.unwrap().as_deref(), Some(NOW));
    assert!(runner.settings().hooks.is_empty());
    assert!(comment_naming_the_word(&rig, &runner).is_ok());
}

#[test]
fn the_next_worker_call_s_guard_runs_a_changed_write_or_command_hook() {
    let rig = Rig::new("shep");
    let runner = rig.open().unwrap();
    rig.forbid_word(&["write", "command"], WORD);
    runner
        .lock()
        .unwrap()
        .reread(rig.settings(), rig.kelpie_settings())
        .unwrap();
    rig.ask(&runner, "add", Some("7"));
    rig.claude.script([Scripted::Say("done")]);

    step(&runner).unwrap();

    let [seen] = rig.claude.seen().try_into().unwrap();
    let fence = seen
        .call
        .reach
        .fence
        .as_deref()
        .expect("a worker is fenced");
    let hooks = &fence.guard.hooks;
    for (point, details) in [
        (
            HookPoint::Write,
            json!({ "path": "a.txt", "content": WORD }),
        ),
        (
            HookPoint::Command,
            json!({ "command": format!("echo {WORD}") }),
        ),
    ] {
        let refused = hooks.check(point, Role::Worker, None, details);
        assert!(refused.is_err(), "{point:?} let the word through");
    }
}

#[test]
fn a_hook_that_does_not_resolve_refuses_the_reread_and_the_old_hooks_stay() {
    let rig = Rig::new("shep");
    rig.forbid_word(&["publish"], WORD);
    let runner = rig.open().unwrap();
    let next = settings_with(&rig, |s| {
        let hook =
            "[[app.dogs.kelpie.hooks]]\npoint = \"commit\"\ncommand = \"kelpie-no-such-check\"\n";
        format!("{s}\n{hook}")
    });
    let mut runner = runner.lock().unwrap();

    let err = runner.reread(next, rig.kelpie_settings()).unwrap_err();

    assert_eq!(
        err.to_string(),
        "setting `hooks`: `kelpie-no-such-check` does not resolve: kelpie-no-such-check is not there"
    );
    assert_eq!(runner.settings().hooks.len(), 1);
    assert!(!runner.hooks.get().has(HookPoint::Commit));
    assert!(
        comment_naming_the_word(&rig, &runner).is_err(),
        "the old hook still runs"
    );
}

#[test]
fn a_guard_hook_that_does_not_resolve_refuses_the_reread() {
    let rig = Rig::new("shep");
    let runner = rig.open().unwrap();
    let next = settings_with(&rig, |s| {
        let hook = "[[app.dogs.kelpie.worker.guard_hooks]]\nevent = \"PreToolUse\"\nmatcher = \"Bash\"\ncommand = \"kelpie-no-such-guard\"\n";
        format!("{s}\n{hook}")
    });
    let mut runner = runner.lock().unwrap();

    let err = runner.reread(next, rig.kelpie_settings()).unwrap_err();

    assert_eq!(
        err.to_string(),
        "setting `worker.guard_hooks`: `kelpie-no-such-guard` does not resolve: kelpie-no-such-guard is not there"
    );
    assert!(runner.settings().worker.guard_hooks.is_empty());
}
