//! The project manager's retro over a batch of finished work items
//!
//! After every [`EVERY`] work items finish, and when a drain starts with any
//! finished since the last retro, the project manager's agent is called once
//! to look back on them. It is a one-off call, not a turn of the standing
//! session, so its report does not grow that session's context. Kelpie
//! builds what it reads in code (see [`digest`]) from records it keeps, and
//! the call's tools read and search and change nothing ([`crate::ports::Tools::Retro`]).
//! Its reply is the report, which the runner saves under `retro-reports/unverified`
//! in kelpie's home. Kelpie never acts on a report: the maintainer reads
//! them, consolidates them and fixes what is worth fixing.
//!
//! A retro that fails, times out, comes back empty or is cut short by a stop
//! is logged and skipped, never retried: the batch it covered is not asked
//! about again. It holds no slot, so no work item waits on it.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;

use super::{Runner, StepReport, panel};
use crate::ports::{AgentCall, AgentError, AgentReply, Session, Timestamp};
use crate::skills::Step;
use crate::state::Finished;
use crate::usage;
use crate::work_item::new_session_id;

mod digest;
pub(super) mod journal;

/// How long a retro gets, in seconds
pub(super) const CEILING: u64 = 15 * 60;

/// How many finished work items start a retro
pub(super) const EVERY: usize = 10;

/// What the runner keeps of the retros, in memory only
#[derive(Debug, Default)]
pub(super) struct Book {
    // The last retro's start and the work items it covered, read from the
    // journal the first time they are asked for
    last: Option<Option<(u64, Vec<u64>)>>,
    /// Whether a drain started since the retro last looked
    pub(super) at_drain: bool,
}

// The part of the prompt that asks. It stands alone, for a step set to run
// kelpie's own prompt, and follows the skill's command otherwise.
fn prompt(project: &str, issues: &[u64], digest: &str) -> String {
    let named: Vec<String> = issues.iter().map(|n| format!("#{n}")).collect();
    format!(
        "{} work items of {project} finished since the last retro: {}. Below is a digest \
         kelpie built from its records: one section for each, then the batch's totals.\n\n\
         Run a retrospective on how the batch went. Your reply is the report, in markdown, \
         in this order:\n\
         1. Patterns across the batch: project manager wakes that repeat for one cause, \
         rulings that waited, review findings that were stale or wrong, steps that cost more \
         than the work they did.\n\
         2. Problems in a single work item.\n\
         3. If nothing is worth the maintainer's time, say so in a sentence and write nothing else.\n\n\
         For each problem, suggest the change that would remove it (a setting, a prompt, a \
         check, a steering file). Where a digest line is not enough, you may read the code \
         and the records in your folders. Edit no file and run no command; nothing in the \
         report is applied, and the maintainer reads it later.\n\n---\n\n{digest}",
        issues.len(),
        named.join(", ")
    )
}

impl Runner {
    /// Adds `entry` to the project's journal, made now
    ///
    /// A journal that cannot be written is told and let go.
    pub(super) fn journal(&mut self, entry: journal::Entry) {
        let at = self.ports.clock.now();
        self.journal_at(at, entry);
    }

    /// Adds `entry` to the project's journal, made at `at`
    pub(super) fn journal_at(&mut self, at: Timestamp, entry: journal::Entry) {
        if let Err(why) = journal::append(&self.paths.journal, at, entry) {
            self.notes.push(why);
        }
    }

    /// Adds what `report` says to the journal, if it says anything the retro reads
    pub(super) fn journal_report(&mut self, report: &StepReport) {
        if let Some(entry) = journal::Entry::of(report) {
            self.journal(entry);
        }
    }

    // The last retro's start and the issues it covered, or none yet
    fn last_retro(&mut self) -> (u64, Vec<u64>) {
        let path = &self.paths.journal;
        let last = self.retro_book.last.get_or_insert_with(|| {
            let entries = journal::read(path);
            entries.into_iter().rev().find_map(|s| match s.entry {
                journal::Entry::Retro { issues } => Some((s.at.0, issues)),
                _ => None,
            })
        });
        last.clone().unwrap_or_default()
    }

    // The work items finished since the last retro, oldest first
    fn finished_since_retro(&mut self) -> Vec<Finished> {
        let (since, covered) = self.last_retro();
        (self.state.history.iter())
            .filter(|f| f.at.0 > since || (f.at.0 == since && !covered.contains(&f.issue)))
            .cloned()
            .collect()
    }

