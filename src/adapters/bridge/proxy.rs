//! The proxy every request of a call's browser goes through
//!
//! Chromium's host resolver rules hold it to names and know nothing of ports,
//! so the browser is launched with `--proxy-server` naming this proxy and
//! `<-loopback>` as its bypass list, which sends loopback through it too. The
//! proxy sits on a loopback port kelpie picked, outside any sandbox, and
//! allows a `CONNECT` tunnel or an absolute-URI request to a listed
//! [`Origins`] entry only. A listed host that resolves to a loopback, private
//! or link-local address is refused, and the address that was checked is the
//! one dialed. Every read of a request is bounded in bytes and in time and the
//! connections are capped. Dropping the [`Proxy`] ends every connection.

use std::collections::HashMap;
use std::io::{self, BufReader, Read, Write};
use std::net::{IpAddr, Shutdown, SocketAddr, TcpListener, TcpStream, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

use crate::adapters::forwarder::http::{Head, Reply, Timed};
use crate::adapters::forwarder::{CONNECT_TIMEOUT, Limits, REPLY_STALL, REQUEST_WAIT};

#[cfg(test)]
mod tests;

/// The host the proxy listens on, which the browser's resolver rules must let through
pub(super) const HOST: &str = "127.0.0.1";

/// The ports of a listed host the browser may reach
const PORTS: [u16; 2] = [80, 443];

/// How many connections are served at once: a page opens several to each host
const MAX_CONNECTIONS: usize = 64;

/// How long a relayed reply may take to say anything more
const REPLY_IDLE: Duration = Duration::from_secs(300);

/// The most a refused client is read on for, in bytes, before its connection is closed
const DRAIN_MAX: usize = 1 << 20;

/// The longest a refused client is read on for
const DRAIN_FOR: Duration = Duration::from_secs(1);

/// The longest host a log line quotes, in characters
const HOST_QUOTED: usize = 100;

/// The hosts and ports a browser may reach
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Origins {
    // As `browser.domains` lists them: a host, or `*.` and a parent, in lower case.
    domains: Vec<String>,
}

impl Origins {
    /// Every one of `domains` on ports 80 and 443
    pub(crate) fn new(domains: &[String]) -> Self {
        Self {
            domains: domains.iter().map(|d| d.to_ascii_lowercase()).collect(),
        }
    }

    // Whether `host` on `port` is listed. A host that is not plain letters, digits,
    // dots and hyphens is never listed, so no encoded name or IPv6 literal is. An
    // IPv4 literal passes that check and is listed only if a domain names it, which
    // the settings refuse.
    fn lists(&self, host: &str, port: u16) -> bool {
        let host = host.to_ascii_lowercase();
        let host = host.strip_suffix('.').unwrap_or(&host);
        let plain = |c: char| c.is_ascii_alphanumeric() || c == '.' || c == '-';
        PORTS.contains(&port)
            && !host.is_empty()
            && host.chars().all(plain)
            && self.domains.iter().any(|d| crate::browse::names(d, host))
    }
}

/// What the proxy reaches the network through, which a test stands in for
pub(super) trait Outside: Send + Sync {
    /// The addresses `host` names
    fn resolve(&self, host: &str) -> io::Result<Vec<IpAddr>>;
    /// A connection to `to`
    fn connect(&self, to: SocketAddr) -> io::Result<TcpStream>;
    /// Says that a request was refused, in one line
    fn note(&self, line: &str);
}

// The network, and the runner's log.
struct System;

impl Outside for System {
    fn resolve(&self, host: &str) -> io::Result<Vec<IpAddr>> {
        let found = (host, 0).to_socket_addrs()?;
        Ok(found.map(|address| address.ip()).collect())
    }

    fn connect(&self, to: SocketAddr) -> io::Result<TcpStream> {
        TcpStream::connect_timeout(&to, CONNECT_TIMEOUT)
    }

    fn note(&self, line: &str) {
        eprintln!("{line}");
    }
}

// Both ends of each connection being served, so a stop can end them.
type Open = Arc<Mutex<HashMap<u64, Ends>>>;

struct Ends {
    client: TcpStream,
    upstream: Option<TcpStream>,
}

struct Shared {
    origins: Origins,
    outside: Arc<dyn Outside>,
    limits: Limits,
    stopping: AtomicBool,
    open: Open,
}

/// The proxy one call's browser goes through, open until dropped
#[derive(Debug)]
pub(crate) struct Proxy {
    port: u16,
    shared: Arc<Shared>,
    thread: Option<thread::JoinHandle<()>>,
}

impl core::fmt::Debug for Shared {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Shared")
            .field("origins", &self.origins)
            .finish_non_exhaustive()
    }
}

