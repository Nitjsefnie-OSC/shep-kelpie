//! A process and everything it started, stopped as one: [`tree`], and the
//! [`interrupt`]s that end kelpie, which it passes on to a tree
//!
//! Uses no other member of the workspace. The only unsafe code in the
//! workspace is the Windows job object calls in `tree/windows.rs`.

#![deny(unsafe_code)]
#![doc(test(attr(deny(warnings))))]

pub mod interrupt;
pub mod tree;
