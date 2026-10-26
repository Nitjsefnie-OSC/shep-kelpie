//! The standards and spec reviewers, which kelpie takes from the code-review skill

use super::{DEFAULTS, baseline, model, name};
use crate::agents::{Agents, Role};
use crate::settings::AgentHarness;

const SKILL: &str = include_str!("../../../../../skills/skills/engineering/code-review/SKILL.md");

#[test]
fn a_standards_reviewer_and_a_spec_reviewer_are_among_the_defaults() {
    let agents = Agents::embedded_with_baselines();
    for axis in ["standards", "spec"] {
        let reviewer = agents.get(&name(axis)).unwrap();
        assert_eq!(reviewer.role, Role::Reviewer, "{axis}");
        assert_eq!(model(reviewer).harness, AgentHarness::ClaudeCode, "{axis}");
        assert!(!reviewer.second_look, "{axis} reads once");
        let prompt = reviewer.prompt.as_deref().unwrap();
        assert!(
            prompt.ends_with("--- diff against {{BASE}} ---\n{{DIFF}}\n--- end ---"),
            "{axis}"
        );
    }
    assert_eq!(
        agents.agents.len(),
        DEFAULTS.len() + baseline::ALL.len(),
        "a default that failed to parse would be dropped"
    );
}

// The smell names the skill lists, as "- **Name**: what it is", between the
// line saying how each reads and the next step's heading.
fn smells() -> Option<Vec<&'static str>> {
    let (_, from) = SKILL.split_once("Each smell reads")?;
    let (list, _) = from.split_once("### 4.")?;
    let names = list
        .lines()
        .filter_map(|line| line.strip_prefix("- **")?.split_once("**:"))
        .map(|(name, _)| name);
    Some(names.collect())
}

#[test]
fn the_standards_prompt_carries_every_smell_the_skill_names() {
    let smells = smells().unwrap_or_default();
    assert!(
        smells.len() >= 12,
        "SKILL.md's smell list moved or changed shape, so this test cannot find it: {smells:?}"
    );
    let agents = Agents::embedded_with_baselines();
    let prompt = agents.get(&name("standards")).unwrap().prompt.clone();
    let prompt = prompt.unwrap();
    for smell in smells {
        assert!(prompt.contains(smell), "the prompt lacks {smell}");
    }
}