impl Proxy {
    /// Opens a proxy on a loopback port that allows `origins` only
    ///
    /// # Errors
    ///
    /// The reason, when the port cannot be bound or the proxy started.
    pub(crate) fn open(origins: Origins) -> Result<Self, String> {
        let limits = Limits {
            connections: MAX_CONNECTIONS,
            wait: REQUEST_WAIT,
        };
        Self::open_with(origins, Arc::new(System), limits)
    }

    /// [`Proxy::open`], reaching the network through `outside` and holding clients to `limits`
    ///
    /// # Errors
    ///
    /// The reason, when the port cannot be bound or the proxy started.
    pub(super) fn open_with(
        origins: Origins,
        outside: Arc<dyn Outside>,
        limits: Limits,
    ) -> Result<Self, String> {
        let listener = TcpListener::bind((HOST, 0))
            .map_err(|e| format!("cannot open the browser's proxy: {e}"))?;
        let port = (listener.local_addr())
            .map_err(|e| format!("cannot name the browser's proxy: {e}"))?
            .port();
        let shared = Arc::new(Shared {
            origins,
            outside,
            limits,
            stopping: AtomicBool::new(false),
            open: Open::default(),
        });
        let held = Arc::clone(&shared);
        let thread = thread::Builder::new()
            .name("browser-proxy".into())
            .spawn(move || accept(&listener, &held))
            .map_err(|e| format!("cannot start the browser's proxy: {e}"))?;
        Ok(Self {
            port,
            shared,
            thread: Some(thread),
        })
    }

    /// The Chromium flags that send every request to this proxy, loopback included
    pub(super) fn flags(&self) -> [String; 2] {
        [
            format!("--proxy-server=http://{HOST}:{}", self.port),
            "--proxy-bypass-list=<-loopback>".to_owned(),
        ]
    }
}

