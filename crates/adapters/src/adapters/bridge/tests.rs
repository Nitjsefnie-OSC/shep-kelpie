use std::time::{Duration, Instant};

use serde_json::{Value, json};

use super::*;
use crate::test::Fake;

// A stand-in server: prints a banner that is not JSON, logs its pid at each start and every
// line it gets, and echoes each line.
fn world() -> (tempfile::TempDir, Server) {
    let dir = crate::test::short_tempdir();
    let root = dir.path().canonicalize().unwrap();
    let script = root.join("server");
    crate::test::write_script(
        &script,
        &format!(
            "#!/bin/sh\necho $$ >> {root}/pids\necho \"$@\" >> {root}/args\necho 'Debugger listening on ws://127.0.0.1:9222'\n\
             while read -r line; do\n  echo \"$line\" >> {root}/got\n  echo \"$line\"\ndone\n",
            root = root.display()
        ),
    );
    let server = Server {
        command: script,
        args: Vec::new(),
        env: BTreeMap::new(),
        cwd: root,
        call_limit: CALL_LIMIT,
    };
    (dir, server)
}

fn open(dir: &tempfile::TempDir, server: Server) -> Bridge {
    let filter = Filter {
        domains: vec!["app.example.com".into()],
        ..Filter::default()
    };
    let folder = dir.path().canonicalize().unwrap();
    Bridge::open(&folder, &folder.join("socket-name"), server, filter).unwrap()
}

fn ask(stream: &mut BufReader<Stream>, line: &str) -> Value {
    writeln!(stream.get_mut(), "{line}").unwrap();
    let mut answer = String::new();
    stream.read_line(&mut answer).unwrap();
    serde_json::from_str(&answer).unwrap()
}

fn pids(dir: &tempfile::TempDir) -> Vec<String> {
    let text = std::fs::read_to_string(dir.path().join("pids")).unwrap_or_default();
    text.lines().map(str::to_owned).collect()
}

fn running(pid: &str) -> bool {
    pid.parse().is_ok_and(crate::tree::alive)
}

#[test]
fn the_socket_is_named_by_32_hex_characters_in_the_folder() {
    let (dir, server) = world();
    let bridge = open(&dir, server);
    let socket = bridge.socket();
    assert_eq!(
        socket.parent(),
        Some(dir.path().canonicalize().unwrap().as_path())
    );
    let name = socket.file_name().unwrap().to_string_lossy();
    let (stem, extension) = name.rsplit_once('.').unwrap();
    assert_eq!(extension, "sock");
    assert_eq!(stem.len(), 32, "{name}");
    assert!(stem.chars().all(|c| c.is_ascii_hexdigit()), "{name}");
}

#[test]
fn a_bridge_opened_again_for_the_same_name_file_binds_the_same_socket() {
    let (dir, server) = world();
    let first = open(&dir, server.clone());
    let socket = first.socket().to_owned();
    drop(first);
    assert!(!socket.exists());
    let again = open(&dir, server);
    assert_eq!(again.socket(), socket);
}

#[cfg(unix)]
#[test]
fn a_socket_a_killed_kelpie_left_behind_does_not_stop_the_next_bridge_binding() {
    let (dir, server) = world();
    let first = open(&dir, server.clone());
    let socket = first.socket().to_owned();
    // A killed kelpie drops nothing: its listener's file stays.
    std::mem::forget(first);
    assert!(socket.exists());
    let next = open(&dir, server);
    assert_eq!(next.socket(), socket);
}

#[test]
fn a_name_file_that_is_not_a_socket_name_is_replaced() {
    let (dir, server) = world();
    let folder = dir.path().canonicalize().unwrap();
    let name = folder.join("socket-name");
    std::fs::write(&name, "../../etc/passwd").unwrap();
    let bridge = Bridge::open(&folder, &name, server, Filter::default()).unwrap();
    assert_eq!(bridge.socket().parent(), Some(folder.as_path()));
    let kept = std::fs::read_to_string(&name).unwrap();
    assert_eq!(kept.len(), 32);
    assert!(kept.chars().all(|c| c.is_ascii_hexdigit()), "{kept}");
}

