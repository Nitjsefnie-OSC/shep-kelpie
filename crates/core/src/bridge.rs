//! The browser's MCP server, reached over a local socket kelpie filters
//!
//! Chromium cannot start inside the sandbox, so the Playwright server runs
//! beside it (the adapters crate holds the socket). The agent starts
//! `kelpie mcp-connect <socket>` in its place, which carries its stdio to the
//! socket. Kelpie reads every message in both directions, because anything
//! inside the sandbox can reach the socket, not only the agent. Only the MCP
//! methods a tool call needs pass, only the tools in [`TOOLS`] are called or
//! listed, and each call's arguments are judged by [`crate::browse::judge`].

use std::io::{BufRead, BufReader, Write};
use std::path::Path;

use serde_json::{Value, json};

use crate::confine::Verdict;
use crate::ipc::blocking::Stream;

/// The server the agent loads the bridge as, which its tools are named after
pub const SERVER: &str = "playwright";

/// The Playwright tools a worker may call, as the server names them
///
/// A tool a Playwright upgrade adds is refused, and left out of the server's
/// listing, until it is added here. Left out on purpose: the tools that run
/// code in the server's own process (`browser_run_code_unsafe`), read a local
/// file (`browser_file_upload`, `browser_drop`, `browser_set_storage_state`)
/// or install a browser (`browser_install`).
pub const TOOLS: [&str; 19] = [
    "browser_click",
    "browser_close",
    "browser_console_messages",
    "browser_drag",
    "browser_evaluate",
    "browser_fill_form",
    "browser_handle_dialog",
    "browser_hover",
    "browser_navigate",
    "browser_navigate_back",
    "browser_network_requests",
    "browser_press_key",
    "browser_resize",
    "browser_select_option",
    "browser_snapshot",
    "browser_tabs",
    "browser_take_screenshot",
    "browser_type",
    "browser_wait_for",
];

/// What passes to the server, and back from it
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Filter {
    /// The project's `browser.domains`, which a URL argument may name
    pub domains: Vec<String>,
    /// The port of the turn's dev server once it answers, which a URL on `localhost` may name
    pub dev_port: crate::ports::DevPort,
}

/// What becomes of one message on its way to the server
#[derive(Debug, Clone, PartialEq)]
pub enum Passage {
    /// It goes on, as it came
    Pass,
    /// It stops here, and this reply goes back, if it wants one
    Stop(Option<Value>),
}

impl Filter {
    /// Judges `line`, one JSON-RPC message from inside the sandbox
    pub fn judge(&self, line: &str) -> Passage {
        let Ok(Value::Object(message)) = serde_json::from_str::<Value>(line) else {
            return Passage::Stop(Some(error(
                &Value::Null,
                "kelpie passes one JSON object a line",
            )));
        };
        let id = message.get("id").cloned();
        let Some(method) = message.get("method") else {
            // A reply to a request the server made, such as for its roots
            return Passage::Pass;
        };
        let reply = |answer: Value| Passage::Stop(id.as_ref().map(|_| answer));
        let id = id.clone().unwrap_or(Value::Null);
        match method.as_str() {
            Some("initialize" | "ping" | "tools/list") => Passage::Pass,
            Some(note) if note.starts_with("notifications/") => Passage::Pass,
            Some("tools/call") => match self.tool_call(&message["params"]) {
                Verdict::Allow => Passage::Pass,
                Verdict::Refuse(why) => reply(json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": { "content": [{ "type": "text", "text": why }], "isError": true },
                })),
            },
            _ => reply(error(&id, "kelpie does not pass this method to the server")),
        }
    }

    /// `line`, one message from the server, with a tool listing cut to [`TOOLS`]
    ///
    /// `None` when it is not a JSON object, which the server has no reason to
    /// send and the agent is not shown.
    pub fn reply(&self, line: &str) -> Option<String> {
        let mut message = match serde_json::from_str::<Value>(line).ok()? {
            Value::Object(message) => message,
            _ => return None,
        };
        if let Some(Value::Array(listed)) =
            message.get_mut("result").and_then(|r| r.get_mut("tools"))
        {
            listed.retain(|tool| tool["name"].as_str().is_some_and(|n| TOOLS.contains(&n)));
        }
        Some(Value::Object(message).to_string())
    }

    fn tool_call(&self, params: &Value) -> Verdict {
        let Some(name) = params["name"].as_str() else {
            return Verdict::Refuse("kelpie cannot read this tool call's name".into());
        };
        if !TOOLS.contains(&name) {
            return Verdict::Refuse(format!(
                "{name} is not one of the browser tools kelpie passes: {}",
                TOOLS.join(", ")
            ));
        }
        let arguments = match &params["arguments"] {
            Value::Null => json!({}),
            other => other.clone(),
        };
        crate::browse::judge(&arguments, &self.domains, self.dev_port.get())
    }
}

fn error(id: &Value, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": -32601, "message": message } })
}

