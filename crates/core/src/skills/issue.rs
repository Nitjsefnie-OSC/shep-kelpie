//! Kelpie's skill for writing an issue an implementer can build from
//!
//! `skills/kelpie/issue/SKILL.md` holds the format `board::check` checks, and
//! tells the agent to run `shep kelpie issue check <n>` once it files one. It
//! is written out as a plugin of its own beside the wording skill, and reaches
//! every harness as any other plugin does.

use std::fs;
use std::io;
use std::path::Path;

/// The plugin's name, which the skill's slash command starts with
pub const PLUGIN: &str = "kelpie-issue";

/// The skill's name
pub const SKILL: &str = "issue";

const TEXT: &str = include_str!("../../../../skills/kelpie/issue/SKILL.md");

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
        BLOCKED_BY, CRITERIA, HARNESS_REACH, HARNESSES, KINDS, TOUCHES, WHAT_TO_BUILD, check,
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
    fn the_skill_names_every_part_and_label_the_check_reads() {
        let format = TEXT.split("## Check it").next().unwrap();
        for part in [TOUCHES, HARNESS_REACH, BLOCKED_BY] {
            assert!(format.contains(&format!("**`{part}`**")), "{part}");
        }
        for heading in [WHAT_TO_BUILD, CRITERIA] {
            assert!(format.contains(&format!("**`## {heading}`**")), "{heading}");
        }
        for label in KINDS.iter().chain(&["priority: P0", "priority: P4"]) {
            assert!(format.contains(&format!("`{label}`")), "{label}");
        }
        for harness in HARNESSES {
            assert!(format.contains(&format!("`{harness}`")), "{harness}");
        }
        assert!(TEXT.contains("`shep kelpie issue check <n>`"), "{TEXT}");
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
