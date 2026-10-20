//! The processes kelpie starts: [`spawn`], the [`terminal`], [`tools`] and git
//! [`worktree`]s
//!
//! Uses no other member of the workspace.

#![forbid(unsafe_code)]
#![doc(test(attr(deny(warnings))))]

pub mod spawn;
pub mod terminal;
pub mod tools;
pub mod worktree;
