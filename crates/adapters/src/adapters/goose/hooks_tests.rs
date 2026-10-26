//! A goose worker's fence: the plugin written for its call, what goose is
//! offered and what the sandbox leaves it

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command as Process;

use serde::Deserialize;
use serde_json::Value;

use super::tests::{FRESH, FRESH_ID, OTHER_ID, URL, World, id, only_skills, stand_in, strings};
use super::*;
use crate::hooks::Hooks;
use crate::lease::door::SOCKET_VAR;
use crate::settings::{HookPoint, NonBlank, ProjectHook};

pub(super) fn worker(w: &World, session: Session) -> AgentCall {
    w.fenced(URL, Path::new("/k/kelpie"), session)
}

// Where goose reads the call's plugin from.
pub(super) fn plugin(w: &World) -> PathBuf {
    w.path("settings.goose/.agents/plugins/kelpie-fence")
}

pub(super) fn json_in(path: &Path) -> Value {
    let text = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    serde_json::from_str(&text).unwrap()
}

fn names(folder: &Path) -> Vec<String> {
    let mut found: Vec<String> = std::fs::read_dir(folder)
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    found.sort();
    found
}

pub(super) fn env_vars(call: &AgentCall) -> Vec<String> {
    let goose = GooseCli::on(&Base::default());
    let server = server(call).unwrap();
    let upstream = goose.upstream(server).unwrap();
    let files = Files::of(&call.settings);
    let vars = env_args(call, server, &files, &upstream, None);
    vars.into_iter().map(|v| v.into_string().unwrap()).collect()
}

pub(super) fn policy_of(w: &World, call: &AgentCall) -> Policy {
    let goose = GooseCli::on(&Base::default());
    let upstream = goose.upstream(server(call).unwrap()).unwrap();
    let files = Files::of(&call.settings);
    policy(
        call,
        &files,
        &upstream,
        &w.path("forwarder.sock"),
        Path::new("/nowhere/goose"),
        &[],
    )
}

pub(super) fn refused(result: Result<(), AgentError>) -> String {
    match result {
        Err(AgentError::Setup(why)) => why,
        other => panic!("not a refusal to set up: {other:?}"),
    }
}

#[test]
fn a_worker_call_gets_a_plugin_with_one_catch_all_pre_tool_use_hook_that_blocks_on_failure() {
    #[derive(Deserialize)]
    struct Config {
        plugins: BTreeMap<String, Entry>,
    }
    #[derive(Deserialize)]
    struct Entry {
        enabled: bool,
    }
    let w = World::new();
    let call = worker(&w, Session::New(id(FRESH_ID)));
    stand_in(&w, FRESH).prepare(&call).unwrap();

    let folder = plugin(&w);
    assert_eq!(names(&folder), ["hooks", "plugin.json"]);
    assert_eq!(names(folder.parent().unwrap()), ["kelpie-fence"]);
    assert_eq!(json_in(&folder.join("plugin.json"))["name"], "kelpie-fence");

    let hooks = json_in(&folder.join("hooks/hooks.json"));
    let events = hooks["hooks"].as_object().unwrap();
    assert_eq!(events.keys().collect::<Vec<_>>(), ["PreToolUse"]);
    let rules = events["PreToolUse"].as_array().unwrap();
    assert_eq!(rules.len(), 1);
    assert!(rules[0].get("matcher").is_none(), "{}", rules[0]);
    let actions = rules[0]["hooks"].as_array().unwrap();
    assert_eq!(actions.len(), 1);
    assert_eq!(actions[0]["type"], "command");
    assert_eq!(actions[0]["on_failure"], "block");
    assert!(actions[0]["command"].as_str().unwrap().contains("guard"));

    // The config goose reads has the plugin on, by the path goose finds it at.
    let text = std::fs::read_to_string(w.path("settings.goose/config/config.yaml")).unwrap();
    let config: Config = serde_saphyr::from_str(&text).unwrap();
    let on: Vec<(&String, bool)> = config.plugins.iter().map(|(k, e)| (k, e.enabled)).collect();
    assert_eq!(on, [(&folder.to_string_lossy().into_owned(), true)]);

    // A call that is not a worker's gets no plugin.
    let other = World::new();
    let research = other.call(URL, Role::Researcher, Session::New(id(FRESH_ID)));
    stand_in(&other, FRESH).prepare(&research).unwrap();
    assert!(!plugin(&other).exists());
}

