//! The rig the verbs' tests use, gathered from the crates that own its parts
//!
//! The core crate holds the stand-ins that name only it, the adapters the ones that
//! name an adapter, and the runner the `Rig` that wires them, with the helpers of
//! `test_support`. Each is kept under the path the verbs' tests already name.

pub use shep_kelpie_adapters::test::*;
pub use shep_kelpie_core::test::*;
pub use shep_kelpie_runner::test::*;
