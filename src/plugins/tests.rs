//! The plugins a repo turns on, through the runner's stand-ins: which calls
//! get them, and what `status` says of each

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::ports::{AgentCall, Role};
use crate::runner::step;
use crate::test::{Rig, Scripted};

const MARKET: &str = "market";

// A marketplace in a folder of the machine, holding each plugin in `names`
// with a skill, an agent, a hook and an MCP server called `<name>_docs`.
fn market_at(at: &Path, names: &[&str]) {
    let listed: Vec<Value> = names
        .iter()
        .map(|n| json!({ "name": n, "source": format!("./plugins/{n}") }))
        .collect();
    let listing = json!({ "name": MARKET, "plugins": listed });
    write(at, ".claude-plugin/marketplace.json", &listing.to_string());
    for name in names {
        let plugin = at.join("plugins").join(name);
        write(
            &plugin,
            ".claude-plugin/plugin.json",
            &json!({ "name": name }).to_string(),
        );
        write(
            &plugin,
            "skills/rules/SKILL.md",
            "---\nname: rules\n---\nFollow them.\n",
        );
        write(
            &plugin,
            "agents/helper.md",
            "---\nname: helper\n---\nHelp.\n",
        );
        write(&plugin, "hooks/hooks.json", "{\"hooks\":{}}");
        let server = json!({ "mcpServers": { format!("{name}_docs"): {
            "command": "${CLAUDE_PLUGIN_ROOT}/serve.sh",
        } } });
        write(&plugin, ".mcp.json", &server.to_string());
        write(&plugin, "serve.sh", "#!/bin/sh\n");
    }
}

fn write(at: &Path, file: &str, text: &str) {
    let file = at.join(file);
    fs::create_dir_all(file.parent().unwrap()).unwrap();
    fs::write(file, text).unwrap();
}

fn market(rig: &Rig, names: &[&str]) -> PathBuf {
    let at = rig.home.path().join("market");
    market_at(&at, names);
    at
}

// `.claude/settings.json` as a repo would commit it, turning `enabled` on or off
fn repo_file(source: &Value, enabled: &[(&str, bool)]) -> String {
    let enabled: serde_json::Map<String, Value> = (enabled.iter())
        .map(|(plugin, on)| (format!("{plugin}@{MARKET}"), Value::from(*on)))
        .collect();
    json!({
        "enabledPlugins": enabled,
        "extraKnownMarketplaces": { MARKET: { "source": source } },
    })
    .to_string()
}

fn directory(at: &Path) -> Value {
    json!({ "source": "directory", "path": at })
}

// Commits the repo's file to origin's default branch.
fn enable(rig: &Rig, at: &Path, enabled: &[(&str, bool)]) {
    let file = repo_file(&directory(at), enabled);
    rig.land(".claude/settings.json", &file);
}

// Appends a `[plugins]` table to the rig's settings, read when a runner next opens.
fn narrow(rig: &Rig, table: &str) {
    rig.edit_settings(|s| format!("{s}\n[app.dogs.kelpie.plugins]\n{table}"));
}

fn installed(rig: &Rig, name: &str) -> PathBuf {
    rig.paths().skills.join("plugins").join(name)
}

fn call_of(rig: &Rig, role: Role) -> AgentCall {
    rig.claude
        .all_calls()
        .into_iter()
        .find(|c| c.role == role)
        .expect("the call ran")
}

// A project manager that picks one of two ready issues on its first step.
fn pm_picks(rig: &Rig) {
    rig.edit_settings(|s| {
        let listed = "\nimplementers = [\"sonnet-high\"]\n";
        assert!(s.contains(listed), "the example's implementers moved");
        s.replace(listed, "\nimplementers = [\"sonnet-high\"]\npm = \"pm\"\n")
    });
    rig.forge.list_ready(9, false);
    rig.forge.list_ready(12, false);
    rig.claude.script([Scripted::Say(
        "```json\n{\"pick\": 12, \"hold\": [], \"unstick\": null, \"reply\": null, \
         \"why\": \"small\"}\n```",
    )]);
}

