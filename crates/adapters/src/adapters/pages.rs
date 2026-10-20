//! Pages opened in the Playwright MCP server's headless Chromium, by kelpie's own client
//!
//! The server runs behind a [`Bridge`] like a worker's, so kelpie's browser is
//! held to the same hosts, the same proxy and the dev server's one port. The
//! bridge and the server end when the call returns.

use std::io::BufReader;
use std::os::unix::net::UnixStream;
use std::time::Duration;

use super::bridge::{Bridge, Client, Navigation, Server};
use crate::bridge::Filter;
use crate::ports::{PageSession, Pages, Visit};

/// How long a page gets after it loads for its requests to settle, in seconds
const SETTLE: u32 = 2;

/// How long the server gets to answer one call, a page's load included
const CALL_WITHIN: Duration = Duration::from_secs(180);

/// Opens pages in the local Chromium, headless
#[derive(Debug, Clone, Copy, Default)]
pub struct PlaywrightPages;

impl Pages for PlaywrightPages {
    fn visit(&self, session: &PageSession, urls: &[String]) -> Result<Vec<Visit>, String> {
        let server = Server::playwright(
            &session.playwright,
            &session.browsers,
            &session.folder,
            false,
        )?;
        let filter = Filter {
            domains: session.domains.clone(),
            dev_port: session.gate.clone(),
        };
        // Ended after the client, which is declared after it.
        let bridge = Bridge::open(
            &session.sockets,
            &session.folder.join("socket-name"),
            server,
            filter,
        )?;
        let stream = UnixStream::connect(bridge.socket())
            .map_err(|e| format!("cannot reach the browser's socket: {e}"))?;
        let _ = stream.set_read_timeout(Some(CALL_WITHIN));
        let reads = stream
            .try_clone()
            .map_err(|e| format!("cannot reach the browser's socket: {e}"))?;
        let mut client = Client::open(BufReader::new(reads), stream)?;
        let mut visits = Vec::new();
        for url in urls {
            visits.push(match client.navigate(url)? {
                Navigation::Failed(why) => Visit {
                    loaded: Err(why),
                    requests: Vec::new(),
                    console_errors: Vec::new(),
                },
                Navigation::Loaded => {
                    client.settle(SETTLE)?;
                    Visit {
                        loaded: Ok(()),
                        console_errors: client.console_errors()?,
                        requests: client.requests()?,
                    }
                }
            });
        }
        Ok(visits)
    }
}
