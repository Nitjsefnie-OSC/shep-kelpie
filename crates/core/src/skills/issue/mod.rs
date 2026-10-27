//! Kelpie's skill for writing an issue an implementer can build from
//!
//! `skills/kelpie/issue/SKILL.md` holds the format `board::check` checks, and
//! tells the agent to run `shep kelpie issue check <n>` once it files one. It
//! is written out as a plugin of its own beside the wording skill, and reaches
//! every harness as any other plugin does, and is linked into each harness's
//! own skills folder for the sessions kelpie does not start.

pub mod user;

use std::fs;
use std::io;
use std::path::Path;

/// The plugin's name, which the skill's slash command starts with
pub const PLUGIN: &str = "kelpie-issue";

/// The skill's name
pub const SKILL: &str = "issue";

const TEXT: &str = include_str!("../../../../../skills/kelpie/issue/SKILL.md");

/// Writes the plugin into `dir`, replacing whatever was there
///
/// # Errors
///
/// The first file or folder that could not be removed or written.
pub(super) fn write(dir: &Path) -> io::Result<()> {
    match fs::remove_dir_all(dir) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => return Err(e),
        _ => {}
    }
    let skill = dir.join("skills").join(SKILL);
    fs::create_dir_all(&skill)?;
    fs::write(skill.join("SKILL.md"), TEXT)?;
    let manifest = serde_json::json!({
        "name": PLUGIN,
        "description": "How to write an issue a kelpie implementer can build from, written by kelpie",
    });
    fs::create_dir_all(dir.join(".claude-plugin"))?;
    fs::write(dir.join(".claude-plugin/plugin.json"), manifest.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board::check::{
        BLOCKED_BY, CRITERIA, Failure, HARNESS_REACH, HARNESSES, KIND, TOUCHES, WHAT_TO_BUILD,
        check, kinds,
    };

    // The skill's example issue, and the labels it says the example is filed with
    fn example() -> (String, Vec<String>) {
        let (_, body) = TEXT.split_once("```markdown\n").expect("the example opens");
        let (body, rest) = body.split_once("\n```").expect("the example closes");
        let line = (rest.lines())
            .find(|l| l.starts_with("It is filed with the labels"))
            .expect("the example's labels");
        let labels = line.split('`').skip(1).step_by(2).map(str::to_owned);
        (body.to_owned(), labels.collect())
    }

    #[test]
    fn the_skills_example_passes_the_check_and_holds_every_part_it_checks() {
        let (body, labels) = example();
        assert_eq!(check("A thing is true", &body, &labels), []);
        for part in [TOUCHES, HARNESS_REACH, BLOCKED_BY] {
            assert!(body.lines().any(|l| l.starts_with(part)), "{part}");
        }
        for heading in [WHAT_TO_BUILD, CRITERIA] {
            assert!(body.contains(&format!("\n## {heading}\n")), "{heading}");
        }
    }

    #[test]
    fn the_skills_example_fails_the_check_without_any_one_of_its_parts() {
        let (body, labels) = example();
        let without = |part: &str| {
            let kept = body.lines().filter(|l| !l.starts_with(part));
            kept.collect::<Vec<_>>().join("\n")
        };
        let renamed = |heading: &str| body.replace(&format!("## {heading}"), "## Notes");
        let adapter = "crates/adapters/src/adapters/pi/skills.rs".to_owned();
        let cut = [
            (without(TOUCHES), Failure::NoTouches),
            (without(HARNESS_REACH), Failure::NoHarnessReach(adapter)),
            (renamed(WHAT_TO_BUILD), Failure::NoWhatToBuild),
            (renamed(CRITERIA), Failure::NoCriteria),
            (
                body.replace("#41", "the other one"),
                Failure::BlockedByNoIssue,
            ),
        ];
        for (body, failure) in cut {
            assert_eq!(
                check("A thing is true", &body, &labels),
                std::slice::from_ref(&failure),
                "{failure}"
            );
        }
        for (dropped, failure) in [(KIND, Failure::NoKind), ("priority:", Failure::NoPriority)] {
            let kept: Vec<String> = labels
                .iter()
                .filter(|l| !l.starts_with(dropped))
                .cloned()
                .collect();
            assert_eq!(
                check("A thing is true", &body, &kept),
                [failure],
                "{dropped}"
            );
        }
    }

    #[test]
    fn the_skill_names_every_part_and_label_the_check_reads() {
        let format = TEXT.split("## Check it").next().unwrap();
        for part in [TOUCHES, HARNESS_REACH, BLOCKED_BY] {
            assert!(format.contains(&format!("**`{part}`**")), "{part}");
        }
        for heading in [WHAT_TO_BUILD, CRITERIA] {
            assert!(format.contains(&format!("**`## {heading}`**")), "{heading}");
        }
        let kind = format!("{KIND}<type>");
        for label in [kind.as_str(), "priority: P0", "priority: P4"] {
            assert!(format.contains(&format!("`{label}`")), "{label}");
        }
        for kind in kinds() {
            assert!(format.contains(&format!("`{kind}`")), "{kind}");
        }
        for harness in HARNESSES {
            assert!(format.contains(&format!("`{harness}`")), "{harness}");
        }
        assert!(TEXT.contains("`shep kelpie issue check <n>`"), "{TEXT}");
        assert!(
            TEXT.contains("shep kelpie issue check --draft <file>"),
            "{TEXT}"
        );
    }

    #[test]
    fn the_skill_is_a_plugin_that_names_itself() {
        let dir = tempfile::tempdir().unwrap();
        let plugin = dir.path().join("plugin");
        fs::create_dir_all(plugin.join("stale")).unwrap();
        write(&plugin).unwrap();
        assert!(!plugin.join("stale").exists());
        let text = fs::read_to_string(plugin.join("skills/issue/SKILL.md")).unwrap();
        assert!(text.starts_with("---\nname: issue\n"), "{text}");
        let manifest = fs::read_to_string(plugin.join(".claude-plugin/plugin.json")).unwrap();
        let manifest: serde_json::Value = serde_json::from_str(&manifest).unwrap();
        assert_eq!(manifest["name"], PLUGIN);
    }
}
