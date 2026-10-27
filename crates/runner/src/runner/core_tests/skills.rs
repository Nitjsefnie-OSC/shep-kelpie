use std::fs;

use serde_json::json;

use crate::ports::{AgentCall, Checks, Role};
use crate::runner::step;
use crate::skills::vendored;
use crate::skills::*;
use crate::test::{Rig, Scripted};
use std::path::Path;

// Appends a `[skills]` table to the rig's settings, read when a runner next opens
fn choose(rig: &Rig, table: &str) {
    rig.edit_settings(|s| {
        let s = s.replace(crate::test::NO_RETRO, "");
        format!("{s}\n[app.dogs.kelpie.skills]\n{table}")
    });
}

// A plugin folder whose manifest names it `name`
fn plugin_folder(at: &Path, name: &str) {
    fs::create_dir_all(at.join(".claude-plugin")).unwrap();
    let manifest = json!({ "name": name }).to_string();
    fs::write(at.join(".claude-plugin/plugin.json"), manifest).unwrap();
}

fn skill_folder(at: &Path, text: &str) {
    fs::create_dir_all(at.join("references")).unwrap();
    fs::write(at.join("SKILL.md"), text).unwrap();
    fs::write(at.join("references/more.md"), "more\n").unwrap();
}

// Asserts `prompt` runs `command` under kelpie's format rule, then asks `then`
#[track_caller]
fn assert_invoked(prompt: &str, command: &str, then: &str) {
    assert!(prompt.starts_with(command), "{prompt}");
    assert!(prompt.contains(FORMAT_RULE.trim_end()), "{prompt}");
    assert!(prompt.contains(&format!("\n{then}")), "{prompt}");
}

// Asserts `prompt` also carries kelpie's headless rules, ahead of the format rule
#[track_caller]
fn assert_headless(prompt: &str) {
    let at = prompt
        .find("Kelpie runs this skill headless.")
        .expect("the headless rules");
    assert!(
        at < prompt.find(FORMAT_RULE.trim_end()).unwrap(),
        "{prompt}"
    );
    assert!(prompt.contains("Run no /code-review"), "{prompt}");
}

// Asserts `prompt` carries none of them, because the instructions already do
#[track_caller]
fn assert_no_headless(prompt: &str) {
    assert!(
        !prompt.contains("Kelpie runs this skill headless"),
        "{prompt}"
    );
}

// The skill folders a plugin folder holds, by name, sorted
fn skills_in(plugin: &Path) -> Vec<String> {
    let Ok(entries) = fs::read_dir(plugin.join("skills")) else {
        return Vec::new();
    };
    let mut found: Vec<String> = entries
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    found.sort();
    found
}

// Every file under `dir`, by its path under it, sorted
fn files_under(dir: &Path) -> Vec<String> {
    fn walk(dir: &Path, root: &Path, found: &mut Vec<String>) {
        for entry in fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk(&path, root, found);
            } else {
                found.push(
                    path.strip_prefix(root)
                        .unwrap()
                        .to_string_lossy()
                        .into_owned(),
                );
            }
        }
    }
    let mut found = Vec::new();
    walk(dir, dir, &mut found);
    found.sort();
    found
}

// The files kelpie's vendored copy holds for `skill`, by their path under its folder
fn vendored_files(skill: &str) -> Vec<String> {
    let mut found: Vec<String> = vendored::FILES
        .iter()
        .filter_map(|(path, _)| {
            let (_group, rest) = path.strip_prefix("skills/")?.split_once('/')?;
            let (name, tail) = rest.split_once('/')?;
            (name == skill).then(|| tail.to_owned())
        })
        .collect();
    found.sort();
    found
}

// Every vendored skill's name, sorted
fn every_vendored_skill() -> Vec<&'static str> {
    let mut names: Vec<&'static str> = vendored::FILES
        .iter()
        .filter_map(|(path, _)| path.strip_prefix("skills/")?.split_once('/'))
        .map(|(_group, rest)| rest.split('/').next().unwrap())
        .collect();
    names.sort_unstable();
    names.dedup();
    names
}

fn call_of(rig: &Rig, role: Role) -> AgentCall {
    rig.claude
        .all_calls()
        .into_iter()
        .find(|c| c.role == role)
        .expect("the call ran")
}

