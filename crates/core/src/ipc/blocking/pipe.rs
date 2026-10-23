//! Blocking calls over named pipes, each run as a task of a private runtime
//!
//! The task is spawned and its result waited for on a channel rather than
//! blocked on with `Handle::block_on`, which panics when its caller is a task
//! of some other runtime, as a runner's thread may be.

use std::future::Future;
use std::io::{self, Read, Write};
use std::net::Shutdown;
use std::path::{Path, PathBuf};
use std::sync::mpsc::sync_channel;
use std::sync::{Arc, LazyLock, Mutex, PoisonError};
use std::time::Duration;

use shep_client::shep_core::transport;
use tokio::net::windows::named_pipe::{NamedPipeClient, NamedPipeServer};
use tokio::runtime::{Builder, Handle};
use tokio::sync::watch;
use tokio::time::Instant;

use crate::ipc::address;

// A runtime that a thread of its own drives for as long as the process lives.
static RUNTIME: LazyLock<Handle> = LazyLock::new(|| {
    let runtime = Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap_or_else(|e| panic!("cannot start the local IPC runtime: {e}"));
    let handle = runtime.handle().clone();
    std::thread::Builder::new()
        .name("ipc".into())
        .spawn(move || runtime.block_on(std::future::pending::<()>()))
        .unwrap_or_else(|e| panic!("cannot start the local IPC thread: {e}"));
    handle
});

// Runs `work` on the runtime and waits for what it returns.
fn run<T: Send + 'static>(
    work: impl Future<Output = io::Result<T>> + Send + 'static,
) -> io::Result<T> {
    let (done, result) = sync_channel(1);
    RUNTIME.spawn(async move {
        let _ = done.send(work.await);
    });
    result
        .recv()
        .unwrap_or_else(|_| Err(io::Error::other("the local IPC runtime stopped")))
}

#[derive(Debug)]
enum Pipe {
    Client(NamedPipeClient),
    Server(NamedPipeServer),
}

// Both ends of a pipe take the same calls, which have no common trait.
macro_rules! either {
    ($pipe:expr, $end:ident => $call:expr) => {
        match $pipe {
            Pipe::Client($end) => $call,
            Pipe::Server($end) => $call,
        }
    };
}

#[derive(Debug)]
struct Shared {
    pipe: Pipe,
    read_for: Mutex<Option<Duration>>,
    write_for: Mutex<Option<Duration>>,
    // Each turns true once, on a shutdown of that side, which every blocked call watches.
    read_closed: watch::Sender<bool>,
    write_closed: watch::Sender<bool>,
}

impl Shared {
    fn limit(slot: &Mutex<Option<Duration>>) -> Option<Instant> {
        let within = *slot.lock().unwrap_or_else(PoisonError::into_inner);
        within.map(|within| Instant::now() + within)
    }

    // Waits for the pipe to be ready, until `by`, or until a shutdown, which is `Ok(false)`.
    async fn ready(&self, reading: bool, by: Option<Instant>) -> io::Result<bool> {
        let mut closed = if reading {
            self.read_closed.subscribe()
        } else {
            self.write_closed.subscribe()
        };
        let ready = async {
            if reading {
                either!(&self.pipe, p => p.readable().await)
            } else {
                either!(&self.pipe, p => p.writable().await)
            }
        };
        let timed = async {
            match by {
                Some(by) => tokio::time::sleep_until(by).await,
                None => std::future::pending().await,
            }
        };
        tokio::select! {
            biased;
            _ = closed.wait_for(|closed| *closed) => Ok(false),
            ready = ready => ready.map(|()| true),
            () = timed => Err(io::ErrorKind::TimedOut.into()),
        }
    }

    async fn read(&self, len: usize) -> io::Result<Vec<u8>> {
        let by = Self::limit(&self.read_for);
        let mut buf = vec![0; len];
        loop {
            if !self.ready(true, by).await? {
                return Ok(Vec::new());
            }
            match either!(&self.pipe, p => p.try_read(&mut buf)) {
                Ok(n) => {
                    buf.truncate(n);
                    return Ok(buf);
                }
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => {}
                // The other end closed.
                Err(e) if e.kind() == io::ErrorKind::BrokenPipe => return Ok(Vec::new()),
                Err(e) => return Err(e),
            }
        }
    }

    async fn write(&self, data: &[u8]) -> io::Result<usize> {
        let by = Self::limit(&self.write_for);
        loop {
            if !self.ready(false, by).await? {
                return Err(io::ErrorKind::BrokenPipe.into());
            }
            match either!(&self.pipe, p => p.try_write(data)) {
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => {}
                done => return done,
            }
        }
    }
}

/// One end of a connection, shared by its clones
#[derive(Debug, Clone)]
pub struct Stream(Arc<Shared>);

