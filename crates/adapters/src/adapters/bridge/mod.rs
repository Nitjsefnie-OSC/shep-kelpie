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

use std::collections::{BTreeMap, HashMap};
use std::fmt::Display;
use std::io::{self, BufRead, BufReader, Write};
use std::net::Shutdown;
use std::path::{Path, PathBuf};
use std::process::{ChildStdin, ChildStdout, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use super::forwarder::unguessable;
use super::process::{Kept, stop_child};
use crate::bridge::{Filter, Passage, ended_call, ended_request, id_key, wait_allowance};
use crate::ipc::blocking::{Listener, Stream};
use crate::ports::{Bridges, OpenBridge, Playwright};
pub(crate) use client::{Client, Navigation};
use proxy::{Origins, Proxy};

mod client;
mod proxy;

/// The file in a server's folder that its stderr and failures to start are appended to
const LOG: &str = "server.log";

/// How long a server gets to end when asked before its tree is killed
const GRACE: Duration = Duration::from_secs(5);

/// How long a `tools/call` may go unanswered before the bridge ends it and restarts the server
///
/// It is longer than Playwright's own 60 s navigation timeout, so Playwright's errors arrive first.
const CALL_LIMIT: Duration = Duration::from_secs(90);

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
    /// How long a call may go unanswered, before a `browser_wait_for` is given its own time on top
    pub(crate) call_limit: Duration,
}

impl Server {
    /// The Playwright MCP server at `mcp`, which launches the Chromium in `browsers`
    ///
    /// It writes its output under `folder`, which is made if it is not there. Headless
    /// unless `headed`.
    ///
    /// # Errors
    ///
    /// The reason, when `folder` or its output folder is not a real folder kelpie made, or
    /// cannot be made. A link a worker could have put there would write anywhere.
    pub(crate) fn playwright(
        mcp: &Path,
        browsers: &Path,
        folder: &Path,
        headed: bool,
    ) -> Result<Self, String> {
        let output = folder.join("output");
        for dir in [folder, output.as_path()] {
            match std::fs::symlink_metadata(dir) {
                Ok(found) if found.is_dir() => {}
                Ok(_) => return Err(format!("{} is not a folder kelpie made", dir.display())),
                Err(_) => std::fs::create_dir_all(dir)
                    .map_err(|e| format!("cannot make {}: {e}", dir.display()))?,
            }
        }
        let mut args = vec![
            mcp.display().to_string(),
            "--isolated".to_owned(),
            "--output-dir".to_owned(),
            output.display().to_string(),
        ];
        if !headed {
            args.push("--headless".to_owned());
        }
        Ok(Self {
            command: "node".into(),
            args,
            env: BTreeMap::from([(
                "PLAYWRIGHT_BROWSERS_PATH".to_owned(),
                browsers.display().to_string(),
            )]),
            cwd: folder.to_owned(),
            call_limit: CALL_LIMIT,
        })
    }
}

/// Opens a bridge to the Playwright MCP server
#[derive(Debug, Clone, Copy, Default)]
pub struct PlaywrightBridges;

impl Bridges for PlaywrightBridges {
    fn open(
        &self,
        worker: &Path,
        name_file: &Path,
        server: &Playwright,
        filter: Filter,
    ) -> Result<Box<dyn OpenBridge>, String> {
        let server =
            Server::playwright(&server.mcp, &server.browsers, &server.folder, server.headed)?;
        let bridge = Bridge::open(worker, name_file, server, filter)?;
        Ok(Box::new(bridge))
    }
}

impl OpenBridge for Bridge {
    fn socket(&self) -> &Path {
        Self::socket(self)
    }
}

