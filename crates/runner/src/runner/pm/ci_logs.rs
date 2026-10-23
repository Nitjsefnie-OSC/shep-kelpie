//! The end of a red run's log for each work item parked on a `StillRed`
//! ruling, written into the project manager's folder before it wakes, since
//! it runs no `gh`
//!
//! They are read as a worker's red prompt reads them, with `failed_run_logs`.
//! The folder is emptied first, so a log never outlives its ruling.
//!
//! Each workflow red on `main` has the end of its latest red run's failed log
//! beside them, as `ci/main-<run>.log`.

use std::collections::BTreeMap;
use std::path::Path;

use crate::runner::Runner;
use crate::state::{RulingKind, Stuck};
use crate::ticket::TicketId;
use crate::work_item::Phase;

/// The folder in the project manager's that holds the logs
const CI: &str = "ci";

impl Runner {
    /// Writes `ci/<issue>.log` in `folder` for each work item parked on a
    /// `StillRed` ruling, and returns what the board says of each
    ///
    /// A log that could not be read or written is noted there and in the log.
    pub(super) fn pm_ci_logs(&mut self, folder: &Path) -> BTreeMap<TicketId, String> {
        let ci = folder.join(CI);
        let cleared = match std::fs::remove_dir_all(&ci) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        };
        let red: Vec<(TicketId, String)> = self
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
        for (ticket, head) in red {
            let file = format!("{CI}/{ticket}.log");
            // A `StillRed` ruling is raised only on a pull request's red run
            // (`ci_failed`), never on a merge queue removal, so not `merge_group`.
            let read = (self.ports.forge)
                .failed_run_logs(&self.remote, &head, false)
                .map(|logs| logs.text)
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
                        "no CI log of {} for the project manager: {why}",
                        ticket.shown()
                    ));
                    format!("no CI log: {why}")
                }
            };
            said.insert(ticket, line);
        }
        said
    }
}

impl Runner {
    /// Writes `ci/main-<run>.log` in `folder` for each workflow red on `main`,
    /// and returns what the board says of each, for the section that names them
    ///
    /// Run after [`Runner::pm_ci_logs`], which empties the folder. A log that
    /// could not be read or written is noted there and in the log.
    pub(super) fn pm_main_logs(&mut self, folder: &Path) -> Vec<String> {
        let reds: Vec<(String, u64)> = (self.state.red.iter())
            .map(|r| (r.workflow.clone(), r.run))
            .collect();
        let mut said = Vec::new();
        for (workflow, run) in reds {
            let file = format!("{CI}/main-{run}.log");
            let read = (self.ports.forge)
                .run_log_tail(&self.remote, run)
                .map_err(|e| format!("kelpie could not read it ({e})"))
                .and_then(|log| match log.trim().is_empty() {
                    true => Err(format!("kelpie found no failed log of run {run}")),
                    false => Ok(log),
                });
            let written = read.and_then(|log| {
                std::fs::create_dir_all(folder.join(CI))
                    .and_then(|()| std::fs::write(folder.join(&file), log))
                    .map_err(|e| format!("cannot write it: {}", e.kind()))
            });
            let line = match written {
                Ok(()) => format!("CI log `{file}`"),
                Err(why) => {
                    self.notes.push(format!(
                        "no CI log of {workflow} on main for the project manager: {why}"
                    ));
                    format!("no CI log: {why}")
                }
            };
            said.push(format!("main, {workflow}: {line}"));
        }
        said
    }
}

fn short(head: &str) -> &str {
    head.get(..7).unwrap_or(head)
}