impl Drop for Proxy {
    fn drop(&mut self) {
        self.shared.stopping.store(true, Ordering::SeqCst);
        // Wakes the accept, which then sees it is stopping.
        let _ = TcpStream::connect((HOST, self.port));
        end_all(&self.shared.open);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn end_all(open: &Open) {
    for ends in open.lock().unwrap_or_else(PoisonError::into_inner).values() {
        let _ = ends.client.shutdown(Shutdown::Both);
        if let Some(upstream) = &ends.upstream {
            let _ = upstream.shutdown(Shutdown::Both);
        }
    }
}

fn accept(listener: &TcpListener, shared: &Arc<Shared>) {
    let ids = AtomicU64::new(0);
    let mut serving = Vec::new();
    for stream in listener.incoming() {
        if shared.stopping.load(Ordering::SeqCst) {
            break;
        }
        let Ok(mut client) = stream else { continue };
        let Ok(held) = client.try_clone() else {
            continue;
        };
        let _ = client.set_write_timeout(Some(REPLY_STALL));
        let id = ids.fetch_add(1, Ordering::Relaxed);
        {
            let mut listed = shared.open.lock().unwrap_or_else(PoisonError::into_inner);
            if listed.len() >= shared.limits.connections {
                drop(listed);
                let busy = Reply::new(503, "kelpie serves only so many browser connections");
                let _ = busy.write(&mut client);
                continue;
            }
            listed.insert(
                id,
                Ends {
                    client: held,
                    upstream: None,
                },
            );
        }
        let shared = Arc::clone(shared);
        let spawned = thread::Builder::new()
            .name("browser-proxying".into())
            .spawn({
                let shared = Arc::clone(&shared);
                move || {
                    serve(client, &shared, id);
                    let mut listed = shared.open.lock().unwrap_or_else(PoisonError::into_inner);
                    listed.remove(&id);
                }
            });
        match spawned {
            Ok(handle) => serving.push(handle),
            // The client is dropped with the closure, which closes its connection.
            Err(_) => {
                let mut listed = shared.open.lock().unwrap_or_else(PoisonError::into_inner);
                listed.remove(&id);
            }
        }
        serving.retain(|t| !t.is_finished());
    }
    // A connection listed as the stop came is ended here.
    end_all(&shared.open);
    for thread in serving {
        let _ = thread.join();
    }
}

fn serve(mut client: TcpStream, shared: &Shared, id: u64) {
    if let Err(reply) = exchange(&mut client, shared, id) {
        let _ = reply.write(&mut client);
        let _ = client.shutdown(Shutdown::Write);
        drain(&mut client);
    }
    let _ = client.shutdown(Shutdown::Both);
}

// Reads what the client still sends, up to a limit, so closing does not reset the
// connection and take the refusal it has not read yet with it.
fn drain(client: &mut TcpStream) {
    let deadline = Instant::now() + DRAIN_FOR;
    let mut left = DRAIN_MAX;
    let mut unused = [0u8; 4096];
    while left > 0 {
        let Some(within) = deadline.checked_duration_since(Instant::now()) else {
            return;
        };
        if client.set_read_timeout(Some(within)).is_err() {
            return;
        }
        match client.read(&mut unused) {
            Ok(n) if n > 0 => left = left.saturating_sub(n),
            _ => return,
        }
    }
}

// What a request asks of the proxy.
enum Ask {
    // A tunnel to the origin
    Tunnel,
    // One request, whose path (and query) is sent on to the origin
    Relay { path: String },
}

// Judges the request, then tunnels it or sends it on and copies the reply
// back. An `Err` is what the browser is told, and nothing was sent to the
// origin for it.
fn exchange(client: &mut TcpStream, shared: &Shared, id: u64) -> Result<(), Reply> {
    let gone = |_: io::Error| Reply::new(400, "kelpie could not read the request");
    let timed = Timed {
        stream: client.try_clone().map_err(gone)?,
        deadline: Instant::now() + shared.limits.wait,
    };
    let mut reader = BufReader::new(timed);
    let head = Head::read(&mut reader)?;
    let (host, port, ask) = aim(&head)?;
    if !shared.origins.lists(&host, port) {
        return Err(refused(shared, &host, port, "not a listed origin"));
    }
    let mut upstream = dial(shared, &host, port)?;
    // Listed under the lock a stop takes, so a stop either sees this stream or is seen here.
    {
        let mut listed = shared.open.lock().unwrap_or_else(PoisonError::into_inner);
        if shared.stopping.load(Ordering::SeqCst) {
            return Err(Reply::new(503, "kelpie is stopping"));
        }
        if let Some(ends) = listed.get_mut(&id) {
            ends.upstream = upstream.try_clone().ok();
        }
    }
    let _ = upstream.set_write_timeout(Some(REPLY_STALL));
    let lost = |e: io::Error| Reply::new(502, format!("kelpie lost the origin: {}", e.kind()));
    match ask {
        Ask::Tunnel => {
            // What followed the head in the same read belongs to the origin. It goes
            // first, so a failure is still a refusal the browser has not been told
            // is a tunnel.
            upstream.write_all(reader.buffer()).map_err(lost)?;
            client
                .write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")
                .map_err(gone)?;
            let _ = client.set_read_timeout(None);
            tunnel(client, &upstream);
        }
        Ask::Relay { path } => {
            if head.expects_continue() {
                client
                    .write_all(b"HTTP/1.1 100 Continue\r\n\r\n")
                    .map_err(gone)?;
            }
            let body = head.body(&mut reader)?;
            let _ = client.set_read_timeout(None);
            let sent = request(&head, &path, &host, port, body.len());
            upstream
                .write_all(&sent)
                .and_then(|()| upstream.write_all(&body))
                .map_err(lost)?;
            let _ = upstream.set_read_timeout(Some(REPLY_IDLE));
            // A reply that failed after its first byte cannot take a 502 behind it.
            let mut sent = Counted {
                to: client,
                bytes: 0,
            };
            if let Err(e) = io::copy(&mut upstream, &mut sent)
                && sent.bytes == 0
            {
                return Err(lost(e));
            }
        }
    }
    Ok(())
}

// A writer that counts the bytes it passes on.
struct Counted<'a> {
    to: &'a mut TcpStream,
    bytes: usize,
}

impl Write for Counted<'_> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let written = self.to.write(buf)?;
        self.bytes += written;
        Ok(written)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.to.flush()
    }
}

// The origin and kind of a request, as its line says: `host:port` for a
// tunnel, an absolute `http` URL otherwise.
fn aim(head: &Head) -> Result<(String, u16, Ask), Reply> {
    let bad = |why: &str| Reply::new(400, format!("kelpie cannot read the request: {why}"));
    if head.method == "CONNECT" {
        let (host, port) =
            authority(&head.target, None).ok_or_else(|| bad("a tunnel names a host and a port"))?;
        return Ok((host, port, Ask::Tunnel));
    }
    let rest = (head.target.strip_prefix("http://"))
        .ok_or_else(|| bad("it is not a tunnel or an absolute http URL"))?;
    let end = rest.find(['/', '?']).unwrap_or(rest.len());
    let (host, port) =
        authority(&rest[..end], Some(80)).ok_or_else(|| bad("its URL names no host"))?;
    let path = match &rest[end..] {
        "" => "/".to_owned(),
        tail if tail.starts_with('?') => format!("/{tail}"),
        tail => tail.to_owned(),
    };
    Ok((host, port, Ask::Relay { path }))
}

