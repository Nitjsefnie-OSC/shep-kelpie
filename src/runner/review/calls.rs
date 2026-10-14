//! Building a reviewer's session call
//!
//! It is not the worker's: it draws a fresh session id and runs under its
//! own throwaway settings, never the worker's own. It keeps its read tools,
//! to check its own work against the worktree, and runs no command, but on
//! Codex, whose only read tool is a shell that kelpie's guard holds to reading.

use std::path::Path;
use std::process::Stdio;

use crate::adapters::CLAUDE_REVIEW_TOOLS;
use crate::ports::{AgentCall, Reach, Role, Session, Severity, Tools};
use crate::settings::{Harness, RoleModel};
use crate::work_item::new_session_id;

/// A fresh session of a reviewer on `model` over issue `issue`'s
/// `worktree`, asked `prompt`, its settings in `worker_folder`
///
/// With `tests`, the fenced reach of a reviewer that may write a failing test
/// for a finding, it also has `Write` on Claude Code, and on pi `write` and on
/// Codex `apply_patch` by the reach alone, which holds each to that folder.
/// A Codex reviewer with no tests to write gets a reach with no write folder,
/// which gives it a shell that only reads.
pub(super) fn reviewer_call(
    issue: u64,
    worktree: &Path,
    worker_folder: &Path,
    model: &RoleModel,
    prompt: String,
    tests: Option<Reach>,
) -> Result<AgentCall, String> {
    let mut call = build_call(Role::Reviewer, issue, worktree, model, prompt)?;
    // Named for the work item, whose reviewer runs beside other items' calls.
    call.settings = worker_folder.join(format!("review-settings-{issue}.json"));
    call.tools = Tools::Review;
    if let Some(reach) = tests {
        // `agent_tools` is Claude's tool list, which pi ignores: its own tools
        // follow the reach's write folder.
        // parity: harness.tool-allow-list
        if model.harness.harness() == Harness::ClaudeCode {
            let own = call.agent_tools.as_deref().unwrap_or(CLAUDE_REVIEW_TOOLS);
            call.agent_tools = Some(format!("{own},Write"));
        }
        call.reach = reach;
    }
    Ok(call)
}

// The shape every call the review makes itself shares: a fresh
// session, the worktree as its folder, no instructions file, no tools, no
// fence and no plugins unless the caller adds them, and the role and prompt.
pub(in crate::runner) fn build_call(
    role: Role,
    issue: u64,
    worktree: &Path,
    model: &RoleModel,
    prompt: String,
) -> Result<AgentCall, String> {
    let session = new_session_id().map_err(|e| format!("cannot draw a session id: {e}"))?;
    Ok(AgentCall {
        issue,
        agent_tools: model.tools.as_ref().map(|t| t.as_str().to_owned()),
        ..AgentCall::new(
            role,
            Tools::Answer,
            model.harness.clone(),
            model.model.as_str().to_owned(),
            model.effort,
            Session::New(session),
            worktree.to_owned(),
            prompt,
        )
    })
}

