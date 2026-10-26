//! Kelpie's own MCP client, for the browser's server
//!
//! Proving a bump drives the browser with no model: kelpie connects to the
//! bridge's socket as the agent's `mcp-connect` does, and calls the same tools.
//! Every message passes the bridge's [`Filter`](crate::bridge::Filter), so the
//! browser keeps the same hosts and the same dev server port.
//!
//! The server answers a tool call with text in sections under `### ` headings.
//! `Result` holds what a tool read and `Error` says why it failed. This reads
//! that text as the pinned `@playwright/mcp` sends it.

use std::io::{BufRead, Write};

use serde_json::{Value, json};

use crate::ports::{ConsoleError, PageRequest};

/// The MCP revision kelpie asks for, which the server answered with the same
const PROTOCOL: &str = "2025-06-18";

/// How one tool call came back
#[derive(Debug)]
struct Reply {
    text: String,
    failed: bool,
}

/// What opening a URL came to
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Navigation {
    /// The page loaded
    Loaded,
    /// The server could not load it, in its words
    Failed(String),
}

/// A connection to the browser's server, initialized
#[derive(Debug)]
pub(crate) struct Client<R, W> {
    from: R,
    to: W,
    id: u64,
}

impl<R: BufRead, W: Write> Client<R, W> {
    /// Begins a session over `from` and `to`, which carry the server's lines
    /// and kelpie's
    ///
    /// # Errors
    ///
    /// The reason, when the server did not answer `initialize`.
    pub(crate) fn open(from: R, to: W) -> Result<Self, String> {
        let mut client = Self { from, to, id: 0 };
        let params = json!({
            "protocolVersion": PROTOCOL,
            "capabilities": {},
            "clientInfo": { "name": "kelpie", "version": env!("CARGO_PKG_VERSION") },
        });
        client.request("initialize", params)?;
        client.send(&json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }))?;
        Ok(client)
    }

    /// Opens `url` in the browser's tab
    ///
    /// # Errors
    ///
    /// The reason, when the call could not be made or answered, or the browser
    /// failed in a way that says nothing of the page. A page that does not
    /// load, by a network error or a timeout, is [`Navigation::Failed`].
    pub(crate) fn navigate(&mut self, url: &str) -> Result<Navigation, String> {
        let reply = self.call("browser_navigate", json!({ "url": url }))?;
        match (reply.failed, unloadable(&reply.text)) {
            (false, _) => Ok(Navigation::Loaded),
            (true, true) => Ok(Navigation::Failed(error_of(&reply.text))),
            (true, false) => Err(format!(
                "browser_navigate failed: {}",
                error_of(&reply.text)
            )),
        }
    }

    /// Waits `seconds` for the page's requests to settle
    ///
    /// # Errors
    ///
    /// The reason, when the call could not be made or answered, or failed.
    pub(crate) fn settle(&mut self, seconds: u32) -> Result<(), String> {
        self.read_call("browser_wait_for", json!({ "time": seconds }))
            .map(drop)
    }

    /// The errors the page has logged to its console since it loaded
    ///
    /// # Errors
    ///
    /// The reason, when the call failed or its answer has no count.
    pub(crate) fn console_errors(&mut self) -> Result<Vec<ConsoleError>, String> {
        let text = self.read_call("browser_console_messages", json!({ "level": "error" }))?;
        let counted = error_count(&text)?;
        let mut errors: Vec<ConsoleError> = section(&text, "Result")
            .lines()
            .filter_map(|line| line.strip_prefix("[ERROR] "))
            .map(console_error)
            .collect();
        // An error the list did not show still counts.
        while errors.len() < counted as usize {
            errors.push(ConsoleError {
                text: "an error the console's list left out".to_owned(),
                url: None,
            });
        }
        Ok(errors)
    }

    /// Every request the page made, static ones too, since the server leaves
    /// out the successful ones otherwise
    ///
    /// # Errors
    ///
    /// The reason, when the call failed.
    pub(crate) fn requests(&mut self) -> Result<Vec<PageRequest>, String> {
        let text = self.read_call("browser_network_requests", json!({ "static": true }))?;
        Ok(section(&text, "Result")
            .lines()
            .filter_map(request)
            .collect())
    }

    // The text of a call that read something, which is an error if the call failed.
    fn read_call(&mut self, tool: &str, arguments: Value) -> Result<String, String> {
        let reply = self.call(tool, arguments)?;
        match reply.failed {
            true => Err(format!("{tool} failed: {}", error_of(&reply.text))),
            false => Ok(reply.text),
        }
    }

    fn call(&mut self, tool: &str, arguments: Value) -> Result<Reply, String> {
        let answer = self.request(
            "tools/call",
            json!({ "name": tool, "arguments": arguments }),
        )?;
        let text = (answer["content"].as_array().into_iter().flatten())
            .filter(|part| part["type"] == "text")
            .filter_map(|part| part["text"].as_str())
            .collect::<Vec<_>>()
            .join("\n");
        Ok(Reply {
            text,
            failed: answer["isError"].as_bool().unwrap_or(false),
        })
    }

    // Sends a request and waits for the reply with its id. The server's own
    // notifications and requests on the way are not answered: kelpie offers
    // it no capability that would call for an answer.
    fn request(&mut self, method: &str, params: Value) -> Result<Value, String> {
        self.id += 1;
        let id = self.id;
        self.send(&json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }))?;
        let mut line = String::new();
        loop {
            line.clear();
            match self.from.read_line(&mut line) {
                Ok(0) => {
                    return Err(format!(
                        "the browser's server closed before it answered {method}"
                    ));
                }
                Ok(_) => {}
                Err(e) => return Err(format!("cannot read the browser's server: {e}")),
            }
            let Ok(message) = serde_json::from_str::<Value>(line.trim_end()) else {
                continue;
            };
            if message.get("method").is_some() || message["id"] != json!(id) {
                continue;
            }
            if let Some(error) = message.get("error") {
                let said = error["message"].as_str().unwrap_or("no reason given");
                return Err(format!("{method} was refused: {said}"));
            }
            return Ok(message["result"].clone());
        }
    }

    fn send(&mut self, message: &Value) -> Result<(), String> {
        writeln!(self.to, "{message}")
            .and_then(|()| self.to.flush())
            .map_err(|e| format!("cannot write to the browser's server: {e}"))
    }
}

