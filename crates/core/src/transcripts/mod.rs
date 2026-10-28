//! Agent sessions' transcript files
//!
//! Claude Code writes them at `projects/<folder>/<session>.jsonl` in the config folder each
//! call keeps under its project's kelpie home. Under the home folder they lie at
//! `.codex/sessions/<year>/<month>/<day>/rollout-*.jsonl` for Codex and
//! `.pi/agent/sessions/<folder>/<time>_<session>.jsonl` for pi.

mod locate;

pub use crate::watch::Format;
pub use locate::{codex_rollout, find, written};

#[cfg(test)]
mod tests;
