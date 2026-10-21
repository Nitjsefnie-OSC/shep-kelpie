//! The skills a goose call loads, and a step's prompt as goose's skill command
//!
//! goose lists every skill it finds in the system prompt and offers a
//! `load_skill` tool for them. Left alone it would list the maintainer's own
//! skills from the real home, so the call is closed to those folders
//! ([`NO_READ`]) and given one of its own: `<root>/config/skills`, which
//! [`link`] rebuilds on every call with a link to the folder of each skill its
//! plugins hold. goose follows the links to the plugin folders, which the
//! sandbox reads. The call cannot write the folder ([`policy`]), so it cannot
//! plant a skill for its next turn.
//!
//! goose also lists two skills of its own, `web-search` and `goose-doc-guide`,
//! unless a discovered skill has the name. A stub of each shadows it
//! ([`STUBS`]), so no worker spends turns on `uvx` or on docs it cannot fetch.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use super::files::Files;
use crate::adapters::harness::step_skill;
use crate::adapters::skills::skill_dirs;
use crate::ports::{AgentCall, AgentError, Policy};

/// The folder of the root's config that goose reads user skills from
const FOLDER: &str = "skills";

/// The skill folders of the maintainer's home that goose reads, which no call may
const NO_READ: [&str; 3] = [
    "~/.agents/skills/**",
    "~/.claude/skills/**",
    "~/.config/agents/skills/**",
];

/// The folders of a worktree that goose reads skills from before the call's own
const REPO_SKILLS: [&str; 3] = [".agents/skills", ".goose/skills", ".claude/skills"];

/// The first messages goose runs as a command of its own, which a skill of the
/// same name would not get, a superset of what 1.54.0 holds because the list
/// could not be read from the source here (#798 settles it)
const COMMANDS: [&str; 14] = [
    "help",
    "exit",
    "quit",
    "prompts",
    "prompt",
    "compact",
    "summarize",
    "clear",
    "skills",
    "doctor",
    "mode",
    "plan",
    "endplan",
    "extension",
];

/// The skills goose lists unless a discovered skill has the name, with the
/// description a stub of each gives
const STUBS: [(&str, &str); 2] = [
    (
        "web-search",
        "Not available in this run. The sandbox reaches no website and has no search tool.",
    ),
    (
        "goose-doc-guide",
        "Not available in this run. goose's documentation cannot be fetched from the sandbox.",
    ),
];

/// The folder goose reads the call's skills from
pub(super) fn folder(files: &Files) -> PathBuf {
    files.config().join(FOLDER)
}

/// Closes the maintainer's skill folders to the call and its own to writes
pub(super) fn fence(policy: &mut Policy, files: &Files) {
    policy.no_read.extend(NO_READ.map(str::to_owned));
    policy.no_write.push(folder(files));
}

