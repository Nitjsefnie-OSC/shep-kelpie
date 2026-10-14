use std::io::{BufRead, Read};
use std::net::TcpListener;

use super::*;

const PUBLIC: &str = "93.184.216.34";

// The network as a test sees it: names map to the addresses listed, and a dial
// to any of them lands on `lands`, a stand-in origin on this machine.
struct Fake {
    names: HashMap<String, Vec<IpAddr>>,
    lands: SocketAddr,
    dialed: Mutex<Vec<SocketAddr>>,
    said: Mutex<Vec<String>>,
}

impl Outside for Fake {
    fn resolve(&self, host: &str) -> io::Result<Vec<IpAddr>> {
        (self.names.get(host).cloned()).ok_or_else(|| io::ErrorKind::NotFound.into())
    }

    fn connect(&self, to: SocketAddr) -> io::Result<TcpStream> {
        self.dialed.lock().unwrap().push(to);
        TcpStream::connect(self.lands)
    }

    fn note(&self, line: &str) {
        self.said.lock().unwrap().push(line.to_owned());
    }
}

struct World {
    proxy: Proxy,
    outside: Arc<Fake>,
}

impl World {
    fn dialed(&self) -> Vec<SocketAddr> {
        self.outside.dialed.lock().unwrap().clone()
    }

    fn said(&self) -> Vec<String> {
        self.outside.said.lock().unwrap().clone()
    }

    fn connect(&self) -> TcpStream {
        let stream = TcpStream::connect((HOST, self.proxy.port)).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        stream
    }

    // Sends `bytes`, then reads what comes back until the proxy ends the connection.
    fn ask(&self, bytes: &[u8]) -> String {
        let mut stream = self.connect();
        // A refusal may come, and the connection close, before all is written.
        let _ = stream.write_all(bytes);
        let mut reply = Vec::new();
        let _ = stream.read_to_end(&mut reply);
        String::from_utf8_lossy(&reply).into_owned()
    }
}

// A proxy listing `app.example.test`, which resolves to a public address, and
// `local.example.test`, which resolves to this machine. `answer` is what the
// origin the dials land on says to what it hears.
fn world(limits: Limits, answer: fn(&[u8]) -> Vec<u8>) -> (World, thread::JoinHandle<Vec<u8>>) {
    world_local(limits, answer, None)
}

// The same, for a turn whose dev server is on `local`.
fn world_local(
    limits: Limits,
    answer: fn(&[u8]) -> Vec<u8>,
    local: Option<u16>,
) -> (World, thread::JoinHandle<Vec<u8>>) {
    let origin = TcpListener::bind((HOST, 0)).unwrap();
    let names = HashMap::from([
        ("app.example.test".to_owned(), vec![PUBLIC.parse().unwrap()]),
        ("cdn.example.test".to_owned(), vec![PUBLIC.parse().unwrap()]),
        (
            "a.cdn.example.test".to_owned(),
            vec![PUBLIC.parse().unwrap()],
        ),
        (
            "local.example.test".to_owned(),
            vec!["127.0.0.1".parse().unwrap()],
        ),
        ("localhost".to_owned(), vec!["127.0.0.1".parse().unwrap()]),
    ]);
    let outside = Arc::new(Fake {
        names,
        lands: origin.local_addr().unwrap(),
        dialed: Mutex::default(),
        said: Mutex::default(),
    });
    let listed = [
        "app.example.test",
        "*.cdn.example.test",
        "local.example.test",
    ];
    let origins = Origins::new(&listed.map(str::to_owned)).with_local(local);
    let proxy =
        Proxy::open_with(origins, Arc::clone(&outside) as Arc<dyn Outside>, limits).unwrap();
    // Hears one connection until it ends, answers once, and returns what it heard.
    let served = thread::spawn(move || {
        let (mut stream, _) = origin.accept().unwrap();
        let mut heard = Vec::new();
        let mut chunk = [0u8; 1024];
        loop {
            let n = stream.read(&mut chunk).unwrap_or(0);
            heard.extend_from_slice(&chunk[..n]);
            if n == 0 || heard.ends_with(b"ping") || heard.windows(4).any(|w| w == b"\r\n\r\n") {
                break;
            }
        }
        let _ = stream.write_all(&answer(&heard));
        heard
    });
    (World { proxy, outside }, served)
}