/// The socket one call reaches its browser through, open until dropped
#[derive(Debug)]
pub(crate) struct Bridge {
    socket: PathBuf,
    stopping: Arc<AtomicBool>,
    // The connection being served, which a drop closes to end it
    serving: Arc<Mutex<Option<Stream>>>,
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
        let proxy = Proxy::open(Origins::new(&filter.domains).with_local(filter.dev_port.clone()))?;
        let config = server.cwd.join("config.json");
        std::fs::write(&config, launch(&filter.domains, &proxy).to_string())
            .map_err(|e| format!("cannot write {}: {e}", config.display()))?;
        server.args.push("--config".to_owned());
        server.args.push(config.display().to_string());
        let listener = Listener::bind(&socket)
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
        // The served connection ends first: a pipe has one instance free for
        // the accept, so a connect made while it is served finds none.
        let held = self.serving.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(stream) = held.as_ref() {
            let _ = stream.shutdown(Shutdown::Both);
        }
        drop(held);
        // Wakes the accept, which then sees it is stopping.
        let _ = Stream::connect(&self.socket);
        let _ = std::fs::remove_file(&self.socket);
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

// A request sent to the server that has no reply yet
struct Waiting {
    id: Value,
    call: Option<Call>,
}

// A `tools/call` waiting on the server
struct Call {
    tool: String,
    // How long it gets, which a `browser_wait_for` has its own time added to
    limit: Duration,
    // When it runs over, if that is a time that can be told
    due: Option<Instant>,
}

impl Call {
    fn new(params: &Value, limit: Duration) -> Self {
        let limit = limit.saturating_add(wait_allowance(params));
        Self {
            tool: params["name"].as_str().unwrap_or_default().to_owned(),
            limit,
            due: Instant::now().checked_add(limit),
        }
    }
}

// What the threads that carry one connection share and change
struct Session {
    // How many servers the connection has had, so a line from a stopped one is known
    generation: u32,
    // Lines for the server, written by a thread of their own so that a server that reads
    // nothing cannot hold this session
    to_server: mpsc::Sender<String>,
    waiting: HashMap<String, Waiting>,
    // The worker's opening lines, which a restarted server is sent again
    initialize: Option<String>,
    initialized: Option<String>,
    // The id of the replayed `initialize`, whose reply the worker has already had
    swallow: Option<String>,
    over: bool,
}

impl Session {
    // Sends `text`, a line the filter passed, noting what it takes to answer it.
    fn pass(&mut self, text: &str, limit: Duration) -> io::Result<()> {
        let message = serde_json::from_str::<Value>(text).unwrap_or_default();
        match message["method"].as_str() {
            Some("initialize") => self.initialize = Some(text.to_owned()),
            Some("notifications/initialized") => self.initialized = Some(text.to_owned()),
            _ => {}
        }
        if let (Some(id), Some(method)) = (message.get("id"), message.get("method"))
            && !id.is_null()
        {
            let call = (method == "tools/call").then(|| Call::new(&message["params"], limit));
            let waiting = Waiting {
                id: id.clone(),
                call,
            };
            self.waiting.insert(id_key(id), waiting);
        }
        self.to_server
            .send(text.to_owned())
            .map_err(io::Error::other)
    }

    // Whether `reply`, a line from the server, goes on to the worker. A reply to nothing
    // waiting is a late one, or the replayed `initialize`'s.
    fn admit(&mut self, reply: &str) -> bool {
        let message = serde_json::from_str::<Value>(reply).unwrap_or_default();
        let Some(id) = message.get("id").filter(|id| !id.is_null()) else {
            return true;
        };
        if message.get("method").is_some() {
            return true;
        }
        let key = id_key(id);
        if self.swallow.as_ref() == Some(&key) {
            self.swallow = None;
            return false;
        }
        self.waiting.remove(&key).is_some()
    }

    fn next_due(&self) -> Option<Instant> {
        let calls = self.waiting.values().filter_map(|w| w.call.as_ref());
        calls.filter_map(|call| call.due).min()
    }

    // The tool and limit of the call that ran over first, if one has by `now`.
    fn ran_over(&self, now: Instant) -> Option<(String, Duration)> {
        let calls = self.waiting.values().filter_map(|w| w.call.as_ref());
        let over = calls.filter(|call| call.due.is_some_and(|due| due <= now));
        let first = over.min_by_key(|call| call.due)?;
        Some((first.tool.clone(), first.limit))
    }
}

struct Shared {
    session: Mutex<Session>,
    // Told when a call is waiting or the connection is over, to the thread that watches the limits
    changed: Condvar,
    // The worker's end, which what the server says and what is refused are written to
    back: Mutex<Stream>,
}

impl Shared {
    fn session(&self) -> MutexGuard<'_, Session> {
        self.session.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn tell(&self, message: &impl Display) -> io::Result<()> {
        let mut back = self.back.lock().unwrap_or_else(PoisonError::into_inner);
        writeln!(back, "{message}")
    }

