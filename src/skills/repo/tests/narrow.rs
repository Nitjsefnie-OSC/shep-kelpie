//! `[app.dogs.kelpie.repo_setup]`: what narrows, and what adds to, the plugins the repo turns on

use std::sync::Mutex;

use serde_json::json;

use super::{call_of, directory, land_settings, marketplace};
use crate::ports::Role;
use crate::runner::{Runner, step};
use crate::test::Rig;

// A runner whose `repo_setup` table holds `keys`, on a repo that turns on `style@house` and
// `quiet@house`, with a worker's first turn run
fn narrowed(keys: &str) -> (Rig, Mutex<Runner>) {
    let rig = Rig::new("shep");
    rig.edit_settings(|s| format!("{s}\n[app.dogs.kelpie.repo_setup]\n{keys}\n"));
    let dir = marketplace(&rig, "house", &[("style", "style"), ("quiet", "quiet")]);
    let enabled = [("style@house", true), ("quiet@house", true)];
    land_settings(&rig, &enabled, &[("house", directory(&dir))]);
    let runner = rig.open().unwrap();
    rig.ask(&runner, "add", Some("7"));
    step(&runner).unwrap();
    (rig, runner)
}

#[test]
fn plugins_false_loads_none_of_the_repos_plugins() {
    let (rig, runner) = narrowed("plugins = false");
    let skills = rig.paths().skills;
    let worker = call_of(&rig, Role::Worker);
    assert!(
        worker
            .plugin_dirs
            .iter()
            .all(|p| !p.starts_with(skills.join("repo"))),
        "{:?}",
        worker.plugin_dirs
    );
    assert!(!skills.join("repo/style").exists());
    assert!(rig.ask(&runner, "status", None).get("plugins").is_none());
}

// A runner on a repo that turns on `style@house`, whose `repo_setup` adds `quiet@lab` from a
// marketplace of the checkout the repo's settings do not name, plus `more`
fn adding(more: &str) -> (Rig, Mutex<Runner>) {
    let rig = Rig::new("shep");
    let lab = marketplace(&rig, "lab", &[("quiet", "quiet")]);
    let extra = format!(
        "extra = {{ \"quiet@lab\" = {:?} }}\n{more}",
        format!("directory:{}", lab.display())
    );
    rig.edit_settings(|s| format!("{s}\n[app.dogs.kelpie.repo_setup]\n{extra}\n"));
    let house = marketplace(&rig, "house", &[("style", "style")]);
    land_settings(
        &rig,
        &[("style@house", true)],
        &[("house", directory(&house))],
    );
    let runner = rig.open().unwrap();
    rig.ask(&runner, "add", Some("7"));
    step(&runner).unwrap();
    (rig, runner)
}

#[test]
fn extra_loads_a_plugin_the_repo_does_not_name() {
    let (rig, runner) = adding("");
    let skills = rig.paths().skills;
    let worker = call_of(&rig, Role::Worker);
    assert!(worker.plugin_dirs.contains(&skills.join("repo/style")));
    assert!(worker.plugin_dirs.contains(&skills.join("repo/quiet")));
    assert!(skills.join("repo/quiet/skills/quiet/SKILL.md").is_file());
    let status = rig.ask(&runner, "status", None);
    assert_eq!(
        status["plugins"],
        json!([
            { "plugin": "style@house", "skills": ["style"] },
            { "plugin": "quiet@lab", "skills": ["quiet"] },
        ])
    );
}

#[test]
fn extra_loads_though_the_repos_own_plugins_are_off() {
    let (rig, runner) = adding("plugins = false");
    let status = rig.ask(&runner, "status", None);
    assert_eq!(
        status["plugins"],
        json!([{ "plugin": "quiet@lab", "skills": ["quiet"] }])
    );
    assert!(!rig.paths().skills.join("repo/style").exists());
}

#[test]
fn a_changed_repo_setup_loads_when_the_runner_rereads() {
    let (rig, runner) = narrowed("");
    rig.edit_settings(|s| {
        s.replace(
            "\n[app.dogs.kelpie.repo_setup]\n",
            "\n[app.dogs.kelpie.repo_setup]\nplugins = false\n",
        )
    });
    let (settings, kelpie) = (rig.settings(), rig.kelpie_settings());
    let line = runner.lock().unwrap().reread(settings, kelpie).unwrap();
    assert!(line.is_some_and(|l| l.contains("repo_setup")));
    assert!(!rig.paths().skills.join("repo/style").exists());
    assert!(rig.ask(&runner, "status", None).get("plugins").is_none());
}

#[test]
fn skip_leaves_one_plugin_out_and_keeps_the_rest() {
    let (rig, runner) = narrowed("skip = [\"quiet@house\"]");
    let skills = rig.paths().skills;
    let worker = call_of(&rig, Role::Worker);
    assert!(worker.plugin_dirs.contains(&skills.join("repo/style")));
    assert!(!worker.plugin_dirs.contains(&skills.join("repo/quiet")));
    assert!(!skills.join("repo/quiet").exists());
    let status = rig.ask(&runner, "status", None);
    assert_eq!(
        status["plugins"],
        json!([{ "plugin": "style@house", "skills": ["style"] }])
    );
}