#[test]
fn each_step_runs_its_default_skill_from_kelpies_own_copy() {
    let (rig, runner, head) = Rig::with_pull_request("shep");
    let plugin = rig.paths().skills.join("mattpocock");
    let worker = call_of(&rig, Role::Worker);
    let (implement, first) = ("/mattpocock:implement", "Your work item is issue #7: ");
    assert_invoked(&worker.prompt, implement, first);
    assert_no_headless(&worker.prompt);
    let text = fs::read_to_string(worker.instructions.expect("a worker has instructions")).unwrap();
    assert_eq!(
        text.matches("Kelpie runs this skill headless.").count(),
        1,
        "the headless rules come once, in the instructions: {text}"
    );
    assert!(text.contains("Run no /code-review"), "{text}");
    let wording = rig.paths().skills.join("kelpie-wording");
    let issue = rig.paths().skills.join("kelpie-issue");
    assert_eq!(worker.plugin_dirs, [plugin.clone(), wording, issue]);
    // A reviewer's prompt is its agent file's body, which runs no skill.
    let reviewer = call_of(&rig, Role::Reviewer);
    assert!(
        reviewer.prompt.starts_with("Review the change against"),
        "{}",
        reviewer.prompt
    );
    assert!(reviewer.plugin_dirs.is_empty());
    let seen = rig.claude.all_seen();
    let reviewed = seen.iter().find(|s| s.call.role == Role::Reviewer).unwrap();
    assert_eq!(
        reviewed.settings["permissions"]["deny"],
        crate::trim::deny_with_trim(&["Agent", "Task", "Bash"])
    );
    for step in Step::WORKER {
        let skill = plugin.join("skills").join(step.default_skill());
        assert!(skill.join("SKILL.md").is_file(), "{step}");
    }

    rig.forge
        .set_checks(&head, Checks::Failed(vec!["test".into()]));
    rig.verdict(&runner);
    rig.claude.script([Scripted::Push("fix.txt", "fixed\n")]);
    step(&runner).unwrap();
    let fix = rig.claude.calls().pop().unwrap();
    let (ci, red) = (
        "/mattpocock:diagnosing-bugs",
        "CI failed on your pull request #71",
    );
    assert_invoked(&fix.prompt, ci, red);
    assert_no_headless(&fix.prompt);

    let status = rig.ask(&runner, "status", None);
    assert_eq!(
        status["skills"][4],
        json!({ "step": "ci_fix", "skill": "/mattpocock:diagnosing-bugs", "fallback": null })
    );
    assert_eq!(status["skills"].as_array().unwrap().len(), Step::ALL.len());
}

#[test]
fn a_projects_skill_folder_replaces_the_default() {
    let (rig, runner, _) = Rig::with_pull_request_set("shep", |rig| {
        let folder = rig.home.path().join("house/build-it");
        skill_folder(&folder, "---\nname: build-it\n---\nBuild it.\n");
        choose(
            rig,
            &format!(
                "implement = {{ kind = \"path\", path = \"{}\" }}\n",
                folder.display()
            ),
        );
    });
    let worker = call_of(&rig, Role::Worker);
    let first = "Your work item is issue #7: ";
    assert_invoked(&worker.prompt, "/kelpie-implement:build-it", first);
    let own = rig.paths().skills.join("implement");
    assert!(
        worker.plugin_dirs.contains(&own),
        "{:?}",
        worker.plugin_dirs
    );
    assert_eq!(
        fs::read_to_string(own.join("skills/build-it/references/more.md")).unwrap(),
        "more\n"
    );
    let manifest = fs::read_to_string(own.join(".claude-plugin/plugin.json")).unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&manifest).unwrap()["name"],
        "kelpie-implement"
    );
    let status = rig.ask(&runner, "status", None);
    assert_eq!(status["skills"][2]["skill"], "/kelpie-implement:build-it");
}

