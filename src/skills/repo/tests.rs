use std::fs;
use std::path::{Path, PathBuf};

use serde_json::json;

use crate::ports::{AgentCall, Role};
use crate::runner::step;
use crate::test::{Rig, Scripted, Synced};

// A `directory` marketplace named `market` in the rig's home, holding each of `plugins` with one skill
fn marketplace(rig: &Rig, market: &str, plugins: &[(&str, &str)]) -> PathBuf {
    let dir = rig.home.path().join("markets").join(market);
    let listed: Vec<_> = plugins
        .iter()
        .map(|(name, _)| json!({ "name": name, "source": format!("./plugins/{name}") }))
        .collect();
    fs::create_dir_all(dir.join(".claude-plugin")).unwrap();
    let file = json!({ "name": market, "plugins": listed });
    fs::write(
        dir.join(".claude-plugin/marketplace.json"),
        file.to_string(),
    )
    .unwrap();
    for (name, skill) in plugins {
        let plugin = dir.join("plugins").join(name);
        fs::create_dir_all(plugin.join(".claude-plugin")).unwrap();
        let manifest = json!({ "name": name, "version": "1.2.0" });
        fs::write(
            plugin.join(".claude-plugin/plugin.json"),
            manifest.to_string(),
        )
        .unwrap();
        let skill = plugin.join("skills").join(skill);
        fs::create_dir_all(&skill).unwrap();
        fs::write(skill.join("SKILL.md"), "---\nname: style\n---\nRules.\n").unwrap();
    }
    dir
}

// A marketplace read where it is
fn directory(dir: &Path) -> serde_json::Value {
    json!({ "source": "directory", "path": dir })
}

// A marketplace on GitHub
fn github(repo: &str) -> serde_json::Value {
    json!({ "source": "github", "repo": repo })
}

// Lands the repo's `.claude/settings.json` on origin's `main`: each of `enabled`
// as `plugin@market` with its flag, and each marketplace in `markets` by its source
fn land_settings(rig: &Rig, enabled: &[(&str, bool)], markets: &[(&str, serde_json::Value)]) {
    let known: serde_json::Map<_, _> = markets
        .iter()
        .map(|(name, source)| ((*name).to_owned(), json!({ "source": source })))
        .collect();
    let enabled: serde_json::Map<_, _> = enabled
        .iter()
        .map(|(plugin, on)| ((*plugin).to_owned(), json!(on)))
        .collect();
    let text = json!({ "extraKnownMarketplaces": known, "enabledPlugins": enabled });
    rig.land(".claude/settings.json", &text.to_string());
}

fn call_of(rig: &Rig, role: Role) -> AgentCall {
    rig.claude
        .all_calls()
        .into_iter()
        .find(|c| c.role == role)
        .expect("the call ran")
}

#[test]
fn a_plugin_the_repo_turns_on_reaches_the_workers_call() {
    let (rig, _runner, _) = Rig::with_pull_request_set("shep", |rig| {
        let dir = marketplace(rig, "house", &[("style", "style")]);
        land_settings(rig, &[("style@house", true)], &[("house", directory(&dir))]);
    });
    let plugin = rig.paths().skills.join("repo/style");
    let worker = call_of(&rig, Role::Worker);
    assert!(
        worker.plugin_dirs.contains(&plugin),
        "{:?}",
        worker.plugin_dirs
    );
    assert!(plugin.join("skills/style/SKILL.md").is_file());
    let manifest = fs::read_to_string(plugin.join(".claude-plugin/plugin.json")).unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&manifest).unwrap()["name"],
        "style"
    );
}

#[test]
fn status_lists_the_plugin_with_its_skills() {
    let (rig, runner, _) = Rig::with_pull_request_set("shep", |rig| {
        let dir = marketplace(rig, "house", &[("style", "style")]);
        land_settings(rig, &[("style@house", true)], &[("house", directory(&dir))]);
    });
    let status = rig.ask(&runner, "status", None);
    assert_eq!(
        status["plugins"],
        json!([{ "plugin": "style@house", "skills": ["style"] }])
    );
}

