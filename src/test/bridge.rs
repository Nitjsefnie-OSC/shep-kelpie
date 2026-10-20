//! The rig's bridges: serve nothing, and keep what each was asked and when it was closed
//!
//! A bridge it opens holds a socket path named by the name file, as the real one does, and a
//! file at that path until the bridge is dropped.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use crate::bridge::Filter;
use crate::ports::{Bridges, OpenBridge, Playwright};

/// One ask to open a bridge
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BridgeAsked {
    /// The folder the socket was to be opened in
    pub(crate) worker: PathBuf,
    /// The file the socket's name was kept in
    pub(crate) name_file: PathBuf,
    /// The server it was to serve
    pub(crate) server: Playwright,
    /// What it was to let through
    pub(crate) filter: Filter,
}

#[derive(Debug, Default)]
struct State {
    asked: Vec<BridgeAsked>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct FakeBridges {
    state: Arc<Mutex<State>>,
    next: Arc<AtomicUsize>,
    open: Arc<AtomicUsize>,
}

impl FakeBridges {
    /// Every ask so far, in order
    pub(crate) fn asked(&self) -> Vec<BridgeAsked> {
        self.state().asked.clone()
    }

    /// How many bridges are open: opened, and not yet dropped
    pub(crate) fn open_now(&self) -> usize {
        self.open.load(Ordering::SeqCst)
    }

    fn state(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl Bridges for FakeBridges {
    fn open(
        &self,
        worker: &Path,
        name_file: &Path,
        server: &Playwright,
        filter: Filter,
    ) -> Result<Box<dyn OpenBridge>, String> {
        self.state().asked.push(BridgeAsked {
            worker: worker.to_owned(),
            name_file: name_file.to_owned(),
            server: server.clone(),
            filter,
        });
        let made = |e: std::io::Error| format!("cannot open a bridge at {}: {e}", worker.display());
        if let Some(folder) = name_file.parent() {
            std::fs::create_dir_all(folder).map_err(made)?;
        }
        let name = match std::fs::read_to_string(name_file) {
            Ok(name) => name,
            Err(_) => {
                let name = format!("stand-in-{}", self.next.fetch_add(1, Ordering::SeqCst));
                std::fs::write(name_file, &name).map_err(made)?;
                name
            }
        };
        let socket = worker.join(format!("{}.sock", name.trim()));
        std::fs::write(&socket, "").map_err(made)?;
        self.open.fetch_add(1, Ordering::SeqCst);
        Ok(Box::new(FakeBridge {
            socket,
            open: Arc::clone(&self.open),
        }))
    }
}

#[derive(Debug)]
struct FakeBridge {
    socket: PathBuf,
    open: Arc<AtomicUsize>,
}

impl OpenBridge for FakeBridge {
    fn socket(&self) -> &Path {
        &self.socket
    }
}

impl Drop for FakeBridge {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.socket);
        self.open.fetch_sub(1, Ordering::SeqCst);
    }
}