    // Closes the worker's end, which lets the worker, and the loop reading it, see the end.
    fn hang_up(&self) {
        let back = self.back.lock().unwrap_or_else(PoisonError::into_inner);
        let _ = back.shutdown(Shutdown::Both);
    }
}

// The server and what replaces it
struct Running<'a> {
    shared: Arc<Shared>,
    server: &'a Server,
    filter: &'a Filter,
    process: Kept,
    relay: thread::JoinHandle<()>,
    // Relays of servers stopped, to be joined once the connection ends
    retired: Vec<thread::JoinHandle<()>>,
}

impl Running<'_> {
    // Ends the calls that ran over, and starts a fresh server for the worker's connection,
    // told what the worker told the last one when it opened.
    fn restart(&mut self, session: &mut Session) {
        let Some((tool, limit)) = session.ran_over(Instant::now()) else {
            return;
        };
        eprintln!("browser {tool} ran over {limit:?}: kelpie ended it and restarted the browser");
        let why = format!(
            "kelpie ended a browser call that ran over its {limit:?} limit and restarted the browser, so the page's state is gone"
        );
        for (_, waiting) in session.waiting.drain() {
            let ended = match waiting.call {
                Some(_) => ended_call(&waiting.id, &why),
                None => ended_request(&waiting.id, &why),
            };
            if self.shared.tell(&ended).is_err() {
                session.over = true;
                return;
            }
        }
        session.generation += 1;
        stop_child(&mut self.process, GRACE);
        let Some((process, to_server, from_server)) = start(self.server) else {
            session.over = true;
            return;
        };
        self.process = process;
        session.to_server = writer(to_server);
        let opening = [&session.initialize, &session.initialized];
        for text in opening.into_iter().flatten() {
            if session.to_server.send(text.clone()).is_err() {
                session.over = true;
            }
        }
        session.swallow = session
            .initialize
            .as_deref()
            .and_then(|text| serde_json::from_str::<Value>(text).ok())
            .and_then(|message| message.get("id").map(id_key));
        let relay = relay(&self.shared, session.generation, from_server, self.filter);
        self.retired.push(std::mem::replace(&mut self.relay, relay));
    }
}

// Starts the server, with its stderr in the log. A server that cannot start is said so in the log.
fn start(server: &Server) -> Option<(Kept, ChildStdin, ChildStdout)> {
    let log = server.cwd.join(LOG);
    let appending = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log);
    let spawned = Kept::spawn(
        crate::spawn::command(&server.command)
            .args(&server.args)
            .envs(&server.env)
            .current_dir(&server.cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(appending.map_or_else(|_| Stdio::null(), Stdio::from)),
    );
    let mut kept = match spawned {
        Ok(kept) => kept,
        Err(e) => {
            let said = format!("cannot start {}: {e}\n", server.command.display());
            let _ = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&log)
                .and_then(|mut file| file.write_all(said.as_bytes()));
            return None;
        }
    };
    let (Some(stdin), Some(stdout)) = (kept.child.stdin.take(), kept.child.stdout.take()) else {
        stop_child(&mut kept, GRACE);
        return None;
    };
    Some((kept, stdin, stdout))
}

// Writes the lines sent on the channel to the server, until the channel closes or the
// server stops taking them.
fn writer(mut to_server: ChildStdin) -> mpsc::Sender<String> {
    let (send, lines) = mpsc::channel::<String>();
    thread::spawn(move || {
        for text in lines {
            if writeln!(to_server, "{text}")
                .and_then(|()| to_server.flush())
                .is_err()
            {
                break;
            }
        }
    });
    send
}

