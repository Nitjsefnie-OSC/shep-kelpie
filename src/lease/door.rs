//! The dog's door: the socket a command asks for a counted lease at
//!
//! A command connects to `$SHEP_HOME/kelpie/dog/lease.sock`, or the socket
//! `KELPIE_LEASE_SOCKET` names, and sends one [`Ask`] line. The dog answers with [`Answer`] lines: queued, then
//! granted. The command holds the lease for as long as it keeps the
//! connection open, so one that ends or dies gives its place back. A
//! worker's sandbox may connect to this socket and to no shep socket.

use std::path::{Component, Path, PathBuf};

use rustix::io::{FdFlags, fcntl_setfd};
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Lines};
use tokio::net::UnixStream;
use tokio::net::unix::{OwnedReadHalf, OwnedWriteHalf};

use crate::home;

/// The variable naming the door, which a runner sets for its workers
pub const SOCKET_VAR: &str = "KELPIE_LEASE_SOCKET";

/// The door's name in the dog's folder
const SOCKET: &str = "lease.sock";

/// The door: `KELPIE_LEASE_SOCKET`, or `lease.sock` in the dog's folder
/// under the shepherd's home
///
/// Never under `KELPIE_HOME`: shep starts the adopted dog without it, so
/// a runner and its workers would look somewhere the dog is not.
///
/// # Errors
///
/// A message when the shepherd's home cannot be worked out.
pub fn socket() -> Result<PathBuf, String> {
    let named = std::env::var_os(SOCKET_VAR).filter(|n| !n.is_empty());
    named.map_or_else(|| Ok(default(&home::shep_home()?)), |n| Ok(n.into()))
}

/// The door of the shepherd at `shep_home`, which the dog binds and a runner
/// lets its workers reach: `KELPIE_LEASE_SOCKET`, unless that is relative or
/// under the shepherd's home outside kelpie's, when it is the default and the
/// message says why
///
/// The path goes into the worker's sandbox, so it must never be shep's
/// own socket. The dog and the runner both ask this, so they cannot
/// disagree. The comparison resolves `..`, symlinks and a volume's case.
pub fn checked_socket(shep_home: &Path) -> (PathBuf, Option<String>) {
    let named = std::env::var_os(SOCKET_VAR).filter(|n| !n.is_empty());
    checked(named.map(PathBuf::from), shep_home)
}

fn default(shep_home: &Path) -> PathBuf {
    home::dog_folder(shep_home).join(SOCKET)
}

fn checked(named: Option<PathBuf>, shep_home: &Path) -> (PathBuf, Option<String>) {
    let default = default(shep_home);
    let Some(named) = named else {
        return (default, None);
    };
    let (real, shep, kelpie) = (
        resolved(&named),
        resolved(shep_home),
        resolved(&home::under(shep_home)),
    );
    let shepherds = real.starts_with(&shep) && !real.starts_with(&kelpie);
    if named.is_absolute() && !shepherds {
        return (named, None);
    }
    let why = format!(
        "{SOCKET_VAR} names {}, which is relative or the shepherd's own: workers get {}",
        named.display(),
        default.display()
    );
    (default, Some(why))
}

// Where `path` leads: `..` applied after the symlinks before it, and each
// part that exists spelled as the volume does, so a case-insensitive one
// cannot be asked for shep's socket by another name. What does not exist
// yet is kept as written.
fn resolved(path: &Path) -> PathBuf {
    let mut real = PathBuf::new();
    for part in path.components() {
        match part {
            Component::ParentDir => {
                real.pop();
            }
            Component::CurDir => {}
            part => {
                real.push(part);
                if let Ok(there) = real.canonicalize() {
                    real = there;
                }
            }
        }
    }
    real
}

/// What a command asks for, its first and only line
// wire format: changing this is a breaking change to the door
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Ask {
    /// The counted lease it wants
    pub take: String,
    /// Its process, for `status`
    pub pid: u32,
    /// What it runs, for `status`
    pub what: String,
    /// Whether its command already runs under a grant from a dog that went
    /// away, so the dog seats it at once rather than counting it twice.
    /// The dog takes it on trust, as it does the rest of the ask.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub running: bool,
}

/// What the dog answers, one line each
// wire format: changing this is a breaking change to the door
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Answer {
    /// Waiting, with this many ahead
    Queued(usize),
    /// Held until the connection closes
    Granted,
    /// Refused, saying why, and the dog closes the connection
    Error(String),
}

/// One command's connection to the door, holding its place while open
#[derive(Debug)]
pub struct Visit {
    answers: Lines<BufReader<OwnedReadHalf>>,
    // Kept open: closing it is how the dog hears the lease given back.
    ask: OwnedWriteHalf,
}

impl Visit {
    /// Connects to the door at `socket` and asks
    ///
    /// # Errors
    ///
    /// A message naming the socket when the dog cannot be reached.
    pub async fn knock(socket: &Path, ask: &Ask) -> Result<Self, String> {
        let stream = UnixStream::connect(socket)
            .await
            .map_err(|e| format!("cannot reach the dog at {}: {e}", socket.display()))?;
        let (read, mut write) = stream.into_split();
        let mut line = serde_json::to_vec(ask).expect("an ask serializes to JSON");
        line.push(b'\n');
        write
            .write_all(&line)
            .await
            .map_err(|e| format!("cannot ask the dog: {e}"))?;
        Ok(Self {
            answers: BufReader::new(read).lines(),
            ask: write,
        })
    }

