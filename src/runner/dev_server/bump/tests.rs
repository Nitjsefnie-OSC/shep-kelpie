use super::from_dev_server;

#[test]
fn a_url_is_the_dev_servers_by_its_loopback_name_and_port() {
    for url in [
        "http://localhost:4421/api",
        "http://127.0.0.1:4421/",
        "http://[::1]:4421/x?y#z",
        "HTTP://LOCALHOST:4421",
    ] {
        assert!(from_dev_server(url, 4421), "{url}");
    }
    for url in [
        "http://localhost:4422/",
        "https://app.example.com/",
        "http://localhost/",
        "http://evil.localhost:4421/",
    ] {
        assert!(!from_dev_server(url, 4421), "{url}");
    }
}

#[test]
fn a_url_with_no_port_is_on_its_schemes_default_one() {
    assert!(from_dev_server("http://localhost/api", 80));
    assert!(from_dev_server("https://127.0.0.1/api", 443));
    assert!(!from_dev_server("https://localhost/api", 80));
    assert!(!from_dev_server("http://localhost/api", 443));
}
