//! The repo's own `.claude` files, read from the default branch, reach the calls that run outside a worktree of them

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::json;

use super::{parked_with, script_all, seen, settle};
use crate::confine::Verdict;
use crate::guard::{self, Checkout};
use crate::ports::Role;
use crate::test::Rig;

// Lands the repo's own skill, agent, command and hook on the default branch, and a plugin `style`
// from a marketplace `house` where `plugin`, all in the one `.claude/settings.json`
fn land_own(rig: &Rig, plugin: bool) {
    rig.land(
        ".claude/skills/house/SKILL.md",
        "---\nname: house\n---\nRules.\n",
    );
    rig.land(
        ".claude/agents/checker.md",
        "---\nname: checker\n---\nCheck.\n",
    );
    rig.land(".claude/commands/lint.md", "Lint it.\n");
    let mut settings = json!({ "hooks": { "PreToolUse": [{
        "matcher": "Bash",
        "hooks": [{ "type": "command", "command": "echo ran > \"$NOTE\"" }],
    }] } });
    if plugin {
        let market = r#"{"name":"house","plugins":[{"name":"style","source":"./style"}]}"#;
        rig.land("markets/house/.claude-plugin/marketplace.json", market);
        rig.land(
            "markets/house/style/skills/style/SKILL.md",
            "---\nname: style\n---\nRules.\n",
        );
        settings["extraKnownMarketplaces"] =
            json!({ "house": { "source": { "source": "directory", "path": "markets/house" } } });
        settings["enabledPlugins"] = json!({ "style@house": true });
    }
    rig.land(".claude/settings.json", &settings.to_string());
}

// A parked ruling on a repo with the own files (and the plugin, where `plugin`) under `keys` of
// `repo_setup`, with every call of the panel made
fn settled(keys: &str, plugin: bool) -> Rig {
    let (rig, runner) = parked_with("koji", true, |rig| {
        land_own(rig, plugin);
        rig.edit_settings(|s| format!("{s}\n[app.dogs.kelpie.repo_setup]\n{keys}\n"));
    });
    script_all(&rig, true);
    settle(&rig, &runner);
    rig
}

fn own_plugin(rig: &Rig) -> PathBuf {
    rig.paths().skills.join("repo/kelpie-repo")
}

#[test]
fn the_default_branchs_files_reach_the_project_manager_the_advisor_and_the_researcher() {
    let rig = settled("", false);
    let plugin = own_plugin(&rig);
    for part in [
        "skills/house/SKILL.md",
        "agents/checker.md",
        "commands/lint.md",
        "hooks/hooks.json",
    ] {
        assert!(plugin.join(part).is_file(), "{part}");
    }
    for role in [Role::Pm, Role::Advisor, Role::Researcher] {
        let calls = seen(&rig, role);
        assert!(!calls.is_empty(), "{role:?} made no call");
        for call in calls {
            assert!(
                call.call.plugin_dirs.contains(&plugin),
                "{role:?}: {:?}",
                call.call.plugin_dirs
            );
        }
    }
}

#[test]
fn a_call_in_a_worktree_that_matches_main_loads_them_from_there_and_not_twice() {
    let rig = settled("", false);
    let plugin = own_plugin(&rig);
    for role in [Role::Worker, Role::Reviewer] {
        let calls = seen(&rig, role);
        assert!(!calls.is_empty(), "{role:?} made no call");
        for call in calls {
            assert!(
                !call.call.plugin_dirs.contains(&plugin),
                "{role:?}: {:?}",
                call.call.plugin_dirs
            );
            for part in [
                "skills/house/SKILL.md",
                "agents/checker.md",
                "commands/lint.md",
            ] {
                let file = call.call.cwd.join(".claude").join(part);
                assert!(file.is_file(), "{role:?} {}", file.display());
            }
        }
    }
}

#[test]
fn own_false_leaves_the_files_out_and_keeps_a_plugin() {
    let rig = settled("own = false", true);
    assert!(!own_plugin(&rig).exists());
    let style = rig.paths().skills.join("repo/style");
    for call in seen(&rig, Role::Advisor) {
        assert!(
            call.call.plugin_dirs.contains(&style),
            "{:?}",
            call.call.plugin_dirs
        );
        assert!(!call.call.plugin_dirs.contains(&own_plugin(&rig)));
    }
}