#[test]
fn the_browsers_launch_names_the_proxy_and_sends_loopback_through_it() {
    let (dir, server) = world();
    let bridge = open(&dir, server);
    let mut stream = BufReader::new(Stream::connect(bridge.socket()).unwrap());
    ask(&mut stream, r#"{"jsonrpc":"2.0","id":1,"method":"ping"}"#);
    let root = dir.path().canonicalize().unwrap();
    let config = std::fs::read_to_string(root.join("config.json")).unwrap();
    let config: Value = serde_json::from_str(&config).unwrap();
    let launched = config["browser"]["launchOptions"]["args"]
        .as_array()
        .unwrap();
    let launched: Vec<&str> = launched.iter().map(|a| a.as_str().unwrap()).collect();
    assert!(
        launched.contains(&"--proxy-bypass-list=<-loopback>"),
        "{launched:?}"
    );
    assert!(
        launched[0].starts_with("--host-resolver-rules=MAP * ~NOTFOUND, EXCLUDE app.example.com"),
        "{launched:?}"
    );
    assert!(launched[0].contains("EXCLUDE 127.0.0.1"), "{launched:?}");
    // The flag names a proxy that is listening, and that holds `localhost` out.
    let named = launched
        .iter()
        .find_map(|a| a.strip_prefix("--proxy-server=http://127.0.0.1:"))
        .unwrap();
    let mut proxy =
        std::net::TcpStream::connect(("127.0.0.1", named.parse::<u16>().unwrap())).unwrap();
    proxy
        .write_all(b"CONNECT localhost:3000 HTTP/1.1\r\n\r\n")
        .unwrap();
    let mut refusal = String::new();
    BufReader::new(proxy).read_line(&mut refusal).unwrap();
    assert!(refusal.contains("403"), "{refusal}");
    let args = std::fs::read_to_string(root.join("args")).unwrap();
    assert!(
        args.contains(&format!("--config {}", root.join("config.json").display())),
        "{args}"
    );
}

#[test]
fn a_folder_too_long_to_bind_in_is_refused_naming_the_path() {
    let (dir, server) = world();
    let folder = PathBuf::from(format!("/{}", "s".repeat(90)));
    let name = dir.path().join("socket-name");
    let error = Bridge::open(&folder, &name, server, Filter::default()).unwrap_err();
    assert!(error.contains(&folder.display().to_string()), "{error}");
    assert!(error.contains("103"), "{error}");
}

#[test]
fn what_passes_reaches_the_server_and_what_is_refused_never_does() {
    let (dir, server) = world();
    let bridge = open(&dir, server);
    let mut stream = BufReader::new(Stream::connect(bridge.socket()).unwrap());
    let list = r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#;
    // The banner the server printed first is not JSON, so it never comes back.
    assert_eq!(ask(&mut stream, list)["id"], 1);
    let escape = r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"browser_navigate","arguments":{"url":"view-source:file:///etc/hosts"}}}"#;
    let refused = ask(&mut stream, escape);
    assert_eq!(refused["result"]["isError"], true, "{refused}");
    let read = r#"{"jsonrpc":"2.0","id":3,"method":"resources/read","params":{"uri":"file:///etc/hosts"}}"#;
    assert_eq!(ask(&mut stream, read)["error"]["code"], -32601);
    let got = std::fs::read_to_string(dir.path().join("got")).unwrap();
    assert_eq!(got.trim(), list);
}