// The host and port of `text`, with `default` standing in for a port it leaves out.
// An IPv6 host or a user name is not read, so it is never listed.
fn authority(text: &str, default: Option<u16>) -> Option<(String, u16)> {
    let (host, port) = match (text.rsplit_once(':'), default) {
        (Some((host, port)), _) if port.bytes().all(|b| b.is_ascii_digit()) => {
            (host, port.parse().ok()?)
        }
        (Some(_), _) | (None, None) => return None,
        (None, Some(port)) => (text, port),
    };
    (!host.is_empty() && !host.contains(['[', ']', '@', ':'])).then(|| (host.to_owned(), port))
}

// Resolves `host`, refusing a name that reaches this machine or its network, and
// connects to an address that was checked.
fn dial(shared: &Shared, host: &str, port: u16) -> Result<TcpStream, Reply> {
    let addresses = (shared.outside.resolve(host)).map_err(|e| {
        Reply::new(
            502,
            format!("kelpie could not resolve the host: {}", e.kind()),
        )
    })?;
    if addresses.is_empty() {
        return Err(Reply::new(502, "kelpie could not resolve the host"));
    }
    if !addresses.iter().copied().all(is_public) {
        return Err(refused(
            shared,
            host,
            port,
            "resolves to a loopback, private or link-local address",
        ));
    }
    let mut last = io::Error::from(io::ErrorKind::NotFound);
    for address in addresses {
        match shared.outside.connect(SocketAddr::new(address, port)) {
            Ok(stream) => return Ok(stream),
            Err(e) => last = e,
        }
    }
    Err(Reply::new(
        502,
        format!("kelpie could not reach the origin: {}", last.kind()),
    ))
}

// Whether `address` is outside this machine and its networks.
fn is_public(address: IpAddr) -> bool {
    match address.to_canonical() {
        IpAddr::V4(v4) => {
            let [first, second, ..] = v4.octets();
            let shared_space = first == 100 && second & 0xc0 == 64;
            !(first == 0
                || v4.is_loopback()
                || v4.is_private()
                || v4.is_link_local()
                || v4.is_broadcast()
                || v4.is_multicast()
                || shared_space)
        }
        IpAddr::V6(v6) => {
            !(v6.is_unspecified()
                || v6.is_loopback()
                || v6.is_multicast()
                || v6.is_unique_local()
                || v6.is_unicast_link_local())
        }
    }
}

// The 403 for a refused request, and the one line that says so: the host and
// port, never the path.
fn refused(shared: &Shared, host: &str, port: u16, why: &str) -> Reply {
    let host: String = (host.chars())
        .filter(|c| !c.is_control())
        .take(HOST_QUOTED)
        .collect();
    shared
        .outside
        .note(&format!("browser proxy refused {host}:{port}: {why}"));
    Reply::new(
        403,
        format!("kelpie's browser proxy refused {host}:{port}: {why}"),
    )
}

// The request as the origin gets it: its own path, a length kelpie counted
// itself, and none of the headers that belong to the hop to the proxy.
fn request(head: &Head, path: &str, host: &str, port: u16, length: usize) -> Vec<u8> {
    const HOP: [&str; 12] = [
        "host",
        "connection",
        "proxy-connection",
        "proxy-authorization",
        "proxy-authenticate",
        "keep-alive",
        "te",
        "trailer",
        "upgrade",
        "expect",
        "content-length",
        "transfer-encoding",
    ];
    let mut text = format!("{} {path} HTTP/1.1\r\nHost: {host}", head.method);
    if port != 80 {
        text.push_str(&format!(":{port}"));
    }
    text.push_str("\r\nConnection: close\r\n");
    for (name, value) in &head.headers {
        if !HOP.contains(&name.as_str()) {
            text.push_str(&format!("{name}: {value}\r\n"));
        }
    }
    let had_body = head
        .headers
        .iter()
        .any(|(n, _)| n == "content-length" || n == "transfer-encoding");
    if had_body {
        text.push_str(&format!("content-length: {length}\r\n"));
    }
    text.push_str("\r\n");
    text.into_bytes()
}

// Copies both ways until each side has said it is done, or either fails.
fn tunnel(client: &TcpStream, upstream: &TcpStream) {
    thread::scope(|scope| {
        let _up = thread::Builder::new()
            .name("browser-tunnel".into())
            .spawn_scoped(scope, || carry(client, upstream));
        carry(upstream, client);
    });
}

// Copies `from` to `to`. A clean end shuts `to`'s write side, as a half-close; a failure ends both.
fn carry(mut from: &TcpStream, mut to: &TcpStream) {
    match io::copy(&mut from, &mut to) {
        Ok(_) => {
            let _ = to.shutdown(Shutdown::Write);
        }
        Err(_) => {
            let _ = from.shutdown(Shutdown::Both);
            let _ = to.shutdown(Shutdown::Both);
        }
    }
}
