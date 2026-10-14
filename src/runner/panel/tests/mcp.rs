//! The MCP servers of the repo's plugins and of its `.mcp.json` on the default branch reach the
//! calls that get the plugins, in their MCP config and their allow-list

use std::fs;
use std::path::Path;

use serde_json::{Value, json};

use super::{parked_with, script_all, seen, settle};
use crate::ports::{Role, Tools};
use crate::runner::step;
use crate::settings::AgentHarness;
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

const PI_IMPLEMENTER: &str = "---\nrole: implementer\nharness: pi\nmodel: qwen3.8:27b\neffort: low\n\
                              url: http://box:11434/v1\ncontext: 65536\n---\n";

const PI_RESEARCHER: &str = "---\nrole: researcher\nharness: pi\nmodel: qwen3.8:27b\neffort: low\n\
                             url: http://box:11434/v1\ncontext: 65536\nusage: none\n---\nVote.\n";

// A parked ruling on a repo with those servers, whose worker and researcher run on pi, with every
// call of the panel made
fn settled_on_pi(own: Option<Value>) -> (Rig, std::sync::Mutex<crate::runner::Runner>) {
    let (rig, runner, _) = Rig::parked_set("koji", |rig| {
        rig.with_app();
        rig.maintainer("octocat");
        land_servers(rig);
        if let Some(own) = own {
            rig.land(".mcp.json", &own.to_string());
        }
        super::name_panel(rig, true);
        rig.write_agent("pi-box", PI_IMPLEMENTER);
        rig.implementers(&["pi-box"]);
        rig.write_agent("pi-researcher", PI_RESEARCHER);
        rig.edit_settings(|s| {
            s.replace(
                "researcher = \"researcher\"",
                "researcher = \"pi-researcher\"",
            )
        });
    });
    script_all(&rig, true);
    settle(&rig, &runner);
    (rig, runner)
}

#[test]
fn a_pi_researcher_gets_the_repos_servers_and_a_pi_retro_vote_gets_none() {
    let (rig, _runner) = settled_on_pi(None);

    let researchers = seen(&rig, Role::Researcher);
    assert!(!researchers.is_empty());
    for call in researchers {
        assert!(matches!(call.call.harness, AgentHarness::Pi(_)));
        let names: Vec<_> = call.call.reach.mcp_servers.keys().collect();
        assert_eq!(names, ["docs", "plugin_style_search"]);
    }
    let (works, votes): (Vec<_>, Vec<_>) =
        (seen(&rig, Role::Worker).into_iter()).partition(|c| c.call.tools != Tools::Retro);
    assert!(!works.is_empty() && !votes.is_empty());
    for call in works {
        assert!(matches!(call.call.harness, AgentHarness::Pi(_)));
        assert_eq!(call.call.reach.mcp_servers.len(), 2);
    }
    for vote in votes {
        assert!(matches!(vote.call.harness, AgentHarness::Pi(_)));
        assert!(vote.call.reach.mcp_servers.is_empty());
    }
}

#[test]
fn a_server_pi_cannot_load_is_dropped_with_a_note() {
    let own = json!({ "mcpServers": {
        "docs": { "type": "http", "url": "https://docs.example.com/mcp" },
        "old": { "type": "sse", "url": "https://old.example.com/sse" },
    } });
    let (rig, runner) = settled_on_pi(Some(own));

    for call in seen(&rig, Role::Researcher) {
        let names: Vec<_> = call.call.reach.mcp_servers.keys().collect();
        assert_eq!(names, ["docs", "plugin_style_search"]);
    }
    let notes = runner.lock().unwrap().take_notes().join("\n");
    assert!(notes.contains("the MCP server `old`"), "{notes}");
    assert!(notes.contains("SSE"), "{notes}");
}

const CODEX_IMPLEMENTER: &str =
    "---\nrole: implementer\nharness: codex\nmodel: gpt-6-sol\neffort: low\n---\n";

const CODEX_RESEARCHER: &str =
    "---\nrole: researcher\nharness: codex\nmodel: gpt-6-sol\neffort: low\n---\nVote.\n";

// A parked ruling on a repo with those servers, whose worker and researcher run on Codex, with
// every call of the panel made
fn settled_on_codex(own: Option<Value>) -> (Rig, std::sync::Mutex<crate::runner::Runner>) {
    let (rig, runner, _) = Rig::parked_set("koji", |rig| {
        rig.with_app();
        rig.maintainer("octocat");
        land_servers(rig);
        if let Some(own) = own {
            rig.land(".mcp.json", &own.to_string());
        }
        super::name_panel(rig, true);
        rig.write_agent("codex-box", CODEX_IMPLEMENTER);
        rig.implementers(&["codex-box"]);
        rig.write_agent("codex-researcher", CODEX_RESEARCHER);
        rig.edit_settings(|s| {
            s.replace(
                "researcher = \"researcher\"",
                "researcher = \"codex-researcher\"",
            )
        });
    });
    script_all(&rig, true);
    settle(&rig, &runner);
    (rig, runner)
}

