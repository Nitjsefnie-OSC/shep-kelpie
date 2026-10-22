use std::io;

use tokio::signal::windows::{CtrlBreak, CtrlC, CtrlClose, ctrl_break, ctrl_c, ctrl_close};

use super::Caught;

#[derive(Debug)]
pub(super) enum Source {
    CtrlC(CtrlC),
    CtrlBreak(CtrlBreak),
    CtrlClose(CtrlClose),
}

impl Source {
    pub(super) fn listen(kind: Caught) -> io::Result<Self> {
        match kind {
            Caught::Interrupt => ctrl_c().map(Self::CtrlC),
            Caught::Terminate => ctrl_break().map(Self::CtrlBreak),
            Caught::Hangup => ctrl_close().map(Self::CtrlClose),
        }
    }

    pub(super) async fn recv(&mut self) {
        match self {
            Self::CtrlC(source) => source.recv().await,
            Self::CtrlBreak(source) => source.recv().await,
            Self::CtrlClose(source) => source.recv().await,
        };
    }
}
