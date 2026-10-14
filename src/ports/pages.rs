//! The pages port: pages opened in the local browser, to prove a web bump
//!
//! The browser is the one a worker's turn drives (ADR 0011), run outside any
//! sandbox and held to the project's hosts and one dev server port. Kelpie
//! drives it itself, with no model, and reads what each page did.

use std::path::PathBuf;

use super::DevPort;

/// The browser to open for one proof
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageSession {
    /// The Playwright MCP server's entry point
    pub playwright: PathBuf,
    /// Where the Chromium it launches is
    pub browsers: PathBuf,
    /// A folder of kelpie's own, where the server's config, output and log go
    pub folder: PathBuf,
    /// The folder its socket is opened in
    pub sockets: PathBuf,
    /// The hosts the browser may reach, as `browser.domains` lists them
    pub domains: Vec<String>,
    /// The dev server's port, open to the browser while the server answers
    pub gate: DevPort,
}

/// A request a page made, as the browser's server listed it
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageRequest {
    /// The method, in capitals
    pub method: String,
    /// The URL, as the server printed it
    pub url: String,
    /// The status it answered, or `None` when it had no answer: it failed, or
    /// had not finished
    pub status: Option<u16>,
}

/// What opening one URL came to
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Visit {
    /// Whether the page loaded, and why not when it did not
    pub loaded: Result<(), String>,
    /// The requests that did not answer with success, and every fetch
    pub requests: Vec<PageRequest>,
    /// The errors the page's console logged
    pub console_errors: Vec<ConsoleError>,
}

/// One error the page's console logged
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConsoleError {
    /// What it said, without where it came from
    pub text: String,
    /// The URL it was logged at, when the server named one
    pub url: Option<String>,
}

/// Opens pages in a headless browser
pub trait Pages: Send + Sync {
    /// Opens each of `urls` in turn in one browser, and reads what each did
    ///
    /// # Errors
    ///
    /// The reason, when the browser could not be opened or driven, which says
    /// nothing about the pages.
    fn visit(&self, session: &PageSession, urls: &[String]) -> Result<Vec<Visit>, String>;
}