/// `kelpie mcp-connect <socket>`: carries stdin to `socket`, and what comes back to stdout
///
/// # Errors
///
/// The reason, when the socket cannot be reached.
pub fn connect(socket: &Path) -> Result<(), String> {
    let stream =
        Stream::connect(socket).map_err(|e| format!("cannot reach {}: {e}", socket.display()))?;
    let mut to_server = stream
        .try_clone()
        .map_err(|e| format!("cannot use {}: {e}", socket.display()))?;
    std::thread::spawn(move || {
        let _ = std::io::copy(&mut std::io::stdin().lock(), &mut to_server);
        let _ = to_server.shutdown(std::net::Shutdown::Write);
    });
    let mut from_server = BufReader::new(stream);
    let mut stdout = std::io::stdout().lock();
    let mut line = String::new();
    while from_server.read_line(&mut line).is_ok_and(|n| n > 0) {
        if stdout
            .write_all(line.as_bytes())
            .and_then(|()| stdout.flush())
            .is_err()
        {
            break;
        }
        line.clear();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn filter() -> Filter {
        Filter {
            domains: vec!["app.example.com".into()],
            ..Filter::default()
        }
    }

    fn call(name: &str, arguments: Value) -> String {
        json!({
            "jsonrpc": "2.0", "id": 4, "method": "tools/call",
            "params": { "name": name, "arguments": arguments },
        })
        .to_string()
    }

    fn refusal(passage: Passage) -> String {
        match passage {
            Passage::Stop(Some(reply)) => {
                assert_eq!(reply["id"], 4);
                assert_eq!(reply["result"]["isError"], true, "{reply}");
                reply["result"]["content"][0]["text"]
                    .as_str()
                    .unwrap()
                    .to_owned()
            }
            other => panic!("went through: {other:?}"),
        }
    }

    #[test]
    fn the_handshake_listing_ping_a_reply_and_an_allowed_call_pass() {
        let f = filter();
        for line in [
            r#"{"jsonrpc":"2.0","id":0,"method":"initialize","params":{}}"#,
            r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
            r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#,
            r#"{"jsonrpc":"2.0","id":2,"method":"ping"}"#,
            r#"{"jsonrpc":"2.0","id":9,"result":{"roots":[]}}"#,
        ] {
            assert_eq!(f.judge(line), Passage::Pass, "{line}");
        }
        let navigate = call(
            "browser_navigate",
            json!({ "url": "https://app.example.com/" }),
        );
        assert_eq!(f.judge(&navigate), Passage::Pass);
        assert_eq!(
            f.judge(&call("browser_snapshot", Value::Null)),
            Passage::Pass
        );
    }

    // What a worker's own code could send down the socket
    #[test]
    fn a_call_browse_refuses_is_refused_on_the_wire() {
        let f = filter();
        for url in [
            "view-source:file:///Users/me/.kelpie/settings.toml",
            "file:///etc/hosts",
            "https://example.com/",
            "http://localhost:5173/",
        ] {
            let why = refusal(f.judge(&call("browser_navigate", json!({ "url": url }))));
            assert!(why.contains("not a browser domain"), "{why}");
        }
        let save = call(
            "browser_take_screenshot",
            json!({ "filename": "/Users/me/.zshrc" }),
        );
        assert!(refusal(f.judge(&save)).contains("filename"));
    }

    #[test]
    fn a_tool_off_the_allow_list_is_refused() {
        let f = filter();
        for tool in [
            "browser_run_code_unsafe",
            "browser_file_upload",
            "browser_drop",
            "browser_set_storage_state",
            "browser_install",
            "browser_a_later_release_adds",
        ] {
            let why = refusal(f.judge(&call(tool, json!({}))));
            assert!(why.contains(tool), "{why}");
        }
        let nameless = r#"{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{}}"#;
        assert!(refusal(f.judge(nameless)).contains("name"));
    }

    #[test]
    fn any_other_method_or_shape_is_refused() {
        let f = filter();
        let read = r#"{"jsonrpc":"2.0","id":3,"method":"resources/read","params":{"uri":"file:///etc/hosts"}}"#;
        let Passage::Stop(Some(reply)) = f.judge(read) else {
            panic!("resources/read went through");
        };
        assert_eq!(reply["id"], 3);
        assert_eq!(reply["error"]["code"], -32601);
        let unknown = r#"{"jsonrpc":"2.0","id":5,"method":"completion/complete"}"#;
        let Passage::Stop(Some(reply)) = f.judge(unknown) else {
            panic!("an unknown method went through");
        };
        assert_eq!(reply["id"], 5);
        for line in [
            "not json",
            "[]",
            r#"[{"jsonrpc":"2.0","id":1,"method":"ping"}]"#,
        ] {
            let Passage::Stop(Some(reply)) = f.judge(line) else {
                panic!("{line} went through");
            };
            assert_eq!(reply["id"], Value::Null, "{line}");
        }
        let quiet = r#"{"jsonrpc":"2.0","method":"sampling/createMessage"}"#;
        assert_eq!(f.judge(quiet), Passage::Stop(None));
    }

    #[test]
    fn a_tool_listing_is_trimmed_to_the_allow_list() {
        let f = filter();
        let listing = json!({
            "jsonrpc": "2.0", "id": 1,
            "result": { "tools": [
                { "name": "browser_navigate" },
                { "name": "browser_run_code_unsafe" },
                { "name": "browser_snapshot" },
                { "name": "browser_a_later_release_adds" },
                { "description": "no name" },
            ] },
        });
        let trimmed: Value = serde_json::from_str(&f.reply(&listing.to_string()).unwrap()).unwrap();
        assert_eq!(
            trimmed["result"]["tools"],
            json!([{ "name": "browser_navigate" }, { "name": "browser_snapshot" }])
        );
        assert_eq!(trimmed["id"], 1);
    }

    #[test]
    fn any_other_reply_goes_back_whole_and_what_is_not_json_does_not() {
        let f = filter();
        let result =
            r#"{"jsonrpc":"2.0","id":4,"result":{"content":[{"type":"text","text":"ok"}]}}"#;
        let back: Value = serde_json::from_str(&f.reply(result).unwrap()).unwrap();
        assert_eq!(back, serde_json::from_str::<Value>(result).unwrap());
        assert_eq!(f.reply("Debugger listening on ws://127.0.0.1:9222"), None);
        assert_eq!(f.reply("[]"), None);
    }
}
