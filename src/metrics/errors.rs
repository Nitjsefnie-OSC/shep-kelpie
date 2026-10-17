//! Each tool call that failed in a call, read from its transcript once it
//! ends, by what made it fail

use super::{Family, Recorder};
use crate::ports::Role;
use crate::ports::SeriesKind::Counter;

const ERRORS: Family = Family {
    name: "kelpie_tool_errors_total",
    kind: Counter,
    help: "Tool calls that failed, by what made them fail.",
};

// The roles whose tool calls are counted, as their requests are. Thirteen kinds for each of the
// three is 39 series; the one-shot roles would add 130 more for no number a retro reads.
const ROLES: [Role; 3] = [Role::Worker, Role::Reviewer, Role::Pm];

impl Recorder {
    /// Counts one failed tool call of `kind`, made by a call of `role`, if the role's
    /// tool calls are counted
    pub(crate) fn tool_error(&self, role: &str, kind: &str) {
        if !ROLES.iter().any(|counted| counted.as_str() == role) {
            return;
        }
        self.add(
            &ERRORS,
            vec![("role", role.to_owned()), ("kind", kind.to_owned())],
            1.0,
        );
    }
}
