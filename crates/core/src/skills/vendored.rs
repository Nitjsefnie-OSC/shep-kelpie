//! Kelpie's copy of mattpocock/skills, built into the binary
//!
//! `scripts/vendor-skills.sh` fetches it into `skills/` at the
//! pinned commit, with its licence and the pin's `git ls-tree` in
//! `blobs.txt`. The runner writes it out as a Claude Code plugin when it
//! starts, so a project installs nothing.
//!
//! A worker's copy holds only the skills its steps run and the ones those
//! call, from `CALLS`: Claude Code lists every skill a plugin holds in
//! every request, so a skill the worker cannot run still costs it tokens.
//! A session the maintainer runs gets the whole copy.

use std::collections::BTreeSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// The upstream repo
pub const UPSTREAM: &str = "https://github.com/mattpocock/skills";

/// The upstream commit every file here was taken from
pub const PIN: &str = "6fd947921b935b7e1e69293a200400f0fdd5c15f";

/// The plugin's name, which each skill's slash command starts with
pub const PLUGIN: &str = "mattpocock";

// Each vendored file, by its path in the upstream repo.
macro_rules! vendored {
    ($($path:literal),* $(,)?) => {
        &[$(($path, include_bytes!(concat!("../../../../skills/", $path)))),*]
    };
}

/// Every vendored file, by its path upstream, and its bytes
pub const FILES: &[(&str, &[u8])] = vendored![
    "LICENSE",
    "skills/engineering/code-review/agents/openai.yaml",
    "skills/engineering/code-review/SKILL.md",
    "skills/engineering/codebase-design/agents/openai.yaml",
    "skills/engineering/codebase-design/DEEPENING.md",
    "skills/engineering/codebase-design/DESIGN-IT-TWICE.md",
    "skills/engineering/codebase-design/SKILL.md",
    "skills/engineering/diagnosing-bugs/agents/openai.yaml",
    "skills/engineering/diagnosing-bugs/scripts/hitl-loop.template.sh",
    "skills/engineering/diagnosing-bugs/SKILL.md",
    "skills/engineering/domain-modeling/ADR-FORMAT.md",
    "skills/engineering/domain-modeling/agents/openai.yaml",
    "skills/engineering/domain-modeling/GLOSSARY-FORMAT.md",
    "skills/engineering/domain-modeling/SKILL.md",
    "skills/engineering/grill-with-docs/agents/openai.yaml",
    "skills/engineering/grill-with-docs/SKILL.md",
    "skills/engineering/implement/agents/openai.yaml",
    "skills/engineering/implement/SKILL.md",
    "skills/engineering/pr/agents/openai.yaml",
    "skills/engineering/pr/CREDITS.md",
    "skills/engineering/pr/SKILL.md",
    "skills/engineering/retro/agents/openai.yaml",
    "skills/engineering/retro/SKILL.md",
    "skills/engineering/tdd/agents/openai.yaml",
    "skills/engineering/tdd/mocking.md",
    "skills/engineering/tdd/SKILL.md",
    "skills/engineering/tdd/tests.md",
    "skills/engineering/to-spec/agents/openai.yaml",
    "skills/engineering/to-spec/SKILL.md",
    "skills/engineering/to-tickets/agents/openai.yaml",
    "skills/engineering/to-tickets/SKILL.md",
    "skills/engineering/triage/AGENT-BRIEF.md",
    "skills/engineering/triage/agents/openai.yaml",
    "skills/engineering/triage/OUT-OF-SCOPE.md",
    "skills/engineering/triage/SKILL.md",
    "skills/productivity/grilling/agents/openai.yaml",
    "skills/productivity/grilling/SKILL.md",
    "skills/productivity/handoff/agents/openai.yaml",
    "skills/productivity/handoff/SKILL.md",
    "skills/productivity/writing-for-agents/agents/openai.yaml",
    "skills/productivity/writing-for-agents/SKILL-MECHANICS.md",
    "skills/productivity/writing-for-agents/SKILL.md",
];

