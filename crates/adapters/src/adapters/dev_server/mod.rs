//! A project's dev server, started inside a sandbox of its own
//!
//! The server is a child of the runner in a process tree of its own, so shep's
//! stop reaches it (ADR 0005). Its output goes to a file in the build folder.
//! The [`Started`] it returns ends the tree, asking and then killing, when it is
//! dropped. Ready is an answer on the port, on either loopback address.

use std::fs::{File, OpenOptions};
use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr, TcpListener, TcpStream};
use std::path::Path;
use std::process::Stdio;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use super::process::{Kept, stop_child};
use crate::ports::{DevRun, DevServers, Sandbox, Started};

/// How long a server gets to end on SIGTERM before its group is killed
const GRACE: Duration = Duration::from_secs(3);

/// How often a starting server is asked whether it answers
const ASK_EVERY: Duration = Duration::from_millis(250);

/// How long one ask of the port may take
const ASK_FOR: Duration = Duration::from_millis(200);

/// Starts dev servers inside the sandbox
#[derive(Debug, Clone)]
pub struct SandboxedServers {
    sandbox: Arc<dyn Sandbox>,
}

impl super::Base {
    /// Dev servers in the sandbox this runs its calls in
    pub fn dev_servers(&self) -> SandboxedServers {
        SandboxedServers {
            sandbox: Arc::clone(&self.sandbox),
        }
    }
}

// The loopback addresses a server may listen on.
fn loopback(port: u16) -> [SocketAddr; 2] {
    [
        SocketAddr::from((Ipv4Addr::LOCALHOST, port)),
        SocketAddr::from((Ipv6Addr::LOCALHOST, port)),
    ]
}

fn answers(port: u16) -> bool {
    loopback(port)
        .iter()
        .any(|address| TcpStream::connect_timeout(address, ASK_FOR).is_ok())
}

// A server's process tree, ended when it is dropped.
struct Group(Kept);

impl Drop for Group {
    fn drop(&mut self) {
        stop_child(&mut self.0, GRACE);
    }
}

impl DevServers for SandboxedServers {
    fn free(&self, port: u16) -> bool {
        // A server bound to every address can still let a bind to one succeed.
        if answers(port) {
            return false;
        }
        let [v4, v6] = loopback(port);
        let v4_free = TcpListener::bind(v4).is_ok();
        // A machine with no IPv6 loopback has nothing to hold there.
        let v6_free = match TcpListener::bind(v6) {
            Ok(_) => true,
            Err(e) => e.kind() == std::io::ErrorKind::AddrNotAvailable,
        };
        v4_free && v6_free
    }

    fn pick(&self) -> Result<u16, String> {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .map_err(|e| format!("cannot find a free port: {e}"))?;
        listener
            .local_addr()
            .map(|address| address.port())
            .map_err(|e| format!("cannot find a free port: {e}"))
    }

    fn start(&self, run: &DevRun) -> Result<Started, String> {
        // The plan checked the port some time ago. Whatever holds it now would
        // answer in the server's place.
        if !self.free(run.port) {
            return Err(format!("port {} is taken", run.port));
        }
        // Its variables go through `env`, so only the server has them: `srt` puts its proxy
        // socket under its own `TMPDIR`, and a path past 104 bytes keeps it from starting.
        let mut command = crate::spawn::command(crate::spawn::env());
        command
            .args(
                run.env
                    .iter()
                    .map(|(name, value)| format!("{name}={value}")),
            )
            .arg(&run.program)
            .args(&run.args)
            .current_dir(&run.cwd);
        let mut wrapped = self
            .sandbox
            .wrap(&run.policy, &run.settings, &command)
            .map_err(|e| e.to_string())?;
        let (out, err) = log_of(&run.log)?;
        wrapped.stdin(Stdio::null()).stdout(out).stderr(err);
        let kept =
            Kept::spawn(&mut wrapped).map_err(|e| format!("cannot start {}: {e}", run.program))?;
        let mut group = Group(kept);
        let deadline = Instant::now() + run.ready_within;
        loop {
            // An answer counts only while the server started here is running.
            let running = matches!(group.0.child.try_wait(), Ok(None));
            if running && answers(run.port) {
                return Ok(Started::new(true, group));
            }
            if !running || Instant::now() >= deadline {
                return Ok(Started::new(false, group));
            }
            thread::sleep(ASK_EVERY.min(deadline.saturating_duration_since(Instant::now())));
        }
    }
}

// The log, made new: a link a worker left in its place is removed, never written through.
fn log_of(log: &Path) -> Result<(File, File), String> {
    match std::fs::remove_file(log) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
            return Err(format!("cannot clear {}: {}", log.display(), e.kind()));
        }
        _ => {}
    }
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(log)
        .map_err(|e| format!("cannot open {}: {}", log.display(), e.kind()))?;
    let twin = file
        .try_clone()
        .map_err(|e| format!("cannot open {}: {}", log.display(), e.kind()))?;
    Ok((file, twin))
}

#[cfg(test)]
mod tests;
