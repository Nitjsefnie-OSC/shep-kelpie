//! A skill file a harness cannot read, and the notice kelpie keeps of it

use std::fs;
use std::sync::Mutex;

use serde_json::json;

use super::{call_of, directory, land_bytes, land_settings, marketplace};
use crate::ports::Role;
use crate::runner::{Runner, step};
use crate::test::Rig;

// A runner on a repo that turns on `style@house`, a plugin with a skill whose `description` holds
// an unquoted `: `, which a strict YAML parser refuses, beside one it reads, with a worker's
// first turn run
fn broken() -> (Rig, Mutex<Runner>) {
    let rig = Rig::new("shep");
    let dir = marketplace(&rig, "house", &[("style", "good")]);
    rig.land(
        "markets/house/plugins/style/skills/bad/SKILL.md",
        "---\nname: bad\ndescription: a: b\n---\nRules.\n",
    );
    land_settings(
        &rig,
        &[("style@house", true)],
        &[("house", directory(&dir))],
    );
    let runner = rig.open().unwrap();
    rig.ask(&runner, "add", Some("7"));
    step(&runner).unwrap();
    (rig, runner)
}

#[test]
fn a_skill_whose_frontmatter_is_not_yaml_is_told_by_name() {
    let (_rig, runner) = broken();
    let told: Vec<String> = runner
        .lock()
        .unwrap()
        .skill_notices()
        .filter(|n| n.contains("style@house"))
        .collect();
    assert_eq!(
        told,
        [
            "the repo's plugin style@house has a skill whose frontmatter is not valid YAML: \
             skills/bad/SKILL.md: mapping values are not allowed in this context at line 2, column 15"
        ]
    );
}

#[test]
fn a_frontmatter_that_never_closes_is_told_too() {
    let rig = Rig::new("shep");
    let dir = marketplace(&rig, "house", &[("style", "good")]);
    rig.land(
        "markets/house/plugins/style/skills/bad/SKILL.md",
        "---\nname: bad\ndescription: a rule\n",
    );
    land_settings(
        &rig,
        &[("style@house", true)],
        &[("house", directory(&dir))],
    );
    let runner = rig.open().unwrap();
    let told: Vec<String> = runner
        .lock()
        .unwrap()
        .skill_notices()
        .filter(|n| n.contains("style@house"))
        .collect();
    assert_eq!(
        told,
        [
            "the repo's plugin style@house has a skill whose frontmatter is not valid YAML: \
             skills/bad/SKILL.md: it opens with `---` and never closes it"
        ]
    );
}

#[test]
fn a_told_skill_leaves_the_plugin_and_its_other_skills_loaded() {
    let (rig, runner) = broken();
    let plugin = rig.paths().skills.join("repo/style");
    assert!(call_of(&rig, Role::Worker).plugin_dirs.contains(&plugin));
    assert!(plugin.join("skills/good/SKILL.md").is_file());
    assert!(plugin.join("skills/bad/SKILL.md").is_file());
    let status = rig.ask(&runner, "status", None);
    assert_eq!(
        status["plugins"],
        json!([{
            "plugin": "style@house",
            "skills": ["bad", "good"],
            "unreadable": ["skills/bad/SKILL.md: mapping values are not allowed in this context at line 2, column 15"],
        }])
    );
}

// A plugin with a skill whose `SKILL.md` is bytes no harness reads as text, beside one it fills in
fn not_text() -> (Rig, Mutex<Runner>) {
    let rig = Rig::new("shep");
    let dir = marketplace(&rig, "house", &[("style", "good")]);
    rig.land(
        "markets/house/plugins/style/skills/good/SKILL.md",
        "---\nname: good\n---\nRead `${CLAUDE_PLUGIN_ROOT}/docs/rules.md`.\n",
    );
    land_bytes(
        &rig,
        "markets/house/plugins/style/skills/odd/SKILL.md",
        b"---\nname: odd\n---\n\xFF\xFE",
    );
    land_settings(
        &rig,
        &[("style@house", true)],
        &[("house", directory(&dir))],
    );
    let runner = rig.open().unwrap();
    rig.ask(&runner, "add", Some("7"));
    step(&runner).unwrap();
    (rig, runner)
}

#[test]
fn a_skill_file_that_is_not_text_leaves_the_plugin_and_its_other_skills() {
    let (rig, runner) = not_text();
    let plugin = rig.paths().skills.join("repo/style");
    assert!(
        call_of(&rig, Role::Worker).plugin_dirs.contains(&plugin),
        "the plugin is installed, not skipped"
    );
    let good = fs::read_to_string(plugin.join("skills/good/SKILL.md")).unwrap();
    assert!(!good.contains("${CLAUDE_PLUGIN_ROOT}"), "{good}");
    // The unreadable file is copied as it stands.
    assert_eq!(
        fs::read(plugin.join("skills/odd/SKILL.md")).unwrap(),
        b"---\nname: odd\n---\n\xFF\xFE".to_vec()
    );
    let status = rig.ask(&runner, "status", None);
    assert_eq!(
        status["plugins"],
        json!([{ "plugin": "style@house", "skills": ["good", "odd"] }])
    );
}