pub(in crate::runner) fn diff_against(
    repo: &Path,
    worktree: &Path,
    base: &str,
) -> Result<String, String> {
    // Neither a program the global config names nor one the worktree's
    // attributes pick turns the diff a reviewer reads into something else.
    let output = crate::worktree::trusted_command(repo, worktree)
        .map_err(|e| e.to_string())?
        .args(["diff", "--no-ext-diff", "--no-textconv"])
        .arg(crate::worktree::change_since(base))
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("cannot run git diff: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "git diff failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// The severity as qwen's own wire format spells it
pub(super) fn severity_tag(severity: Severity) -> &'static str {
    match severity {
        Severity::Low => "LOW",
        Severity::Medium => "MEDIUM",
        Severity::High => "HIGH",
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::ports::CLEAN;
    use crate::runner::step;
    use crate::settings::AgentHarness;
    use crate::test::{Rig, Scripted, git, linked_worktree, main_commits};
    use crate::trim::deny_with_trim;

    // `origin/main` at the commit the branch was cut from, and one commit of
    // the branch's own on top: `work.txt`.
    fn branch_cut_from_main(home: &Path) -> (PathBuf, PathBuf) {
        let (repo, worktree) = linked_worktree(home);
        git(&repo, &["update-ref", "refs/remotes/origin/main", "main"]);
        std::fs::write(worktree.join("work.txt"), "work\n").unwrap();
        git(&worktree, &["add", "."]);
        git(&worktree, &["commit", "--quiet", "-m", "work"]);
        (repo, worktree)
    }

    #[test]
    fn main_gaining_a_commit_after_the_branch_is_cut_leaves_the_reviewers_diff_alone() {
        let home = tempfile::tempdir().unwrap();
        let (repo, worktree) = branch_cut_from_main(home.path());
        main_commits(&repo, "only_main.txt", "only main has this\n");

        let diff = diff_against(&repo, &worktree, "origin/main").unwrap();

        assert!(diff.contains("+++ b/work.txt"), "{diff}");
        assert!(!diff.contains("only_main.txt"), "{diff}");
    }

    #[test]
    fn an_adopted_item_still_reviews_only_what_came_after_its_arrived_head() {
        let home = tempfile::tempdir().unwrap();
        let (repo, worktree) = branch_cut_from_main(home.path());
        let arrived = git(&worktree, &["rev-parse", "HEAD"]);
        std::fs::write(worktree.join("later.txt"), "later\n").unwrap();
        git(&worktree, &["add", "."]);
        git(&worktree, &["commit", "--quiet", "-m", "later"]);
        main_commits(&repo, "only_main.txt", "only main has this\n");

        let diff = diff_against(&repo, &worktree, &arrived).unwrap();

        assert!(diff.contains("+++ b/later.txt"), "{diff}");
        assert!(!diff.contains("work.txt"), "{diff}");
        assert!(!diff.contains("only_main.txt"), "{diff}");
    }

    #[test]
    fn a_reviewers_session_keeps_its_read_tools_to_check_its_own_work() {
        let rig = Rig::new("shep");
        let runner = rig.open().unwrap();
        rig.ask(&runner, "add", Some("7"));
        rig.forge.open_pull_request(71, "kelpie/7", &[7]);
        rig.claude
            .script([Scripted::Push("work.txt", "work\n"), Scripted::Text(CLEAN)]);
        step(&runner).unwrap(); // the worker's first turn
        step(&runner).unwrap(); // round 1, qwen: clean by default
        step(&runner).unwrap(); // round 2, claude: scripted clean above

        let all = rig.claude.all_seen();
        let reviewer = all
            .iter()
            .find(|s| s.call.role == Role::Reviewer)
            .expect("a reviewer round ran");
        assert_eq!(
            reviewer.settings["permissions"]["deny"],
            deny_with_trim(&["Agent", "Task", "Bash"]),
            "Read, Grep and Glob stay"
        );
        assert!(!reviewer.settings.to_string().contains("\"Read\""));
    }

    #[test]
    fn a_reviewer_writes_only_its_review_tests_folder_when_a_test_command_is_set() {
        let rig = Rig::new("shep");
        rig.test_command("cargo test --test {name}");
        let runner = rig.open().unwrap();
        rig.ask(&runner, "add", Some("7"));
        rig.forge.open_pull_request(71, "kelpie/7", &[7]);
        rig.claude
            .script([Scripted::Push("work.txt", "work\n"), Scripted::Text(CLEAN)]);
        step(&runner).unwrap(); // the worker's first turn
        step(&runner).unwrap(); // round 1, qwen: clean by default
        step(&runner).unwrap(); // round 2, claude: scripted clean above

        let build = runner.lock().unwrap().current().unwrap().build.clone();
        let folder = build.join("review-tests");
        assert!(folder.is_dir(), "kelpie made it before the call");
        let all = rig.claude.all_seen();
        let reviewer = all
            .iter()
            .find(|s| s.call.role == Role::Reviewer)
            .expect("a reviewer round ran");
        assert_eq!(
            reviewer.call.agent_tools.as_deref(),
            Some("Read,Glob,Grep,Write")
        );
        let settings = &reviewer.settings;
        let deny: Vec<&str> = (settings["permissions"]["deny"].as_array().unwrap().iter())
            .filter_map(|d| d.as_str())
            .collect();
        assert!(
            ["Agent", "Task", "Bash"].iter().all(|t| deny.contains(t)),
            "{deny:?}"
        );
        assert!(!deny.contains(&"Write"), "{deny:?}");
        assert_eq!(
            settings["permissions"]["allow"],
            serde_json::json!([format!("Write(/{}/**)", folder.display())])
        );
        let confine = settings["hooks"]["PreToolUse"]
            .as_array()
            .unwrap()
            .iter()
            .find(|e| e["matcher"].as_str().is_some_and(|m| m.contains("Write")))
            .expect("a hook holds the file tools")["hooks"][0]["command"]
            .as_str()
            .unwrap()
            .to_owned();
        assert!(confine.contains("'confine' "), "{confine}");
        // What follows `confine` is every folder it lets the tools write.
        let allowed: Vec<&str> = (confine.split("'confine' ").nth(1).unwrap())
            .split_whitespace()
            .map(|a| a.trim_matches('\''))
            .collect();
        let folder = folder.display().to_string();
        assert_eq!(allowed, [folder.as_str(), folder.as_str()], "{confine}");
    }

    #[test]
    fn a_pi_reviewer_writes_only_its_review_tests_folder_when_a_test_command_is_set() {
        let rig = Rig::new("shep");
        rig.test_command("cargo test --test {name}");
        rig.write_agent(
            "pi-reviewer",
            "---\nrole: reviewer\nharness: pi\nmodel: qwen3.8:27b\neffort: low\n\
             url: http://box:11434/v1\ncontext: 65536\n---\nReview the change against \
             {{BASE}}. Output one finding per line as SEVERITY|file:line|what|why, or CLEAN.\n",
        );
        rig.reviewers(&["pi-reviewer"]);
        let runner = rig.open().unwrap();
        rig.ask(&runner, "add", Some("7"));
        rig.forge.open_pull_request(71, "kelpie/7", &[7]);
        rig.claude
            .script([Scripted::Push("work.txt", "work\n"), Scripted::Text(CLEAN)]);
        step(&runner).unwrap(); // the worker's first turn
        step(&runner).unwrap(); // round 1, the pi reviewer: scripted clean above

        let build = runner.lock().unwrap().current().unwrap().build.clone();
        let folder = build.join("review-tests");
        assert!(folder.is_dir(), "kelpie made it before the call");
        let all = rig.claude.all_seen();
        let reviewer = all
            .iter()
            .find(|s| s.call.role == Role::Reviewer)
            .expect("a reviewer round ran");
        assert!(matches!(reviewer.call.harness, AgentHarness::Pi(_)));
        assert_eq!(reviewer.call.agent_tools, None, "that list is Claude's");
        let fence = reviewer.call.reach.fence.as_deref().expect("a fence");
        assert_eq!(fence.write, std::slice::from_ref(&folder));
        let asked = &reviewer.call.prompt;
        assert!(asked.contains("with the write tool"), "{asked}");
        assert!(asked.contains(&folder.display().to_string()), "{asked}");
    }

    // A reviewer of `harness`'s first round, on a project with no test command.
    fn reviewer_of(harness: &str) -> (Rig, PathBuf, crate::test::Seen) {
        let rig = Rig::new("shep");
        let agent = format!(
            "---\nrole: reviewer\nharness: {harness}\nmodel: m\neffort: low\n{}---\n\
             Review the change against {{{{BASE}}}}. Output one finding per line as \
             SEVERITY|file:line|what|why, or CLEAN.\n",
            match harness {
                "pi" => "url: http://box:11434/v1\ncontext: 65536\n",
                _ => "",
            }
        );
        rig.write_agent("the-reviewer", &agent);
        rig.reviewers(&["the-reviewer"]);
        let runner = rig.open().unwrap();
        rig.ask(&runner, "add", Some("7"));
        rig.forge.open_pull_request(71, "kelpie/7", &[7]);
        rig.claude
            .script([Scripted::Push("work.txt", "work\n"), Scripted::Text(CLEAN)]);
        step(&runner).unwrap(); // the worker's first turn
        step(&runner).unwrap(); // round 1, the reviewer: scripted clean above

        let build = runner.lock().unwrap().current().unwrap().build.clone();
        let seen = rig
            .claude
            .all_seen()
            .into_iter()
            .find(|s| s.call.role == Role::Reviewer)
            .expect("a reviewer round ran");
        (rig, build, seen)
    }

    #[test]
    fn a_codex_reviewer_gets_a_fence_with_no_write_folder() {
        let (_rig, build, seen) = reviewer_of("codex");

        let fence = seen.call.reach.fence.as_deref().expect("a fence");
        assert!(fence.write.is_empty(), "{:?}", fence.write);
        let own = &fence.guard.worktree;
        assert!(own.is_dir(), "kelpie made its empty folder");
        assert_eq!(std::fs::read_dir(own).unwrap().count(), 0);
        assert!(own.starts_with(&build), "{}", own.display());
        assert!(!own.starts_with(&seen.call.cwd), "{}", own.display());
        assert_eq!(seen.call.reach.read, std::slice::from_ref(&seen.call.cwd));
    }

    #[test]
    fn a_claude_code_or_pi_reviewer_gets_no_fence_without_a_test_command() {
        for harness in ["claude-code", "pi"] {
            let (_rig, _, seen) = reviewer_of(harness);

            assert_eq!(seen.call.reach.fence, None, "{harness}");
        }
    }
}