#[test]
fn each_connection_gets_a_fresh_server() {
    let (dir, server) = world();
    let bridge = open(&dir, server);
    let ping = json!({ "jsonrpc": "2.0", "id": 1, "method": "ping" }).to_string();
    for _ in 0..2 {
        let mut stream = BufReader::new(Stream::connect(bridge.socket()).unwrap());
        ask(&mut stream, &ping);
    }
    let started = pids(&dir);
    assert_eq!(started.len(), 2, "{started:?}");
    assert_ne!(started[0], started[1]);
    let first = started[0].clone();
    let deadline = Instant::now() + Duration::from_secs(10);
    while running(&first) {
        assert!(
            Instant::now() < deadline,
            "the first connection's server stayed"
        );
        std::thread::yield_now();
    }
}

#[test]
fn dropping_the_bridge_stops_the_server_and_removes_the_socket() {
    let (dir, server) = world();
    let bridge = open(&dir, server);
    let socket = bridge.socket().to_owned();
    let mut stream = BufReader::new(Stream::connect(&socket).unwrap());
    ask(&mut stream, r#"{"jsonrpc":"2.0","id":1,"method":"ping"}"#);
    let pid = pids(&dir).remove(0);
    assert!(running(&pid));

    // The client is still connected, as a lingering `mcp-connect` would be.
    drop(bridge);
    assert!(!running(&pid), "the server outlived the bridge");
    assert!(!socket.exists());
}

// Serves one connection of `server` over a pair of sockets, which binds nothing.
fn served_once(server: &Server) {
    served_until(server, "");
}

// As `served_once`, with the client holding its end open until the server's
// log says `wanted`, so a server is not asked to end before it has spoken.
fn served_until(server: &Server, wanted: &str) {
    let (near, far) = Stream::pair().unwrap();
    let (log, wanted) = (server.cwd.join(LOG), wanted.to_owned());
    let client = thread::spawn(move || {
        let started = Instant::now();
        while started.elapsed() < crate::test::CALL_PATIENCE
            && !std::fs::read_to_string(&log).is_ok_and(|text| text.contains(&wanted))
        {
            thread::sleep(Duration::from_millis(10));
        }
        drop(far);
    });
    let held = Mutex::new(None);
    serve(
        near,
        server,
        &Filter::default(),
        &held,
        &AtomicBool::new(false),
    );
    client.join().unwrap();
}

#[test]
fn a_server_that_cannot_start_is_named_in_its_log() {
    let (dir, mut server) = world();
    server.command = dir.path().join("no-such-node");
    served_once(&server);
    let log = std::fs::read_to_string(server.cwd.join("server.log")).unwrap();
    assert!(log.contains("cannot start"), "{log}");
    assert!(log.contains("no-such-node"), "{log}");
}

#[test]
fn what_a_server_prints_on_stderr_is_kept_in_its_log() {
    let (dir, mut server) = world();
    let crashes = Fake::new().prints_to_stderr("Error: no browser").exits(1);
    server.command = crate::test::write_fake(dir.path(), "crashes", &crashes);
    served_until(&server, "Error: no browser");
    let log = std::fs::read_to_string(server.cwd.join("server.log")).unwrap();
    assert!(log.contains("Error: no browser"), "{log}");
}

#[test]
fn a_server_that_exits_ends_the_connection_instead_of_leaving_it_open() {
    let (dir, mut server) = world();
    server.command = crate::test::write_fake(dir.path(), "exits", &Fake::new());
    let (near, far) = Stream::pair().unwrap();
    let (done, ended) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let held = Mutex::new(None);
        serve(
            near,
            &server,
            &Filter::default(),
            &held,
            &AtomicBool::new(false),
        );
        let _ = done.send(());
    });
    // `far` stays open, as a worker's `mcp-connect` would: only the bridge can end it.
    ended
        .recv_timeout(Duration::from_secs(10))
        .expect("the connection stayed open after its server exited");
    let mut rest = String::new();
    assert_eq!(BufReader::new(far).read_line(&mut rest).unwrap(), 0);
}

