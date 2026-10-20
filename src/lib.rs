//! Kelpie, a shep dog that runs Claude Code workers from a planned work item
//! to a merged pull request
//!
//! Each project runs as a sheep, kelpie's project runner, under kelpie's own
//! shepherd. `GLOSSARY.md` at the repo root holds the vocabulary, and
//! `docs/design/` the design, read from its README.

#![forbid(unsafe_code)]
#![doc(test(attr(deny(warnings))))]

pub mod adapters;
pub mod agents;
pub mod board;
pub mod bridge;
pub mod browse;
pub mod coderabbit;
pub mod codex;
pub mod confine;
pub mod cubic;
mod dep_sources;
pub mod doctor;
pub mod dog;
pub mod fence;
pub mod flock;
pub mod forge_ledger;
pub mod forwarder;
pub mod github;
pub mod guard;
pub mod home;
pub mod hooks;
pub mod issues;
pub mod lease;
pub mod local_paths;
pub mod local_tickets;
pub mod metrics;
pub mod pacer;
pub mod ports;
pub mod profile;
pub mod review_bot;
pub mod runner;
pub mod schema;
pub mod settings;
pub mod sheep;
pub mod shep_home;
pub mod shepherd;
pub mod skills;
pub mod state;
pub mod ticket;
pub mod totp;
mod transcripts;
pub mod upgrade;
pub mod usage;
pub mod watch;
pub mod webhook;
pub mod work_item;

// The leaf crates under `crates/`, each module kept under the name it had here
use shep_kelpie_facts::{mcp_use, request_use, tool_error};
pub use shep_kelpie_panel::panel;
pub use shep_kelpie_process::{spawn, terminal, tools, worktree};
use shep_kelpie_trim::trim;
use shep_kelpie_worker_folder::worker_folder;

#[cfg(test)]
mod parity;
#[cfg(test)]
mod test;

#[cfg(test)]
mod tests {
    // `tool_error` is in a crate whose own name is not the binary's, so it holds the
    // binary's name as a constant, and this keeps the two the same.
    #[test]
    fn the_program_tool_error_reads_as_kelpies_own_is_this_binary() {
        assert_eq!(
            shep_kelpie_facts::tool_error::KELPIE_PROGRAM,
            env!("CARGO_PKG_NAME")
        );
    }
}
