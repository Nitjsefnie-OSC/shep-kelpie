//! The advisor's and the researcher's agent files: a session on any harness
//! with a prompt, and no other key a role of theirs lacks

use super::{folder, name, pair, refused};
use crate::agents::{Agents, Role};
use crate::settings::{AgentHarness, Effort, Harness};

#[test]
fn kelpies_own_advisor_is_opus_at_high_with_a_prompt_that_keeps_it_to_the_code() {
    let agents = Agents::embedded();
    let advisor = agents.get(&name("advisor")).unwrap();
    assert_eq!(advisor.role, Role::Advisor);
    assert_eq!(pair(advisor), ("claude-opus-5-5", Effort::High));
    let prompt = advisor.prompt.as_deref().unwrap();
    assert!(prompt.contains("`diff.patch`"), "{prompt}");
    assert!(prompt.contains("\"choice\""), "{prompt}");
}

#[test]
fn kelpies_own_researcher_reads_what_is_outside_the_code() {
    let agents = Agents::embedded();
    let researcher = agents.get(&name("researcher")).unwrap();
    assert_eq!(researcher.role, Role::Researcher);
    assert_eq!(
        researcher.runs.session().unwrap().0.harness,
        AgentHarness::ClaudeCode
    );
    let prompt = researcher.prompt.as_deref().unwrap();
    for file in ["`rulings.md`", "`threads/`", "`worker.md`", "`maintainer/`"] {
        assert!(prompt.contains(file), "{file}: {prompt}");
    }
}

#[test]
fn the_advisor_and_the_researcher_run_on_any_harness_kelpie_runs() {
    let codex = "---\nrole: advisor\nharness: codex\nmodel: gpt-5.5\neffort: high\nusage: codex\n\
                 ---\nVote.\n";
    let pi = "---\nrole: researcher\nharness: pi\nmodel: qwen3.8:27b\neffort: low\n\
              url: http://box:11434/v1\ncontext: 65536\nusage: none\nlease: gpu\n---\nVote.\n";
    let dir = folder(&[("a.md", codex), ("r.md", pi)]);
    let agents = Agents::load(dir.path()).unwrap();
    let harness = |n: &str| {
        agents
            .get(&name(n))
            .unwrap()
            .runs
            .session()
            .unwrap()
            .0
            .harness
            .harness()
    };
    assert_eq!(harness("a"), Harness::Codex);
    assert_eq!(harness("r"), Harness::Pi);
}

#[test]
fn an_advisor_or_researcher_without_a_prompt_or_with_a_key_of_another_role_is_refused() {
    for role in ["advisor", "researcher"] {
        let head = format!("---\nrole: {role}\nharness: claude-code\nmodel: m\neffort: low\n");
        let err = refused(&[("mine.md", &format!("{head}---\n"))]);
        assert!(err.contains("prompt is the file's body"), "{role}: {err}");
        for key in ["tools: Bash", "paths: [\"x\"]", "second_look: true"] {
            let err = refused(&[("mine.md", &format!("{head}{key}\n---\nVote.\n"))]);
            assert!(
                err.contains(key.split(':').next().unwrap()),
                "{role} {key}: {err}"
            );
        }
    }
}
