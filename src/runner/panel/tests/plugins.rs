//! Which calls get the plugins the repo turns on, when the project's `repo_setup` names roles

use super::{name_panel, parked_with, script_all, seen, settle};
use crate::ports::Role;
use crate::settings::AgentHarness;
use crate::test::Rig;

const PI: &str = "---\nrole: implementer\nharness: pi\nmodel: qwen3.8:27b\neffort: low\n\
                  url: http://box:11434/v1\ncontext: 65536\n---\n";

// Lands a marketplace `house` holding the plugin `style` on the default branch, and the repo's
// settings turning it on
fn land_style(rig: &Rig) {
    let market = r#"{"name":"house","plugins":[{"name":"style","source":"./style"}]}"#;
    rig.land("markets/house/.claude-plugin/marketplace.json", market);
    rig.land(
        "markets/house/style/skills/style/SKILL.md",
        "---\nname: style\n---\nRules.\n",
    );
    let settings = r#"{
        "extraKnownMarketplaces": {"house": {"source": {"source": "directory", "path": "markets/house"}}},
        "enabledPlugins": {"style@house": true}
    }"#;
    rig.land(".claude/settings.json", settings);
}

#[test]
fn roles_worker_leaves_every_other_calls_without_the_plugins() {
    let (rig, runner) = parked_with("koji", true, |rig| {
        land_style(rig);
        rig.edit_settings(|s| format!("{s}\n[app.dogs.kelpie.repo_setup]\nroles = [\"worker\"]\n"));
    });
    script_all(&rig, true);
    settle(&rig, &runner);
    let plugin = rig.paths().skills.join("repo/style");
    for role in [
        Role::Worker,
        Role::Reviewer,
        Role::Pm,
        Role::Advisor,
        Role::Researcher,
    ] {
        let calls = seen(&rig, role);
        assert!(!calls.is_empty(), "{role:?} made no call");
        for call in calls {
            let gets = call.call.plugin_dirs.contains(&plugin);
            assert_eq!(
                gets,
                role == Role::Worker,
                "{role:?}: {:?}",
                call.call.plugin_dirs
            );
        }
    }
}

// A pi call loads nothing from the worktree's `.claude`, so the repo's own files reach it as a
// plugin whether or not its worktree matches `main`.
#[test]
fn a_pi_call_gets_the_repos_own_claude_files_whatever_its_worktree_holds() {
    let (rig, runner, _) = Rig::parked_set("koji", |rig| {
        rig.with_app();
        rig.maintainer("octocat");
        rig.land(
            ".claude/skills/house/SKILL.md",
            "---\nname: house\n---\nRules.\n",
        );
        name_panel(rig, true);
        rig.write_agent("pi-box", PI);
        rig.implementers(&["pi-box"]);
    });
    script_all(&rig, true);
    settle(&rig, &runner);
    let own = rig.paths().skills.join("repo/kelpie-repo");
    let calls = seen(&rig, Role::Worker);
    assert!(!calls.is_empty(), "the worker made no call");
    for call in calls {
        assert!(matches!(call.call.harness, AgentHarness::Pi(_)));
        assert!(
            call.call.plugin_dirs.contains(&own),
            "{:?}",
            call.call.plugin_dirs
        );
        assert!(own.join("skills/house/SKILL.md").is_file());
    }
}
