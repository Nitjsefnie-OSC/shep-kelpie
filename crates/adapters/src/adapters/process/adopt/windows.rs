//! There is no handover on Windows, so no call is adopted

use std::io;
use std::process::Output;

use super::super::{Processes, RunError};
use crate::ports::{Adopted, Ending};

impl Processes {
    /// A call is never adopted here, so asking to finish one is an
    /// [`io::ErrorKind::Unsupported`] error
    pub(in crate::adapters) fn output_of_adopted(
        &self,
        _adopted: &Adopted,
        _ending: &Ending,
    ) -> Result<Output, RunError> {
        Err(RunError::Io(io::Error::from(io::ErrorKind::Unsupported)))
    }
}

/// Nothing is adopted here, so there is no group to end
pub fn end_groups(_pgids: &[u32]) {}
