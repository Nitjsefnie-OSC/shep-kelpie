use std::time::{Duration, Instant};

use serde_json::{Value, json};

use super::*;

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
    };
    (dir, server)
}

fn open(dir: &tempfile::TempDir, server: Server) -> Bridge {
    let filter = Filter {
        domains: vec!["app.example.com".into()],
        ..Filter::default()
    };
    Bridge::open(&dir.path().canonicalize().unwrap(), server, filter).unwrap()
}

fn ask(stream: &mut BufReader<UnixStream>, line: &str) -> Value {
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
    crate::spawn::command("kill")
        .args(["-0", pid])
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
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
fn the_browsers_launch_names_the_proxy_and_sends_loopback_through_it() {
    let (dir, server) = world();
    let bridge = open(&dir, server);
    let mut stream = BufReader::new(UnixStream::connect(bridge.socket()).unwrap());
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
    let (_dir, server) = world();
    let folder = PathBuf::from(format!("/{}", "s".repeat(90)));
    let error = Bridge::open(&folder, server, Filter::default()).unwrap_err();
    assert!(error.contains(&folder.display().to_string()), "{error}");
    assert!(error.contains("103"), "{error}");
}

#[test]
fn what_passes_reaches_the_server_and_what_is_refused_never_does() {
    let (dir, server) = world();
    let bridge = open(&dir, server);
    let mut stream = BufReader::new(UnixStream::connect(bridge.socket()).unwrap());
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
        let mut stream = BufReader::new(UnixStream::connect(bridge.socket()).unwrap());
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
    let mut stream = BufReader::new(UnixStream::connect(&socket).unwrap());
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
    let (near, far) = UnixStream::pair().unwrap();
    drop(far);
    let held = Mutex::new(None);
    serve(
        near,
        server,
        &Filter::default(),
        &held,
        &AtomicBool::new(false),
    );
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
    let script = dir.path().join("crashes");
    crate::test::write_script(&script, "#!/bin/sh\necho 'Error: no browser' >&2\nexit 1\n");
    server.command = script;
    served_once(&server);
    let log = std::fs::read_to_string(server.cwd.join("server.log")).unwrap();
    assert!(log.contains("Error: no browser"), "{log}");
}

#[test]
fn a_server_that_exits_ends_the_connection_instead_of_leaving_it_open() {
    let (dir, mut server) = world();
    let script = dir.path().join("exits");
    crate::test::write_script(&script, "#!/bin/sh\nexit 0\n");
    server.command = script;
    let (near, far) = UnixStream::pair().unwrap();
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