#[test]
fn the_hook_carries_the_guard_arguments_the_worker_s_fence_names() {
    let w = World::new();
    let mut call = worker(&w, Session::New(id(FRESH_ID)));
    // A stand-in kelpie that keeps the arguments it was run with.
    let kelpie = w.path("kelpie-bin");
    let seen = w.path("seen");
    let script = format!("#!/bin/sh\nprintf '%s\\n' \"$@\" > {}\n", seen.display());
    crate::test::write_script(&kelpie, &script);
    let list = [ProjectHook {
        point: HookPoint::Command,
        command: NonBlank::try_from("check-words".to_owned()).unwrap(),
    }];
    let hooks = Hooks::new("koji", "acme/koji", Path::new("/k/wt"), &list);
    let fence = call.reach.fence.as_mut().unwrap();
    fence.guard.kelpie = kelpie;
    fence.guard.folders = vec![PathBuf::from("/k/home"), PathBuf::from("/k/o'brien")];
    fence.guard.hooks = hooks.clone();
    fence.guard.base = "v2".to_owned();
    let guard = fence.guard.clone();
    stand_in(&w, FRESH).prepare(&call).unwrap();

    // goose runs the command with `sh -c`.
    let hooks_json = json_in(&plugin(&w).join("hooks/hooks.json"));
    let command = hooks_json["hooks"]["PreToolUse"][0]["hooks"][0]["command"]
        .as_str()
        .unwrap();
    let ran = Process::new("sh").args(["-c", command]).status().unwrap();
    assert!(ran.success());
    let seen = std::fs::read_to_string(&seen).unwrap();
    assert_eq!(
        seen.lines().collect::<Vec<_>>(),
        [
            "guard".to_owned(),
            guard.git_common_dir.display().to_string(),
            guard.worktree.display().to_string(),
            "--folder=/k/home".to_owned(),
            "--folder=/k/o'brien".to_owned(),
            hooks.flag().unwrap(),
            "--base=v2".to_owned(),
            format!("--goose={}", guard.build.display()),
        ]
    );
}