/// The skills each vendored skill's text calls, by name
///
/// This is what a worker's copy is closed over: a step's skill is written
/// with every skill it calls, so the worker can run the call its skill
/// names. A skill that calls none has no entry. A test fails when the
/// vendored text names a call this table does not, or names a skill the
/// table lists that its text no longer calls.
pub(super) const CALLS: [(&str, &[&str]); 5] = [
    ("implement", &["tdd", "code-review"]),
    ("tdd", &["codebase-design", "code-review"]),
    ("triage", &["grilling", "domain-modeling"]),
    ("grill-with-docs", &["grilling", "domain-modeling"]),
    ("retro", &["writing-for-agents"]),
];

/// Which of the vendored skills a plugin folder is written with
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Which {
    /// Every vendored skill, for a session the maintainer runs, which can
    /// call any of them
    All,
    /// Only these, which the caller closed over [`CALLS`] already
    Only(BTreeSet<&'static str>),
}

/// `names`, and every skill they call, transitively, by [`CALLS`]
pub(super) fn with_their_calls(
    names: impl IntoIterator<Item = &'static str>,
) -> BTreeSet<&'static str> {
    let mut chosen: BTreeSet<&'static str> = names.into_iter().collect();
    let mut fresh: Vec<&'static str> = chosen.iter().copied().collect();
    while let Some(name) = fresh.pop() {
        let called = CALLS
            .iter()
            .find(|(caller, _)| *caller == name)
            .map_or(&[][..], |(_, called)| called);
        fresh.extend(called.iter().filter(|c| chosen.insert(c)));
    }
    chosen
}

/// Writes the plugin into `dir`, replacing whatever was there
///
/// A skill lands at `skills/<name>/`, dropping upstream's grouping folder,
/// where Claude Code looks for a plugin's skills. A skill's folder is written
/// whole, with every file the vendored copy has in it. The licence and the
/// manifest are written for `Which::Only` too, and the plugin keeps its name.
///
/// # Errors
///
/// The first file or folder that could not be removed or written.
pub(super) fn write(dir: &Path, which: Which) -> io::Result<()> {
    match fs::remove_dir_all(dir) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => return Err(e),
        _ => {}
    }
    let wanted = |path: &str| match &which {
        Which::All => true,
        Which::Only(names) => skill_of(path).is_none_or(|skill| names.contains(skill)),
    };
    for (path, bytes) in FILES {
        if !wanted(path) {
            continue;
        }
        let to = dir.join(plugin_path(path));
        if let Some(parent) = to.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(to, bytes)?;
    }
    let manifest = serde_json::json!({
        "name": PLUGIN,
        "description": format!("mattpocock/skills at {PIN}, vendored by kelpie"),
        "repository": UPSTREAM,
        "license": "MIT",
    });
    let manifest = serde_json::to_string_pretty(&manifest).expect("the manifest is JSON");
    fs::create_dir_all(dir.join(".claude-plugin"))?;
    fs::write(dir.join(".claude-plugin/plugin.json"), manifest)
}

// The skill a vendored file belongs to, and none for the licence at the
// plugin's root, which every copy holds.
fn skill_of(upstream: &str) -> Option<&str> {
    let (_group, rest) = upstream.strip_prefix("skills/")?.split_once('/')?;
    rest.split('/').next()
}

