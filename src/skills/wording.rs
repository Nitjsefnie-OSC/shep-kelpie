//! Kelpie's writing skill for a worker's pull request body
//!
//! `skills/kelpie/wording/SKILL.md` is the wording test's `mix-v2` guide
//! (`docs/specs/wording-results.md`) cut to the rules that fit a body the
//! maintainer reads. It is written out beside the vendored plugin as a
//! plugin of its own, so the vendored copy stays byte-identical, and it
//! reaches every harness as any other plugin does.

use std::fs;
use std::io;
use std::path::Path;

/// The plugin's name, which the skill's slash command starts with
pub const PLUGIN: &str = "kelpie-wording";

/// The skill's name
pub const SKILL: &str = "wording";

const TEXT: &str = include_str!("../../skills/kelpie/wording/SKILL.md");

/// The skill's slash command
pub(super) fn command() -> String {
    format!("/{PLUGIN}:{SKILL}")
}

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
        "description": "How a worker words its pull request's title and body, written by kelpie",
    });
    fs::create_dir_all(dir.join(".claude-plugin"))?;
    fs::write(dir.join(".claude-plugin/plugin.json"), manifest.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    // The guide `docs/specs/wording-results.md` ends with
    fn guide() -> String {
        let spec = Path::new(env!("CARGO_MANIFEST_DIR")).join("docs/specs/wording-results.md");
        let text = fs::read_to_string(spec).expect("the wording spec reads");
        let (_, guide) = text.split_once("```markdown\n").expect("the guide opens");
        let (guide, _) = guide.split_once("\n```").expect("the guide closes");
        guide.to_owned()
    }

    // The guide's text before its first section, and each section by its heading line
    fn sections(guide: &str) -> (String, Vec<(String, String)>) {
        let mut parts = guide.split("\n## ");
        let head = parts.next().unwrap().to_owned();
        let sections = parts
            .map(|part| {
                let (heading, _) = part.split_once('\n').unwrap();
                (format!("## {heading}"), format!("## {part}"))
            })
            .collect();
        (head, sections)
    }

    #[test]
    fn the_skill_holds_the_guides_rules_for_a_body_with_its_licence_notices() {
        let (head, sections) = sections(&guide());
        let (_, intro) = head.split_once("\n\n").expect("an intro follows the title");
        for notice in [
            "`humanizer` (MIT, Siqi Chen)",
            "`i-have-adhd` (MIT, Ayoub Ghriss)",
        ] {
            assert!(intro.contains(notice), "{intro}");
        }
        let kept = [
            "## Lead with what they need",
            "## Keep it short",
            "## Patterns that read as a bot",
        ];
        assert!(TEXT.contains(intro), "{TEXT}");
        for (heading, section) in &sections {
            let held = TEXT.contains(section.as_str());
            assert_eq!(held, kept.contains(&heading.as_str()), "{heading}");
            assert_eq!(TEXT.contains(heading), held, "{heading}");
        }
    }

    #[test]
    fn the_skill_is_a_plugin_that_names_itself_for_the_slash_command() {
        let dir = tempfile::tempdir().unwrap();
        let plugin = dir.path().join("plugin");
        fs::create_dir_all(plugin.join("stale")).unwrap();
        write(&plugin).unwrap();
        assert!(!plugin.join("stale").exists());
        let text = fs::read_to_string(plugin.join("skills/wording/SKILL.md")).unwrap();
        assert!(text.starts_with("---\nname: wording\n"), "{text}");
        let manifest = fs::read_to_string(plugin.join(".claude-plugin/plugin.json")).unwrap();
        let manifest: serde_json::Value = serde_json::from_str(&manifest).unwrap();
        assert_eq!(
            command(),
            format!("/{}:wording", manifest["name"].as_str().unwrap())
        );
    }
}