#[test]
fn a_codex_researcher_gets_the_repos_servers_and_a_codex_retro_vote_gets_none() {
    let (rig, _runner) = settled_on_codex(None);

    let researchers = seen(&rig, Role::Researcher);
    assert!(!researchers.is_empty());
    for call in researchers {
        assert_eq!(call.call.harness, AgentHarness::Codex);
        let names: Vec<_> = call.call.reach.mcp_servers.keys().collect();
        assert_eq!(names, ["docs", "plugin_style_search"]);
    }
    let (works, votes): (Vec<_>, Vec<_>) =
        (seen(&rig, Role::Worker).into_iter()).partition(|c| c.call.tools != Tools::Retro);
    assert!(!works.is_empty() && !votes.is_empty());
    for call in works {
        assert_eq!(call.call.harness, AgentHarness::Codex);
        assert_eq!(call.call.reach.mcp_servers.len(), 2);
    }
    for vote in votes {
        assert_eq!(vote.call.harness, AgentHarness::Codex);
        assert!(vote.call.reach.mcp_servers.is_empty());
    }
}

#[test]
fn a_server_codex_cannot_load_is_dropped_with_a_note() {
    let own = json!({ "mcpServers": {
        "docs": { "type": "http", "url": "https://docs.example.com/mcp" },
        "old": { "type": "sse", "url": "https://old.example.com/sse" },
    } });
    let (rig, runner) = settled_on_codex(Some(own));

    for call in seen(&rig, Role::Researcher) {
        let names: Vec<_> = call.call.reach.mcp_servers.keys().collect();
        assert_eq!(names, ["docs", "plugin_style_search"]);
    }
    let notes = runner.lock().unwrap().take_notes().join("\n");
    assert!(notes.contains("the MCP server `old`"), "{notes}");
    assert!(notes.contains("SSE"), "{notes}");
}

const GOOSE_IMPLEMENTER: &str = "---\nrole: implementer\nharness: goose\nmodel: qwen3.8:27b\neffort: low\n\
                                 url: http://box:11434/v1\ncontext: 65536\n---\n";

const GOOSE_RESEARCHER: &str = "---\nrole: researcher\nharness: goose\nmodel: qwen3.8:27b\neffort: low\n\
                                url: http://box:11434/v1\ncontext: 65536\nusage: none\n---\nVote.\n";

// A parked ruling on a repo with those servers, whose worker and researcher run on goose, with
// every call of the panel made
fn settled_on_goose(own: Option<Value>) -> (Rig, std::sync::Mutex<crate::runner::Runner>) {
    let (rig, runner, _) = Rig::parked_set("koji", |rig| {
        rig.with_app();
        rig.maintainer("octocat");
        land_servers(rig);
        if let Some(own) = own {
            rig.land(".mcp.json", &own.to_string());
        }
        super::name_panel(rig, true);
        rig.write_agent("goose-box", GOOSE_IMPLEMENTER);
        rig.implementers(&["goose-box"]);
        rig.write_agent("goose-researcher", GOOSE_RESEARCHER);
        rig.edit_settings(|s| {
            s.replace(
                "researcher = \"researcher\"",
                "researcher = \"goose-researcher\"",
            )
        });
    });
    script_all(&rig, true);
    settle(&rig, &runner);
    (rig, runner)
}

#[test]
fn a_goose_researcher_gets_the_repos_servers_and_a_goose_retro_vote_gets_none() {
    let (rig, _runner) = settled_on_goose(None);

    let researchers = seen(&rig, Role::Researcher);
    assert!(!researchers.is_empty());
    for call in researchers {
        assert!(matches!(call.call.harness, AgentHarness::Goose(_)));
        let names: Vec<_> = call.call.reach.mcp_servers.keys().collect();
        assert_eq!(names, ["docs", "plugin_style_search"]);
    }
    let (works, votes): (Vec<_>, Vec<_>) =
        (seen(&rig, Role::Worker).into_iter()).partition(|c| c.call.tools != Tools::Retro);
    assert!(!works.is_empty() && !votes.is_empty());
    for call in works {
        assert!(matches!(call.call.harness, AgentHarness::Goose(_)));
        assert_eq!(call.call.reach.mcp_servers.len(), 2);
    }
    for vote in votes {
        assert!(matches!(vote.call.harness, AgentHarness::Goose(_)));
        assert!(vote.call.reach.mcp_servers.is_empty());
    }
}

#[test]
fn a_server_goose_cannot_load_is_dropped_with_a_note() {
    let own = json!({ "mcpServers": {
        "docs": { "type": "http", "url": "https://docs.example.com/mcp" },
        "keyed": { "type": "http", "url": "https://keyed.example.com/mcp",
                   "headers": { "Authorization": "Bearer x" } },
        "old": { "type": "sse", "url": "https://old.example.com/sse" },
        "quoted": { "command": "/bin/sh", "args": ["-c", "echo \"hi\""] },
        "skips": { "command": "/bin/true", "env": { "PATH": "/x", "TOKEN": "t" } },
    } });
    let (rig, runner) = settled_on_goose(Some(own));

    for call in seen(&rig, Role::Researcher) {
        let names: Vec<_> = call.call.reach.mcp_servers.keys().collect();
        assert_eq!(names, ["docs", "plugin_style_search", "skips"]);
    }
    let notes = runner.lock().unwrap().take_notes().join("\n");
    for (name, why) in [
        ("keyed", "`headers`"),
        ("old", "SSE"),
        ("quoted", "double quote"),
    ] {
        assert!(
            notes.contains(&format!("the MCP server `{name}`")),
            "{notes}"
        );
        assert!(notes.contains(why), "{why}: {notes}");
    }
    // A server whose only trouble is a skipped env key stays, and the note says what it loses.
    assert!(
        notes.contains("the MCP server `skips` without the env keys `PATH`"),
        "{notes}"
    );
}
