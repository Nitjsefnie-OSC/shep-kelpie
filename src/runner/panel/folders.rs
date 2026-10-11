//! What kelpie writes into the advisor's and the researcher's folders
//!
//! Neither runs git, `gh` or a command, so each reads what is written for it
//! before it is called. The folder is emptied first, so nothing of an
//! earlier ruling is left to read.
//!
//! The advisor reads the codebase: the ruling, the work item's diff from
//! main, and where the worktree and the checkout are. The researcher reads
//! what is outside it: earlier rulings and their answers, the issue's and the
//! pull request's threads, the worker's session, and the maintainer's own
//! sessions where the project names their folders.

use std::fmt::Write as _;
use std::path::Path;

use crate::runner::Runner;
use crate::state::Ruling;
use crate::transcripts::{self, Keep};

/// How far back the issue's and the pull request's comments are read, in seconds
const THREADS_BACK: u64 = 14 * 24 * 60 * 60;

/// The most comments kept for a thread, the latest of them
const COMMENTS: usize = 80;

/// The most one comment keeps, in characters
const COMMENT: usize = 3_000;

/// The most of the worker's session kept, in bytes
const WORKER_SESSION: usize = 128 * 1024;

/// `text` cut to `most` characters, with an ellipsis
pub(in crate::runner) fn clip(text: &str, most: usize) -> String {
    match text.char_indices().nth(most) {
        Some((at, _)) => format!("{}…", &text[..at]),
        None => text.to_owned(),
    }
}

// An empty `folder`, made if it was not there.
fn emptied(folder: &Path) -> Result<(), String> {
    let failed = |e: std::io::Error| format!("cannot empty {}: {}", folder.display(), e.kind());
    match std::fs::remove_dir_all(folder) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(failed(e)),
        _ => {}
    }
    std::fs::create_dir_all(folder).map_err(failed)
}

// `text` written to `name` in `folder`.
fn put(folder: &Path, name: &str, text: &[u8]) -> Result<(), String> {
    let file = folder.join(name);
    if let Some(parent) = file.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("cannot make {}: {}", parent.display(), e.kind()))?;
    }
    std::fs::write(&file, text)
        .map_err(|e| format!("cannot write {}: {}", file.display(), e.kind()))
}

impl Runner {
    /// Writes the advisor's folder for `ruling`, which `brief` is the text of
    ///
    /// # Errors
    ///
    /// Why the folder could not be written.
    pub(in crate::runner) fn advisor_folder(
        &self,
        ruling: &Ruling,
        brief: &str,
    ) -> Result<(), String> {
        let folder = &self.paths.advisor;
        emptied(folder)?;
        put(
            folder,
            "ruling.md",
            format!("# Ruling {}\n\n{brief}\n", ruling.id).as_bytes(),
        )?;
        let mut where_ = String::from("# Where to read\n\n");
        let item = ruling.issue.and_then(|n| self.state.item(n));
        match item {
            Some(item) => {
                let _ = writeln!(
                    where_,
                    "- The work item's worktree: `{}`, on branch `{}`",
                    item.worktree.display(),
                    item.branch
                );
            }
            None => where_.push_str("- The ruling has no work item open, so no worktree.\n"),
        }
        let _ = writeln!(
            where_,
            "- The project's checkout: `{}`, on its default branch",
            self.settings.git.checkout.display()
        );
        match item.and_then(|i| self.brief_patch(i.issue)) {
            Some(Ok(diff)) => {
                put(folder, "diff.patch", diff)?;
                where_.push_str("- The branch's diff from main: `diff.patch`\n");
            }
            Some(Err(why)) => {
                let _ = writeln!(where_, "- No diff of the branch, since git said: {why}");
            }
            None => where_.push_str("- No diff of the branch: the board has not read it yet.\n"),
        }
        put(folder, "where.md", where_.as_bytes())
    }

