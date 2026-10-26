//! Folders and files only their owner can use, and the check that one is: [`owner_only`]
//!
//! Uses no other member of the workspace. The only other unsafe code in the
//! workspace than the job object calls is the Windows security calls in
//! `owner_only/windows.rs`.

#![deny(unsafe_code)]
#![doc(test(attr(deny(warnings))))]

pub mod owner_only;