#[test]
fn a_reviewer_gets_the_plugin_as_well() {
    let (rig, _runner, _) = Rig::with_pull_request_set("shep", |rig| {
        let dir = marketplace(rig, "house", &[("style", "style")]);
        land_settings(rig, &[("style@house", true)], &[("house", directory(&dir))]);
    });
    let plugin = rig.paths().skills.join("repo/style");
    let reviewer = call_of(&rig, Role::Reviewer);
    assert_eq!(reviewer.plugin_dirs, [plugin]);
}

#[test]
fn the_project_manager_gets_the_plugin_as_well() {
    let rig = Rig::new("koji");
    rig.edit_settings(|s| {
        let listed = "\nimplementers = [\"sonnet-high\"]\n";
        assert!(s.contains(listed), "the example's implementers moved");
        s.replace(listed, "\nimplementers = [\"sonnet-high\"]\npm = \"pm\"\n")
    });
    let dir = marketplace(&rig, "house", &[("style", "style")]);
    land_settings(
        &rig,
        &[("style@house", true)],
        &[("house", directory(&dir))],
    );
    let runner = rig.open().unwrap();
    rig.forge.list_ready(9, false);
    rig.forge.list_ready(12, false);
    rig.claude
        .script([Scripted::Say("{\"pick\": 12, \"hold\": []}")]);
    step(&runner).unwrap();
    let plugin = rig.paths().skills.join("repo/style");
    let pm = call_of(&rig, Role::Pm);
    assert_eq!(pm.plugin_dirs, [plugin]);
}

#[test]
fn a_plugin_the_repo_sets_to_false_does_not_load() {
    let (rig, runner, _) = Rig::with_pull_request_set("shep", |rig| {
        let dir = marketplace(rig, "house", &[("style", "style"), ("quiet", "quiet")]);
        let enabled = [("style@house", true), ("quiet@house", false)];
        land_settings(rig, &enabled, &[("house", directory(&dir))]);
    });
    let skills = rig.paths().skills;
    let worker = call_of(&rig, Role::Worker);
    assert!(worker.plugin_dirs.contains(&skills.join("repo/style")));
    assert!(!worker.plugin_dirs.contains(&skills.join("repo/quiet")));
    assert!(!skills.join("repo/quiet").exists());
    let status = rig.ask(&runner, "status", None);
    assert_eq!(status["plugins"].as_array().unwrap().len(), 1);
}

#[test]
fn a_plugin_a_workers_branch_turns_on_is_not_loaded() {
    let rig = Rig::new("shep");
    let dir = marketplace(&rig, "house", &[("style", "style")]);
    let runner = rig.open().unwrap();
    rig.ask(&runner, "add", Some("7"));
    rig.forge.open_pull_request(71, "kelpie/7", &[7]);
    // The worker's branch turns the plugin on for itself.
    let branch = format!(
        "{{\"extraKnownMarketplaces\":{{\"house\":{{\"source\":{{\"source\":\"directory\",\
         \"path\":{:?}}}}}}},\"enabledPlugins\":{{\"style@house\":true}}}}",
        dir.display().to_string()
    );
    let branch: &'static str = Box::leak(branch.into_boxed_str());
    rig.claude
        .script([Scripted::Push(".claude/settings.json", branch)]);
    step(&runner).unwrap();
    // A runner that rereads its settings still reads the default branch.
    let (settings, kelpie) = (rig.settings(), rig.kelpie_settings());
    runner.lock().unwrap().reread(settings, kelpie).unwrap();
    step(&runner).unwrap();
    for call in rig.claude.all_calls() {
        assert!(call.plugin_dirs.iter().all(|p| !p.ends_with("repo/style")));
    }
    assert!(!rig.paths().skills.join("repo").join("style").exists());
    assert!(rig.ask(&runner, "status", None).get("plugins").is_none());
}

