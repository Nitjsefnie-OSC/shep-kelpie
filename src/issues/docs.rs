//! The docs branch an interactive issue writer's session works on
//!
//! `grill-with-docs` runs `domain-modeling`, which writes glossary terms and
//! ADRs as decisions settle. Those go on a branch of their own, cut from
//! `origin/main` into a worktree the session runs in, where its file tools
//! may write `GLOSSARY.md` and `docs/adr/` and nothing else. When the session
//! ends, kelpie commits what it wrote, pushes the branch, opens a draft pull
//! request and links it from the GitHub issues the session filed.

use std::path::{Path, PathBuf};

use crate::ports::Forge;
use crate::settings::ForgeSlug;
use crate::ticket::TicketId;
use crate::worktree::{self, BASE, Remote, Start, WorktreeError};

use super::Project;

/// The glossary, which the session may write
const GLOSSARY: &str = "GLOSSARY.md";

/// The folder of ADRs, which the session may write
const ADRS: &str = "docs/adr";

/// The longest a request's head may run in a title
const HEAD: usize = 50;

/// A docs branch and the worktree the session works in
#[derive(Debug, Clone)]
pub struct Docs {
    branch: String,
    worktree: PathBuf,
    build: PathBuf,
}

impl Docs {
    /// Cuts a branch named for `run` from a fresh `origin/main`, in a worktree
    /// under the project's issues folder
    ///
    /// # Errors
    ///
    /// A message naming the git command or folder that failed.
    pub(super) fn open(project: &Project<'_>, run: &str) -> Result<Self, String> {
        let folder = project.paths.issues();
        let name = run.chars().take(8).collect::<String>();
        let docs = Self {
            branch: format!("kelpie/docs-{name}"),
            worktree: folder.join(format!("docs-{name}")),
            build: folder.join(format!("docs-{name}.build")),
        };
        let repo = &project.settings.git.checkout;
        let prepared = worktree::prepare(
            repo,
            &docs.worktree,
            &docs.branch,
            Start::Main,
            &Remote::Origin,
            &docs.build,
        );
        if let Err(e) = prepared {
            // A step after the worktree was added may have failed, and the
            // next run names a branch of its own. A branch or folder that
            // was already there is not kelpie's to remove.
            if !matches!(e, WorktreeError::Foreign(_) | WorktreeError::BranchTaken(_)) {
                docs.discard(project);
            }
            return Err(format!("cannot cut a docs branch from main: {e}"));
        }
        Ok(docs)
    }

    /// The branch the docs go on
    pub fn branch(&self) -> &str {
        &self.branch
    }

    /// The worktree the session runs in
    pub fn worktree(&self) -> &Path {
        &self.worktree
    }

    /// What the session's file tools may write: the glossary and the folder
    /// of ADRs, in the worktree with its links resolved
    pub(super) fn writable(&self) -> Vec<PathBuf> {
        let root = (self.worktree.canonicalize()).unwrap_or_else(|_| self.worktree.clone());
        vec![root.join(GLOSSARY), root.join(ADRS)]
    }

    /// Removes the worktree, the branch and the build folder, which hold no
    /// work worth keeping
    pub(super) fn discard(&self, project: &Project<'_>) {
        let repo = &project.settings.git.checkout;
        // Best effort: what is left behind is only disk.
        let _ = worktree::remove(repo, &self.worktree, &self.branch, &self.build, None);
    }