// A stand-in server that answers `initialize` and every call but two: `browser_snapshot` and
// `browser_wait_for` never get a reply. With `late`, it ignores the request to end and
// answers `browser_click` a second afterwards, as a server cut off mid-reply would.
fn stalling_world(call_limit: Duration, late: bool) -> (tempfile::TempDir, Server) {
    let (dir, mut server) = world();
    let root = server.cwd.clone();
    let script = root.join("stalls");
    let (trap, click) = if late {
        ("trap '' TERM", "sleep 1; echo \"$reply\"; exit 0")
    } else {
        ("", "echo \"$reply\"")
    };
    let text = r#"#!/bin/sh
TRAP
echo $$ >> ROOT/pids
while read -r line; do
  echo "$line" >> ROOT/got
  id=${line#*\"id\":}; id=${id%%,*}
  reply="{\"jsonrpc\":\"2.0\",\"id\":$id,\"result\":{\"content\":[]}}"
  case "$line" in
    *'"method":"initialize"'*) echo "$reply" ;;
    *'"name":"browser_snapshot"'*|*'"name":"browser_wait_for"'*) ;;
    *'"name":"browser_click"'*) CLICK ;;
    *'"method":"tools/call"'*) echo "$reply" ;;
  esac
done
"#;
    let text = text
        .replace("TRAP", trap)
        .replace("CLICK", click)
        .replace("ROOT", &root.display().to_string());
    crate::test::write_script(&script, &text);
    server.command = script;
    server.call_limit = call_limit;
    (dir, server)
}

fn call_with(id: u32, tool: &str, arguments: &Value) -> String {
    json!({ "jsonrpc": "2.0", "id": id, "method": "tools/call", "params": { "name": tool, "arguments": arguments } })
        .to_string()
}

fn call(id: u32, tool: &str) -> String {
    call_with(id, tool, &json!({}))
}

// What the worker hears next, if anything comes within `wait`.
#[cfg(unix)]
fn heard_within(stream: &mut BufReader<Stream>, wait: Duration) -> Option<Value> {
    stream.get_ref().set_read_timeout(Some(wait)).unwrap();
    let mut line = String::new();
    let heard = stream.read_line(&mut line).is_ok_and(|n| n > 0);
    stream.get_ref().set_read_timeout(None).unwrap();
    heard.then(|| serde_json::from_str(&line).unwrap())
}

fn initialize() -> String {
    json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {} }).to_string()
}

const INITIALIZED: &str = r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#;

// A worker that has opened its connection to `server` and said hello. Dropping it ends the
// connection, and the server with it.
fn greeted(server: Server) -> BufReader<Stream> {
    let (near, far) = Stream::pair().unwrap();
    thread::spawn(move || {
        let held = Mutex::new(None);
        serve(
            near,
            &server,
            &Filter::default(),
            &held,
            &AtomicBool::new(false),
        );
    });
    let mut stream = BufReader::new(far);
    assert_eq!(ask(&mut stream, &initialize())["id"], 1);
    writeln!(stream.get_mut(), "{INITIALIZED}").unwrap();
    stream
}