fn limits() -> Limits {
    Limits {
        connections: 8,
        wait: Duration::from_secs(10),
    }
}

fn pong(_: &[u8]) -> Vec<u8> {
    b"pong".to_vec()
}

fn status(reply: &str) -> &str {
    reply.lines().next().unwrap_or("")
}

#[test]
fn a_tunnel_to_a_listed_origin_carries_bytes_both_ways() {
    let (world, origin) = world(limits(), pong);
    let mut stream = world.connect();
    stream
        .write_all(b"CONNECT app.example.test:443 HTTP/1.1\r\nHost: app.example.test:443\r\n\r\n")
        .unwrap();
    let mut established = String::new();
    let mut lines = BufReader::new(stream.try_clone().unwrap());
    while !established.ends_with("\r\n\r\n") {
        lines.read_line(&mut established).unwrap();
    }
    assert!(established.starts_with("HTTP/1.1 200"), "{established}");
    stream.write_all(b"ping").unwrap();
    let mut back = [0u8; 4];
    lines.read_exact(&mut back).unwrap();
    assert_eq!(&back, b"pong");
    assert_eq!(origin.join().unwrap(), b"ping");
    assert_eq!(
        world.dialed(),
        [SocketAddr::new(PUBLIC.parse().unwrap(), 443)]
    );
}

#[test]
fn another_port_on_a_listed_host_is_refused_and_never_dialed() {
    let (world, _origin) = world(limits(), pong);
    let reply = world.ask(b"CONNECT app.example.test:8080 HTTP/1.1\r\n\r\n");
    assert!(status(&reply).contains("403"), "{reply}");
    assert!(world.dialed().is_empty());
    assert_eq!(world.said().len(), 1);
    assert!(
        world.said()[0].contains("app.example.test:8080"),
        "{:?}",
        world.said()
    );
}

#[test]
fn localhost_on_an_unlisted_port_is_refused() {
    let (world, _origin) = world(limits(), pong);
    let reply = world.ask(b"CONNECT localhost:3000 HTTP/1.1\r\n\r\n");
    assert!(status(&reply).contains("403"), "{reply}");
    assert!(world.dialed().is_empty());
}

#[test]
fn the_dev_servers_address_is_dialed_on_the_loopback_on_its_port_only() {
    let (world, origin) = world_local(limits(), pong, Some(4421));
    let mut stream = world.connect();
    stream
        .write_all(b"CONNECT localhost:4421 HTTP/1.1\r\nHost: localhost:4421\r\n\r\n")
        .unwrap();
    let mut established = String::new();
    let mut lines = BufReader::new(stream.try_clone().unwrap());
    while !established.ends_with("\r\n\r\n") {
        lines.read_line(&mut established).unwrap();
    }
    assert!(established.starts_with("HTTP/1.1 200"), "{established}");
    stream.write_all(b"ping").unwrap();
    let mut back = [0u8; 4];
    lines.read_exact(&mut back).unwrap();
    assert_eq!(&back, b"pong");
    assert_eq!(origin.join().unwrap(), b"ping");
    assert_eq!(
        world.dialed(),
        [SocketAddr::new(Ipv4Addr::LOCALHOST.into(), 4421)],
        "no lookup, and the loopback only"
    );
}

#[test]
fn a_request_to_the_dev_server_is_sent_on_in_its_own_form() {
    let (world, origin) = world_local(
        limits(),
        |_| b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nhi".to_vec(),
        Some(4421),
    );
    let reply =
        world.ask(b"GET http://127.0.0.1:4421/app?x=1 HTTP/1.1\r\nAccept: text/html\r\n\r\n");
    assert!(reply.starts_with("HTTP/1.1 200 OK"), "{reply}");
    let heard = String::from_utf8(origin.join().unwrap()).unwrap();
    assert!(heard.starts_with("GET /app?x=1 HTTP/1.1\r\n"), "{heard}");
    assert!(heard.contains("Host: 127.0.0.1:4421\r\n"), "{heard}");
}