// `skills/<group>/<name>/<rest>` upstream is `skills/<name>/<rest>` in the plugin.
fn plugin_path(upstream: &str) -> PathBuf {
    match upstream
        .strip_prefix("skills/")
        .and_then(|p| p.split_once('/'))
    {
        Some((_group, rest)) => Path::new("skills").join(rest),
        None => PathBuf::from(upstream),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use sha1::{Digest, Sha1};

    use super::*;

    // The folder `scripts/vendor-skills.sh` fills
    fn vendor_folder() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../skills")
    }

    // The id git gives a file's contents, as `git ls-tree` prints it
    fn blob_id(bytes: &[u8]) -> String {
        let mut hash = Sha1::new();
        hash.update(format!("blob {}\0", bytes.len()).as_bytes());
        hash.update(bytes);
        hash.finalize().iter().map(|b| format!("{b:02x}")).collect()
    }

    fn files_under(dir: &Path, root: &Path, found: &mut Vec<String>) {
        for entry in fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                files_under(&path, root, found);
            } else {
                let relative = path.strip_prefix(root).unwrap();
                found.push(relative.to_str().unwrap().to_owned());
            }
        }
    }

    // Names a skill's text calls: "Skill tool with `x`", with "x" or for "x" and
    // "y", "the `x` skill", and a slash command in backticks. The caller joins
    // the text's lines first, so a call split across two lines still counts.
    fn called_skills(text: &str) -> Vec<&str> {
        let mut called = Vec::new();
        for line in text.lines() {
            let mut from = 0;
            while let Some(at) = line[from..].find("Skill tool") {
                from += at + "Skill tool".len();
                let rest = &line[from..];
                let Some(open) = rest.find(['"', '`']) else {
                    continue;
                };
                if rest[..open].contains('.') || open > 20 {
                    continue;
                }
                let mut rest = &rest[open..];
                while let Some(quote) = rest.chars().next().filter(|c| matches!(c, '"' | '`')) {
                    let Some(len) = rest[1..].find(quote) else {
                        break;
                    };
                    called.push(&rest[1..=len]);
                    rest = &rest[len + 2..];
                    match rest.strip_prefix(" and ").or(rest.strip_prefix(", ")) {
                        Some(next) => rest = next,
                        None => break,
                    }
                }
            }
            for part in line.split("`/").skip(1) {
                if let Some((name, _)) = part.split_once('`')
                    && !name.is_empty()
                    && name.chars().all(|c| c.is_ascii_lowercase() || c == '-')
                {
                    called.push(name);
                }
            }
            let mut rest = line;
            while let Some(at) = rest.find("` skill") {
                if let Some(open) = rest[..at].rfind('`') {
                    let name = &rest[open + 1..at];
                    if !name.is_empty() && name.chars().all(|c| c.is_ascii_lowercase() || c == '-')
                    {
                        called.push(name);
                    }
                }
                rest = &rest[at + "` skill".len()..];
            }
        }
        called
    }

    #[test]
    fn a_call_in_prose_or_split_across_lines_is_found() {
        let text = "Follow the `writing-for-agents` skill.\nCall the Skill\ntool with `tdd`.";
        let joined = text.replace('\n', " ");
        let called = called_skills(&joined);
        assert!(called.contains(&"writing-for-agents"), "{called:?}");
        assert!(called.contains(&"tdd"), "{called:?}");
    }

    #[test]
    fn the_vendored_copies_match_the_pinned_version() {
        let folder = vendor_folder();
        let upstream = fs::read_to_string(folder.join("UPSTREAM")).unwrap();
        assert_eq!(upstream, format!("{UPSTREAM}\n{PIN}\n"));
        let pinned: BTreeMap<&str, &str> = include_str!("../../../../skills/blobs.txt")
            .lines()
            .map(|line| {
                let (id, path) = line.split_once("  ").unwrap();
                (path, id)
            })
            .collect();
        let built: BTreeMap<&str, String> = FILES
            .iter()
            .map(|(path, bytes)| (*path, blob_id(bytes)))
            .collect();
        let built: BTreeMap<&str, &str> = built.iter().map(|(p, id)| (*p, id.as_str())).collect();
        assert_eq!(built, pinned);
    }

    #[test]
    fn every_vendored_file_is_built_in() {
        let folder = vendor_folder();
        let mut found = Vec::new();
        files_under(&folder, &folder, &mut found);
        // Kelpie's own skills sit beside the vendored copy and are not part of it.
        found.retain(|path| {
            path != "UPSTREAM" && path != "blobs.txt" && !path.starts_with("kelpie/")
        });
        found.sort();
        let mut built: Vec<&str> = FILES.iter().map(|(path, _)| *path).collect();
        built.sort_unstable();
        assert_eq!(found, built);
    }

    // Called on purpose without being vendored
    const NOT_VENDORED: &[&str] = &[
        // Repo setup, which a project does once. Kelpie's repo has docs/agents/
        // for it, and a worker must never run it.
        "setup-matt-pocock-skills",
        // A path in backticks, not a skill
        "tmp",
    ];

    #[test]
    fn every_skill_a_vendored_skill_calls_is_vendored() {
        let vendored: Vec<&str> = FILES
            .iter()
            .filter_map(|(path, _)| path.strip_suffix("/SKILL.md"))
            .filter_map(|dir| dir.rsplit('/').next())
            .collect();
        let mut missing = Vec::new();
        for (path, bytes) in FILES
            .iter()
            .filter(|(path, _)| path.ends_with(".md") && path.starts_with("skills/"))
        {
            let text = std::str::from_utf8(bytes).unwrap().replace('\n', " ");
            for name in called_skills(&text) {
                if !vendored.contains(&name) && !NOT_VENDORED.contains(&name) {
                    missing.push(format!("{path} calls {name}"));
                }
            }
        }
        assert!(missing.is_empty(), "{missing:#?}");
    }

    // Names a vendored skill, from its path in `FILES`.
    fn vendored_names() -> BTreeSet<&'static str> {
        FILES
            .iter()
            .filter_map(|(path, _)| path.strip_prefix("skills/")?.split_once('/'))
            .map(|(_group, rest)| rest.split('/').next().unwrap())
            .collect()
    }

    // The skills `name`'s own files call, as its text names them
    fn named_calls(name: &str) -> BTreeSet<String> {
        let mut called = BTreeSet::new();
        for (_, bytes) in FILES
            .iter()
            .filter(|(path, _)| skill_of(path) == Some(name) && path.ends_with(".md"))
        {
            let text = std::str::from_utf8(bytes).unwrap().replace('\n', " ");
            called.extend(
                called_skills(&text)
                    .into_iter()
                    .filter(|c| *c != name && !NOT_VENDORED.contains(c))
                    .map(str::to_owned),
            );
        }
        called
    }

    // The skills [`CALLS`] says `name` calls
    fn listed_calls(name: &str) -> BTreeSet<String> {
        CALLS
            .iter()
            .find(|(caller, _)| *caller == name)
            .into_iter()
            .flat_map(|(_, calls)| calls.iter())
            .map(|call| (*call).to_owned())
            .collect()
    }

    #[test]
    fn calls_match_what_the_vendored_skills_name() {
        let mut unlisted = Vec::new();
        let mut unnamed = Vec::new();
        for name in vendored_names() {
            let (named, listed) = (named_calls(name), listed_calls(name));
            for call in named.difference(&listed) {
                unlisted.push(format!("{name} calls {call}, which CALLS does not list"));
            }
            for call in listed.difference(&named) {
                unnamed.push(format!("CALLS has {name} call {call}, which no text names"));
            }
        }
        assert_eq!(unlisted, Vec::<String>::new(), "a call the text names");
        assert_eq!(unnamed, Vec::<String>::new(), "an entry no text names");
    }

    #[test]
    fn every_skill_named_in_the_calls_table_is_vendored() {
        let vendored = vendored_names();
        let missing: Vec<String> = CALLS
            .iter()
            .flat_map(|(caller, calls)| std::iter::once(*caller).chain(calls.iter().copied()))
            .filter(|name| !vendored.contains(name))
            .map(str::to_owned)
            .collect();
        assert_eq!(missing, Vec::<String>::new());
    }

    #[test]
    fn the_plugin_holds_each_skill_under_its_name_with_the_licence() {
        let dir = tempfile::tempdir().unwrap();
        let plugin = dir.path().join("plugin");
        fs::create_dir_all(plugin.join("stale")).unwrap();
        write(&plugin, Which::All).unwrap();
        assert!(!plugin.join("stale").exists());
        let tests = fs::read(plugin.join("skills/tdd/tests.md")).unwrap();
        assert_eq!(
            tests,
            fs::read(vendor_folder().join("skills/engineering/tdd/tests.md")).unwrap()
        );
        assert!(plugin.join("skills/handoff/SKILL.md").is_file());
        assert!(plugin.join("skills/grill-with-docs/SKILL.md").is_file());
        let licence = fs::read_to_string(plugin.join("LICENSE")).unwrap();
        assert!(licence.starts_with("MIT License"), "{licence}");
        let manifest = fs::read_to_string(plugin.join(".claude-plugin/plugin.json")).unwrap();
        let manifest: serde_json::Value = serde_json::from_str(&manifest).unwrap();
        assert_eq!(manifest["name"], PLUGIN);
    }
}