/// Rebuilds the call's skills folder: a link to each skill its plugins hold,
/// named by the skill's `name`, and a stub for each skill goose has of its own
///
/// The step's skill comes first, then each plugin's in the order the call lists
/// them, and a name already linked is not linked again. Folders inside one
/// plugin go by name, so two skills of one name in it are not left to the file
/// system's order. A skill with no `name`, or a `/` in it, is not linked.
pub(super) fn link(call: &AgentCall, files: &Files) -> Result<(), AgentError> {
    let folder = folder(files);
    let setup = |what: &Path, e: std::io::Error| {
        AgentError::Setup(format!("cannot link {}: {}", what.display(), e.kind()))
    };
    match std::fs::symlink_metadata(&folder) {
        Ok(found) if found.is_dir() => {
            std::fs::remove_dir_all(&folder).map_err(|e| setup(&folder, e))?
        }
        Ok(_) => std::fs::remove_file(&folder).map_err(|e| setup(&folder, e))?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(setup(&folder, e)),
    }
    std::fs::create_dir_all(&folder).map_err(|e| setup(&folder, e))?;

    let mut skills: Vec<PathBuf> = step_skill(call).map(|step| step.dir).into_iter().collect();
    for dir in skill_dirs(call) {
        let entries = std::fs::read_dir(&dir).map_err(|e| setup(&dir, e))?;
        let mut found = Vec::new();
        for entry in entries {
            found.push(entry.map_err(|e| setup(&dir, e))?.path());
        }
        found.sort();
        skills.extend(found);
    }
    let mut taken = HashSet::new();
    for skill in skills {
        let Some(name) = name_of(&skill) else {
            continue;
        };
        if !taken.insert(name.clone()) {
            continue;
        }
        let link = folder.join(&name);
        match std::os::unix::fs::symlink(&skill, &link) {
            // A file system that folds case already holds this name.
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            result => result.map_err(|e| setup(&link, e))?,
        }
    }
    for (name, description) in STUBS {
        // A file system that folds case holds `Web-Search` as `web-search`, and
        // writing into a link would write into the plugin's own folder.
        let held = taken.iter().any(|held| held.eq_ignore_ascii_case(name));
        let stub = folder.join(name);
        if held || std::fs::symlink_metadata(&stub).is_ok() {
            continue;
        }
        std::fs::create_dir_all(&stub).map_err(|e| setup(&stub, e))?;
        let text = format!(
            "---\nname: {name}\ndescription: {description}\n---\n\n\
             This skill is not available in this run.\n"
        );
        let file = stub.join("SKILL.md");
        std::fs::write(&file, text).map_err(|e| setup(&file, e))?;
    }
    Ok(())
}

/// The prompt as goose's skill command, when the call's prompt runs a skill
/// goose can inject
///
/// Kelpie starts a step's prompt with Claude Code's `/<plugin>:<skill>`. goose
/// runs `/<name> <rest>` itself: the model gets the skill's text and then
/// `ARGUMENTS: <rest>`. It splits `<rest>` like a shell when the skill's text
/// holds a placeholder, and a quote in it that never closes ends the turn with
/// no model call. Such a skill keeps the prompt as written, and the model loads
/// it with `load_skill`, when the call `offers_tools`. A call that offers none
/// cannot load it, so its prompt is the skill's text with the placeholders filled
/// in as goose fills them, then `ARGUMENTS: <rest>`. So is a skill the worktree
/// holds a skill of the same name beside ([`REPO_SKILLS`]): goose would give the
/// repo's, and `load_skill` would too.
///
/// `None` leaves the prompt as it is, as it does for a skill no plugin holds
/// and for a skill named like one of goose's own commands ([`COMMANDS`]), which
/// goose would run in the skill's place.
pub(super) fn prompt(call: &AgentCall, offers_tools: bool) -> Option<String> {
    let step = step_skill(call)?;
    let name = name_of(&step.dir)?;
    if COMMANDS.contains(&name.to_ascii_lowercase().as_str()) {
        return None;
    }
    let text = std::fs::read_to_string(step.dir.join("SKILL.md")).ok()?;
    // goose looks in the worktree's folders before the call's, and the first of a name wins.
    let shadowed = repo_holds(&call.cwd, &name);
    if !shadowed && !has_placeholder(&text) {
        return Some(format!("/{name} {}", step.rest).trim_end().to_owned());
    }
    if !shadowed && offers_tools {
        return None;
    }
    let body = with_arguments(body_of(&text), &step.rest);
    Some(
        format!("{body}\n\nARGUMENTS: {}", step.rest)
            .trim_end()
            .to_owned(),
    )
}

// Whether a skill called `name` is in one of the worktree's skill folders
fn repo_holds(cwd: &Path, name: &str) -> bool {
    REPO_SKILLS
        .iter()
        .filter_map(|folder| std::fs::read_dir(cwd.join(folder)).ok())
        .flatten()
        .filter_map(Result::ok)
        .any(|entry| name_of(&entry.path()).as_deref() == Some(name))
}