// Carries what server number `generation` says to the worker, through the filter.
fn relay(
    shared: &Arc<Shared>,
    generation: u32,
    from_server: ChildStdout,
    filter: &Filter,
) -> thread::JoinHandle<()> {
    let (shared, filter) = (Arc::clone(shared), filter.clone());
    thread::spawn(move || {
        let mut lines = BufReader::new(from_server);
        let mut line = String::new();
        while lines.read_line(&mut line).is_ok_and(|n| n > 0) {
            if let Some(reply) = filter.reply(line.trim_end()) {
                let mut session = shared.session();
                // A server stopped on purpose: what it says now is late.
                if session.generation != generation {
                    return;
                }
                if session.admit(&reply) && shared.tell(&reply).is_err() {
                    break;
                }
            }
            line.clear();
        }
        // The server is gone: closing the client's end lets the loop reading it, and
        // the `mcp-connect` behind it, see the end rather than wait on a dead server.
        if shared.session().generation == generation {
            shared.hang_up();
        }
    })
}

// Carries what the worker says to the server, through the filter, until the worker ends it.
fn carry(shared: &Shared, stream: Stream, filter: &Filter, limit: Duration) {
    let mut lines = BufReader::new(stream);
    let mut line = String::new();
    while lines.read_line(&mut line).is_ok_and(|n| n > 0) {
        let text = line.trim_end();
        let sent = match filter.judge(text) {
            Passage::Pass => {
                let sent = shared.session().pass(text, limit);
                shared.changed.notify_all();
                sent
            }
            Passage::Stop(Some(reply)) => shared.tell(&reply),
            Passage::Stop(None) => Ok(()),
        };
        if sent.is_err() {
            break;
        }
        line.clear();
    }
    shared.session().over = true;
    shared.changed.notify_all();
}

// Serves one connection with a fresh server, filtering both ways, until either
// end closes. The server runs in its own process tree, stopped whole. A call that
// runs past its limit is answered with an error, and the server is replaced by a
// fresh one on the same connection.
fn serve(
    stream: Stream,
    server: &Server,
    filter: &Filter,
    serving: &Mutex<Option<Stream>>,
    stopping: &AtomicBool,
) {
    let Some((mut process, to_server, from_server)) = start(server) else {
        return;
    };
    let (Ok(back), Ok(held)) = (stream.try_clone(), stream.try_clone()) else {
        stop_child(&mut process, GRACE);
        return;
    };
    *serving.lock().unwrap_or_else(PoisonError::into_inner) = Some(held);
    // Stopped between the accept and here: nothing else would end this one.
    if stopping.load(Ordering::SeqCst) {
        let _ = stream.shutdown(Shutdown::Both);
    }
    let shared = Arc::new(Shared {
        session: Mutex::new(Session {
            generation: 0,
            to_server: writer(to_server),
            waiting: HashMap::new(),
            initialize: None,
            initialized: None,
            swallow: None,
            over: false,
        }),
        changed: Condvar::new(),
        back: Mutex::new(back),
    });
    let mut running = Running {
        shared: Arc::clone(&shared),
        server,
        filter,
        process,
        relay: relay(&shared, 0, from_server, filter),
        retired: Vec::new(),
    };
    let carrier = {
        let (shared, filter, limit) = (Arc::clone(&shared), filter.clone(), server.call_limit);
        thread::spawn(move || carry(&shared, stream, &filter, limit))
    };
    let mut session = shared.session();
    while !session.over {
        session = match session.next_due() {
            Some(due) if due <= Instant::now() => {
                running.restart(&mut session);
                session
            }
            Some(due) => {
                let wait = due.saturating_duration_since(Instant::now());
                let (session, _) = shared
                    .changed
                    .wait_timeout(session, wait)
                    .unwrap_or_else(PoisonError::into_inner);
                session
            }
            None => shared
                .changed
                .wait(session)
                .unwrap_or_else(PoisonError::into_inner),
        };
    }
    drop(session);
    stop_child(&mut running.process, GRACE);
    shared.hang_up();
    for relay in running.retired.drain(..) {
        let _ = relay.join();
    }
    let _ = running.relay.join();
    let _ = carrier.join();
    *serving.lock().unwrap_or_else(PoisonError::into_inner) = None;
}

#[cfg(test)]
mod tests;
