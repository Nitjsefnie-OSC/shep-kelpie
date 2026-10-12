//! The report `shep kelpie usage --forge` prints

use super::Line;
use crate::ports::Timestamp;

/// The report over `lines`, those at or after `since` when it is given
pub fn report(lines: &[Line], since: Option<Timestamp>) -> Vec<String> {
    let _ = (lines, since);
    Vec::new()
}
