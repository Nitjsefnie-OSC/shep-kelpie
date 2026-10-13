//! A shepherd that lives in a test's own memory
//!
//! The code under test dials it through [`Dial`] and asks it through a
//! [`Connection`], the interface the real socket serves. Nothing is bound,
//! so a test that uses it runs in a worker's sandbox.

use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use shep_client::shep_core::protocol::request::Response;
use shep_client::shep_core::protocol::{HelloAck, Request};
use shep_client::testing::sample_ack;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};

use crate::shepherd::{Answer, ConnectRefused, Connection, Dial, Opened};

type Answering = dyn FnMut(&Request) -> Response + Send;

struct Inner {
    home: PathBuf,
    ack: HelloAck,
    answering: Mutex<Box<Answering>>,
    sent: UnboundedSender<Request>,
}

/// A shepherd at a shep home on a shep version, answering each request with a closure
#[derive(Clone)]
pub(crate) struct Memory(Arc<Inner>);

impl fmt::Debug for Memory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Memory")
            .field("home", &self.0.home)
            .field("ack", &self.0.ack)
            .finish_non_exhaustive()
    }
}

impl Memory {
    /// A shepherd at `home` that says it runs `version` and answers every
    /// request with `answer`, and what it is sent, in order
    ///
    /// `answer` panics on a request the test did not expect, and the
    /// panic reaches the code that asked.
    pub(crate) fn answering(
        home: &Path,
        version: &str,
        answer: impl FnMut(&Request) -> Response + Send + 'static,
    ) -> (Self, UnboundedReceiver<Request>) {
        let (sent, heard) = unbounded_channel();
        let mut ack = sample_ack();
        ack.daemon_version = version.into();
        let shepherd = Self(Arc::new(Inner {
            home: home.to_owned(),
            ack,
            answering: Mutex::new(Box::new(answer)),
            sent,
        }));
        (shepherd, heard)
    }
}

impl Dial for Memory {
    fn open<'a>(&'a self, shep_home: &'a Path) -> Opened<'a> {
        Box::pin(async move {
            if shep_home != self.0.home {
                return Err(ConnectRefused::Unreachable(format!(
                    "no shepherd listens at {}",
                    shep_home.display()
                )));
            }
            Ok(Box::new(self.clone()) as Box<dyn Connection>)
        })
    }
}

impl Connection for Memory {
    fn send(&self, body: Request, _deadline: Option<Duration>) -> Answer<'_> {
        Box::pin(async move {
            // A request on a socket gives up the thread while it waits.
            tokio::task::yield_now().await;
            // The test may have stopped listening to what was sent.
            let _ = self.0.sent.send(body.clone());
            let mut answering = (self.0.answering.lock()).unwrap_or_else(PoisonError::into_inner);
            Ok((*answering)(&body))
        })
    }

    fn daemon(&self) -> HelloAck {
        self.0.ack.clone()
    }
}
