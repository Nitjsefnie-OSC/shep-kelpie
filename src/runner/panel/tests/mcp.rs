//! The MCP servers of the repo's plugins and of its `.mcp.json` on the default branch reach the
//! calls that get the plugins, in their MCP config and their allow-list

use std::fs;
use std::path::Path;

use serde_json::{Value, json};

use super::{parked_with, script_all, seen, settle};
use crate::ports::{Role, Tools};
use crate::runner::step;
use crate::test::{Rig, Scripted};

// Lands the plugin `style` of the marketplace `house` with an MCP server `search` in its
// `.mcp.json`, and the repo's own `.mcp.json` with a server `docs`
fn land_servers(rig: &Rig) {
    let market = r#"{"name":"house","plugins":[{"name":"style","source":"./style"}]}"#;
    rig.land("markets/house/.claude-plugin/marketplace.json", market);
    rig.land(
        "markets/house/style/skills/style/SKILL.md",
        "---\nname: style\n---\nRules.\n",
    );
    let plugin = json!({ "mcpServers": { "search": {
        "command": "${CLAUDE_PLUGIN_ROOT}/bin/search",
        "args": ["--stdio"],
    } } });
    rig.land("markets/house/style/.mcp.json", &plugin.to_string());
    rig.land("markets/house/style/bin/search", "#!/bin/sh\n");
    let settings = r#"{
        "extraKnownMarketplaces": {"house": {"source": {"source": "directory", "path": "markets/house"}}},
        "enabledPlugins": {"style@house": true}
    }"#;
    rig.land(".claude/settings.json", settings);
    let own = json!({ "mcpServers": { "docs": { "type": "http", "url": "https://docs.example.com/mcp" } } });
    rig.land(".mcp.json", &own.to_string());
}

// A parked ruling on a repo with those servers, under `keys` of `repo_setup`, with every call of
// the panel made
fn settled(keys: &str) -> Rig {
    let (rig, runner) = parked_with("koji", true, |rig| {
        land_servers(rig);
        rig.edit_settings(|s| format!("{s}\n[app.dogs.kelpie.repo_setup]\n{keys}\n"));
    });
    script_all(&rig, true);
    settle(&rig, &runner);
    rig
}

fn read(path: &Path) -> Value {
    serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap()
}

// The servers in the MCP config a call was given, by name
fn servers(call: &crate::test::Seen) -> Vec<String> {
    let file = call.call.settings.with_extension("mcp.json");
    if !file.exists() {
        return Vec::new();
    }
    let config = read(&file);
    config["mcpServers"]
        .as_object()
        .unwrap()
        .keys()
        .cloned()
        .collect()
}

// The rules a call's settings allow
fn allowed(call: &crate::test::Seen) -> Vec<String> {
    call.settings["permissions"]["allow"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|rule| rule.as_str().map(str::to_owned))
        .collect()
}

#[test]
fn a_plugins_server_and_the_default_branchs_reach_the_workers_the_reviewers_and_the_researchers_calls()
 {
    let rig = settled("");
    let mut checked = 0;
    for role in [Role::Worker, Role::Reviewer, Role::Researcher] {
        let calls = seen(&rig, role);
        assert!(!calls.is_empty(), "{role:?} made no call");
        for call in calls.iter().filter(|c| c.call.tools != Tools::Retro) {
            assert_eq!(servers(call), ["docs", "plugin_style_search"], "{role:?}");
            let allowed = allowed(call);
            for rule in ["mcp__docs", "mcp__plugin_style_search"] {
                assert!(allowed.contains(&rule.to_owned()), "{role:?}: {allowed:?}");
            }
            checked += 1;
        }
    }
    assert!(checked >= 3);
}