#[test]
fn a_worker_is_offered_developer_and_nothing_else_in_auto_mode() {
    let w = World::new();
    let new = worker(&w, Session::New(id(FRESH_ID)));
    let resumed = worker(&w, Session::Resume(id(FRESH_ID)));

    assert_eq!(
        strings(&new),
        [
            "run",
            "--no-profile",
            "--output-format",
            "stream-json",
            "--text=go",
            "--with-builtin",
            "developer",
            "--with-builtin",
            "skills",
            "--name",
            FRESH_ID,
        ]
    );
    // A resume offers the builtin again: goose keeps one, and ends a run that
    // is given an extension of the same name twice.
    assert_eq!(
        strings(&resumed)[5..],
        [
            "--with-builtin",
            "developer",
            "--with-builtin",
            "skills",
            "--resume",
            "--name",
            FRESH_ID
        ]
    );
    for call in [&new, &resumed] {
        assert!(!strings(call).iter().any(|a| a == "--with-extension"));
    }

    let vars = env_vars(&new);
    assert!(vars.contains(&"GOOSE_MODE=auto".to_owned()), "{vars:?}");
    assert_eq!(
        vars.iter().filter(|v| v.starts_with("GOOSE_MODE=")).count(),
        1
    );
    assert!(
        vars.contains(&r#"CONTEXT_FILE_NAMES=["AGENTS.md","CLAUDE.md"]"#.to_owned()),
        "{vars:?}"
    );

    // A researcher is offered no tool and loads no repo file.
    let research = w.call(URL, Role::Researcher, Session::New(id(OTHER_ID)));
    let vars = env_vars(&research);
    assert!(vars.contains(&"GOOSE_MODE=chat".to_owned()), "{vars:?}");
    assert!(!vars.iter().any(|v| v.starts_with("CONTEXT_FILE_NAMES=")));
    assert!(only_skills(&research));
}

#[test]
fn a_workers_shell_reaches_the_lease_door_and_the_build_folder_as_pi_s_bash_does() {
    let w = World::new();
    let call = worker(&w, Session::New(id(FRESH_ID)));
    let fence = call.reach.fence.clone().unwrap();

    let vars = env_vars(&call);
    assert!(!fence.env.is_empty());
    for (name, value) in &fence.env {
        assert!(
            vars.contains(&format!("{name}={value}")),
            "{name}: {vars:?}"
        );
    }
    assert!(vars.contains(&format!("{SOCKET_VAR}=/k/dog/lease.sock")));
    let policy = policy_of(&w, &call);
    assert!(policy.sockets.contains(&PathBuf::from("/k/dog/lease.sock")));
    assert!(
        policy.write.contains(&w.path("build")),
        "{:?}",
        policy.write
    );
    assert!(policy.write.contains(&w.path("wt")), "{:?}", policy.write);
}

#[test]
fn the_roots_plugin_and_settings_folders_are_not_writable_to_the_call() {
    let w = World::new();
    let call = worker(&w, Session::New(id(FRESH_ID)));
    let policy = policy_of(&w, &call);
    let root = w.path("settings.goose");

    // Nothing the call may write is, holds or sits under what goose loads
    // its plugins and settings from.
    for guarded in [
        root.clone(),
        root.join(".agents"),
        root.join(".agents/plugins"),
        root.join(".agents/plugins/kelpie-fence"),
        root.join(".agents/plugins/kelpie-fence/hooks/hooks.json"),
        root.join(".config"),
        root.join(".config/goose/settings.json"),
        root.join("config"),
        root.join("config/config.yaml"),
    ] {
        let writable = policy.write.iter().find(|p| guarded.starts_with(p));
        assert_eq!(writable, None, "{guarded:?} is writable");
    }
    // Its own data and state stay writable, and the root readable.
    for own in [root.join("data"), root.join("state")] {
        assert!(policy.write.contains(&own), "{own:?}");
    }
    assert!(policy.read.contains(&root), "{:?}", policy.read);
}

#[test]
fn a_worktree_with_project_plugins_or_goose_settings_is_refused_before_the_call() {
    let w = World::new();
    let call = worker(&w, Session::New(id(FRESH_ID)));
    let goose = stand_in(&w, FRESH);
    for (folder, file) in [
        (".agents/plugins/kelpie-fence", "plugin.json"),
        (".agents/plugins/other", "plugin.json"),
        (".config/goose", "settings.local.json"),
        (".config/goose", "settings.json"),
    ] {
        let found = w.path("wt").join(folder);
        std::fs::create_dir_all(&found).unwrap();
        std::fs::write(found.join(file), r#"{"disabledPlugins":["kelpie-fence"]}"#).unwrap();
        let top = w.path("wt").join(folder.split('/').next().unwrap());
        let why = refused(goose.prepare(&call));
        assert!(why.contains("whatever `--no-profile` says"), "{why}");
        assert!(!w.path("settings.goose").exists(), "nothing was made");
        std::fs::remove_dir_all(top).unwrap();
    }
    goose.prepare(&call).unwrap();
}

#[test]
fn a_plugin_besides_the_fence_in_the_roots_folder_refuses_the_call_and_the_fence_is_rewritten() {
    let w = World::new();
    let call = worker(&w, Session::New(id(FRESH_ID)));
    let goose = stand_in(&w, FRESH);
    goose.prepare(&call).unwrap();

    // Another plugin there would run its hooks unjudged.
    let other = plugin(&w).with_file_name("other");
    std::fs::create_dir_all(&other).unwrap();
    let why = refused(goose.prepare(&call));
    assert!(why.contains(other.to_str().unwrap()), "{why}");
    std::fs::remove_dir_all(&other).unwrap();

    // The fence's own files are written afresh for each call, a resume's too.
    let hooks = plugin(&w).join("hooks/hooks.json");
    std::fs::write(&hooks, "{}").unwrap();
    std::fs::write(plugin(&w).join("extra.txt"), "x").unwrap();
    let resumed = worker(&w, Session::Resume(id(FRESH_ID)));
    goose.prepare(&resumed).unwrap();
    assert!(json_in(&hooks)["hooks"]["PreToolUse"].is_array());
    assert_eq!(names(&plugin(&w)), ["hooks", "plugin.json"]);
}

#[test]
fn a_worker_with_no_fence_is_refused_and_a_command_built_for_one_is_a_researchers() {
    let w = World::new();
    let goose = stand_in(&w, FRESH);

    // A worker with no fence has no guard to write: ADR 0006.
    let mut unfenced = w.call(URL, Role::Worker, Session::New(id(FRESH_ID)));
    unfenced.tools = Tools::Work;
    let why = refused(goose.prepare(&unfenced));
    assert!(why.contains("outside the kelpie fence"), "{why}");
    assert!(!w.path("settings.goose").exists(), "nothing was made");
    // And a command built for one anyway is a researcher's: no tool, no auto mode.
    assert!(only_skills(&unfenced));
    let vars = env_vars(&unfenced);
    assert!(vars.contains(&"GOOSE_MODE=chat".to_owned()), "{vars:?}");
}

#[test]
fn a_representatives_call_with_no_fence_is_set_up_with_no_tool_and_in_chat_mode() {
    let w = World::new();
    let goose = stand_in(&w, FRESH);

    let mut call = w.call(URL, Role::Representative, Session::New(id(FRESH_ID)));
    call.tools = Tools::Answer;
    goose.prepare(&call).unwrap();

    assert!(only_skills(&call));
    let vars = env_vars(&call);
    assert!(vars.contains(&"GOOSE_MODE=chat".to_owned()), "{vars:?}");
}

#[test]
fn a_vote_resuming_the_workers_session_under_its_vote_settings_is_refused_and_finds_no_session() {
    // The runner resumes a worker's session for a vote under
    // `vote-settings-<n>.json`, with a retro's tools. pi and Codex keep their
    // sessions beside the settings path, so a resume under another finds none
    // and is `NoSession`. goose does the same, a retro's tools or a worker's.
    let w = World::new();
    let goose = stand_in(&w, FRESH);
    // The worker's first turn ran, so its own root holds its session.
    let first = worker(&w, Session::New(id(FRESH_ID)));
    goose.prepare(&first).unwrap();
    let files = Files::of(&first.settings);
    files.keep(&id(FRESH_ID), output::Spent::default()).unwrap();
    assert!(files.spent(&id(FRESH_ID)).unwrap().is_some());

    let mut vote = worker(&w, Session::Resume(id(FRESH_ID)));
    vote.settings = w.path("vote-settings-7.json");
    for tools in [Tools::Retro, Tools::Work] {
        vote.tools = tools;
        goose.prepare(&vote).unwrap();
        let err = goose.run(&vote, &Ending::default()).unwrap_err();
        assert_eq!(err, AgentError::NoSession(Harness::Goose, id(FRESH_ID)));
    }
}

#[test]
fn a_config_file_with_other_settings_keeps_them_and_has_the_plugin_on() {
    let w = World::new();
    let call = worker(&w, Session::New(id(FRESH_ID)));
    let goose = stand_in(&w, FRESH);
    goose.prepare(&call).unwrap();
    let config = w.path("settings.goose/config/config.yaml");
    let ours = plugin(&w).to_string_lossy().into_owned();

    // goose's own keys and another plugin's entry stay; ours is put back on.
    let older = format!(
        "GOOSE_PROVIDER: openai\nplugins:\n  /elsewhere/x:\n    enabled: false\n  {ours}:\n    enabled: false\n"
    );
    std::fs::write(&config, older).unwrap();
    goose.prepare(&call).unwrap();
    let kept = json_in(&config);
    assert_eq!(kept["GOOSE_PROVIDER"], "openai");
    assert_eq!(kept["plugins"]["/elsewhere/x"]["enabled"], false);
    assert_eq!(kept["plugins"][&ours]["enabled"], true);

    // A file that is no map is replaced by one that has the plugin on.
    std::fs::write(&config, "- not\n- a map\n").unwrap();
    goose.prepare(&call).unwrap();
    assert_eq!(json_in(&config)["plugins"][&ours]["enabled"], true);
}

#[test]
fn a_goose_settings_file_in_the_roots_own_folder_refuses_the_call() {
    let w = World::new();
    let call = worker(&w, Session::New(id(FRESH_ID)));
    let goose = stand_in(&w, FRESH);
    let folder = w.path("settings.goose/.config/goose");
    std::fs::create_dir_all(&folder).unwrap();
    for name in ["settings.json", "settings.local.json"] {
        let file = folder.join(name);
        std::fs::write(&file, r#"{"disabledPlugins":["kelpie-fence"]}"#).unwrap();
        let why = refused(goose.prepare(&call));
        assert!(why.contains(file.to_str().unwrap()), "{why}");
        std::fs::remove_file(&file).unwrap();
    }
    goose.prepare(&call).unwrap();
}

#[test]
fn a_triagers_review_call_is_set_up_in_an_empty_fence() {
    let w = World::new();
    let goose = stand_in(&w, FRESH);
    let mut call = worker(&w, Session::New(id(FRESH_ID)));
    call.role = Role::Triager;
    call.tools = Tools::Review;
    call.reach.fence.as_mut().unwrap().write.clear();

    goose.prepare(&call).unwrap();

    assert!(env_vars(&call).contains(&"GOOSE_MODE=auto".to_owned()));
    let hooks = json_in(&plugin(&w).join("hooks/hooks.json"));
    let command = hooks["hooks"]["PreToolUse"][0]["hooks"][0]["command"]
        .as_str()
        .unwrap();
    assert!(command.contains("'--read-only'"), "{command}");
}
