//! The domains a sandbox may reach, `worker.allowed_domains` and `researcher.allowed_domains`

use serde::de::Error;
use serde::{Deserialize, Deserializer};

use super::NonBlank;

#[cfg(test)]
mod tests;

/// Reads a table's `allowed_domains`, refusing the first entry the sandbox would
///
/// The sandbox runtime (`srt`) refuses to start on an entry it does not
/// accept, which a worker's next turn would find out as a `stuck` ruling. An
/// entry is accepted here as the runtime accepts it: one optional leading
/// `*.` and then a host, with an optional `:port` after it, and an IPv6
/// address in brackets. A `*` anywhere else, a scheme, a path, a user or a
/// blank is refused. The port is allowed because the runtime reads it
/// (measured with `srt` 0.0.77, see the design log), and the pi harness
/// check in `settings::agents` reads it too.
///
/// The rule is written from the design log's measurements of the runtime and
/// the failure that showed it (`productionresultssa*.blob.core.windows.net`),
/// not from the runtime's source, which could not be read when it was
/// written. The runtime stays the last word on a host's spelling.
pub(super) fn allowed_domains<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Vec<NonBlank>, D::Error> {
    let entries = Vec::<String>::deserialize(deserializer)?;
    entries
        .into_iter()
        .map(|entry| {
            let named = |why: &str| D::Error::custom(format!("`{entry}` {why}"));
            if entry.trim().is_empty() {
                return Err(named("must not be blank"));
            }
            check(&entry).map_err(named)?;
            NonBlank::try_from(entry.clone()).map_err(named)
        })
        .collect()
}

// Why `entry` is not a domain the sandbox takes, or Ok when it is.
fn check(entry: &str) -> Result<(), &'static str> {
    let rest = entry.strip_prefix("*.").unwrap_or(entry);
    if rest.contains('*') {
        return Err("is refused: `*` may only be one leading `*.` label, as in `*.example.com`");
    }
    if rest.contains("://") {
        return Err("is refused: give the host alone, with no scheme");
    }
    if rest.contains(['/', '?', '#', '@']) || rest.contains(char::is_whitespace) {
        return Err("is refused: give the host alone, with no path, user or space");
    }
    let host = match rest.strip_prefix('[') {
        Some(inner) => {
            let (address, after) = inner
                .split_once(']')
                .ok_or("is refused: an IPv6 address needs its closing `]`")?;
            if address.is_empty()
                || !address
                    .chars()
                    .all(|c| c.is_ascii_hexdigit() || c == ':' || c == '.')
            {
                return Err("is refused: not an IPv6 address");
            }
            return match after {
                "" => Ok(()),
                _ => after
                    .strip_prefix(':')
                    .map_or(Err("is refused: only a `:port` may follow `]`"), port),
            };
        }
        None => match rest.split_once(':') {
            Some((host, number)) => {
                port(number)?;
                host
            }
            None => rest,
        },
    };
    if host.is_empty() {
        return Err("is refused: it names no host");
    }
    let plain = |c: char| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.');
    if host.starts_with('.') || host.contains("..") || !host.chars().all(plain) {
        return Err("is refused: a host is letters, digits, `-` and `_` in labels split by `.`");
    }
    Ok(())
}

// Whether `number` is a TCP port as written, 1 to 65535 with no leading zero.
fn port(number: &str) -> Result<(), &'static str> {
    let valid = !number.starts_with('0') && number.parse::<u16>().is_ok();
    if valid {
        Ok(())
    } else {
        Err("is refused: the port must be a number from 1 to 65535")
    }
}