    /// The dog's next answer, or `None` once it has closed the connection
    ///
    /// # Errors
    ///
    /// A message when the answer cannot be read or is not one.
    pub async fn answer(&mut self) -> Result<Option<Answer>, String> {
        let line = self
            .answers
            .next_line()
            .await
            .map_err(|e| format!("cannot hear the dog: {e}"))?;
        line.map(|line| {
            serde_json::from_str(&line).map_err(|e| format!("the dog answered {line:?} ({e})"))
        })
        .transpose()
    }

    /// Lets processes started from now on inherit the connection, or not
    ///
    /// A command started while it is inheritable holds the lease with its
    /// whole process tree, however the process that asked ends.
    ///
    /// # Errors
    ///
    /// A message when the connection's flags cannot be changed.
    pub fn inheritable(&self, inherit: bool) -> Result<(), String> {
        let flags = if inherit {
            FdFlags::empty()
        } else {
            FdFlags::CLOEXEC
        };
        fcntl_setfd(self.ask.as_ref(), flags)
            .map_err(|e| format!("cannot pass the lease to the command: {e}"))
    }

    /// Gives the lease back, whatever else still holds the connection
    ///
    /// Shutting the socket ends it for every process holding a copy, such
    /// as one the command left running, so the dog hears the end at once.
    pub async fn give_back(mut self) {
        let _ = self.ask.shutdown().await;
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn a_worker_never_gets_shep_s_socket_or_a_relative_one() {
        let shep = Path::new("/home/me/.shep");
        let default = PathBuf::from("/home/me/.shep/kelpie/dog/lease.sock");
        assert_eq!(checked(None, shep), (default.clone(), None));
        for fine in ["/k/door.sock", "/home/me/.shep/kelpie/dog/other.sock"] {
            let fine = PathBuf::from(fine);
            assert_eq!(checked(Some(fine.clone()), shep), (fine, None));
        }
        for bad in ["/home/me/.shep/run/shep.sock", "door.sock"] {
            let (door, why) = checked(Some(PathBuf::from(bad)), shep);
            assert_eq!(door, default, "{bad}");
            assert!(why.unwrap().contains(bad));
        }
    }

    // The dog binds what the runner hands its workers, so both ask `checked`.
    #[test]
    fn the_dog_and_the_runner_resolve_one_door() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path().canonicalize().unwrap();
        let shep = base.join("shep");
        std::fs::create_dir_all(shep.join("run")).unwrap();
        std::fs::create_dir_all(home::dog_folder(&shep)).unwrap();
        let outside = base.join("outside");
        std::fs::create_dir_all(&outside).unwrap();
        std::os::unix::fs::symlink(shep.join("run"), outside.join("link")).unwrap();
        let default = default(&shep);

        let mut bad = vec![
            PathBuf::from("door.sock"),
            shep.join("run/shep.sock"),
            shep.join("kelpie/../run/shep.sock"),
            outside.join("link/shep.sock"),
            outside.join("link/../shep/run/shep.sock"),
        ];
        // Only a case-insensitive volume finds the folder spelled this way.
        let shouting = shep.join("RUN/shep.sock");
        if shouting.parent().unwrap().exists() {
            bad.push(shouting);
        }
        for named in bad {
            let (door, why) = checked(Some(named.clone()), &shep);
            assert_eq!(door, default, "{}", named.display());
            assert!(why.unwrap().contains(&named.display().to_string()));
        }

        let fine = [
            outside.join("door.sock"),
            shep.join("kelpie/dog/other.sock"),
            shep.join("kelpie/../kelpie/dog/other.sock"),
        ];
        for named in fine {
            assert_eq!(checked(Some(named.clone()), &shep), (named, None));
        }
    }

    #[test]
    fn the_wire_format_is_pinned() {
        let ask = Ask {
            take: "cargo-test".into(),
            pid: 42,
            what: "cargo test".into(),
            running: false,
        };
        assert_eq!(
            serde_json::to_value(&ask).unwrap(),
            json!({ "take": "cargo-test", "pid": 42, "what": "cargo test" })
        );
        let rejoining = Ask {
            running: true,
            ..ask.clone()
        };
        let wire = r#"{"take":"cargo-test","pid":42,"what":"cargo test"}"#;
        assert_eq!(serde_json::from_str::<Ask>(wire).unwrap(), ask);
        assert_eq!(
            serde_json::to_value(&rejoining).unwrap()["running"],
            json!(true)
        );
        assert_eq!(
            serde_json::to_value([
                Answer::Queued(2),
                Answer::Granted,
                Answer::Error("no".into())
            ])
            .unwrap(),
            json!([{ "queued": 2 }, "granted", { "error": "no" }])
        );
        let read: Vec<Answer> =
            serde_json::from_str(r#"[{"queued":2},"granted",{"error":"no"}]"#).unwrap();
        assert_eq!(
            read,
            [
                Answer::Queued(2),
                Answer::Granted,
                Answer::Error("no".into())
            ]
        );
    }
}
