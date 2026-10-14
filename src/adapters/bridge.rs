//! The outside half of [`crate::bridge`]: the browser's MCP server behind one socket, for one call
//!
//! The socket has a name no one can guess, in the call's settings folder, and
//! the sandbox may connect to that path alone. The name is kept in a file, so
//! a later bridge for the same work item binds the same path: a goose session
//! keeps the command it began with, which names that path. Connections are served one at
//! a time, each by a fresh server that stops when it ends. Every line goes
//! through the [`Filter`] on its way in and out. Dropping the [`Bridge`] stops
//! the server and removes the socket. What a server prints on stderr, and why
//! one could not start, go to `server.log` in its folder.
//!
//! The browser the server launches is held to the project's hosts twice: by
//! Chromium's resolver rules, and by a [`Proxy`] that every request goes
//! through, which also holds it to ports. The bridge writes the config that
//! says so, `config.json` in the server's folder.

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Write};
use std::net::Shutdown;
use std::os::unix::net::{UnixListener, UnixStream};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;
use std::time::Duration;

use serde_json::json;

use super::forwarder::unguessable;
use super::process::stop_child;
use crate::bridge::{Filter, Passage};
use proxy::{Origins, Proxy};

mod proxy;

/// The file in a server's folder that its stderr and failures to start are appended to
const LOG: &str = "server.log";

/// How long a server gets to end on SIGTERM before its group is killed
const GRACE: Duration = Duration::from_secs(5);

/// The command that serves one connection, which runs outside the sandbox
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Server {
    /// The program
    pub(crate) command: PathBuf,
    /// Its arguments, without `--config`, which the bridge adds
    pub(crate) args: Vec<String>,
    /// Variables set for it, over kelpie's own
    pub(crate) env: BTreeMap<String, String>,
    /// The folder it starts in, where the bridge writes its config
    pub(crate) cwd: PathBuf,
}

/// The socket one call reaches its browser through, open until dropped
#[derive(Debug)]
pub(crate) struct Bridge {
    socket: PathBuf,
    stopping: Arc<AtomicBool>,
    // The connection being served, which a drop closes to end it
    serving: Arc<Mutex<Option<UnixStream>>>,
    thread: Option<thread::JoinHandle<()>>,
    // Ended after the server it holds, by being dropped last
    _proxy: Proxy,
}

impl Bridge {
    /// Opens a socket in `folder`, served by `server` through `filter`
    ///
    /// The socket's name is the one `name_file` holds, drawn and written there if it holds none.
    /// A socket a killed kelpie left under that name is replaced.
    ///
    /// # Errors
    ///
    /// The reason, when the name cannot be read or kept, the socket's path is too long to bind, it
    /// cannot be opened, or the proxy or the browser's config cannot be.
    pub(crate) fn open(
        folder: &Path,
        name_file: &Path,
        mut server: Server,
        filter: Filter,
    ) -> Result<Self, String> {
        let socket = folder.join(format!("{}.sock", kept_name(name_file)?));
        crate::home::socket_fits(&socket)?;
        let _ = std::fs::remove_file(&socket);
        let proxy = Proxy::open(Origins::new(&filter.domains))?;
        let config = server.cwd.join("config.json");
        std::fs::write(&config, launch(&filter.domains, &proxy).to_string())
            .map_err(|e| format!("cannot write {}: {e}", config.display()))?;
        server.args.push("--config".to_owned());
        server.args.push(config.display().to_string());
        let listener = UnixListener::bind(&socket)
            .map_err(|e| format!("cannot open {}: {e}", socket.display()))?;
        let stopping = Arc::new(AtomicBool::new(false));
        let serving = Arc::new(Mutex::new(None));
        let (stop, held) = (Arc::clone(&stopping), Arc::clone(&serving));
        let thread = thread::spawn(move || {
            for stream in listener.incoming() {
                if stop.load(Ordering::SeqCst) {
                    break;
                }
                if let Ok(stream) = stream {
                    serve(stream, &server, &filter, &held, &stop);
                }
            }
        });
        Ok(Self {
            socket,
            stopping,
            serving,
            thread: Some(thread),
            _proxy: proxy,
        })
    }

    /// The socket the call connects to
    pub(crate) fn socket(&self) -> &Path {
        &self.socket
    }
}

// The name in `file`, if it is one `unguessable` draws, and else a new one written there.
fn kept_name(file: &Path) -> Result<String, String> {
    if let Ok(text) = std::fs::read_to_string(file) {
        let name = text.trim();
        if name.len() == 32 && name.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Ok(name.to_owned());
        }
    }
    let name = unguessable()?;
    std::fs::write(file, &name).map_err(|e| format!("cannot write {}: {e}", file.display()))?;
    Ok(name)
}