#[test]
fn a_plugins_server_runs_from_its_copy_in_kelpies_home() {
    let rig = settled("");
    let call = &seen(&rig, Role::Researcher)[0];
    let config = read(&call.call.settings.with_extension("mcp.json"));
    let server = &config["mcpServers"]["plugin_style_search"];
    let root = rig.paths().skills.join("repo/style/mcp-root");
    assert_eq!(server["command"], format!("{}/bin/search", root.display()));
    assert!(root.join("bin/search").is_file());
    assert!(
        call.call
            .plugin_dirs
            .contains(&rig.paths().skills.join("repo/style"))
    );
}

#[test]
fn a_call_that_reads_only_gets_no_server_it_could_write_with() {
    let rig = settled("");
    let votes: Vec<_> = (seen(&rig, Role::Worker).into_iter())
        .filter(|c| c.call.tools == Tools::Retro)
        .collect();
    assert!(!votes.is_empty());
    for vote in votes {
        assert!(vote.call.reach.mcp_servers.is_empty());
        assert!(!allowed(&vote).iter().any(|rule| rule.starts_with("mcp__")));
    }
}

#[test]
fn own_false_leaves_the_default_branchs_server_out_and_keeps_the_plugins() {
    let rig = settled("own = false");
    for call in seen(&rig, Role::Researcher) {
        assert_eq!(servers(&call), ["plugin_style_search"]);
        assert!(!allowed(&call).contains(&"mcp__docs".to_owned()));
    }
}

#[test]
fn plugins_false_leaves_the_plugins_server_out_and_keeps_the_default_branchs() {
    let rig = settled("plugins = false");
    for call in seen(&rig, Role::Researcher) {
        assert_eq!(servers(&call), ["docs"]);
    }
}

#[test]
fn parts_without_mcp_leaves_both_out() {
    let rig = settled("parts = [\"skills\", \"agents\", \"commands\", \"hooks\"]");
    for role in [Role::Worker, Role::Researcher, Role::Advisor] {
        for call in seen(&rig, role) {
            assert!(servers(&call).is_empty(), "{role:?}");
            assert!(call.call.reach.mcp_servers.is_empty());
            assert!(!allowed(&call).iter().any(|rule| rule.starts_with("mcp__")));
        }
    }
    assert!(!rig.paths().skills.join("repo/style/mcp-root").exists());
}

#[test]
fn a_reviewer_the_roles_leave_out_gets_neither_and_a_worker_gets_both() {
    let rig = settled("roles = [\"worker\", \"researcher\"]");
    for role in [Role::Reviewer, Role::Advisor, Role::Pm] {
        let calls = seen(&rig, role);
        assert!(!calls.is_empty(), "{role:?} made no call");
        for call in calls {
            assert!(servers(&call).is_empty(), "{role:?}");
            assert!(call.call.reach.mcp_servers.is_empty());
        }
    }
    let works: Vec<_> = (seen(&rig, Role::Worker).into_iter())
        .filter(|c| c.call.tools == Tools::Work)
        .collect();
    assert!(!works.is_empty());
    for call in works {
        assert_eq!(servers(&call), ["docs", "plugin_style_search"]);
    }
}

#[test]
fn a_server_a_workers_branch_adds_to_mcp_json_is_not_loaded() {
    let rig = Rig::new("shep");
    land_servers(&rig);
    let runner = rig.open().unwrap();
    rig.ask(&runner, "add", Some("7"));
    rig.forge.open_pull_request(71, "kelpie/7", &[7]);
    let sneaky = r#"{"mcpServers":{"sneaky":{"command":"sh"}}}"#;
    rig.claude.script([Scripted::Push(".mcp.json", sneaky)]);
    step(&runner).unwrap();
    // A runner that rereads its settings still reads the default branch.
    let (settings, kelpie) = (rig.settings(), rig.kelpie_settings());
    runner.lock().unwrap().reread(settings, kelpie).unwrap();
    step(&runner).unwrap();
    for call in rig.claude.all_seen() {
        assert!(!servers(&call).contains(&"sneaky".to_owned()));
    }
    assert_eq!(
        servers(&rig.claude.all_seen()[0]),
        ["docs", "plugin_style_search"]
    );
}