    /// Commits what the session wrote, pushes the branch, opens a draft pull
    /// request into `main` and links it from the issues `filed`, then removes
    /// the worktree. Nothing written leaves nothing to do.
    ///
    /// Returns the lines to tell the maintainer.
    ///
    /// # Errors
    ///
    /// A message naming what failed. A session that wrote a file other than
    /// the glossary and ADRs, or whose commit, push or pull request failed,
    /// keeps its worktree and local branch, and the message names them. A
    /// pushed branch whose pull request failed is deleted from the remote.
    pub(super) fn land(
        &self,
        project: &Project<'_>,
        forge: &dyn Forge,
        request: &str,
        filed: &[TicketId],
    ) -> Result<Vec<String>, String> {
        let repo = &project.settings.git.checkout;
        let files =
            worktree::uncommitted(repo, &self.worktree).map_err(|e| self.kept(format!("{e}")))?;
        if files.is_empty() {
            self.discard(project);
            return Ok(Vec::new());
        }
        let outside: Vec<&str> = (files.iter().map(String::as_str))
            .filter(|f| !is_docs(f))
            .collect();
        if !outside.is_empty() {
            return Err(self.kept(format!(
                "the issue writer changed {}, and may write only {GLOSSARY} and {ADRS}/. \
                 Nothing was committed",
                outside.join(", ")
            )));
        }
        let head = head_of(request);
        let title = format!("docs: record what planning \"{head}\" settled");
        let body = pull_request_body(&head, &files, filed);
        let git = worktree::trusted(repo, &self.worktree).map_err(|e| self.kept(e.to_string()))?;
        let full_ref = format!("refs/heads/{}", self.branch);
        let target = format!("HEAD:{full_ref}");
        let pushed = git(&["add", "--all"])
            .and_then(|_| git(&["commit", "--quiet", "-m", &title, "-m", &body]))
            .and_then(|_| git(&["push", "--quiet", "origin", &target]));
        pushed.map_err(|e| self.kept(format!("cannot commit and push it: {e}")))?;
        let remote = project.remote;
        let number = forge
            .create_pull_request(remote, &self.branch, BASE, &title, &body)
            .map_err(|e| {
                // Nothing links the pushed branch, so take it off the remote
                // again; the commit stays in the local branch.
                let removed = git(&["push", "--quiet", "origin", "--delete", &full_ref]);
                let remote_branch = match removed {
                    Ok(_) => "removed it from the remote again".to_owned(),
                    Err(e) => format!("could not remove it from the remote: {e}"),
                };
                self.kept(format!(
                    "pushed {} but cannot open a pull request: {e}, and {remote_branch}",
                    self.branch
                ))
            })?;
        self.discard(project);
        let mut lines = vec![format!(
            "opened draft pull request #{number} on {} with {}",
            remote.as_str(),
            files.join(", ")
        )];
        lines.extend(link(forge, remote, number, filed));
        match lines.iter().any(|l| l.starts_with("cannot link")) {
            true => Err(lines.join("\n")),
            false => Ok(lines),
        }
    }

    // The message, with where the session's docs are kept.
    fn kept(&self, why: String) -> String {
        format!(
            "{why}. What it wrote is kept on branch {} in {}",
            self.branch,
            self.worktree.display()
        )
    }
}

// Whether a changed file is one the session may write.
fn is_docs(file: &str) -> bool {
    file == GLOSSARY
        || file
            .strip_prefix(ADRS)
            .is_some_and(|rest| rest.starts_with('/'))
}

// The first line of the request, cut to a title's length.
fn head_of(request: &str) -> String {
    let line = request.lines().next().unwrap_or_default().trim();
    let mut head: String = line.chars().take(HEAD).collect();
    if line.chars().count() > HEAD {
        head.push_str("...");
    }
    head.replace('"', "'")
}

fn pull_request_body(head: &str, files: &[String], filed: &[TicketId]) -> String {
    let mut lines = vec![
        format!(
            "Glossary terms and ADRs settled while planning \"{head}\" with `shep kelpie issue --interactive`."
        ),
        String::new(),
    ];
    lines.extend(files.iter().map(|f| format!("- `{f}`")));
    if !filed.is_empty() {
        let issues: Vec<String> = filed.iter().map(TicketId::shown).collect();
        lines.push(String::new());
        lines.push(format!("Filed beside it: {}", issues.join(", ")));
    }
    lines.push(String::new());
    lines.push(
        "The interactive issue writer may write `GLOSSARY.md` and `docs/adr/` and nothing else."
            .to_owned(),
    );
    lines.join("\n")
}

// Comments the pull request on each issue, and says which could not take it.
fn link(forge: &dyn Forge, repo: &ForgeSlug, number: u64, filed: &[TicketId]) -> Vec<String> {
    let text = format!("The glossary terms and ADRs settled while planning this are in #{number}.");
    let mut lines = Vec::new();
    for ticket in filed {
        // A local ticket has no thread on GitHub to link from.
        let Some(issue) = ticket.github() else {
            lines.push(format!("{ticket} is a local ticket, so it is not linked"));
            continue;
        };
        match forge.comment(repo, issue, &text) {
            Ok(_) => lines.push(format!("linked it from #{issue}")),
            Err(e) => lines.push(format!("cannot link it from #{issue}: {e}")),
        }
    }
    lines
}
