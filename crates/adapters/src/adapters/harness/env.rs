//! A call's variables as `env` sets them, inside the sandbox

use std::ffi::OsString;

/// `vars` as the `NAME=VALUE` arguments `env` takes before its program
///
/// Set this way rather than on the command, since the sandbox sets some of
/// the same names after the command's own: crabpot's `GIT_CONFIG_PARAMETERS`
/// would replace the fence's.
pub(crate) fn assignments(
    vars: impl IntoIterator<Item = (String, OsString)>,
) -> impl Iterator<Item = OsString> {
    vars.into_iter().map(|(name, value)| {
        let mut pair = OsString::from(format!("{name}="));
        pair.push(value);
        pair
    })
}