#[test]
fn every_other_local_address_and_port_is_refused_beside_the_dev_server() {
    let (world, _origin) = world_local(limits(), pong, Some(4421));
    for target in [
        "localhost:3000",
        "localhost:443",
        "127.0.0.1:3000",
        "0.0.0.0:4421",
        "10.0.0.5:4421",
        "local.example.test:4421",
        "evil.localhost:4421",
    ] {
        let request = format!("CONNECT {target} HTTP/1.1\r\n\r\n");
        let reply = world.ask(request.as_bytes());
        assert!(status(&reply).contains("403"), "{target}: {reply}");
    }
    // An IPv6 literal is not read as a host at all, so it never reaches the lists.
    let reply = world.ask(b"CONNECT [::1]:4421 HTTP/1.1\r\n\r\n");
    assert!(status(&reply).contains("400"), "{reply}");
    assert!(world.dialed().is_empty(), "{:?}", world.dialed());
}

#[test]
fn a_turn_without_a_dev_server_dials_no_local_address() {
    let (world, _origin) = world(limits(), pong);
    let reply = world.ask(b"CONNECT localhost:4421 HTTP/1.1\r\n\r\n");
    assert!(status(&reply).contains("403"), "{reply}");
    assert!(world.dialed().is_empty());
}

#[test]
fn a_listed_host_that_resolves_to_this_machine_is_refused() {
    let (world, _origin) = world(limits(), pong);
    let reply = world.ask(b"CONNECT local.example.test:443 HTTP/1.1\r\n\r\n");
    assert!(status(&reply).contains("403"), "{reply}");
    assert!(world.dialed().is_empty());
    assert!(
        world.said()[0].contains("local.example.test:443"),
        "{:?}",
        world.said()
    );
}

#[test]
fn a_wildcard_lists_the_subdomains_and_not_the_parent() {
    let (world, origin) = world(limits(), pong);
    let apex = world.ask(b"CONNECT cdn.example.test:443 HTTP/1.1\r\n\r\n");
    assert!(status(&apex).contains("403"), "{apex}");
    let mut stream = world.connect();
    stream
        .write_all(b"CONNECT a.cdn.example.test:443 HTTP/1.1\r\n\r\n")
        .unwrap();
    let mut first = String::new();
    BufReader::new(stream).read_line(&mut first).unwrap();
    assert!(first.contains("200"), "{first}");
    drop(origin);
}

#[test]
fn a_request_to_a_listed_origin_is_sent_on_in_its_own_form() {
    let (world, origin) = world(limits(), |_| {
        b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nhi".to_vec()
    });
    let reply = world.ask(
        b"GET http://app.example.test/a/b?x=1 HTTP/1.1\r\nHost: elsewhere.test\r\n\
          Proxy-Connection: keep-alive\r\nAccept: text/html\r\n\r\n",
    );
    assert!(reply.starts_with("HTTP/1.1 200 OK"), "{reply}");
    assert!(reply.ends_with("hi"), "{reply}");
    let heard = String::from_utf8(origin.join().unwrap()).unwrap();
    assert!(heard.starts_with("GET /a/b?x=1 HTTP/1.1\r\n"), "{heard}");
    assert!(heard.contains("Host: app.example.test\r\n"), "{heard}");
    assert!(heard.contains("accept: text/html\r\n"), "{heard}");
    assert!(
        !heard.to_ascii_lowercase().contains("proxy-connection"),
        "{heard}"
    );
    assert!(!heard.contains("elsewhere"), "{heard}");
    assert_eq!(
        world.dialed(),
        [SocketAddr::new(PUBLIC.parse().unwrap(), 80)]
    );
}

