use std::io::Cursor;

use serde_json::Value;

use super::*;

// A session with the Playwright MCP server of `@playwright/mcp` 0.0.82, as kelpie speaks
// it to a page that fails in three ways. Its `initialize` answer was captured from the
// server. The server's tool answers were written from that version's source
// (`Response.serialize` and the console and network tools), since Chromium cannot start in a
// worker's sandbox to capture them. See `docs/facts/the-browser.md`.
const SESSION: &str = include_str!("../../../../fixtures/playwright-mcp-session.jsonl");

// What the recording says kelpie sent, and what the server sent back.
fn recorded() -> (Vec<Value>, String) {
    let (mut sent, mut heard) = (Vec::new(), String::new());
    for line in SESSION.lines() {
        let line: Value = serde_json::from_str(line).unwrap();
        match (line.get("to"), line.get("from")) {
            (Some(_), _) => sent.push(line["message"].clone()),
            (_, Some(_)) => heard.push_str(&format!("{}\n", line["message"])),
            _ => panic!("a line of the recording says neither to nor from"),
        }
    }
    (sent, heard)
}

#[test]
fn a_navigation_the_console_and_the_requests_read_as_the_server_sent_them() {
    let (mut expected, heard) = recorded();
    let mut wrote = Vec::new();
    let mut client = Client::open(Cursor::new(heard), &mut wrote).unwrap();

    assert_eq!(
        client.navigate("http://localhost:5173/").unwrap(),
        Navigation::Loaded
    );
    client.settle(2).unwrap();
    assert_eq!(client.console_errors().unwrap(), 2);
    let requests = client.requests().unwrap();
    let got: Vec<(&str, &str, Option<u16>)> = requests
        .iter()
        .map(|r| (r.method.as_str(), r.url.as_str(), r.status))
        .collect();
    assert_eq!(
        got,
        [
            ("GET", "http://localhost:5173/api/items", Some(503)),
            ("GET", "http://localhost:5173/app.js", Some(200)),
            ("GET", "http://localhost:5173/api/ok", Some(200)),
            ("GET", "http://localhost:5173/logo.png", Some(502)),
            ("GET", "http://localhost:5173/missing.css", Some(404)),
            ("GET", "https://fonts.example.com/f.woff2", None),
            ("POST", "http://localhost:5173/api/ping", None),
        ]
    );
    // A page the server could not load is a result, in the server's first line.
    let gone = client.navigate("http://localhost:5173/gone").unwrap();
    assert_eq!(
        gone,
        Navigation::Failed(
            "Error: page.goto: net::ERR_CONNECTION_REFUSED at http://localhost:5173/gone".into()
        )
    );
    assert_eq!(client.console_errors().unwrap(), 0);

    // Kelpie sent what the recording says it did, but for its own version.
    expected[0]["params"]["clientInfo"]["version"] = env!("CARGO_PKG_VERSION").into();
    let sent: Vec<Value> = String::from_utf8(wrote)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(sent, expected);
}

#[test]
fn a_server_that_closes_before_it_answers_is_an_error() {
    let heard = Cursor::new(String::new());
    let error = Client::open(heard, Vec::new()).unwrap_err();
    assert!(
        error.contains("closed before it answered initialize"),
        "{error}"
    );
}

#[test]
fn a_call_the_bridge_refuses_is_an_error_naming_its_reason() {
    let init = r#"{"jsonrpc":"2.0","id":1,"result":{}}"#;
    let refused = r#"{"jsonrpc":"2.0","id":2,"error":{"code":-32600,"message":"not on the list"}}"#;
    let heard = Cursor::new(format!("{init}\n{refused}\n"));
    let mut client = Client::open(heard, Vec::new()).unwrap();

    let error = client.navigate("http://localhost:1/").unwrap_err();

    assert!(error.contains("not on the list"), "{error}");
}

#[test]
fn a_console_answer_with_no_count_is_not_read_as_clean() {
    let init = r#"{"jsonrpc":"2.0","id":1,"result":{}}"#;
    let odd = r####"{"jsonrpc":"2.0","id":2,"result":{"content":[{"type":"text","text":"### Result\nnothing to see"}]}}"####;
    let mut client = Client::open(Cursor::new(format!("{init}\n{odd}\n")), Vec::new()).unwrap();

    assert!(client.console_errors().is_err());
}

#[test]
fn a_message_the_server_sends_on_its_own_does_not_stand_in_for_the_reply() {
    let init = r#"{"jsonrpc":"2.0","id":1,"result":{}}"#;
    let list = r#"{"jsonrpc":"2.0","method":"notifications/tools/list_changed"}"#;
    let ask = r#"{"jsonrpc":"2.0","id":9,"method":"ping"}"#;
    let reply = r####"{"jsonrpc":"2.0","id":2,"result":{"content":[{"type":"text","text":"### Result\nTotal messages: 1 (Errors: 1, Warnings: 0)\n"}]}}"####;
    let heard = format!("{init}\nnot json at all\n{list}\n{ask}\n{reply}\n");
    let mut client = Client::open(Cursor::new(heard), Vec::new()).unwrap();

    assert_eq!(client.console_errors().unwrap(), 1);
}

#[test]
fn a_navigation_that_fails_for_the_browser_and_not_the_page_is_an_error() {
    let init = r#"{"jsonrpc":"2.0","id":1,"result":{}}"#;
    let closed = r####"{"jsonrpc":"2.0","id":2,"result":{"isError":true,"content":[{"type":"text","text":"### Error\nError: page.goto: Target page, context or browser has been closed\n"}]}}"####;
    let slow = r####"{"jsonrpc":"2.0","id":3,"result":{"isError":true,"content":[{"type":"text","text":"### Error\nError: page.goto: Timeout 60000ms exceeded.\n"}]}}"####;
    let heard = Cursor::new(format!("{init}\n{closed}\n{slow}\n"));
    let mut client = Client::open(heard, Vec::new()).unwrap();

    let error = client.navigate("http://localhost:1/").unwrap_err();
    assert!(error.contains("has been closed"), "{error}");
    // A page that never answers is the page's failure.
    assert!(matches!(
        client.navigate("http://localhost:1/").unwrap(),
        Navigation::Failed(_)
    ));
}