#[test]
fn a_skill_in_a_projects_plugin_replaces_the_default() {
    let (rig, _runner, _) = Rig::with_pull_request_set("shep", |rig| {
        let plugin = rig.home.path().join("house-plugin");
        plugin_folder(&plugin, "house");
        skill_folder(&plugin.join("skills/build-hard"), "Build hard.\n");
        choose(
            rig,
            &format!(
                "implement = {{ kind = \"plugin\", plugin = \"{}\", skill = \"build-hard\" }}\n",
                plugin.display()
            ),
        );
    });
    let worker = call_of(&rig, Role::Worker);
    let first = "Your work item is issue #7: ";
    assert_invoked(&worker.prompt, "/house:build-hard", first);
    let plugin = rig.home.path().join("house-plugin");
    assert!(
        worker.plugin_dirs.contains(&plugin),
        "{:?}",
        worker.plugin_dirs
    );
}

#[test]
fn a_skill_that_cannot_load_falls_back_to_kelpies_prompt_and_says_why() {
    let rig = Rig::new("shep");
    let gone = rig.home.path().join("gone");
    let plugin = rig.home.path().join("empty-plugin");
    plugin_folder(&plugin, "e");
    choose(
        &rig,
        &format!(
            "implement = {{ kind = \"path\", path = \"{}\" }}\n\
             ci_fix = {{ kind = \"plugin\", plugin = \"{}\", skill = \"debug\" }}\n",
            gone.display(),
            plugin.display()
        ),
    );
    let runner = rig.open().unwrap();
    let notices: Vec<String> = runner.lock().unwrap().skill_notices().collect();
    assert_eq!(
        notices,
        [
            format!(
                "the implement step's skill cannot load, so it runs kelpie's own prompt: \
                 {} holds no SKILL.md",
                gone.display()
            ),
            format!(
                "the ci_fix step's skill cannot load, so it runs kelpie's own prompt: \
                 the plugin in {} has no skill debug",
                plugin.display()
            ),
        ]
    );
    let status = rig.ask(&runner, "status", None);
    assert_eq!(status["skills"][2]["skill"], json!(null));
    assert_eq!(
        status["skills"][2]["fallback"],
        format!("{} holds no SKILL.md", gone.display())
    );

    rig.ask(&runner, "add", Some("7"));
    rig.claude.script([Scripted::Push("work.txt", "work\n")]);
    step(&runner).unwrap();
    let worker = call_of(&rig, Role::Worker);
    assert!(
        worker.prompt.starts_with("Your work item is issue #7: "),
        "{}",
        worker.prompt
    );
}

#[test]
fn a_step_set_to_none_runs_kelpies_prompt_with_no_notice() {
    let (rig, runner, _) = Rig::with_pull_request_set("shep", |rig| {
        choose(rig, "implement = { kind = \"none\" }\n");
    });
    let worker = call_of(&rig, Role::Worker);
    assert!(
        worker.prompt.starts_with("Your work item is issue #7: "),
        "{}",
        worker.prompt
    );
    assert_eq!(runner.lock().unwrap().skill_notices().count(), 0);
    let status = rig.ask(&runner, "status", None);
    assert_eq!(
        status["skills"][2],
        json!({ "step": "implement", "skill": null, "fallback": null })
    );
}

#[test]
fn a_worker_with_no_skill_of_its_own_keeps_the_headless_rules_in_its_prompt() {
    let (rig, _runner, _) = Rig::with_pull_request_set("shep", |rig| {
        choose(
            rig,
            "tests = { kind = \"none\" }\npr = { kind = \"none\" }\n",
        );
    });
    let worker = call_of(&rig, Role::Worker);
    assert_invoked(
        &worker.prompt,
        "/mattpocock:implement",
        "Your work item is issue #7: ",
    );
    assert_headless(&worker.prompt);
    let text = fs::read_to_string(worker.instructions.expect("a worker has instructions")).unwrap();
    assert!(
        !text.contains("Kelpie runs this skill headless"),
        "nothing carries the block but the prompt: {text}"
    );
}