// The lines under `### <title>`, up to the next heading.
fn section<'a>(text: &'a str, title: &str) -> &'a str {
    let heading = format!("### {title}\n");
    let start = if text.starts_with(&heading) {
        Some(0)
    } else {
        text.find(&format!("\n{heading}")).map(|at| at + 1)
    };
    let Some(start) = start else {
        return "";
    };
    let body = &text[start + heading.len()..];
    let end = body.find("\n### ").unwrap_or(body.len());
    &body[..end]
}

// Whether a failed navigation's error says the page could not be loaded, as a
// network error or a timeout does, rather than that the browser or its server
// failed, as a closed tab or a missing browser does.
fn unloadable(text: &str) -> bool {
    let said = section(text, "Error");
    let said = if said.is_empty() { text } else { said };
    said.contains("net::ERR_") || said.contains("Timeout")
}

// A failed call's reason: the server's `Error` section, else all it said.
fn error_of(text: &str) -> String {
    let said = section(text, "Error");
    let said = if said.is_empty() { text } else { said };
    said.lines().next().unwrap_or_default().to_owned()
}

// The console tool's header: `Total messages: 3 (Errors: 2, Warnings: 1)`.
fn error_count(text: &str) -> Result<u32, String> {
    section(text, "Result")
        .lines()
        .find_map(|line| line.strip_prefix("Total messages: "))
        .and_then(|counts| {
            counts
                .split_once("(Errors: ")?
                .1
                .split_once(',')?
                .0
                .parse()
                .ok()
        })
        .ok_or_else(|| "the console's answer has no count of errors".to_owned())
}

// One console message, `<text> @ <url>:<line>`.
fn console_error(message: &str) -> ConsoleError {
    let (text, url) = match message.rsplit_once(" @ ") {
        Some((text, at)) => (text, Some(at.rsplit_once(':').map_or(at, |(url, _)| url))),
        None => (message, None),
    };
    ConsoleError {
        text: text.to_owned(),
        url: url.map(str::to_owned),
    }
}

// One line of the network tool's list: `3. [GET] <url> => [503] Service Unavailable`.
fn request(line: &str) -> Option<PageRequest> {
    let (number, rest) = line.split_once(". ")?;
    number.parse::<u32>().ok()?;
    let rest = rest.strip_prefix('[')?;
    let (method, rest) = rest.split_once("] ")?;
    let (url, status) = match rest.rsplit_once(" => ") {
        Some((url, status)) => (url, Some(status)),
        None => (rest, None),
    };
    let status = status
        .and_then(|s| s.strip_prefix('['))
        .and_then(|s| s.split_once(']'))
        .and_then(|(code, _)| code.parse().ok());
    Some(PageRequest {
        method: method.to_owned(),
        url: url.to_owned(),
        status,
    })
}

#[cfg(test)]
mod tests;
