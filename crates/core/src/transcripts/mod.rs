//! Agent sessions' transcript files
//!
//! Under the home folder they lie at `.claude/projects/<folder>/<session>.jsonl`
//! for Claude Code, `.codex/sessions/<year>/<month>/<day>/rollout-*.jsonl` for
//! Codex and `.pi/agent/sessions/<folder>/<time>_<session>.jsonl` for pi.

mod locate;

pub use crate::watch::Format;
pub use locate::{codex_rollout, find, written};

#[cfg(test)]
mod tests;
