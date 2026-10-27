use std::path::Path;

use super::tests::{WORKER_ID, World, id, strings};
use crate::ports::Session;

// The project's hooks are not Claude Code's, so Codex gets them as `kelpie
// guard` runs them.
#[test]
fn the_projects_hooks_reach_a_codex_workers_guard() {
    let w = World::new();
    let mut call = w.fenced(Path::new("/k/kelpie"), Session::New(id(WORKER_ID)));
    let hook = crate::settings::ProjectHook {
        point: crate::settings::HookPoint::Commit,
        command: "check-words".to_owned().try_into().unwrap(),
    };
    let hooks = crate::hooks::Hooks::new("koji", "acme/koji", Path::new("/k/wt"), &[hook]);
    let flag = hooks.flag().unwrap();
    call.reach.fence.as_mut().unwrap().guard.hooks = hooks;

    let argv = strings(&call, None, None);

    let hooks = argv
        .iter()
        .find_map(|a| a.strip_prefix("hooks.PreToolUse="))
        .unwrap();
    let hooks: toml::Value = toml::from_str(&format!("h = {hooks}")).unwrap();
    let guard = hooks["h"][1]["hooks"][0]["command"].as_str().unwrap();
    assert!(guard.contains(&format!("'{flag}'")), "{guard}");
}

fn hook_at(point: crate::settings::HookPoint) -> crate::settings::ProjectHook {
    crate::settings::ProjectHook {
        point,
        command: "check-words".to_owned().try_into().unwrap(),
    }
}

// The hook list Codex is given for `event`, if it is given one.
pub(super) fn codex_hooks(argv: &[String], event: &str) -> Option<toml::Value> {
    let hooks = argv
        .iter()
        .find_map(|a| a.strip_prefix(&format!("hooks.{event}=")))?;
    let hooks: toml::Value = toml::from_str(&format!("h = {hooks}")).unwrap();
    Some(hooks["h"].clone())
}

// `write` hooks ride on `confine` for `apply_patch`, and `after_command` ones on
// a second guard entry for `PostToolUse`, as the guard does `PreToolUse`.
#[test]
fn the_projects_write_and_after_command_hooks_reach_a_codex_worker() {
    use crate::settings::HookPoint;
    let w = World::new();
    let mut call = w.fenced(Path::new("/k/kelpie"), Session::New(id(WORKER_ID)));
    let list = [
        hook_at(HookPoint::Write),
        hook_at(HookPoint::AfterCommand),
        hook_at(HookPoint::Command),
    ];
    let hooks = crate::hooks::Hooks::new("koji", "acme/koji", Path::new("/k/wt"), &list);
    let flag = hooks.flag().unwrap();
    call.reach.fence.as_mut().unwrap().guard.hooks = hooks;

    let argv = strings(&call, None, None);

    let pre = codex_hooks(&argv, "PreToolUse").unwrap();
    let confine = pre[0]["hooks"][0]["command"].as_str().unwrap();
    assert!(confine.contains("'confine'"), "{confine}");
    assert!(confine.ends_with(&format!("'{flag}'")), "{confine}");
    let post = codex_hooks(&argv, "PostToolUse").unwrap();
    let post = post.as_array().unwrap();
    assert_eq!(post.len(), 1);
    assert_eq!(post[0]["matcher"].as_str(), Some("^Bash$"));
    let after = post[0]["hooks"][0]["command"].as_str().unwrap();
    assert!(after.starts_with("'/k/kelpie' 'guard' "), "{after}");
    assert!(after.contains(&format!("'{flag}'")), "{after}");
    assert!(after.ends_with(" '--after'"), "{after}");
    assert!(!after.contains("--pin-folder"), "{after}");
}

#[test]
fn a_codex_worker_with_no_write_or_after_command_hook_gets_neither() {
    use crate::settings::HookPoint;
    let w = World::new();
    let mut call = w.fenced(Path::new("/k/kelpie"), Session::New(id(WORKER_ID)));
    let list = [hook_at(HookPoint::Commit)];
    let hooks = crate::hooks::Hooks::new("koji", "acme/koji", Path::new("/k/wt"), &list);
    call.reach.fence.as_mut().unwrap().guard.hooks = hooks;

    let argv = strings(&call, None, None);

    assert_eq!(codex_hooks(&argv, "PostToolUse"), None);
    let pre = codex_hooks(&argv, "PreToolUse").unwrap();
    let confine = pre[0]["hooks"][0]["command"].as_str().unwrap();
    assert!(!confine.contains("--hooks="), "{confine}");
}

#[test]
fn the_guard_is_told_the_base_branch_only_when_it_is_not_the_default() {
    let w = World::new();
    let guard_command = |base: &str| {
        let mut call = w.fenced(Path::new("/k/kelpie"), Session::New(id(WORKER_ID)));
        call.reach.fence.as_mut().unwrap().guard.base = base.to_owned();
        let argv = strings(&call, None, None);
        let hooks = argv
            .iter()
            .find_map(|a| a.strip_prefix("hooks.PreToolUse="));
        let hooks: toml::Value = toml::from_str(&format!("h = {}", hooks.unwrap())).unwrap();
        hooks["h"][1]["hooks"][0]["command"]
            .as_str()
            .unwrap()
            .to_owned()
    };
    assert!(guard_command("v2").contains(" '--base=v2'"));
    assert!(!guard_command("main").contains("--base"));
}

#[test]
fn a_codex_worker_reads_the_script_its_command_hook_runs_and_its_folder() {
    let w = World::new();
    let mut call = w.fenced(Path::new("/k/kelpie"), Session::New(id(WORKER_ID)));
    let folder = w.path("x");
    call.reach.fence.as_mut().unwrap().guard.hooks = crate::test::hook_beside_another(&folder);
    let files = super::Files::of(&call);
    let program = Path::new("/nowhere/codex");
    let policy = super::policy(&call, &w.path("login"), &files, program, &[]);
    crate::test::assert_reads_the_hook_and_its_folder(&policy, &folder);
}
