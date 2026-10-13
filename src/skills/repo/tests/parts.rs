//! A repo plugin's agents, commands and hooks load beside its skills, and `repo_setup.parts` narrows them

use std::fs;
use std::process::Command;
use std::sync::Mutex;

use serde_json::json;

use super::{call_of, directory, land_settings, marketplace};
use crate::ports::Role;
use crate::runner::{Runner, step};
use crate::test::Rig;

const PLUGIN: &str = "markets/house/plugins/style";

// A runner whose `repo_setup` table holds `keys`, on a repo that turns on `style@house`, a
// plugin with a skill, an agent, a command and a hook, with a worker's first turn run
fn with_parts(keys: &str) -> (Rig, Mutex<Runner>) {
    let rig = Rig::new("shep");
    rig.edit_settings(|s| format!("{s}\n[app.dogs.kelpie.repo_setup]\n{keys}\n"));
    let dir = marketplace(&rig, "house", &[("style", "style")]);
    rig.land(
        &format!("{PLUGIN}/agents/checker.md"),
        "---\nname: checker\n---\nCheck.\n",
    );
    rig.land(&format!("{PLUGIN}/commands/lint.md"), "Lint it.\n");
    let hooks = json!({ "hooks": { "PreToolUse": [{
        "matcher": "Bash",
        "hooks": [{
            "type": "command",
            "command": "sh \"${CLAUDE_PLUGIN_ROOT}/hooks/note.sh\"",
        }],
    }] } });
    rig.land(&format!("{PLUGIN}/hooks/hooks.json"), &hooks.to_string());
    rig.land(&format!("{PLUGIN}/hooks/note.sh"), "echo ran > \"$NOTE\"\n");
    land_settings(
        &rig,
        &[("style@house", true)],
        &[("house", directory(&dir))],
    );
    let runner = rig.open().unwrap();
    rig.ask(&runner, "add", Some("7"));
    step(&runner).unwrap();
    (rig, runner)
}

#[test]
fn a_plugins_agent_command_and_hook_reach_the_workers_plugin_copy() {
    let (rig, _runner) = with_parts("");
    let plugin = rig.paths().skills.join("repo/style");
    assert!(call_of(&rig, Role::Worker).plugin_dirs.contains(&plugin));
    assert!(plugin.join("skills/style/SKILL.md").is_file());
    assert!(plugin.join("agents/checker.md").is_file());
    assert!(plugin.join("commands/lint.md").is_file());
    assert!(plugin.join("hooks/hooks.json").is_file());
}

#[test]
fn a_plugin_hook_runs_and_kelpies_guard_still_blocks() {
    let (rig, _runner) = with_parts("");
    let plugin = rig.paths().skills.join("repo/style");
    let hooks: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(plugin.join("hooks/hooks.json")).unwrap())
            .unwrap();
    let command = hooks["hooks"]["PreToolUse"][0]["hooks"][0]["command"]
        .as_str()
        .unwrap();
    let note = rig.home.path().join("note");
    let ran = Command::new("sh")
        .args(["-c", command])
        .env("CLAUDE_PLUGIN_ROOT", &plugin)
        .env("NOTE", &note)
        .status()
        .unwrap();
    assert!(ran.success());
    assert_eq!(fs::read_to_string(note).unwrap(), "ran\n");

    // The plugin's hooks sit in its own folder: the worker's settings file is kelpie's alone.
    let worker = rig.claude.seen().into_iter().next().unwrap();
    let guards = worker.settings["hooks"]["PreToolUse"].as_array().unwrap();
    assert!(
        guards.iter().any(|h| h["hooks"][0]["command"]
            .as_str()
            .is_some_and(|c| c.contains("'guard'"))),
        "{guards:?}"
    );
    assert!(!worker.settings.to_string().contains("note.sh"));
}

#[test]
fn parts_skills_copies_only_the_skills() {
    let (rig, runner) = with_parts("parts = [\"skills\"]");
    let plugin = rig.paths().skills.join("repo/style");
    assert!(plugin.join("skills/style/SKILL.md").is_file());
    for left_out in ["agents", "commands", "hooks"] {
        assert!(!plugin.join(left_out).exists(), "{left_out}");
    }
    let status = rig.ask(&runner, "status", None);
    assert_eq!(status["plugins"][0]["skills"], json!(["style"]));
}

#[test]
fn parts_without_skills_loads_a_plugin_for_its_agents_alone() {
    let (rig, runner) = with_parts("parts = [\"agents\"]");
    let plugin = rig.paths().skills.join("repo/style");
    assert!(plugin.join("agents/checker.md").is_file());
    assert!(!plugin.join("skills").exists());
    assert!(!plugin.join("hooks").exists());
    assert!(call_of(&rig, Role::Worker).plugin_dirs.contains(&plugin));
    let status = rig.ask(&runner, "status", None);
    assert_eq!(status["plugins"][0].get("skipped"), None);
}

#[test]
fn a_plugin_with_none_of_the_parts_asked_for_is_skipped_with_its_reason() {
    let rig = Rig::new("shep");
    rig.edit_settings(|s| format!("{s}\n[app.dogs.kelpie.repo_setup]\nparts = [\"agents\"]\n"));
    let dir = marketplace(&rig, "house", &[("style", "style")]);
    land_settings(
        &rig,
        &[("style@house", true)],
        &[("house", directory(&dir))],
    );
    let runner = rig.open().unwrap();
    let status = rig.ask(&runner, "status", None);
    let skipped = status["plugins"][0]["skipped"].as_str().unwrap();
    assert!(skipped.contains("agents"), "{skipped}");
    assert!(!rig.paths().skills.join("repo/style").exists());
}