#[test]
fn a_plugin_the_repo_turns_on_reaches_the_worker_a_reviewer_and_the_project_manager() {
    let (rig, _runner, _) = Rig::with_pull_request_set("shep", |rig| {
        let at = market(rig, &["house"]);
        enable(rig, &at, &[("house", true)]);
    });
    let house = installed(&rig, "house");
    for role in [Role::Worker, Role::Reviewer] {
        let call = call_of(&rig, role);
        assert!(
            call.plugin_dirs.contains(&house),
            "{role:?}: {:?}",
            call.plugin_dirs
        );
    }
    assert!(house.join("skills/rules/SKILL.md").is_file());

    let rig = Rig::new("shep");
    let at = market(&rig, &["house"]);
    enable(&rig, &at, &[("house", true)]);
    pm_picks(&rig);
    let runner = rig.open().unwrap();
    step(&runner).unwrap();
    let pm = call_of(&rig, Role::Pm);
    assert!(
        pm.plugin_dirs.contains(&installed(&rig, "house")),
        "{:?}",
        pm.plugin_dirs
    );
}

#[test]
fn a_plugin_the_repo_turns_off_is_not_loaded() {
    let (rig, runner, _) = Rig::with_pull_request_set("shep", |rig| {
        let at = market(rig, &["house", "unused"]);
        enable(rig, &at, &[("house", true), ("unused", false)]);
    });
    let worker = call_of(&rig, Role::Worker);
    assert!(worker.plugin_dirs.contains(&installed(&rig, "house")));
    assert!(!worker.plugin_dirs.contains(&installed(&rig, "unused")));
    assert!(!installed(&rig, "unused").exists());
    let status = rig.ask(&runner, "status", None);
    assert_eq!(status["plugins"].as_array().unwrap().len(), 1);
}

#[test]
fn a_plugins_mcp_server_reaches_the_calls_mcp_config_and_its_tools_the_allow_list() {
    let (rig, _runner, _) = Rig::with_pull_request_set("shep", |rig| {
        let at = market(rig, &["house"]);
        enable(rig, &at, &[("house", true)]);
    });
    let seen = rig.claude.all_seen();
    let worker = seen.iter().find(|s| s.call.role == Role::Worker).unwrap();
    let [server] = worker.call.mcp_servers.as_slice() else {
        panic!("{:?}", worker.call.mcp_servers);
    };
    assert_eq!(server.name, "house_docs");
    assert_eq!(
        server.config["command"],
        format!("{}/serve.sh", installed(&rig, "house").display())
    );
    assert_eq!(
        worker.settings["permissions"]["allow"],
        json!(["mcp__house_docs"])
    );
    let config = worker.call.settings.with_extension("mcp.json");
    let named: Value = serde_json::from_str(&fs::read_to_string(config).unwrap()).unwrap();
    assert_eq!(named["mcpServers"]["house_docs"], server.config);
    // The server moved to the call's config, so the plugin names none itself.
    assert!(!installed(&rig, "house").join(".mcp.json").exists());
}

