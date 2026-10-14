//! A project's dev server, started inside a sandbox of its own
//!
//! The server is a child of the runner in a process group of its own, so shep's
//! stop reaches it (ADR 0005). Its output goes to a file in the build folder.
//! The [`Started`] it returns ends the group, TERM and then KILL, when it is
//! dropped. Ready is an answer on the port, on either loopback address.

use std::fs::{File, OpenOptions};
use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr, TcpListener, TcpStream};
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Child, Stdio};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use super::process::stop_child;
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

impl super::ClaudeCli {
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

// A server's process group, ended when it is dropped.
struct Group(Child);

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
        let mut command = crate::spawn::command(&run.program);
        command.args(&run.args).current_dir(&run.cwd).envs(&run.env);
        let mut wrapped = self
            .sandbox
            .wrap(&run.policy, &run.settings, &command)
            .map_err(|e| e.to_string())?;
        let (out, err) = log_of(&run.log)?;
        wrapped
            .stdin(Stdio::null())
            .stdout(out)
            .stderr(err)
            .process_group(0);
        let child = wrapped
            .spawn()
            .map_err(|e| format!("cannot start {}: {e}", run.program))?;
        let mut group = Group(child);
        let deadline = Instant::now() + run.ready_within;
        loop {
            if answers(run.port) {
                return Ok(Started::new(true, group));
            }
            // A server that has exited will never answer.
            if !matches!(group.0.try_wait(), Ok(None)) || Instant::now() >= deadline {
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