impl Stream {
    fn of(pipe: Pipe) -> Self {
        Self(Arc::new(Shared {
            pipe,
            read_for: Mutex::new(None),
            write_for: Mutex::new(None),
            read_closed: watch::channel(false).0,
            write_closed: watch::channel(false).0,
        }))
    }

    /// Connects to the endpoint `socket` names
    ///
    /// # Errors
    ///
    /// Whatever the OS says, such as nothing listening there.
    pub fn connect(socket: impl AsRef<Path>) -> io::Result<Self> {
        let address = address(socket.as_ref());
        run(async move { transport::connect(&address).await }).map(|c| Self::of(Pipe::Client(c)))
    }

    /// Two ends connected to each other
    ///
    /// # Errors
    ///
    /// Whatever the OS says.
    pub fn pair() -> io::Result<(Self, Self)> {
        let (server, client) = run(async { transport::connected_pair().await })?;
        Ok((
            Self::of(Pipe::Server(server)),
            Self::of(Pipe::Client(client)),
        ))
    }

    /// Another handle on the same connection
    ///
    /// # Errors
    ///
    /// Never, here: it is `Result` because std's is.
    #[allow(clippy::unnecessary_wraps)]
    pub fn try_clone(&self) -> io::Result<Self> {
        Ok(self.clone())
    }

    /// Gives every read a limit, or none
    ///
    /// # Errors
    ///
    /// Never, here: it is `Result` because std's is.
    #[allow(clippy::unnecessary_wraps)]
    pub fn set_read_timeout(&self, within: Option<Duration>) -> io::Result<()> {
        *self
            .0
            .read_for
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = within;
        Ok(())
    }

    /// Gives every write a limit, or none
    ///
    /// # Errors
    ///
    /// Never, here: it is `Result` because std's is.
    #[allow(clippy::unnecessary_wraps)]
    pub fn set_write_timeout(&self, within: Option<Duration>) -> io::Result<()> {
        *self
            .0
            .write_for
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = within;
        Ok(())
    }

    /// Ends reads, writes or both for every handle on the connection, and wakes any that block
    ///
    /// A pipe has no half to close, so this is kept here and the peer is not
    /// told: it finds the end when the last handle drops. Closing one side
    /// leaves the other working. Reads then find the end, and writes fail. What
    /// was written already stays for the peer to read.
    ///
    /// # Errors
    ///
    /// Never, here: it is `Result` because std's is.
    #[allow(clippy::unnecessary_wraps)]
    pub fn shutdown(&self, how: Shutdown) -> io::Result<()> {
        if matches!(how, Shutdown::Read | Shutdown::Both) {
            self.0.read_closed.send_replace(true);
        }
        if matches!(how, Shutdown::Write | Shutdown::Both) {
            self.0.write_closed.send_replace(true);
        }
        Ok(())
    }
}

impl Read for &Stream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        let (shared, len) = (Arc::clone(&self.0), buf.len());
        let got = run(async move { shared.read(len).await })?;
        buf[..got.len()].copy_from_slice(&got);
        Ok(got.len())
    }
}

impl Write for &Stream {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        let (shared, data) = (Arc::clone(&self.0), buf.to_vec());
        run(async move { shared.write(&data).await })
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Read for Stream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        (&*self).read(buf)
    }
}

impl Write for Stream {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        (&*self).write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// Where connections to an endpoint arrive, one thread accepting at a time
#[derive(Debug)]
pub struct Listener {
    inner: Arc<tokio::sync::Mutex<transport::Listener>>,
    socket: PathBuf,
}

impl Listener {
    /// Listens at the endpoint `socket` names
    ///
    /// # Errors
    ///
    /// Whatever the OS says, such as the pipe being served already.
    pub fn bind(socket: impl AsRef<Path>) -> io::Result<Self> {
        let socket = socket.as_ref().to_owned();
        let address = address(&socket);
        // Made on the runtime, whose reactor the pipe registers with.
        let listener = run(async move { transport::Listener::bind(&address) })?;
        Ok(Self {
            inner: Arc::new(tokio::sync::Mutex::new(listener)),
            socket,
        })
    }

    /// Waits for the next connection
    ///
    /// # Errors
    ///
    /// Whatever the OS says.
    pub fn accept(&self) -> io::Result<(Stream, PathBuf)> {
        let inner = Arc::clone(&self.inner);
        let served = run(async move { inner.lock().await.accept().await })?;
        Ok((Stream::of(Pipe::Server(served)), self.socket.clone()))
    }

    /// The connections as they arrive, with no end
    pub fn incoming(&self) -> Incoming<'_> {
        Incoming(self)
    }
}

/// What [`Listener::incoming`] iterates
#[derive(Debug)]
pub struct Incoming<'a>(&'a Listener);

impl Iterator for Incoming<'_> {
    type Item = io::Result<Stream>;

    fn next(&mut self) -> Option<Self::Item> {
        Some(self.0.accept().map(|(stream, _)| stream))
    }
}
