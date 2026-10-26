//! The issue writer's agent file: a Claude Code session whose body is its prompt

use super::{folder, model, name, pair, refused};
use crate::agents::{Agents, ISSUE_WRITER, Role};
use crate::settings::{Account, AgentHarness, Effort, Limit};

const WRITER: &str = "---\nrole: issue-writer\nharness: claude-code\nmodel: claude-opus-5-5\n\
                      effort: high\n---\nWrite the issues.\n";

#[test]
fn kelpies_issue_writer_is_opus_at_medium_with_its_rules_as_its_prompt() {
    let agents = Agents::embedded();
    let writer = agents.get(&name(ISSUE_WRITER)).unwrap();
    assert_eq!(writer.role, Role::IssueWriter);
    assert_eq!(pair(writer), ("claude-opus-5-5", Effort::Medium));
    assert_eq!(model(writer).harness, AgentHarness::ClaudeCode);
    assert_eq!(super::limit(writer), &Limit::Account(Account::Claude));
    let prompt = writer.prompt.as_deref().unwrap();
    for rule in [
        "One issue is one pull request that works, tests and ships on its own",
        "Split a request into the smallest such pieces",
        "Never cut a piece short to look small",
        "`Touches:` line",
        "Size a piece by what its change reaches",
        "search the repo with Grep for the whole word, listing files only",
        "A piece whose `Touches:` passes 13 files splits by module",
        "the new shape first, with a shim the old readers keep using",
        "A piece that cannot split says why in its body",
        "only after its links are in",
        "## Acceptance criteria",
        "At most one `agent:<name>` label",
    ] {
        assert!(prompt.contains(rule), "{rule}");
    }
    // An agent is never asked to weigh what a model costs.
    for word in ["cost", "price", "cheap", "expensive", "budget", "spend"] {
        assert!(!prompt.to_lowercase().contains(word), "{word}");
    }
}

#[test]
fn a_file_with_the_issue_writer_role_replaces_kelpies() {
    let dir = folder(&[("issue-writer.md", WRITER)]);
    let agents = Agents::load(dir.path()).unwrap();
    let writer = agents.get(&name(ISSUE_WRITER)).unwrap();
    assert_eq!(pair(writer), ("claude-opus-5-5", Effort::High));
    assert_eq!(writer.prompt.as_deref(), Some("Write the issues."));
}

#[test]
fn an_issue_writer_runs_on_claude_code_with_a_prompt_and_no_other_roles_keys() {
    let pi = WRITER.replace("claude-code", "pi");
    let paths = WRITER.replace("effort: high\n", "effort: high\npaths: [\"src/**\"]\n");
    let cases = [
        (pi.as_str(), "the issue writer runs on claude-code, not pi"),
        (
            "---\nrole: issue-writer\nharness: claude-code\nmodel: m\neffort: low\n---\n",
            "the issue writer's prompt is the file's body",
        ),
        (paths.as_str(), "unknown field `paths`"),
    ];
    for (text, why) in cases {
        let err = refused(&[("x.md", text)]);
        assert!(err.contains(why), "{err}\nwanted: {why}");
    }
}
