//! Which calls get the plugins the repo turns on, when the project's `repo_setup` names roles

use super::{parked_with, script_all, seen, settle};
use crate::ports::Role;
use crate::test::Rig;

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