// `text` with goose's placeholders filled in: `$ARGUMENTS` is `rest` whole, and
// `$ARGUMENTS[n]` and `$n` are its nth word (from 0), split like a shell, or
// nothing past the last. A quote that never closes splits `rest` at blanks, where
// goose ends the turn.
fn with_arguments(text: &str, rest: &str) -> String {
    let words =
        shell_words(rest).unwrap_or_else(|| rest.split_whitespace().map(str::to_owned).collect());
    let nth = |digits: &str| {
        digits
            .parse()
            .ok()
            .and_then(|n: usize| words.get(n))
            .cloned()
    };
    let mut out = String::new();
    let mut tail = text;
    while let Some(at) = tail.find('$') {
        out.push_str(&tail[..at]);
        tail = &tail[at..];
        if let Some(after) = tail.strip_prefix("$ARGUMENTS") {
            let index = after
                .strip_prefix('[')
                .and_then(|inner| inner.split_once(']'))
                .filter(|(digits, _)| {
                    !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit())
                });
            match index {
                Some((digits, more)) => {
                    out.push_str(&nth(digits).unwrap_or_default());
                    tail = more;
                }
                None => {
                    out.push_str(rest);
                    tail = after;
                }
            }
            continue;
        }
        let digits = tail[1..].bytes().take_while(u8::is_ascii_digit).count();
        if digits == 0 {
            out.push('$');
            tail = &tail[1..];
        } else {
            out.push_str(&nth(&tail[1..=digits]).unwrap_or_default());
            tail = &tail[1 + digits..];
        }
    }
    out.push_str(tail);
    out
}

// `text` split into words like a POSIX shell: blanks separate, single quotes
// keep what is in them, double quotes keep it too but for a backslash before
// `"` or `\`, and a backslash outside quotes keeps the next character. `None`
// if a quote never closes.
fn shell_words(text: &str) -> Option<Vec<String>> {
    let mut words = Vec::new();
    let mut word: Option<String> = None;
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        match c {
            c if c.is_whitespace() => words.extend(word.take()),
            '\'' => {
                let word = word.get_or_insert_with(String::new);
                loop {
                    match chars.next()? {
                        '\'' => break,
                        c => word.push(c),
                    }
                }
            }
            '"' => {
                let word = word.get_or_insert_with(String::new);
                loop {
                    match chars.next()? {
                        '"' => break,
                        '\\' => match chars.next()? {
                            c @ ('"' | '\\') => word.push(c),
                            c => {
                                word.push('\\');
                                word.push(c);
                            }
                        },
                        c => word.push(c),
                    }
                }
            }
            '\\' => word.get_or_insert_with(String::new).push(chars.next()?),
            c => word.get_or_insert_with(String::new).push(c),
        }
    }
    words.extend(word);
    Some(words)
}

// The text of a `SKILL.md` after its front matter
fn body_of(text: &str) -> &str {
    let Some(rest) = text.strip_prefix("---") else {
        return text.trim();
    };
    match rest.split_once("\n---") {
        Some((_, after)) => after.trim(),
        None => text.trim(),
    }
}

// The skill's name goose lists it by: the `name` line of the `SKILL.md` in `dir`'s
// front matter, if it can be a file's name too
fn name_of(dir: &Path) -> Option<String> {
    let text = std::fs::read_to_string(dir.join("SKILL.md")).ok()?;
    let mut lines = text.lines();
    if lines.next()?.trim_end() != "---" {
        return None;
    }
    let name = lines
        .take_while(|line| line.trim_end() != "---")
        .find_map(|line| line.strip_prefix("name:"))?;
    let name = name.trim();
    let name = ['"', '\'']
        .iter()
        .find_map(|quote| name.strip_prefix(*quote)?.strip_suffix(*quote))
        .unwrap_or(name);
    let fits = !name.is_empty() && !name.contains(['/', '\0']) && name != "." && name != "..";
    fits.then(|| name.to_owned())
}

// Whether goose splits the arguments for `text`: it holds `$ARGUMENTS`, which
// `$ARGUMENTS[n]` starts with, or a `$` and a number.
fn has_placeholder(text: &str) -> bool {
    text.contains("$ARGUMENTS")
        || text
            .as_bytes()
            .windows(2)
            .any(|pair| pair[0] == b'$' && pair[1].is_ascii_digit())
}
