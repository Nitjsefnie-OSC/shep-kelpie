use std::io;

use tokio::signal::unix::{Signal, SignalKind, signal};

use super::Caught;

#[derive(Debug)]
pub(super) struct Source(Signal);

impl Source {
    pub(super) fn listen(kind: Caught) -> io::Result<Self> {
        signal(match kind {
            Caught::Interrupt => SignalKind::interrupt(),
            Caught::Terminate => SignalKind::terminate(),
            Caught::Hangup => SignalKind::hangup(),
        })
        .map(Self)
    }

    pub(super) async fn recv(&mut self) {
        self.0.recv().await;
    }
}