impl Drop for Bridge {
    fn drop(&mut self) {
        self.stopping.store(true, Ordering::SeqCst);
        // Wakes the accept, which then sees it is stopping.
        let _ = UnixStream::connect(&self.socket);
        let _ = std::fs::remove_file(&self.socket);
        let held = self.serving.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(stream) = held.as_ref() {
            let _ = stream.shutdown(Shutdown::Both);
        }
        drop(held);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

// Playwright's config for the browser: Chromium launched with every host but the
// project's mapped to nothing (which also keeps an IP address and a WebSocket from
// reaching anywhere else), and every request sent through the proxy. The proxy's
// own address is let through the rules, as an IP address is mapped away too.
fn launch(domains: &[String], proxy: &Proxy) -> serde_json::Value {
    let mut rules = "--host-resolver-rules=MAP * ~NOTFOUND".to_owned();
    for host in domains.iter().map(String::as_str).chain([proxy::HOST]) {
        rules.push_str(", EXCLUDE ");
        rules.push_str(host);
    }
    let mut args = vec![rules];
    args.extend(proxy.flags());
    json!({ "browser": { "browserName": "chromium", "launchOptions": { "args": args } } })
}

// Serves one connection with a fresh server, filtering both ways, until either
// end closes. The server runs in its own process group, stopped whole.
fn serve(
    stream: UnixStream,
    server: &Server,
    filter: &Filter,
    serving: &Mutex<Option<UnixStream>>,
    stopping: &AtomicBool,
) {
    let log = server.cwd.join(LOG);
    let appending = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log);
    let spawned = crate::spawn::command(&server.command)
        .args(&server.args)
        .envs(&server.env)
        .current_dir(&server.cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(appending.map_or_else(|_| Stdio::null(), Stdio::from))
        .process_group(0)
        .spawn();
    let mut child = match spawned {
        Ok(child) => child,
        Err(e) => {
            let said = format!("cannot start {}: {e}\n", server.command.display());
            let _ = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&log)
                .and_then(|mut file| file.write_all(said.as_bytes()));
            return;
        }
    };
    let (Ok(back), Ok(held)) = (stream.try_clone(), stream.try_clone()) else {
        stop_child(&mut child, GRACE);
        return;
    };
    *serving.lock().unwrap_or_else(PoisonError::into_inner) = Some(held);
    // Stopped between the accept and here: nothing else would end this one.
    if stopping.load(Ordering::SeqCst) {
        let _ = stream.shutdown(Shutdown::Both);
    }
    let back = Arc::new(Mutex::new(back));
    let replies = Arc::clone(&back);
    let from_server = child.stdout.take();
    let reply_filter = filter.clone();
    let out = thread::spawn(move || {
        let Some(from_server) = from_server else {
            return;
        };
        let mut lines = BufReader::new(from_server);
        let mut line = String::new();
        while lines.read_line(&mut line).is_ok_and(|n| n > 0) {
            if let Some(reply) = reply_filter.reply(line.trim_end()) {
                let mut back = replies.lock().unwrap_or_else(PoisonError::into_inner);
                if writeln!(back, "{reply}").is_err() {
                    break;
                }
            }
            line.clear();
        }
        // The server is gone: closing the client's end lets the loop below, and
        // the `mcp-connect` behind it, see the end rather than wait on a dead server.
        let _ = (replies.lock().unwrap_or_else(PoisonError::into_inner)).shutdown(Shutdown::Both);
    });
    if let Some(mut to_server) = child.stdin.take() {
        let mut lines = BufReader::new(stream);
        let mut line = String::new();
        while lines.read_line(&mut line).is_ok_and(|n| n > 0) {
            let sent = match filter.judge(line.trim_end()) {
                Passage::Pass => to_server
                    .write_all(line.as_bytes())
                    .and_then(|()| to_server.flush()),
                Passage::Stop(Some(reply)) => {
                    let mut back = back.lock().unwrap_or_else(PoisonError::into_inner);
                    writeln!(back, "{reply}")
                }
                Passage::Stop(None) => Ok(()),
            };
            if sent.is_err() {
                break;
            }
            line.clear();
        }
    }
    stop_child(&mut child, GRACE);
    let _ = back
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .shutdown(Shutdown::Both);
    let _ = out.join();
    *serving.lock().unwrap_or_else(PoisonError::into_inner) = None;
}

#[cfg(test)]
mod tests;
