//! Kelpie, a shep dog that runs Claude Code workers from a planned work item
//! to a merged pull request
//!
//! Each project runs as a sheep, kelpie's project runner, under kelpie's own
//! shepherd. `GLOSSARY.md` at the repo root holds the vocabulary, and
//! `docs/design/` the design, read from its README.

#![forbid(unsafe_code)]
#![doc(test(attr(deny(warnings))))]

pub mod adapters;
pub mod doctor;
pub mod flock;
pub mod github;
pub mod issues;
pub mod runner;
pub mod schema;
pub mod sheep;
pub mod upgrade;

// The modules of `shep-kelpie-core`, each kept under the name it had here
pub use shep_kelpie_core::{
    agents, board, bridge, browse, coderabbit, codex, confine, cubic, fence, forge_ledger,
    forwarder, guard, home, hooks, lease, local_paths, local_tickets, metrics, pacer, ports,
    profile, review_bot, settings, shep_home, shepherd, skills, state, ticket, totp, usage, watch,
    webhook, work_item,
};
use shep_kelpie_core::{dep_sources, transcripts};

// The dog, kept under the name it had here
pub use shep_kelpie_dog::dog;

// The leaf crates under `crates/`, each module kept under the name it had here
use shep_kelpie_facts::{mcp_use, request_use, tool_error};
pub use shep_kelpie_panel::panel;
pub use shep_kelpie_process::{spawn, terminal, tools, worktree};
use shep_kelpie_trim::trim;
use shep_kelpie_worker_folder::worker_folder;

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