#[test]
fn a_worker_gets_only_the_skills_its_steps_use_and_those_they_call() {
    let (rig, _runner, _) = Rig::with_pull_request("shep");
    let plugin = rig.paths().skills.join("mattpocock");
    // The worker's four steps, and the two `tdd` and `implement` call.
    let held = skills_in(&plugin);
    assert_eq!(
        held,
        [
            "code-review",
            "codebase-design",
            "diagnosing-bugs",
            "implement",
            "pr",
            "tdd",
        ]
    );
    // Each is the vendored folder, file for file.
    for skill in &held {
        assert_eq!(
            files_under(&plugin.join("skills").join(skill)),
            vendored_files(skill),
            "{skill}"
        );
    }
    // The nine no worker step runs, or calls, are not written at all.
    for skill in [
        "domain-modeling",
        "grilling",
        "grill-with-docs",
        "handoff",
        "retro",
        "to-spec",
        "to-tickets",
        "triage",
        "writing-for-agents",
    ] {
        assert!(!plugin.join("skills").join(skill).exists(), "{skill}");
    }
    // It is still kelpie's vendored plugin, named the same and licensed.
    let manifest = fs::read_to_string(plugin.join(".claude-plugin/plugin.json")).unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&manifest).unwrap()["name"],
        "mattpocock"
    );
    assert!(plugin.join("LICENSE").is_file());
}

#[test]
fn a_step_set_to_none_drops_its_skill_and_what_only_that_step_calls() {
    let (rig, _runner, _) = Rig::with_pull_request_set("shep", |rig| {
        choose(
            rig,
            "implement = { kind = \"none\" }\ntests = { kind = \"none\" }\n",
        );
    });
    let plugin = rig.paths().skills.join("mattpocock");
    assert_eq!(skills_in(&plugin), ["diagnosing-bugs", "pr"]);
}

#[test]
fn a_step_set_to_a_path_or_a_plugin_adds_nothing_from_kelpies_copy() {
    let (rig, _runner, _) = Rig::with_pull_request_set("shep", |rig| {
        let folder = rig.home.path().join("house/build-it");
        skill_folder(&folder, "---\nname: build-it\n---\nBuild it.\n");
        let plugin = rig.home.path().join("house-plugin");
        plugin_folder(&plugin, "house");
        skill_folder(&plugin.join("skills/build-hard"), "Build hard.\n");
        choose(
            rig,
            &format!(
                "implement = {{ kind = \"path\", path = \"{}\" }}\n\
                 ci_fix = {{ kind = \"plugin\", plugin = \"{}\", skill = \"build-hard\" }}\n",
                folder.display(),
                plugin.display()
            ),
        );
    });
    // Only the two steps left on kelpie's copy, and what `tdd` calls.
    let mattpocock = rig.paths().skills.join("mattpocock");
    assert_eq!(
        skills_in(&mattpocock),
        ["code-review", "codebase-design", "pr", "tdd"]
    );
}

#[test]
fn a_retro_named_kelpies_writes_the_retro_skill_and_the_one_it_calls() {
    let (rig, _runner, _) = Rig::with_pull_request_set("shep", |rig| {
        choose(rig, "retro = { kind = \"kelpie\" }\n");
    });
    let plugin = rig.paths().skills.join("mattpocock");
    let held = skills_in(&plugin);
    for skill in ["retro", "writing-for-agents"] {
        assert!(held.contains(&skill.to_owned()), "{skill}: {held:?}");
    }
}