#[test]
fn a_plugin_the_repo_turns_on_after_the_runner_started_loads_when_it_rereads() {
    let rig = Rig::new("shep");
    let dir = marketplace(&rig, "house", &[("style", "style")]);
    let runner = rig.open().unwrap();
    land_settings(
        &rig,
        &[("style@house", true)],
        &[("house", directory(&dir))],
    );
    let (settings, kelpie) = (rig.settings(), rig.kelpie_settings());
    let line = runner.lock().unwrap().reread(settings, kelpie).unwrap();
    assert!(line.is_some(), "a new plugin is a change");
    rig.ask(&runner, "add", Some("7"));
    step(&runner).unwrap();
    let worker = call_of(&rig, Role::Worker);
    assert!(
        worker
            .plugin_dirs
            .contains(&rig.paths().skills.join("repo/style"))
    );
}

// What `status` says of the only plugin the repo turns on
fn skipped(rig: &Rig, runner: &std::sync::Mutex<crate::runner::Runner>) -> serde_json::Value {
    let status = rig.ask(runner, "status", None);
    let plugins = status["plugins"].as_array().unwrap();
    assert_eq!(plugins.len(), 1, "{plugins:?}");
    assert_eq!(plugins[0]["skills"], json!([]));
    plugins[0].clone()
}

#[test]
fn a_plugin_the_marketplace_lacks_is_skipped_with_its_reason_and_the_run_goes_on() {
    let (rig, runner, _) = Rig::with_pull_request_set("shep", |rig| {
        let dir = marketplace(rig, "house", &[("style", "style")]);
        land_settings(
            rig,
            &[("absent@house", true)],
            &[("house", directory(&dir))],
        );
    });
    let plugin = skipped(&rig, &runner);
    assert_eq!(plugin["plugin"], "absent@house");
    assert_eq!(plugin["skipped"], "the marketplace lists no plugin absent");
    let worker = call_of(&rig, Role::Worker);
    assert_eq!(worker.plugin_dirs, [rig.paths().skills.join("mattpocock")]);
}

#[test]
fn a_plugin_with_no_marketplace_entry_is_skipped() {
    let (rig, runner, _) = Rig::with_pull_request_set("shep", |rig| {
        land_settings(rig, &[("style@nowhere", true)], &[]);
    });
    let plugin = skipped(&rig, &runner);
    assert_eq!(
        plugin["skipped"],
        ".claude/settings.json has no extraKnownMarketplaces entry for nowhere"
    );
}

#[test]
fn kelpies_own_plugin_names_are_refused() {
    let (rig, runner, _) = Rig::with_pull_request_set("shep", |rig| {
        let dir = marketplace(rig, "house", &[("mattpocock", "tdd"), ("kelpie-pr", "pr")]);
        let enabled = [("mattpocock@house", true), ("kelpie-pr@house", true)];
        land_settings(rig, &enabled, &[("house", directory(&dir))]);
    });
    let status = rig.ask(&runner, "status", None);
    assert_eq!(
        status["plugins"],
        json!([
            {
                "plugin": "kelpie-pr@house",
                "skills": [],
                "skipped": "it is named kelpie-pr, which is kelpie's own"
            },
            {
                "plugin": "mattpocock@house",
                "skills": [],
                "skipped": "it is named mattpocock, which is kelpie's own"
            },
        ])
    );
    // The vendored plugin is the only one the worker gets, and it is kelpie's.
    let worker = call_of(&rig, Role::Worker);
    assert_eq!(worker.plugin_dirs, [rig.paths().skills.join("mattpocock")]);
    assert!(!rig.paths().skills.join("repo").join("mattpocock").exists());
}

const HOUSE: &str = "https://github.com/shep-pm/house.git";