#[test]
fn roles_worker_leaves_the_advisor_without_them() {
    let rig = settled("roles = [\"worker\"]", true);
    let calls = seen(&rig, Role::Advisor);
    assert!(!calls.is_empty());
    for call in calls {
        assert!(
            call.call
                .plugin_dirs
                .iter()
                .all(|dir| dir.strip_prefix(rig.paths().skills.join("repo")).is_err()),
            "{:?}",
            call.call.plugin_dirs
        );
    }
}

#[test]
fn parts_skills_leaves_the_agents_commands_and_hooks_out() {
    let rig = settled("parts = [\"skills\"]", false);
    let plugin = own_plugin(&rig);
    assert!(plugin.join("skills/house/SKILL.md").is_file());
    for left_out in ["agents", "commands", "hooks"] {
        assert!(!plugin.join(left_out).exists(), "{left_out}");
    }
}

// The commands of `hooks`, a `{ event: [{ matcher?, hooks: [{ command }] }] }` object, that Claude
// Code runs before a `Bash` call
fn bash_hooks(hooks: &serde_json::Value) -> Vec<String> {
    bash_hooks_of(hooks, "Bash")
}

// The same for a call of `tool`
fn bash_hooks_of(hooks: &serde_json::Value, tool: &str) -> Vec<String> {
    hooks["PreToolUse"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|entry| {
            entry["matcher"]
                .as_str()
                .is_none_or(|m| m.split('|').any(|name| name == tool))
        })
        .flat_map(|entry| entry["hooks"].as_array().into_iter().flatten())
        .filter_map(|hook| hook["command"].as_str().map(str::to_owned))
        .collect()
}

#[test]
fn the_repos_hook_runs_and_kelpies_guard_still_blocks_what_it_blocks() {
    let rig = settled("", false);
    let repos: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(own_plugin(&rig).join("hooks/hooks.json")).unwrap(),
    )
    .unwrap();
    let worker = seen(&rig, Role::Worker).into_iter().next().unwrap();
    let repos = bash_hooks(&repos["hooks"]);
    let kelpies = bash_hooks(&worker.settings["hooks"]);
    assert_eq!(repos.len(), 1, "{repos:?}");

    let note = rig.home.path().join("note");
    let ran = Command::new("sh")
        .args(["-c", &repos[0]])
        .env("NOTE", &note)
        .status()
        .unwrap();
    assert!(ran.success());
    assert_eq!(fs::read_to_string(note).unwrap(), "ran\n");

    let guards: Vec<_> = kelpies.iter().filter(|c| c.contains("'guard'")).collect();
    assert_eq!(guards.len(), 1, "{kelpies:?}");
    // `'kelpie' 'guard' '<git dir>' '<worktree>' '--folder=<folder>'...`
    let words: Vec<String> = guards[0]
        .trim_matches('\'')
        .split("' '")
        .map(str::to_owned)
        .collect();
    let (git_dir, worktree) = (Path::new(&words[2]), Path::new(&words[3]));
    let checkout = || Checkout {
        git_common_dir: git_dir,
        worktree,
        base: "main",
    };
    let local = || guard::local_paths(None, &words[4..]).unwrap();
    let merge = json!({
        "tool_name": "Bash",
        "cwd": worker.call.cwd,
        "tool_input": { "command": "gh pr merge 5" },
    })
    .to_string();
    let verdict = guard::judge(merge.as_bytes(), None, local(), checkout());
    assert!(matches!(verdict, Verdict::Refuse(_)), "{verdict:?}");

    // The repo's hook is in the plugin's own folder, never in kelpie's settings file.
    assert!(!worker.settings.to_string().contains("NOTE"));

    // The project manager gets the plugin, and its settings file still holds kelpie's hook.
    let pm = seen(&rig, Role::Pm).into_iter().next().unwrap();
    assert!(pm.call.plugin_dirs.contains(&own_plugin(&rig)));
    let confine = bash_hooks_of(&pm.settings["hooks"], "Edit");
    assert!(
        confine.iter().any(|c| c.contains("'confine'")),
        "{confine:?}"
    );
    assert!(!pm.settings.to_string().contains("NOTE"));
}