#[test]
fn parts_leave_out_a_plugins_hooks_and_mcp_server() {
    let (rig, runner, _) = Rig::with_pull_request_set("shep", |rig| {
        let at = market(rig, &["house"]);
        enable(rig, &at, &[("house", true)]);
        narrow(rig, "parts = [\"skills\"]\n");
    });
    let house = installed(&rig, "house");
    assert!(house.join("skills/rules/SKILL.md").is_file());
    assert!(!house.join("hooks").exists());
    assert!(!house.join("agents").exists());
    assert!(!house.join(".mcp.json").exists());
    let worker = call_of(&rig, Role::Worker);
    assert!(worker.plugin_dirs.contains(&house));
    assert!(worker.mcp_servers.is_empty());
    let manifest: Value = serde_json::from_str(
        &fs::read_to_string(house.join(".claude-plugin/plugin.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(manifest, json!({ "name": "house" }));
    let status = rig.ask(&runner, "status", None);
    assert_eq!(
        status["plugins"],
        json!([{ "plugin": "house@market", "from": "repo", "loaded": ["skills"] }])
    );
}

#[test]
fn roles_say_which_calls_get_the_plugins() {
    let (rig, _runner, _) = Rig::with_pull_request_set("shep", |rig| {
        let at = market(rig, &["house"]);
        enable(rig, &at, &[("house", true)]);
        narrow(rig, "roles = [\"worker\"]\n");
    });
    let house = installed(&rig, "house");
    assert!(call_of(&rig, Role::Worker).plugin_dirs.contains(&house));
    let reviewer = call_of(&rig, Role::Reviewer);
    assert!(
        !reviewer.plugin_dirs.contains(&house),
        "{:?}",
        reviewer.plugin_dirs
    );
    assert!(reviewer.mcp_servers.is_empty());
}

#[test]
fn skip_leaves_out_a_plugin_the_repo_turns_on_and_says_so() {
    let (rig, runner, _) = Rig::with_pull_request_set("shep", |rig| {
        let at = market(rig, &["house", "other"]);
        enable(rig, &at, &[("house", true), ("other", true)]);
        narrow(rig, "skip = [\"house@market\"]\n");
    });
    let worker = call_of(&rig, Role::Worker);
    assert!(!worker.plugin_dirs.contains(&installed(&rig, "house")));
    assert!(worker.plugin_dirs.contains(&installed(&rig, "other")));
    let status = rig.ask(&runner, "status", None);
    assert_eq!(
        status["plugins"][0],
        json!({
            "plugin": "house@market",
            "from": "repo",
            "skipped": "it is named in plugins.skip",
        })
    );
}

#[test]
fn extra_loads_a_plugin_the_repo_does_not_turn_on_from_the_marketplace_given() {
    let (rig, runner, _) = Rig::with_pull_request_set("shep", |rig| {
        let at = market(rig, &["house", "extras"]);
        // The repo names a different marketplace, and turns on only `house`.
        let elsewhere = rig.home.path().join("elsewhere");
        market_at(&elsewhere, &[]);
        let file = repo_file(&directory(&elsewhere), &[]);
        rig.land(".claude/settings.json", &file);
        narrow(
            rig,
            &format!(
                "extra = [\"extras@market\"]\n\
                 [app.dogs.kelpie.plugins.marketplaces.market]\n\
                 source = \"directory\"\npath = \"{}\"\n",
                at.display()
            ),
        );
    });
    let worker = call_of(&rig, Role::Worker);
    assert!(worker.plugin_dirs.contains(&installed(&rig, "extras")));
    let status = rig.ask(&runner, "status", None);
    assert_eq!(status["plugins"][0]["plugin"], "extras@market");
    assert_eq!(status["plugins"][0]["from"], "extra");
}

#[test]
fn repo_false_loads_none_of_the_repos_plugins() {
    let (rig, runner, _) = Rig::with_pull_request_set("shep", |rig| {
        let at = market(rig, &["house"]);
        enable(rig, &at, &[("house", true)]);
        narrow(rig, "repo = false\n");
    });
    let worker = call_of(&rig, Role::Worker);
    assert!(!worker.plugin_dirs.contains(&installed(&rig, "house")));
    assert!(!installed(&rig, "house").exists());
    let status = rig.ask(&runner, "status", None);
    assert_eq!(status["plugins"], json!([]));
}

#[test]
fn a_plugin_a_workers_branch_turns_on_is_not_loaded() {
    let rig = Rig::new("shep");
    let at = market(&rig, &["house"]);
    let repo = rig.repo();
    let git = |args: &[&str]| crate::test::git(&repo, args);
    git(&["checkout", "--quiet", "-b", "kelpie/7"]);
    write(
        &repo,
        ".claude/settings.json",
        &repo_file(&directory(&at), &[("house", true)]),
    );
    git(&["add", ".claude/settings.json"]);
    git(&["commit", "--quiet", "-m", "turn it on"]);
    git(&["checkout", "--quiet", "-"]);
    // A file in the checkout that no branch holds is no more the repo's.
    write(
        &repo,
        ".claude/settings.json",
        &repo_file(&directory(&at), &[("house", true)]),
    );
    let runner = rig.open().unwrap();
    assert_eq!(runner.lock().unwrap().status().plugins, []);
    assert!(!installed(&rig, "house").exists());
}

#[test]
fn a_plugin_that_cannot_load_is_skipped_with_its_reason_and_the_rest_load() {
    let (rig, runner, _) = Rig::with_pull_request_set("shep", |rig| {
        let at = market(rig, &["house"]);
        enable(rig, &at, &[("house", true), ("missing", true)]);
    });
    let worker = call_of(&rig, Role::Worker);
    assert!(worker.plugin_dirs.contains(&installed(&rig, "house")));
    let status = rig.ask(&runner, "status", None);
    let plugins = status["plugins"].as_array().unwrap();
    let missing = plugins
        .iter()
        .find(|p| p["plugin"] == "missing@market")
        .unwrap();
    assert_eq!(
        missing["skipped"],
        "marketplace market lists no plugin missing"
    );
    assert!(
        plugins
            .iter()
            .any(|p| p["plugin"] == "house@market" && p.get("skipped").is_none())
    );
}

#[test]
fn a_plugin_named_for_a_marketplace_the_repo_does_not_list_is_skipped() {
    let (rig, runner, _) = Rig::with_pull_request_set("shep", |rig| {
        let file = json!({ "enabledPlugins": { "house@lost": true } }).to_string();
        rig.land(".claude/settings.json", &file);
    });
    let status = rig.ask(&runner, "status", None);
    let skipped = status["plugins"][0]["skipped"].as_str().unwrap();
    assert!(skipped.starts_with("no marketplace lost"), "{skipped}");
}

#[test]
fn a_plugin_named_for_kelpie_is_refused() {
    let (rig, runner, _) = Rig::with_pull_request_set("shep", |rig| {
        let at = market(rig, &["mattpocock", "kelpie-extra", "house"]);
        let all = [
            ("mattpocock", true),
            ("kelpie-extra", true),
            ("house", true),
        ];
        enable(rig, &at, &all);
    });
    let worker = call_of(&rig, Role::Worker);
    // Only kelpie's own copy of mattpocock is a plugin of that name.
    assert!(
        worker
            .plugin_dirs
            .contains(&rig.paths().skills.join("mattpocock"))
    );
    assert!(!installed(&rig, "mattpocock").exists());
    assert!(!installed(&rig, "kelpie-extra").exists());
    let status = rig.ask(&runner, "status", None);
    let reason = |plugin: &str| {
        let all = status["plugins"].as_array().unwrap();
        let one = all.iter().find(|p| p["plugin"] == plugin).unwrap();
        one["skipped"].as_str().unwrap().to_owned()
    };
    assert_eq!(
        reason("mattpocock@market"),
        "mattpocock is a name kelpie keeps for its own plugins"
    );
    assert!(reason("kelpie-extra@market").contains("keeps for its own plugins"));
}

// A GitHub marketplace, which only the stand-in git can fetch.
fn github_market(rig: &Rig) -> String {
    let listing = json!({ "name": MARKET, "plugins": [{ "name": "house", "source": "./house" }] });
    let files = [
        (".claude-plugin/marketplace.json", listing.to_string()),
        (
            "house/.claude-plugin/plugin.json",
            json!({ "name": "house" }).to_string(),
        ),
        ("house/skills/rules/SKILL.md", "Follow them.\n".to_owned()),
    ];
    let files: Vec<(&str, &str)> = files.iter().map(|(p, t)| (*p, t.as_str())).collect();
    let url = "https://github.com/acme/market.git";
    rig.marketplaces.hold(url, &files);
    let source = json!({ "source": "github", "repo": "acme/market" });
    rig.land(
        ".claude/settings.json",
        &repo_file(&source, &[("house", true)]),
    );
    url.to_owned()
}

#[test]
fn a_github_marketplace_is_fetched_into_kelpies_home_through_the_git_port() {
    let rig = Rig::new("shep");
    let url = github_market(&rig);
    let runner = rig.open().unwrap();
    let into = rig.paths().kelpie_home.join("marketplaces").join(MARKET);
    assert_eq!(rig.marketplaces.fetched(), [(url, into)]);
    let status = runner.lock().unwrap().status().plugins.to_vec();
    assert_eq!(status[0].loaded, ["skills"]);
    assert!(
        installed(&rig, "house")
            .join("skills/rules/SKILL.md")
            .is_file()
    );
}

#[test]
fn a_fetch_that_fails_keeps_the_copy_from_the_last_good_one() {
    let rig = Rig::new("shep");
    github_market(&rig);
    let runner = rig.open().unwrap();
    rig.marketplaces.set_down(true);
    // The repo's file moving is a reason to load again.
    let source = json!({ "source": "github", "repo": "acme/market" });
    let moved = repo_file(&source, &[("house", true)]).replace('{', "{\"a\":1,");
    rig.land(".claude/settings.json", &moved);
    let mut runner = runner.lock().unwrap();
    let line = runner
        .reread(rig.settings(), rig.kelpie_settings())
        .unwrap()
        .unwrap();
    assert!(line.contains("the last good fetch"), "{line}");
    assert!(
        installed(&rig, "house")
            .join("skills/rules/SKILL.md")
            .is_file()
    );
    let status = runner.status().plugins.to_vec();
    assert_eq!(status[0].loaded, ["skills"]);
    assert_eq!(status[0].skipped, None);
}

#[test]
fn a_fetch_that_fails_with_no_copy_skips_the_plugin() {
    let rig = Rig::new("shep");
    github_market(&rig);
    rig.marketplaces.set_down(true);
    let runner = rig.open().unwrap();
    let status = runner.lock().unwrap().status().plugins.to_vec();
    let reason = status[0].skipped.clone().unwrap();
    assert!(
        reason
            .starts_with("cannot fetch marketplace market from https://github.com/acme/market.git"),
        "{reason}"
    );
    assert!(!installed(&rig, "house").exists());
}

#[test]
fn a_changed_plugins_table_takes_effect_at_once() {
    let rig = Rig::new("shep");
    let at = market(&rig, &["house"]);
    enable(&rig, &at, &[("house", true)]);
    let runner = rig.open().unwrap();
    assert!(installed(&rig, "house").is_dir());
    narrow(&rig, "skip = [\"house@market\"]\n");
    let mut runner = runner.lock().unwrap();
    let line = runner
        .reread(rig.settings(), rig.kelpie_settings())
        .unwrap()
        .unwrap();
    assert!(
        line.starts_with("settings changed: plugins now in effect"),
        "{line}"
    );
    assert!(line.contains("the plugin house@market is skipped: it is named in plugins.skip"));
    assert!(!installed(&rig, "house").exists());
}

#[test]
fn the_repos_file_moving_loads_its_plugins_again() {
    let rig = Rig::new("shep");
    let at = market(&rig, &["house", "other"]);
    enable(&rig, &at, &[("house", true)]);
    let runner = rig.open().unwrap();
    assert!(!installed(&rig, "other").exists());
    enable(&rig, &at, &[("house", true), ("other", true)]);
    let mut runner = runner.lock().unwrap();
    let line = runner
        .reread(rig.settings(), rig.kelpie_settings())
        .unwrap();
    assert_eq!(
        line.as_deref(),
        Some("settings changed: plugins now in effect")
    );
    assert!(installed(&rig, "other").is_dir());
    let same = runner
        .reread(rig.settings(), rig.kelpie_settings())
        .unwrap();
    assert_eq!(same, None);
}
