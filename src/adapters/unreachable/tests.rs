use super::reaches_no_host;

const STATUSES: [&str; 2] = ["502", "503"];

fn reads(detail: &str) -> bool {
    reaches_no_host(detail, &STATUSES, &[])
}

#[test]
fn a_status_the_text_leads_with_or_a_refused_or_reset_connection_reaches_no_host() {
    for detail in [
        r#"502 "unreachable""#,
        "503 Service Unavailable",
        "  503",
        "fetch failed: connect ECONNREFUSED 192.0.2.9:11434",
        "read ECONNRESET",
        "tcp connect error: Connection refused (os error 61)",
        "Connection reset by peer",
    ] {
        assert!(reads(detail), "{detail}");
    }
}

#[test]
fn an_error_that_leads_with_another_status_is_the_calls_own() {
    for detail in [
        r#"400 "context length exceeded""#,
        r#"400 "upstream connection refused""#,
        "429 read ECONNRESET while retrying",
        "504 Gateway Timeout",
    ] {
        assert!(!reads(detail), "{detail}");
    }
}

#[test]
fn a_status_number_inside_other_text_is_not_the_hosts_status() {
    for detail in [
        "the model said 502 things",
        "502things",
        "5021 went wrong",
        "",
    ] {
        assert!(!reads(detail), "{detail}");
    }
}

#[test]
fn the_statuses_and_signs_are_the_callers() {
    assert!(reaches_no_host("504 Gateway Timeout", &["504"], &[]));
    assert!(!reaches_no_host("503", &["504"], &[]));
    let signs = ["high demand"];
    assert!(reaches_no_host(
        "We're in HIGH DEMAND",
        &[],
        &["high demand"]
    ));
    assert!(!reaches_no_host("We're in HIGH DEMAND", &[], &[]));
    assert!(!reaches_no_host("400 high demand", &[], &signs));
}
