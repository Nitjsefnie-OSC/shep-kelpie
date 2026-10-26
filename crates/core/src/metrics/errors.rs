//! Each tool call that failed in a call, read from its transcript once it
//! ends, by what made it fail

use super::{Family, Recorder};
use crate::ports::Role;
use crate::ports::SeriesKind::Counter;

const ERRORS: Family = Family {
    name: "kelpie_tool_errors_total",
    kind: Counter,
    help: "Tool calls that failed, by what made them fail for the worker and as kind any for the reviewer and the project manager.",
};

// The roles whose tool calls are counted, as their requests are. The worker's are kept by kind,
// thirteen series. The reviewer's and the project manager's are one `any` series each, so their
// error rate stays visible for 2 series and not 26. The one-shot roles would add 130 more for no
// number a retro reads.
const ROLES: [Role; 3] = [Role::Worker, Role::Reviewer, Role::Pm];

// The kind of every error of a role that is not counted by kind
const ANY: &str = "any";

impl Recorder {
    /// Counts one failed tool call of `kind`, made by a call of `role`, if the role's
    /// tool calls are counted: by kind for the worker's, and as `any` for the rest
    pub fn tool_error(&self, role: &str, kind: &str) {
        let Some(counted) = ROLES.iter().find(|counted| counted.as_str() == role) else {
            return;
        };
        let kind = if *counted == Role::Worker { kind } else { ANY };
        self.add(
            &ERRORS,
            vec![("role", role.to_owned()), ("kind", kind.to_owned())],
            1.0,
        );
    }
}