    /// Writes the researcher's folder for `ruling`, and returns the names of
    /// the files and folders it holds
    ///
    /// `worker` is the worker's session as text, when it has one.
    ///
    /// # Errors
    ///
    /// Why the folder could not be written.
    pub(in crate::runner) fn researcher_folder(
        &self,
        ruling: &Ruling,
        brief: &str,
        worker: Option<String>,
    ) -> Result<Vec<String>, String> {
        let folder = &self.paths.researcher;
        emptied(folder)?;
        let mut names = vec!["ruling.md".to_owned(), "rulings.md".to_owned()];
        put(
            folder,
            "ruling.md",
            format!("# Ruling {}\n\n{brief}\n", ruling.id).as_bytes(),
        )?;
        put(
            folder,
            "rulings.md",
            super::history::read(&self.paths.rulings).as_bytes(),
        )?;
        let item = ruling.issue.and_then(|n| self.state.item(n));
        let pull_request = ruling.pull_request.or(item.and_then(|i| i.pull_request));
        let threads = self.threads_text(ruling.issue, pull_request);
        if !threads.is_empty() {
            for (name, text) in threads {
                put(folder, &format!("threads/{name}"), text.as_bytes())?;
            }
            names.push("threads/".to_owned());
        }
        if let Some(text) = worker.filter(|t| !t.is_empty()) {
            put(folder, "worker.md", text.as_bytes())?;
            names.push("worker.md".to_owned());
        }
        let theirs = &self.settings.researcher.transcripts;
        let sessions = transcripts::sessions_in(theirs, &self.settings.git.checkout);
        for (n, found) in sessions.iter().enumerate() {
            let name = format!("maintainer/{:02}-{}.md", n + 1, found.harness);
            put(folder, &name, found.text.as_bytes())?;
        }
        if !sessions.is_empty() {
            names.push("maintainer/".to_owned());
        }
        Ok(names)
    }

    // The issue's and the pull request's threads, each a file's name and text.
    fn threads_text(&self, issue: Option<u64>, pull_request: Option<u64>) -> Vec<(String, String)> {
        let forge = &self.ports.forge;
        let mut files = Vec::new();
        let now = self.ports.clock.now();
        let since = crate::ports::Timestamp(now.0.saturating_sub(THREADS_BACK));
        let comments = forge.comments_since(&self.remote, since);
        let mut thread = |name: String, heading: String, number: u64| {
            let mut text = format!("# {heading}\n\n");
            match &comments {
                Ok(all) => {
                    let mine: Vec<_> = all.iter().filter(|c| c.thread == number).collect();
                    let skip = mine.len().saturating_sub(COMMENTS);
                    if mine.is_empty() {
                        text.push_str("(no comments in the last 14 days)\n");
                    }
                    for comment in &mine[skip..] {
                        let bot = if comment.bot { " (a bot)" } else { "" };
                        let _ = write!(
                            text,
                            "## {}{bot}\n\n{}\n\n",
                            comment.author,
                            clip(comment.body.trim(), COMMENT)
                        );
                    }
                }
                Err(e) => {
                    let _ = writeln!(text, "(the comments could not be read: {e})");
                }
            }
            files.push((name, text));
        };
        let mut pieces = Vec::new();
        if let Some(n) = issue {
            let heading = match forge.issue(&self.remote, n) {
                Ok(found) => format!(
                    "Issue #{n}: {}\n\n{}",
                    found.title,
                    clip(found.body.trim(), COMMENT * 2)
                ),
                Err(e) => format!("Issue #{n} (it could not be read: {e})"),
            };
            pieces.push((format!("issue-{n}.md"), heading, n));
        }
        if let Some(n) = pull_request {
            let heading = match forge.reviewed(&self.remote, n) {
                Ok(found) => format!(
                    "Pull request #{n}: {}\n\n{}",
                    found.title,
                    clip(found.body.trim(), COMMENT * 2)
                ),
                Err(e) => format!("Pull request #{n} (it could not be read: {e})"),
            };
            pieces.push((format!("pull-request-{n}.md"), heading, n));
        }
        for (name, heading, number) in pieces {
            thread(name, heading, number);
        }
        files
    }

    /// The worker's session at `transcript` as text, for the researcher
    pub(in crate::runner) fn worker_session_text(
        &self,
        call: &crate::ports::AgentCall,
    ) -> Option<String> {
        let transcript = self.ports.agents.transcript(call)?;
        let format = transcripts::format_of(call.harness.harness())?;
        let at =
            jiff::Timestamp::from_second(i64::try_from(self.ports.clock.now().0).ok()?).ok()?;
        let session = call.session.id().0.as_str();
        let file =
            crate::watch::session_file(format, session, transcript.folder, transcript.threads, at)?;
        let text = std::fs::read_to_string(file).ok()?;
        Some(transcripts::render(
            format,
            &text,
            Keep::Work,
            WORKER_SESSION,
        ))
    }
}