// A step left out of `[skills]` has a skill only where the runner seeded one,
// so `status` never names a command for a skill the written folder lacks.
#[test]
fn a_step_outside_the_workers_four_names_no_skill_until_the_project_names_one() {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("skills");
    let plugin = folder.join(PLUGIN);
    let chosen = |table: &str| -> crate::settings::StepSkills { toml::from_str(table).unwrap() };

    let skills = Skills::load(&crate::settings::StepSkills::default(), &folder);
    for step in Step::WORKER {
        let command = format!("/mattpocock:{}", step.default_skill());
        assert_eq!(skills.command(step), Some(command.as_str()), "{step}");
        assert!(
            plugin.join("skills").join(step.default_skill()).is_dir(),
            "{step}"
        );
    }
    // `triage`, `spec`, `reset` and `retro` run kelpie's own prompt: no
    // command, no notice that a skill failed, and no folder to point at.
    for step in [Step::Triage, Step::Spec, Step::Reset, Step::Retro] {
        let shown = skills.status().iter().find(|s| s.step == step).unwrap();
        assert_eq!(shown.skill, None, "{step}");
        assert_eq!(shown.fallback, None, "{step}");
        assert_eq!(skills.plugin_dir(step), None, "{step}");
        assert!(
            !plugin.join("skills").join(step.default_skill()).exists(),
            "{step}"
        );
    }
    assert_eq!(skills.notices().count(), 0, "nothing failed to load");

    // Naming one gives it its command, its folder, and what its text calls.
    let skills = Skills::load(
        &chosen("triage = { kind = \"kelpie\" }\nretro = { kind = \"kelpie\" }\n"),
        &folder,
    );
    for step in [Step::Triage, Step::Retro] {
        let command = format!("/mattpocock:{}", step.default_skill());
        assert_eq!(skills.command(step), Some(command.as_str()), "{step}");
        assert_eq!(skills.plugin_dir(step), Some(plugin.as_path()), "{step}");
    }
    assert_eq!(
        skills_in(&plugin),
        [
            "code-review",
            "codebase-design",
            "diagnosing-bugs",
            "domain-modeling",
            "grilling",
            "implement",
            "pr",
            "retro",
            "tdd",
            "triage",
            "writing-for-agents",
        ]
    );
}

#[test]
fn the_issue_writers_copy_still_holds_every_vendored_skill() {
    let dir = tempfile::tempdir().unwrap();
    let plugin = dir.path().join("plugin");
    let command = write_vendored(&plugin, GRILL_WITH_DOCS).unwrap();
    assert_eq!(command, "/mattpocock:grill-with-docs");
    assert_eq!(skills_in(&plugin), every_vendored_skill());
}

#[test]
fn a_misspelt_step_or_skill_name_stops_the_runner() {
    let rig = Rig::new("shep");
    choose(&rig, "reveiw = { kind = \"none\" }\n");
    let err = rig.open().unwrap_err().to_string();
    assert!(err.contains("reveiw"), "{err}");

    let rig = Rig::new("shep");
    choose(
        &rig,
        "ci_fix = { kind = \"plugin\", plugin = \"/p\", skill = \"../up\" }\n",
    );
    let err = rig.open().unwrap_err().to_string();
    assert!(err.contains("must be a skill's name"), "{err}");
}

#[test]
fn a_changed_skill_takes_effect_without_a_restart() {
    let rig = Rig::new("shep");
    let runner = rig.open().unwrap();
    choose(&rig, "ci_fix = { kind = \"none\" }\n");
    let line = runner
        .lock()
        .unwrap()
        .reread(rig.settings(), rig.kelpie_settings())
        .unwrap();
    assert_eq!(
        line.as_deref(),
        Some("settings changed: skills now in effect")
    );
    let status = rig.ask(&runner, "status", None);
    assert_eq!(status["skills"][4]["skill"], json!(null));
    assert_eq!(status["skills"][2]["skill"], "/mattpocock:implement");
}

#[test]
fn a_folder_of_the_projects_inside_kelpies_own_stops_the_runner() {
    let rig = Rig::new("shep");
    let mine = rig.paths().skills.join("ci_fix");
    skill_folder(&mine, "Review.\n");
    choose(
        &rig,
        "ci_fix = { kind = \"path\", path = \"skills/ci_fix\" }\n",
    );
    let err = rig.open().unwrap_err().to_string();
    assert!(
        err.starts_with("setting `skills`: the ci_fix step's "),
        "{err}"
    );
    assert!(mine.join("SKILL.md").is_file(), "the folder is left alone");

    let rig = Rig::new("shep");
    let runner = rig.open().unwrap();
    choose(
        &rig,
        "ci_fix = { kind = \"plugin\", plugin = \"skills/mattpocock\", skill = \"x\" }\n",
    );
    let refused = runner
        .lock()
        .unwrap()
        .reread(rig.settings(), rig.kelpie_settings());
    assert!(refused.is_err(), "{refused:?}");
}