    /// Starts the batch retro if one is due and the project manager can run
    /// it, and says whether it did
    ///
    /// Due is [`EVERY`] work items finished since the last retro, or any
    /// when a drain has just started. A draining runner starts no other.
    /// The retro is off unless the skills table names it. A batch that
    /// cannot be asked for now, because the project manager is busy or held
    /// back, is asked for on a later pass, except at a drain, which asks once.
    pub(super) fn retro_launched(&mut self) -> bool {
        let draining_now = std::mem::take(&mut self.retro_book.at_drain);
        let Some(agent) = self.agents.pm.clone() else {
            return false;
        };
        if self.flights.retro_running() || self.skills.off(Step::Retro) {
            return false;
        }
        let batch = self.finished_since_retro();
        let due = batch.len() >= EVERY || (draining_now && !batch.is_empty());
        if !due || (self.draining && !draining_now) {
            return false;
        }
        // A wake in flight has the project manager's account; the retro follows.
        if self.pm.waking() {
            self.retro_book.at_drain |= draining_now;
            return false;
        }
        if !self.pm_available() {
            if draining_now {
                self.notes
                    .push("no retro at the drain, since the project manager is held back".into());
            }
            return false;
        }
        let issues: Vec<u64> = batch.iter().map(|f| f.issue).collect();
        let built = self.retro_call(&agent, &batch);
        // Marked before the call starts: a stop or a crash in the middle of
        // it does not ask again, and a batch that cannot start is let go.
        self.journal(journal::Entry::Retro {
            issues: issues.clone(),
        });
        let now = self.ports.clock.now().0;
        self.retro_book.last = Some(Some((now, issues.clone())));
        match built {
            Ok(call) => {
                self.launch_retro(call, CEILING, agent.name.as_str());
                true
            }
            Err(why) => {
                self.notes
                    .push(format!("no retro of {issues:?}, since {why}"));
                false
            }
        }
    }

    // The project manager's agent called fresh with the digest, in its own
    // folder, with tools that change nothing.
    fn retro_call(
        &mut self,
        agent: &crate::settings::PmAgent,
        batch: &[Finished],
    ) -> Result<AgentCall, String> {
        let (since, _) = self.last_retro();
        let journal = journal::read(&self.paths.journal);
        let rulings = panel::records(&self.paths.rulings);
        let ledger = usage::read(self.ledger.path()).unwrap_or_default();
        let digest = digest::build(&digest::Records {
            project: self.project.as_str(),
            batch,
            journal: &journal,
            rulings: &rulings,
            ledger: &ledger,
            since,
        });
        let issues: Vec<u64> = batch.iter().map(|f| f.issue).collect();
        let ask = prompt(self.project.as_str(), &issues, &digest);
        let ask = self.skills.invoke(Step::Retro, &ask);
        let id = new_session_id().map_err(|e| format!("cannot draw a session id: {e}"))?;
        self.pm_retro_call(agent, Session::New(id), ask)
    }

    /// Records the end of the retro in flight: its reply is saved as a
    /// report, and anything else is logged and let go
    pub(super) fn retro_ended(&mut self, result: Result<AgentReply, AgentError>) {
        let skipped =
            |why: String| format!("the batch retro is skipped and not asked again, since {why}");
        let note = match result {
            Ok(reply) if reply.text.trim().is_empty() => skipped("it came back empty".into()),
            Ok(reply) => match self.save_retro(&reply) {
                Ok(file) => format!("the batch retro is saved unverified in {}", file.display()),
                Err(why) => skipped(format!("it cannot be saved: {why}")),
            },
            Err(AgentError::Stopped) => skipped("the runner stopped during it".into()),
            Err(e) => skipped(format!("it failed: {e}")),
        };
        self.notes.push(note);
    }

    // Writes the report for the batch the last retro covered, under the
    // project's name and the time, so no two collide.
    fn save_retro(&mut self, reply: &AgentReply) -> Result<PathBuf, String> {
        let (_, issues) = self.last_retro();
        let folder = &self.paths.retro_reports;
        fs::create_dir_all(folder).map_err(|e| format!("{}: {}", folder.display(), e.kind()))?;
        let now = self.ports.clock.now();
        let at = i64::try_from(now.0)
            .ok()
            .and_then(|s| jiff::Timestamp::from_second(s).ok())
            .map_or_else(
                || now.0.to_string(),
                |t| t.strftime("%Y%m%dT%H%M%SZ").to_string(),
            );
        let project = self.project.as_str();
        let file = folder.join(format!("{project}-batch-{at}.md"));
        let named: Vec<String> = issues.iter().map(|n| format!("#{n}")).collect();
        let named = named.join(", ");
        let agent = self.agents.pm.as_ref().map_or("none", |a| a.name.as_str());
        let text = format!(
            "# Batch retro: {project} {named}\n\n\
             - Project: {project}\n\
             - Work items: {named}\n\
             - Agent: {agent}\n\
             - Session: {}\n\n\
             ---\n\n\
             {}\n",
            reply.session_id.0,
            reply.text.trim()
        );
        let written = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&file)
            .and_then(|mut f| f.write_all(text.as_bytes()));
        written.map_err(|e| format!("{}: {}", file.display(), e.kind()))?;
        Ok(file)
    }
}

#[cfg(test)]
mod tests;
