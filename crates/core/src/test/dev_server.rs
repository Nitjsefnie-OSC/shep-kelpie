//! The rig's dev servers: starts nothing, and keeps every run it was asked for
//! with whether it is still running
//!
//! A test names the ports that are taken, the port the next `pick` returns,
//! and whether a server answers on its port.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use crate::ports::{DevRun, DevServers, Started};

/// The port `pick` returns when a test queues none
pub const PICKED: u16 = 5199;

#[derive(Debug, Default)]
struct State {
    taken: Vec<u16>,
    picks: VecDeque<u16>,
    silent: bool,
    unstartable: Option<String>,
    started: Vec<DevRun>,
}

/// Stand-in dev servers that record what is started and stopped
#[derive(Debug, Clone, Default)]
pub struct FakeDevServers {
    state: Arc<Mutex<State>>,
    live: Arc<AtomicUsize>,
}

impl FakeDevServers {
    /// Holds `port`, so nothing is free on it
    pub fn take(&self, port: u16) {
        self.state.lock().unwrap().taken.push(port);
    }

    /// Queues the port the next `pick` returns
    pub fn pick_next(&self, port: u16) {
        self.state.lock().unwrap().picks.push_back(port);
    }

    /// Has every later server start and never answer
    pub fn never_answer(&self) {
        self.state.lock().unwrap().silent = true;
    }

    /// Has every later start fail for `why`
    pub fn cannot_start(&self, why: &str) {
        self.state.lock().unwrap().unstartable = Some(why.to_owned());
    }

    /// Every run started, oldest first
    pub fn started(&self) -> Vec<DevRun> {
        self.state.lock().unwrap().started.clone()
    }

    /// How many started servers have not been dropped yet
    pub fn live(&self) -> usize {
        self.live.load(Ordering::SeqCst)
    }
}

// Counts a server as stopped when it is dropped.
struct Live(Arc<AtomicUsize>);

impl Drop for Live {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

impl DevServers for FakeDevServers {
    fn free(&self, port: u16) -> bool {
        !self.state.lock().unwrap().taken.contains(&port)
    }

    fn pick(&self) -> Result<u16, String> {
        let mut state = self.state.lock().unwrap();
        let port = state.picks.pop_front().unwrap_or(PICKED);
        state.taken.push(port);
        Ok(port)
    }

    fn start(&self, run: &DevRun) -> Result<Started, String> {
        let mut state = self.state.lock().unwrap();
        if let Some(why) = &state.unstartable {
            return Err(why.clone());
        }
        state.started.push(run.clone());
        self.live.fetch_add(1, Ordering::SeqCst);
        Ok(Started::new(!state.silent, Live(Arc::clone(&self.live))))
    }
}
