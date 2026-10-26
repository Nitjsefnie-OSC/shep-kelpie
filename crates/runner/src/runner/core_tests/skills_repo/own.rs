//! The repo's own `.claude` files load from the default branch, as one more plugin

use std::fs;

use serde_json::json;

use crate::runner::step;
use crate::test::{Rig, Scripted};

const SKILL: &str = "---\nname: house\n---\nRules.\n";

#[test]
fn a_skill_a_workers_branch_adds_is_not_loaded() {
    let rig = Rig::new("shep");
    rig.land(".claude/skills/house/SKILL.md", SKILL);
    let runner = rig.open().unwrap();
    rig.ask(&runner, "add", Some("7"));
    rig.forge.open_pull_request(71, "kelpie/7", &[7]);
    rig.claude.script([Scripted::Push(
        ".claude/skills/sneaky/SKILL.md",
        "---\nname: sneaky\n---\nDo it.\n",
    )]);
    step(&runner).unwrap();
    // A runner that rereads its settings still reads the default branch.
    let (settings, kelpie) = (rig.settings(), rig.kelpie_settings());
    runner.lock().unwrap().reread(settings, kelpie).unwrap();
    step(&runner).unwrap();
    let plugin = rig.paths().skills.join("repo/kelpie-repo");
    assert!(plugin.join("skills/house/SKILL.md").is_file());
    assert!(!plugin.join("skills/sneaky").exists());
    let status = rig.ask(&runner, "status", None);
    assert_eq!(
        status["plugins"],
        json!([{ "plugin": ".claude", "skills": ["house"] }])
    );
}

#[test]
fn a_repo_with_no_claude_files_gets_no_plugin_and_status_lists_none() {
    let rig = Rig::new("shep");
    let runner = rig.open().unwrap();
    assert!(!rig.paths().skills.join("repo/kelpie-repo").exists());
    assert!(rig.ask(&runner, "status", None).get("plugins").is_none());
}

#[test]
fn a_skill_the_default_branch_gains_after_the_runner_started_loads_when_it_looks_again() {
    let rig = Rig::new("shep");
    let runner = rig.open().unwrap();
    rig.land(".claude/skills/house/SKILL.md", SKILL);
    super::fetch(&rig);
    let line = runner.lock().unwrap().reread_plugins();
    assert!(line.is_some(), "a new skill is a change");
    let plugin = rig.paths().skills.join("repo/kelpie-repo");
    assert!(plugin.join("skills/house/SKILL.md").is_file());
    assert_eq!(
        fs::read_to_string(plugin.join("skills/house/SKILL.md")).unwrap(),
        SKILL
    );
    assert_eq!(runner.lock().unwrap().reread_plugins(), None);
}

#[cfg(unix)]
#[test]
fn a_link_in_the_default_branchs_claude_folder_is_left_out() {
    let rig = Rig::new("shep");
    rig.land(".claude/skills/house/SKILL.md", SKILL);
    super::land_link(&rig, ".claude/skills/house/passwd", "/etc/passwd");
    let _runner = rig.open().unwrap();
    let plugin = rig.paths().skills.join("repo/kelpie-repo");
    assert!(plugin.join("skills/house/SKILL.md").is_file());
    assert!(!plugin.join("skills/house/passwd").exists());
}

#[test]
fn a_server_the_default_branch_gains_after_the_runner_started_loads_when_it_looks_again() {
    let rig = Rig::new("shep");
    let runner = rig.open().unwrap();
    rig.land(".mcp.json", r#"{"mcpServers":{"docs":{"command":"docs"}}}"#);
    super::fetch(&rig);
    assert!(runner.lock().unwrap().reread_plugins().is_some());
    rig.ask(&runner, "add", Some("7"));
    step(&runner).unwrap();
    let worker = super::call_of(&rig, crate::ports::Role::Worker);
    assert_eq!(
        worker.reach.mcp_servers.keys().collect::<Vec<_>>(),
        ["docs"]
    );
    assert_eq!(runner.lock().unwrap().reread_plugins(), None);
}

#[test]
fn a_launch_file_the_default_branch_gains_turns_the_dev_server_gauge_on_when_it_looks_again() {
    let rig = Rig::new("shep");
    let off = "[app.dogs.kelpie.browser]\nenabled = false\nheaded = false\ndomains = []";
    let on = "[app.dogs.kelpie.browser]\nenabled = true\nheaded = false\n\
              domains = [\"app.example.com\"]";
    rig.edit_settings(|s| s.replace(off, on));
    let runner = rig.open().unwrap();
    let gauge = || {
        let labels = [("feature", "dev_server")];
        rig.samples.value("kelpie_feature_enabled", &labels)
    };
    assert_eq!(gauge(), Some(0.0), "main has no launch file");

    let web = r#"{"configurations":[{"name":"web","runtimeExecutable":"npm","port":4421}]}"#;
    rig.land(".claude/launch.json", web);
    super::fetch(&rig);
    runner.lock().unwrap().reread_plugins();
    assert_eq!(gauge(), Some(1.0));
}

#[test]
fn a_skill_of_the_repo_names_the_copy_where_it_names_the_plugin_root() {
    let rig = Rig::new("shep");
    rig.land(
        ".claude/skills/house/SKILL.md",
        "---\nname: house\n---\nRead `${CLAUDE_PLUGIN_ROOT}/skills/house/rules.md`.\n",
    );
    rig.land(".claude/skills/house/rules.md", "one file per rule\n");
    rig.open().unwrap();
    let plugin = rig.paths().skills.join("repo/kelpie-repo");
    let skill = fs::read_to_string(plugin.join("skills/house/SKILL.md")).unwrap();
    assert!(!skill.contains("${CLAUDE_PLUGIN_ROOT}"), "{skill}");
    let named = skill
        .split('`')
        .nth(1)
        .unwrap_or_else(|| panic!("the skill names no path: {skill}"));
    assert_eq!(
        named,
        plugin.join("skills/house/rules.md").display().to_string()
    );
    assert!(plugin.join("skills/house/rules.md").is_file());
}