#[test]
fn a_projects_plugin_may_not_take_kelpies_own_names() {
    for name in ["mattpocock", "kelpie-ci_fix"] {
        let rig = Rig::new("shep");
        let plugin = rig.home.path().join("theirs");
        plugin_folder(&plugin, name);
        skill_folder(&plugin.join("skills/code-review"), "Review.\n");
        choose(
            &rig,
            &format!(
                "ci_fix = {{ kind = \"plugin\", plugin = \"{}\", skill = \"code-review\" }}\n",
                plugin.display()
            ),
        );
        let runner = rig.open().unwrap();
        let status = rig.ask(&runner, "status", None);
        assert_eq!(status["skills"][4]["skill"], json!(null), "{name}");
        let fallback = status["skills"][4]["fallback"].as_str().unwrap();
        assert!(fallback.ends_with(&format!("is named {name}, which is kelpie's own")));
    }
}

#[test]
fn the_wording_skill_reaches_a_worker_call_as_a_plugin_beside_the_vendored_one() {
    let (rig, _runner, _) = Rig::with_pull_request("shep");
    let worker = call_of(&rig, Role::Worker);
    let wording = rig.paths().skills.join("kelpie-wording");
    assert!(
        worker.plugin_dirs.contains(&wording),
        "{:?}",
        worker.plugin_dirs
    );
    let text = fs::read_to_string(wording.join("skills/wording/SKILL.md")).unwrap();
    assert!(text.starts_with("---\nname: wording\n"), "{text}");
    let manifest = fs::read_to_string(wording.join(".claude-plugin/plugin.json")).unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&manifest).unwrap()["name"],
        "kelpie-wording"
    );
    // The reviewer's call loads no plugin.
    assert!(call_of(&rig, Role::Reviewer).plugin_dirs.is_empty());
}

#[test]
fn a_replaced_pr_skill_keeps_the_wording_skill_and_a_pr_set_to_none_drops_it() {
    let (rig, _runner, _) = Rig::with_pull_request_set("shep", |rig| {
        let folder = rig.home.path().join("house/body");
        skill_folder(&folder, "---\nname: body\n---\nWrite it.\n");
        choose(
            rig,
            &format!(
                "pr = {{ kind = \"path\", path = \"{}\" }}\n",
                folder.display()
            ),
        );
    });
    let wording = rig.paths().skills.join("kelpie-wording");
    let worker = call_of(&rig, Role::Worker);
    assert!(
        worker.plugin_dirs.contains(&wording),
        "{:?}",
        worker.plugin_dirs
    );

    let (rig, _runner, _) = Rig::with_pull_request_set("shep", |rig| {
        choose(rig, "pr = { kind = \"none\" }\n");
    });
    let worker = call_of(&rig, Role::Worker);
    // The issue skill runs in no step, so `pr = none` keeps it.
    assert_eq!(
        worker.plugin_dirs,
        [
            rig.paths().skills.join("mattpocock"),
            rig.paths().skills.join("kelpie-issue")
        ]
    );
    assert!(!rig.paths().skills.join("kelpie-wording").exists());
    assert_eq!(
        skills_in(&rig.paths().skills.join("mattpocock")),
        [
            "code-review",
            "codebase-design",
            "diagnosing-bugs",
            "implement",
            "tdd",
        ]
    );
}

#[test]
fn a_worker_with_only_the_wording_skill_still_gets_the_headless_rules() {
    let dir = tempfile::tempdir().unwrap();
    let gone = dir.path().join("gone");
    let chosen = |table: &str| -> crate::settings::StepSkills { toml::from_str(table).unwrap() };
    let only_wording = chosen(&format!(
        "tests = {{ kind = \"none\" }}\npr = {{ kind = \"path\", path = \"{}\" }}\n",
        gone.display()
    ));
    let skills = Skills::load(&only_wording, &dir.path().join("skills"));
    assert_eq!(skills.command(Step::Tests), None);
    assert_eq!(skills.command(Step::Pr), None);
    assert!(skills.wording().is_some());
    let rules = skills
        .worker_rules()
        .expect("the wording skill runs headless too");
    assert!(
        rules.starts_with("Kelpie runs this skill headless."),
        "{rules}"
    );

    let none = chosen("tests = { kind = \"none\" }\npr = { kind = \"none\" }\n");
    let skills = Skills::load(&none, &dir.path().join("skills"));
    assert_eq!(skills.worker_rules(), None);
}
