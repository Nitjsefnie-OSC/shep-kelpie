//! What the harness adapters share, so none builds on another
//!
//! The process and sandbox [`Base`] each adapter is made on, the policy every
//! call starts from, and the small helpers that read a CLI's output and files.

mod adopted;
mod base;
mod carried;
mod labels;
mod policy;
mod records;
mod skill;
mod text;

pub(crate) use adopted::adoption_of;
pub use base::Base;
pub(crate) use carried::{Carried, Noted};
pub use labels::LambLabels;
pub use policy::fence_policy;
pub(crate) use policy::{bind_in_scratch, call_policy, dial_the_forwarder};
pub(crate) use records::Records;
pub(crate) use skill::{StepSkill, step_skill};
#[cfg(test)]
pub(crate) use text::ERROR_TAIL;
pub(crate) use text::{host_of, kept_server, prompt_file, tail};