#[test]
fn a_github_marketplace_is_fetched_into_kelpies_home_through_git() {
    let rig = Rig::new("shep");
    let served = marketplace(&rig, "served", &[("style", "style")]);
    rig.git.serve(HOUSE, &served);
    land_settings(
        &rig,
        &[("style@house", true)],
        &[("house", github("shep-pm/house"))],
    );
    let runner = rig.open().unwrap();
    let into = rig.paths().kelpie_home.join("marketplaces/house");
    assert_eq!(
        rig.git.asked(),
        [Synced {
            url: HOUSE.to_owned(),
            reference: None,
            into: into.clone()
        }]
    );
    assert!(into.join(".claude-plugin/marketplace.json").is_file());
    rig.ask(&runner, "add", Some("7"));
    step(&runner).unwrap();
    let worker = call_of(&rig, Role::Worker);
    assert!(
        worker
            .plugin_dirs
            .contains(&rig.paths().skills.join("repo/style"))
    );
}

#[test]
fn a_git_marketplace_is_fetched_at_its_ref() {
    let rig = Rig::new("shep");
    let served = marketplace(&rig, "served", &[("style", "style")]);
    rig.git
        .serve("ssh://git@example.invalid/house.git", &served);
    let source =
        json!({ "source": "git", "url": "ssh://git@example.invalid/house.git", "ref": "v2" });
    land_settings(&rig, &[("style@house", true)], &[("house", source)]);
    rig.open().unwrap();
    let [synced] = rig.git.asked().try_into().unwrap();
    assert_eq!(synced.reference.as_deref(), Some("v2"));
}

#[test]
fn a_failed_fetch_keeps_the_last_copy() {
    let rig = Rig::new("shep");
    let served = marketplace(&rig, "served", &[("style", "style")]);
    rig.git.serve(HOUSE, &served);
    land_settings(
        &rig,
        &[("style@house", true)],
        &[("house", github("shep-pm/house"))],
    );
    let runner = rig.open().unwrap();
    rig.git.go_down();
    // A runner that starts with the network down, and one that rereads, both keep the copy.
    let (settings, kelpie) = (rig.settings(), rig.kelpie_settings());
    runner.lock().unwrap().reread(settings, kelpie).unwrap();
    for runner in [&runner, &rig.open().unwrap()] {
        let status = rig.ask(runner, "status", None);
        let plugin = &status["plugins"][0];
        assert_eq!(plugin["skills"], json!(["style"]));
        assert_eq!(plugin["skipped"], json!(null));
        assert_eq!(
            plugin["stale"],
            "fetching the house marketplace failed: could not resolve host"
        );
    }
    rig.ask(&runner, "add", Some("7"));
    step(&runner).unwrap();
    let worker = call_of(&rig, Role::Worker);
    assert!(
        worker
            .plugin_dirs
            .contains(&rig.paths().skills.join("repo/style"))
    );
}

#[test]
fn a_failed_fetch_with_no_copy_skips_the_plugin() {
    let rig = Rig::new("shep");
    land_settings(
        &rig,
        &[("style@house", true)],
        &[("house", github("shep-pm/house"))],
    );
    rig.git.go_down();
    let runner = rig.open().unwrap();
    let status = rig.ask(&runner, "status", None);
    assert_eq!(
        status["plugins"],
        json!([{
            "plugin": "style@house",
            "skills": [],
            "skipped": "fetching the house marketplace failed: could not resolve host"
        }])
    );
}

#[test]
fn an_address_git_would_take_for_an_option_or_a_program_is_refused() {
    let rig = Rig::new("shep");
    let option = json!({ "source": "git", "url": "--upload-pack=touch x" });
    let program = json!({ "source": "git", "url": "ext::sh -c touch% x" });
    land_settings(
        &rig,
        &[("a@option", true), ("b@program", true)],
        &[("option", option), ("program", program)],
    );
    rig.open().unwrap();
    assert_eq!(rig.git.asked(), []);
}
