//! The end of a red run's log for each work item parked on a `StillRed`
//! ruling, written into the project manager's folder before it wakes, since
//! it runs no `gh`
//!
//! They are read as a worker's red prompt reads them, with `failed_run_logs`.
//! The folder is emptied first, so a log never outlives its ruling.

use std::collections::BTreeMap;
use std::path::Path;

use crate::runner::Runner;
use crate::state::{RulingKind, Stuck};
use crate::work_item::Phase;

/// The folder in the project manager's that holds the logs
const CI: &str = "ci";

impl Runner {
    /// Writes `ci/<issue>.log` in `folder` for each work item parked on a
    /// `StillRed` ruling, and returns what the board says of each
    ///
    /// A log that could not be read or written is noted there and in the log.
    pub(super) fn pm_ci_logs(&mut self, folder: &Path) -> BTreeMap<u64, String> {
        let ci = folder.join(CI);
        let cleared = match std::fs::remove_dir_all(&ci) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        };
        let red: Vec<(u64, String)> = self
            .state
            .work_items
            .iter()
            .filter_map(|item| {
                let Phase::Ruling { id } = item.phase else {
                    return None;
                };
                match &self.state.rulings.iter().find(|r| r.id == id)?.kind {
                    RulingKind::Stuck(Stuck::StillRed { head, .. }) => {
                        Some((item.issue, head.clone()))
                    }
                    _ => None,
                }
            })
            .collect();
        let mut said = BTreeMap::new();
        for (issue, head) in red {
            let file = format!("{CI}/{issue}.log");
            // A `StillRed` ruling is raised only on a pull request's red run
            // (`ci_failed`), never on a merge queue removal, so not `merge_group`.
            let read = (self.ports.forge)
                .failed_run_logs(&self.remote, &head, false)
                .map_err(|e| format!("kelpie could not read it ({e})"))
                .and_then(|logs| match logs.trim().is_empty() {
                    true => Err(format!("kelpie found no failed run of {}", short(&head))),
                    false => Ok(logs),
                });
            let written = match (&cleared, read) {
                (Err(e), _) => Err(format!(
                    "cannot clear {}: {}, so any log left there is stale",
                    ci.display(),
                    e.kind()
                )),
                (Ok(()), Err(why)) => Err(why),
                (Ok(()), Ok(logs)) => std::fs::create_dir_all(&ci)
                    .and_then(|()| std::fs::write(folder.join(&file), logs))
                    .map_err(|e| format!("cannot write it: {}", e.kind())),
            };
            let line = match written {
                Ok(()) => format!("CI log `{file}`"),
                Err(why) => {
                    self.notes.push(format!(
                        "no CI log of #{issue} for the project manager: {why}"
                    ));
                    format!("no CI log: {why}")
                }
            };
            said.insert(issue, line);
        }
        said
    }
}

fn short(head: &str) -> &str {
    head.get(..7).unwrap_or(head)
}