#[test]
fn a_refused_request_is_logged_by_host_and_port_and_never_by_path() {
    let (world, _origin) = world(limits(), pong);
    let reply = world.ask(b"GET http://localhost:3000/secret/token?key=1 HTTP/1.1\r\n\r\n");
    assert!(status(&reply).contains("403"), "{reply}");
    let said = world.said();
    assert_eq!(said.len(), 1, "{said:?}");
    assert!(said[0].contains("localhost:3000"), "{said:?}");
    assert!(
        !said[0].contains("secret") && !said[0].contains("key"),
        "{said:?}"
    );
    assert!(!reply.contains("secret"), "{reply}");
}

#[test]
fn an_oversized_request_is_cut() {
    let (world, _origin) = world(limits(), pong);
    let mut request = b"CONNECT app.example.test:443 HTTP/1.1\r\nX-Pad: ".to_vec();
    request.resize(200 * 1024, b'a');
    let reply = world.ask(&request);
    assert!(status(&reply).contains("400"), "{reply}");
    assert!(world.dialed().is_empty());
}

#[test]
fn a_request_that_is_slow_to_come_is_cut() {
    let limits = Limits {
        connections: 8,
        wait: Duration::from_millis(300),
    };
    let (world, _origin) = world(limits, pong);
    let mut stream = world.connect();
    stream.write_all(b"CONNECT app.exa").unwrap();
    let mut reply = String::new();
    let _ = stream.read_to_string(&mut reply);
    assert!(status(&reply).contains("408"), "{reply}");
    assert!(world.dialed().is_empty());
}

#[test]
fn connections_past_the_cap_are_refused() {
    let limits = Limits {
        connections: 1,
        wait: Duration::from_secs(10),
    };
    let (world, _origin) = world(limits, pong);
    // Held open without a request, it takes the one place.
    let _held = world.connect();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let reply = world.ask(b"");
        if status(&reply).contains("503") {
            break;
        }
        assert!(Instant::now() < deadline, "never refused: {reply}");
        thread::yield_now();
    }
}

#[test]
fn dropping_the_proxy_ends_a_connection_it_is_serving() {
    let (world, _origin) = world(limits(), pong);
    let mut stream = world.connect();
    stream.write_all(b"CONNECT app.exa").unwrap();
    let World { proxy, outside: _ } = world;
    drop(proxy);
    // Whether it ends in a close or a reset, it ends: a wait would time out.
    let ended = stream.read_to_end(&mut Vec::new());
    let waited = |e: &io::Error| {
        matches!(
            e.kind(),
            io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
        )
    };
    assert!(!ended.as_ref().is_err_and(waited), "{ended:?}");
}

#[test]
fn the_flags_name_the_proxy_and_send_loopback_through_it() {
    let (world, _origin) = world(limits(), pong);
    let flags = world.proxy.flags();
    assert_eq!(
        flags[0],
        format!("--proxy-server=http://127.0.0.1:{}", world.proxy.port)
    );
    assert_eq!(flags[1], "--proxy-bypass-list=<-loopback>");
}

#[test]
fn only_addresses_outside_this_machine_and_its_networks_are_public() {
    for address in [
        "127.0.0.1",
        "10.1.2.3",
        "172.16.0.1",
        "192.168.1.1",
        "169.254.169.254",
        "100.64.0.1",
        "0.0.0.0",
        "198.18.0.1",
        "198.19.255.255",
        "192.0.0.1",
        "192.0.2.1",
        "198.51.100.1",
        "203.0.113.1",
        "224.0.0.1",
        "240.0.0.1",
        "255.255.255.255",
        "::1",
        "64:ff9b::7f00:1",
        "2002:7f00:1::",
        "2001:db8::1",
        "2001::1",
        "ff02::1",
        "::",
        "fe80::1",
        "fd00::1",
        "::ffff:127.0.0.1",
        "::ffff:10.0.0.1",
    ] {
        assert!(!is_public(address.parse().unwrap()), "{address}");
    }
    for address in [
        "93.184.216.34",
        "8.8.8.8",
        "198.17.255.255",
        "198.20.0.1",
        "172.32.0.1",
        "100.128.0.1",
        "2606:2800:220:1::1",
        "2001:4860:4860::8888",
    ] {
        assert!(is_public(address.parse().unwrap()), "{address}");
    }
}
