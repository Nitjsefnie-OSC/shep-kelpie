//! The skill a step's prompt starts with

use std::path::{Path, PathBuf};

use crate::ports::AgentCall;
use crate::skills::split_command;

/// The skill a step's prompt starts with, as Claude Code's `/<plugin>:<skill>`
pub(crate) struct StepSkill {
    /// The skill's name
    pub(crate) name: String,
    /// Its folder, which holds its `SKILL.md`
    pub(crate) dir: PathBuf,
    /// The prompt after the command
    pub(crate) rest: String,
}

/// The skill `call`'s prompt runs, from one of its plugins, if any
pub(crate) fn step_skill(call: &AgentCall) -> Option<StepSkill> {
    let (Some(command), rest) = split_command(&call.prompt) else {
        return None;
    };
    let (plugin, skill) = command.trim_start_matches('/').split_once(':')?;
    let dir = call
        .plugin_dirs
        .iter()
        .filter(|dir| plugin_name(dir).as_deref() == Some(plugin))
        .map(|dir| dir.join("skills").join(skill))
        .find(|dir| dir.join("SKILL.md").is_file())?;
    Some(StepSkill {
        name: skill.to_owned(),
        dir,
        rest: rest.to_owned(),
    })
}

// A plugin's name, from its manifest
fn plugin_name(dir: &Path) -> Option<String> {
    let text = std::fs::read_to_string(dir.join(".claude-plugin/plugin.json")).ok()?;
    let manifest: serde_json::Value = serde_json::from_str(&text).ok()?;
    manifest["name"].as_str().map(str::to_owned)
}