#[cfg(unix)]
#[test]
fn a_call_with_no_reply_in_its_limit_gets_an_error_result_naming_the_limit() {
    let (_dir, server) = stalling_world(Duration::from_millis(300), false);
    let mut stream = greeted(server);
    let started = Instant::now();
    let ended = ask(&mut stream, &call(2, "browser_snapshot"));
    assert!(started.elapsed() >= Duration::from_millis(300));
    assert_eq!(ended["id"], 2, "{ended}");
    assert_eq!(ended["result"]["isError"], true, "{ended}");
    let text = ended["result"]["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("300ms"), "{text}");
    assert!(text.contains("restarted the browser"), "{text}");
}

#[cfg(unix)]
#[test]
fn a_reply_after_its_limit_never_reaches_the_worker() {
    let (dir, server) = stalling_world(Duration::from_millis(300), true);
    let mut stream = greeted(server);
    // The stand-in ignores the request to end, and answers this call a second after it came.
    assert_eq!(
        ask(&mut stream, &call(2, "browser_click"))["result"]["isError"],
        true
    );
    // The late answer for 2 would be read before, or between, these.
    assert_eq!(ask(&mut stream, &call(3, "browser_hover"))["id"], 3);
    assert_eq!(ask(&mut stream, &call(4, "browser_hover"))["id"], 4);
    assert!(!running(&pids(&dir)[0]));
}

#[cfg(unix)]
#[test]
fn the_call_after_a_timeout_reaches_a_fresh_server_on_the_same_connection() {
    let (dir, server) = stalling_world(Duration::from_millis(300), false);
    let mut stream = greeted(server);
    assert_eq!(
        ask(&mut stream, &call(2, "browser_snapshot"))["result"]["isError"],
        true
    );
    let answered = ask(&mut stream, &call(3, "browser_hover"));
    assert_eq!(answered["id"], 3, "{answered}");
    assert_eq!(answered["result"]["isError"], Value::Null, "{answered}");
    let started = pids(&dir);
    assert_eq!(started.len(), 2, "{started:?}");
    assert!(!running(&started[0]), "the stuck server stayed");
    assert!(running(&started[1]));
    let got = std::fs::read_to_string(dir.path().join("got")).unwrap();
    let got: Vec<&str> = got.lines().collect();
    assert_eq!(
        got,
        [
            initialize().as_str(),
            INITIALIZED,
            call(2, "browser_snapshot").as_str(),
            initialize().as_str(),
            INITIALIZED,
            call(3, "browser_hover").as_str(),
        ]
    );
}

#[cfg(unix)]
#[test]
fn the_replayed_initialize_reply_is_not_sent_to_the_worker() {
    let (_dir, server) = stalling_world(Duration::from_millis(300), false);
    let mut stream = greeted(server);
    assert_eq!(ask(&mut stream, &call(2, "browser_snapshot"))["id"], 2);
    // The new server answers the `initialize` it was sent again, with the id 1 as well.
    assert_eq!(ask(&mut stream, &call(3, "browser_hover"))["id"], 3);
    assert_eq!(heard_within(&mut stream, Duration::from_millis(200)), None);
}

#[cfg(unix)]
#[test]
fn a_browser_wait_for_gets_its_time_on_top_of_the_limit() {
    let (dir, server) = stalling_world(Duration::from_millis(400), false);
    let mut stream = greeted(server);
    let started = Instant::now();
    let wait = call_with(2, "browser_wait_for", &json!({ "time": 1 }));
    writeln!(stream.get_mut(), "{wait}").unwrap();
    assert_eq!(heard_within(&mut stream, Duration::from_millis(800)), None);
    assert_eq!(pids(&dir).len(), 1);
    let ended = heard_within(&mut stream, crate::test::CALL_PATIENCE).unwrap();
    assert!(started.elapsed() >= Duration::from_millis(1400));
    assert_eq!(ended["id"], 2, "{ended}");
    assert_eq!(ended["result"]["isError"], true, "{ended}");
    assert!(
        ended["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("1.4s")
    );
}

#[cfg(unix)]
#[test]
fn a_call_answered_in_time_is_not_cut_short() {
    let (dir, server) = stalling_world(Duration::from_millis(500), false);
    let mut stream = greeted(server);
    let answered = ask(&mut stream, &call(2, "browser_hover"));
    assert_eq!(answered["id"], 2, "{answered}");
    assert_eq!(answered["result"]["isError"], Value::Null, "{answered}");
    // Past the limit, in which nothing may restart the server for a call it answered.
    thread::sleep(Duration::from_millis(900));
    assert_eq!(ask(&mut stream, &call(3, "browser_hover"))["id"], 3);
    assert_eq!(pids(&dir).len(), 1);
}
