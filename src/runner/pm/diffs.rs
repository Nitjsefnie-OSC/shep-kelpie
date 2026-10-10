//! Each open branch's diff from `main`, written into the project manager's
//! folder before it wakes, since it runs no git, and the board section that
//! names them beside each work item's worktree
//!
//! The diffs are those the board's last write read, with the runner's lock
//! let go, so a wake runs no git. The folder is emptied first, so a diff
//! never outlives its work item or a write whose git failed for it.

use std::fmt::Write as _;
use std::path::Path;

use crate::runner::Runner;

/// The folder in the project manager's that holds the diffs
const DIFFS: &str = "diffs";

impl Runner {
    /// Writes `diffs/<issue>.diff` in `folder` for each open work item,
    /// and returns the board's section naming them and each worktree
    ///
    /// A diff git could not give is left out, and noted there and in the log.
    pub(super) fn pm_diffs(&mut self, folder: &Path) -> String {
        let diffs = folder.join(DIFFS);
        let cleared = match std::fs::remove_dir_all(&diffs) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
            _ => std::fs::create_dir_all(&diffs),
        };
        let mut out = String::from(
            "## Code\n\nEach open work item's worktree, which you may read, and its branch's \
             diff from main, written in your folder as you woke.\n\n",
        );
        if let Err(e) = cleared {
            let why = format!("cannot clear {}: {}", diffs.display(), e.kind());
            self.notes
                .push(format!("the project manager's diffs: {why}"));
            let _ = writeln!(out, "(no diffs: {why})");
            return out;
        }
        if self.state.work_items.is_empty() {
            out.push_str("(none)\n");
        }
        let issues: Vec<u64> = self.state.work_items.iter().map(|i| i.issue).collect();
        for issue in issues {
            let file = format!("{DIFFS}/{issue}.diff");
            let written = match self.brief_patch(issue) {
                Some(Ok(diff)) => std::fs::write(folder.join(&file), diff)
                    .map_err(|e| format!("cannot write it: {}", e.kind())),
                Some(Err(why)) => Err(why.clone()),
                None => Err("the board has not read its branch yet".to_owned()),
            };
            let diff = match written {
                Ok(()) => format!("diff `{file}`"),
                Err(why) => {
                    self.notes.push(format!(
                        "no diff of #{issue} for the project manager: {why}"
                    ));
                    format!("no diff: {why}")
                }
            };
            let worktree = self.paths.worktree(issue);
            let _ = writeln!(out, "- #{issue}: worktree `{}`, {diff}", worktree.display());
        }
        out
    }
}
