//! Paddock's own routes, for a gateway of kind `paddock`, a shep dog
//!
//! [`Paddock`] is the one adapter of the gateway port.
//!
//! `GET /v1/models` lists its models and needs no key. `GET /paddock/status`
//! needs one, so it tells a key paddock takes from one it refuses. A call or
//! a review round on a model behind it holds a heartbeat lease, so nothing
//! evicts the model midway: `POST /paddock/leases` waits for the grant, a
//! `PUT` renews it, and a `DELETE` releases it when the call ends. A take
//! paddock turns away as busy is asked again until the call gives up. Each
//! request is one HTTP/1.0 exchange over plain `http://`, as the forwarder's
//! are.

use std::io::{Read, Write};
use std::net::{Shutdown, TcpStream, ToSocketAddrs};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::forwarder::Upstream;
use crate::ports::{Ending, KeyCheck, ModelGateway, PaddockHolder, Stand, Taking, Unheld};

// Paddock's default is 60s. Twice that rides out one lost renewal.
const TTL: &str = "120s";

/// How often a take waiting for its grant looks for a reason to give up
const POLL: Duration = Duration::from_millis(100);

/// How long `status` waits on a gateway's own status, since it is asked while a trigger is answered
const STATUS_WITHIN: Duration = Duration::from_secs(2);

/// The most of an answer read, in bytes
const ANSWER_MAX: u64 = 1 << 20;

/// How long each step of talking to paddock may take
#[derive(Debug, Clone, Copy)]
pub(crate) struct Timing {
    /// How long a request gets to connect, and one other than a take to answer
    pub(crate) answer: Duration,
    /// How long the release gets, since a turn's end waits on it
    pub(crate) release: Duration,
    /// How often a held lease is renewed: a quarter of its ttl
    pub(crate) renew: Duration,
    /// How long a take paddock turned away as busy waits to ask again
    pub(crate) retry: Duration,
    /// How long after its last renewal paddock took the lease counts as lost, if none since was taken: its ttl
    pub(crate) lapse: Duration,
}

impl Timing {
    /// What a runner uses
    pub(crate) const RUNNER: Self = Self {
        answer: Duration::from_secs(10),
        release: Duration::from_secs(3),
        renew: Duration::from_secs(30),
        retry: Duration::from_secs(30),
        lapse: Duration::from_secs(120),
    };
}

/// The model names `GET /v1/models` lists, which needs no key
///
/// # Errors
///
/// Why the gateway did not answer with its list.
pub(crate) fn models(gateway: &Upstream) -> Result<Vec<String>, String> {
    list(gateway, false)
}

/// The model names `GET /v1/models` lists, with the gateway's key when `keyed`
///
/// # Errors
///
/// Why the gateway did not answer with its list.
pub(crate) fn list(gateway: &Upstream, keyed: bool) -> Result<Vec<String>, String> {
    let asked = ("GET", "/v1/models");
    let (status, body) = send(gateway, asked, None, keyed, Timing::RUNNER.answer)?;
    if status != 200 {
        return Err(format!("`GET /v1/models` answered HTTP {status}"));
    }
    let not_a_list = || "`GET /v1/models` answered something other than a model list".to_owned();
    let list: Value = serde_json::from_str(&body).map_err(|_| not_a_list())?;
    let entries = list["data"].as_array().ok_or_else(not_a_list)?;
    let id = |m: &Value| m["id"].as_str().map(str::to_owned).ok_or_else(not_a_list);
    entries.iter().map(id).collect()
}

/// Whether paddock takes the gateway's key, by `GET /paddock/status`
///
/// # Errors
///
/// Why it could not be asked, or an answer that is neither yes nor no.
pub(crate) fn key_taken(gateway: &Upstream) -> Result<bool, String> {
    let asked = ("GET", "/paddock/status");
    match send(gateway, asked, None, true, Timing::RUNNER.answer)?.0 {
        200 => Ok(true),
        401 => Ok(false),
        status => Err(format!("`GET /paddock/status` answered HTTP {status}")),
    }
}

/// Who holds a lease on `model`, by `GET /paddock/status`
///
/// `None` when no lease on the model is held, or one a revoke has ended. A
/// model that serves one sequence has at most one such lease.
///
/// # Errors
///
/// Why the gateway did not answer with its status.
pub(crate) fn holder(gateway: &Upstream, model: &str) -> Result<Option<PaddockHolder>, String> {
    let asked = ("GET", "/paddock/status");
    let (status, body) = send(gateway, asked, None, true, STATUS_WITHIN)?;
    if status != 200 {
        return Err(format!("`GET /paddock/status` answered HTTP {status}"));
    }
    holder_in(&body, model)
}

// The first lease on `model` in a status body that a revoke has not ended.
fn holder_in(body: &str, model: &str) -> Result<Option<PaddockHolder>, String> {
    let read: Value = serde_json::from_str(body)
        .map_err(|_| "`GET /paddock/status` answered something other than JSON".to_owned())?;
    let mut held = read["leases"].as_array().into_iter().flatten();
    let text = |lease: &Value, field: &str| lease[field].as_str().unwrap_or_default().to_owned();
    Ok(held
        .find(|l| l["model"] == model && l["revoked"].is_null())
        .map(|lease| PaddockHolder {
            lease: text(lease, "id"),
            client: text(lease, "client"),
            what: text(lease, "note"),
            since: text(lease, "since"),
        }))
}

/// The gateway port for a paddock, taking its leases on `timing`'s schedule
#[derive(Debug, Clone, Copy)]
pub(crate) struct Paddock {
    pub(crate) timing: Timing,
}

impl Default for Paddock {
    fn default() -> Self {
        Self {
            timing: Timing::RUNNER,
        }
    }
}

impl ModelGateway for Paddock {
    fn models(&self, gateway: &Upstream) -> Result<Vec<String>, String> {
        models(gateway)
    }

    fn key(&self, gateway: &Upstream) -> KeyCheck {
        match key_taken(gateway) {
            Ok(true) => KeyCheck::Taken,
            Ok(false) => KeyCheck::Refused,
            Err(why) => KeyCheck::Unsure(why),
        }
    }

    fn lease(
        &self,
        gateway: &Upstream,
        taking: &Taking<'_>,
        call: &mut dyn FnMut(&str),
    ) -> Option<Result<(), Unheld>> {
        (taking.tell)(Stand::Asking);
        let taken = Lease::take(
            gateway,
            taking.model,
            taking.note,
            taking.give_up,
            taking.tell,
            taking.ending,
            self.timing,
        );
        let mut lease = match taken {
            Ok(Some(lease)) => lease,
            Ok(None) => return Some(Err(Unheld::GaveUp)),
            Err(why) => return Some(Err(Unheld::Refused(why))),
        };
        (taking.tell)(Stand::Granted);
        call(lease.id());
        // Finishing it first settles that no later renewal can end the call.
        Some(match lease.finish() {
            Some(why) => Err(Unheld::Lost(why)),
            None => Ok(()),
        })
    }
}

/// A heartbeat lease on one model, renewed until dropped and then released
///
/// A renewal paddock refuses, or renewals it leaves unanswered for the lease's
/// whole ttl, mean the lease is gone: the call's [`Ending`] is asked to end
/// and [`Lease::finish`] says why.
#[derive(Debug)]
pub(crate) struct Lease {
    gateway: Upstream,
    id: String,
    release: Duration,
    held: Arc<Mutex<Held>>,
    // Dropping the sender stops the renewals, which are not waited for.
    renewing: Option<mpsc::Sender<()>>,
}

// What a lease is, as its holder and the renewing thread see it under one lock.
#[derive(Debug)]
enum Held {
    Renewing,
    /// Why it cannot be renewed, with the call asked to end
    Lost(String),
    /// Let go of by its holder, so nothing may end the call for it any more
    Finished,
}

impl Lease {
    /// Takes a lease on `model`, labelled `note`, waiting for its grant
    /// until `give_up` says to stop, and telling `busy` of each busy answer,
    /// as [`Stand::Turn`] or [`Stand::Busy`]
    ///
    /// `None` when it gave up, which hangs up and so leaves paddock's queue.
    /// Once held, a lease it can no longer renew asks `ending` to end the call.
    ///
    /// # Errors
    ///
    /// Why paddock refused it, other than as busy, or could not be asked.
    pub(crate) fn take(
        gateway: &Upstream,
        model: &str,
        note: &str,
        give_up: &dyn Fn() -> bool,
        busy: &dyn Fn(Stand),
        ending: &Ending,
        timing: Timing,
    ) -> Result<Option<Self>, String> {
        let body = json!({ "model": model, "hold": "heartbeat", "ttl": TTL, "note": note });
        let (status, answer) = loop {
            let Some((status, answer)) = ask_once(gateway, &body, give_up)? else {
                return Ok(None);
            };
            if status != 503 {
                break (status, answer);
            }
            busy(busy_stand(&answer));
            if !wait(timing.retry, give_up) {
                return Ok(None);
            }
        };
        let read: Value = serde_json::from_str(&answer).unwrap_or(Value::Null);
        let Some(id) = read["id"].as_str().filter(|_| status == 200) else {
            let error = read["error"].as_str().unwrap_or("no reason given");
            let why = read["reason"].as_str().or(read["detail"].as_str());
            let why = why.map(|why| format!(": {why}")).unwrap_or_default();
            return Err(format!(
                "`POST /paddock/leases` answered HTTP {status}, {error}{why}"
            ));
        };
        // The id goes in a header and a path, so it is held to what paddock writes.
        if id.is_empty() || !id.bytes().all(|b| b.is_ascii_alphanumeric()) {
            return Err(format!(
                "`POST /paddock/leases` granted the id {id:?}, which cannot go in a \
                 header, so the take is refused"
            ));
        }
        Ok(Some(Self::renewed(gateway, id, ending, timing)))
    }

    /// The id paddock granted, exactly as it wrote it
    pub(crate) fn id(&self) -> &str {
        &self.id
    }

    /// Stops the lease's renewals, and says why it was lost, if it was
    ///
    /// Once this returns, a renewal still in flight can no longer end the
    /// call: the thread settles that under the same lock.
    pub(crate) fn finish(&mut self) -> Option<String> {
        drop(self.renewing.take());
        match std::mem::replace(&mut *self.held.lock().unwrap(), Held::Finished) {
            Held::Lost(why) => Some(why),
            Held::Renewing | Held::Finished => None,
        }
    }

    fn renewed(gateway: &Upstream, id: &str, ending: &Ending, timing: Timing) -> Self {
        let (stop, stopped) = mpsc::channel::<()>();
        let held = Arc::new(Mutex::new(Held::Renewing));
        let (renewer, path) = (gateway.clone(), format!("/paddock/leases/{id}"));
        let (record, ending) = (Arc::clone(&held), ending.clone());
        // Not joined: a renewal in flight must not hold up the release.
        thread::spawn(move || {
            let mut renewed_at = Instant::now();
            let mut last = "no renewal was due before it ran out".to_owned();
            let mut told = false;
            let why = loop {
                // The ttl runs from the last renewal paddock took, so the lease
                // is lost at that boundary and not before: tries go on up to it,
                // the last of them cut short so none runs past it.
                let left = timing.lapse.saturating_sub(renewed_at.elapsed());
                if left.is_zero() {
                    break last;
                }
                let waited = stopped.recv_timeout(timing.renew.min(left));
                if !matches!(waited, Err(RecvTimeoutError::Timeout)) {
                    return;
                }
                let left = timing.lapse.saturating_sub(renewed_at.elapsed());
                if left.is_zero() {
                    continue;
                }
                let within = timing.answer.min(left);
                match send(&renewer, ("PUT", &path), None, true, within) {
                    Ok((204 | 200, _)) => renewed_at = Instant::now(),
                    Ok((status, _)) => break format!("paddock answered HTTP {status}"),
                    Err(why) => {
                        if !told {
                            eprintln!("kelpie: cannot renew the gateway's lease {path}: {why}");
                            told = true;
                        }
                        last = why;
                    }
                }
            };
            // Under the lock the holder lets go under, so a lease let go of
            // has its call over already and is not ended.
            let mut held = record.lock().unwrap();
            if matches!(*held, Held::Renewing) {
                *held = Held::Lost(format!(
                    "the gateway's lease {path} cannot be renewed: {why}"
                ));
                ending.end();
            }
        });
        Self {
            gateway: gateway.clone(),
            id: id.to_owned(),
            release: timing.release,
            held,
            renewing: Some(stop),
        }
    }
}

impl Drop for Lease {
    fn drop(&mut self) {
        self.finish();
        // A release that fails still ends once the ttl runs out unrenewed.
        let path = format!("/paddock/leases/{}", self.id);
        let _ = send(&self.gateway, ("DELETE", &path), None, true, self.release);
    }
}

// What a busy answer says paddock turned the take away for.
fn busy_stand(answer: &str) -> Stand {
    let read: Value = serde_json::from_str(answer).unwrap_or(Value::Null);
    match read["reason_kind"].as_str() {
        Some("turn") => Stand::Turn,
        _ => Stand::Busy,
    }
}

// One take: its status and body, or `None` once `give_up` says to stop.
fn ask_once(
    gateway: &Upstream,
    body: &Value,
    give_up: &dyn Fn() -> bool,
) -> Result<Option<(u16, String)>, String> {
    let stream = open(gateway, Timing::RUNNER.answer)?;
    let hang_up = stream.try_clone().map_err(|e| e.to_string())?;
    let request = request(gateway, ("POST", "/paddock/leases"), Some(body), true)?;
    let (sent, answered) = mpsc::channel();
    thread::spawn(move || {
        let _ = sent.send(exchange(&stream, &request, None));
    });
    loop {
        match answered.recv_timeout(POLL) {
            Ok(answer) => return answer.map(Some),
            Err(RecvTimeoutError::Timeout) if give_up() => {
                let _ = hang_up.shutdown(Shutdown::Both);
                return Ok(None);
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return Err("the take was lost".into()),
        }
    }
}

// Waits `pause`, or less when `give_up` says to stop, which it returns false for.
fn wait(pause: Duration, give_up: &dyn Fn() -> bool) -> bool {
    let mut waited = Duration::ZERO;
    while waited < pause {
        if give_up() {
            return false;
        }
        let nap = POLL.min(pause - waited);
        thread::sleep(nap);
        waited += nap;
    }
    !give_up()
}

fn open(gateway: &Upstream, within: Duration) -> Result<TcpStream, String> {
    let cannot = |e: std::io::Error| format!("cannot reach the gateway: {}", e.kind());
    let mut last = std::io::Error::from(std::io::ErrorKind::NotFound);
    for address in gateway.address().to_socket_addrs().map_err(cannot)? {
        match TcpStream::connect_timeout(&address, within) {
            Ok(stream) => return Ok(stream),
            Err(e) => last = e,
        }
    }
    Err(cannot(last))
}

// One request on a connection of its own, and its status and body, each step within `within`.
pub(crate) fn send(
    gateway: &Upstream,
    asked: (&str, &str),
    body: Option<&Value>,
    keyed: bool,
    within: Duration,
) -> Result<(u16, String), String> {
    let request = request(gateway, asked, body, keyed)?;
    exchange(&open(gateway, within)?, &request, Some(within))
}

// The request's bytes, with the gateway's key when `keyed`.
fn request(
    gateway: &Upstream,
    (method, path): (&str, &str),
    body: Option<&Value>,
    keyed: bool,
) -> Result<String, String> {
    let body = body.map(Value::to_string).unwrap_or_default();
    let mut head = format!(
        "{method} {path} HTTP/1.0\r\nHost: {}\r\nContent-Length: {}\r\n",
        gateway.address(),
        body.len()
    );
    if !body.is_empty() {
        head.push_str("Content-Type: application/json\r\n");
    }
    if keyed {
        let key = gateway.key().ok_or("the gateway has no key")?;
        head.push_str(&format!("Authorization: Bearer {}\r\n", key.expose()));
    }
    Ok(format!("{head}\r\n{body}"))
}

// Sends `request` and reads the answer to its end, for at most `within`.
fn exchange(
    stream: &TcpStream,
    request: &str,
    within: Option<Duration>,
) -> Result<(u16, String), String> {
    let lost = |e: std::io::Error| format!("the gateway did not answer: {}", e.kind());
    stream.set_read_timeout(within).map_err(lost)?;
    stream.set_write_timeout(within).map_err(lost)?;
    let mut writer = stream;
    writer.write_all(request.as_bytes()).map_err(lost)?;
    let mut bytes = Vec::new();
    stream
        .take(ANSWER_MAX)
        .read_to_end(&mut bytes)
        .map_err(lost)?;
    let text = String::from_utf8_lossy(&bytes);
    let (head, body) = text.split_once("\r\n\r\n").unwrap_or((&text, ""));
    let status = head.split(' ').nth(1).and_then(|s| s.parse().ok());
    let status = status.ok_or("the gateway's answer is not HTTP")?;
    Ok((status, body.to_owned()))
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Instant;

    use super::*;
    use crate::settings::{EndpointUrl, GatewayKey};
    use crate::test::{StandInEndpoint, Take};

    const QUICK: Timing = Timing {
        answer: Duration::from_secs(5),
        release: Duration::from_secs(5),
        renew: Duration::from_millis(20),
        retry: Duration::from_millis(20),
        lapse: Duration::from_secs(5),
    };

    const LEASES: &str = r#"{"leases":[
        {"id":"L1","client":"a","model":"coder","since":"t1","note":null,"revoked":{"by":"admin","note":"x"}},
        {"id":"L2","client":"b","model":"other","since":"t2","note":"n","revoked":null},
        {"id":"L3","client":"c","model":"coder","since":"t3","note":"mine","revoked":null}
    ],"waiters":[]}"#;

    #[test]
    fn the_holder_is_the_live_lease_on_the_model() {
        let holder = holder_in(LEASES, "coder").unwrap().unwrap();
        assert_eq!(
            (
                holder.lease.as_str(),
                holder.client.as_str(),
                holder.what.as_str(),
                holder.since.as_str()
            ),
            ("L3", "c", "mine", "t3")
        );
        assert_eq!(holder_in(LEASES, "idle"), Ok(None));
        assert_eq!(holder_in("{}", "coder"), Ok(None));
        assert!(holder_in("<html>", "coder").is_err());
    }

    const BUSY: &str = r#"{"error":"busy","reason":"qwen is loading"}"#;

    const TURN: &str = r#"{"error":"busy","reason":"qwen is serving kelpie #9, next in line","reason_kind":"turn"}"#;

    fn gateway(server: &StandInEndpoint) -> Upstream {
        let base = EndpointUrl::try_from(server.url().to_owned()).unwrap();
        Upstream::new(&base)
            .unwrap()
            .with_key(GatewayKey::for_test("pk"))
    }

    fn calls(server: &StandInEndpoint) -> Vec<String> {
        let seen = server.seen().into_iter();
        seen.filter_map(|l| l.strip_suffix(" HTTP/1.0").map(str::to_owned))
            .collect()
    }

    #[test]
    fn a_200_that_is_not_a_model_list_is_an_error() {
        for body in [
            "{}",
            r#"{"data":"none"}"#,
            r#"{"data":[{"name":"qwen"}]}"#,
            "[]",
        ] {
            let server = StandInEndpoint::start([])
                .like_paddock("pk", &[])
                .with_models_body(body);
            let err = models(&gateway(&server)).unwrap_err();
            assert_eq!(
                err, "`GET /v1/models` answered something other than a model list",
                "{body}"
            );
        }
        let server = StandInEndpoint::start([]).like_paddock("pk", &["qwen"]);
        assert_eq!(models(&gateway(&server)), Ok(vec!["qwen".to_owned()]));
        let empty = StandInEndpoint::start([]).like_paddock("pk", &[]);
        assert_eq!(models(&gateway(&empty)), Ok(vec![]));
    }

    #[test]
    fn a_take_asks_for_a_heartbeat_renews_it_and_releases_it_when_dropped() {
        let server = StandInEndpoint::start([]).like_paddock("pk", &[]);
        let lease = Lease::take(
            &gateway(&server),
            "qwen",
            "kelpie #7 worker",
            &|| false,
            &|_| {},
            &Ending::default(),
            QUICK,
        );
        let lease = lease.unwrap().unwrap();
        let asked = &server.requests()[0];
        let wanted = json!({ "model": "qwen", "hold": "heartbeat", "ttl": "120s",
                             "note": "kelpie #7 worker" });
        assert_eq!(asked, &wanted);
        let until = Instant::now() + Duration::from_secs(5);
        while !calls(&server).contains(&"PUT /paddock/leases/L1".to_owned()) {
            assert!(Instant::now() < until, "no renewal: {:?}", server.seen());
            thread::sleep(Duration::from_millis(10));
        }
        drop(lease);
        assert_eq!(calls(&server).last().unwrap(), "DELETE /paddock/leases/L1");
    }

    #[test]
    fn a_lease_gives_the_id_paddock_granted() {
        let server = StandInEndpoint::start([]).like_paddock("pk", &[]);
        let lease = held(&server, &Ending::default(), QUICK);
        assert_eq!(lease.id(), "L1");
    }

    #[test]
    fn a_granted_id_that_cannot_go_in_a_header_fails_the_take() {
        for body in [r#"{"id":"L1\r\nx-evil: 1"}"#, r#"{"id":"L 1"}"#] {
            let server = StandInEndpoint::start([])
                .like_paddock("pk", &[])
                .with_takes([Take::Status(200, body)]);
            let err = Lease::take(
                &gateway(&server),
                "qwen",
                "n",
                &|| false,
                &|_| {},
                &Ending::default(),
                QUICK,
            )
            .unwrap_err();
            assert!(err.contains("`POST /paddock/leases`"), "{err}");
            assert!(err.contains("refused"), "{err}");
            assert_eq!(calls(&server), ["POST /paddock/leases"], "{body}");
        }
    }

    fn held(server: &StandInEndpoint, ending: &Ending, timing: Timing) -> Lease {
        let taken = Lease::take(
            &gateway(server),
            "qwen",
            "n",
            &|| false,
            &|_| {},
            ending,
            timing,
        );
        taken.unwrap().unwrap()
    }

    fn until(what: &str, server: &StandInEndpoint, done: impl Fn() -> bool) {
        let until = Instant::now() + Duration::from_secs(5);
        while !done() {
            assert!(Instant::now() < until, "no {what}: {:?}", server.seen());
            thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn a_renewal_the_gateway_refuses_ends_the_turn_with_a_failure() {
        let server = StandInEndpoint::start([])
            .like_paddock("pk", &[])
            .refusing_renewals(404);
        let ending = Ending::default();
        let mut lease = held(&server, &ending, QUICK);
        until("end", &server, || ending.asked());
        let lost = lease.finish().unwrap();
        assert!(lost.contains("L1") && lost.contains("HTTP 404"), "{lost}");
    }

    #[test]
    fn renewals_the_gateway_leaves_unanswered_end_the_turn_once_the_ttl_has_run_out() {
        let server = StandInEndpoint::start([])
            .like_paddock("pk", &[])
            .hanging_renewals();
        let timing = Timing {
            answer: Duration::from_millis(50),
            lapse: Duration::from_millis(300),
            ..QUICK
        };
        let ending = Ending::default();
        let mut lease = held(&server, &ending, timing);
        until("end", &server, || ending.asked());
        assert!(lease.finish().unwrap().contains("did not answer"));
    }

    #[test]
    fn unanswered_renewals_are_tried_until_the_ttl_has_run_out_and_not_before() {
        let server = StandInEndpoint::start([])
            .like_paddock("pk", &[])
            .hanging_renewals();
        let timing = Timing {
            answer: Duration::from_millis(20),
            renew: Duration::from_millis(100),
            lapse: Duration::from_millis(400),
            ..QUICK
        };
        let ending = Ending::default();
        let started = Instant::now();
        let _lease = held(&server, &ending, timing);
        until("end", &server, || ending.asked());
        assert!(started.elapsed() >= timing.lapse, "{:?}", started.elapsed());
    }

    #[test]
    fn a_lease_let_go_of_with_a_renewal_in_flight_never_ends_the_call() {
        let server = StandInEndpoint::start([])
            .like_paddock("pk", &[])
            .hanging_renewals();
        let timing = Timing {
            answer: Duration::from_secs(2),
            lapse: Duration::from_millis(500),
            ..QUICK
        };
        let ending = Ending::default();
        let mut lease = held(&server, &ending, timing);
        until("renewal", &server, || {
            calls(&server).contains(&"PUT /paddock/leases/L1".to_owned())
        });
        assert_eq!(lease.finish(), None);
        // The renewal in flight is cut short at the lapse, after the lease was let go of.
        thread::sleep(Duration::from_millis(900));
        assert!(!ending.asked());
    }

    #[test]
    fn a_healthy_lease_is_not_lost() {
        let server = StandInEndpoint::start([]).like_paddock("pk", &[]);
        let ending = Ending::default();
        let mut lease = held(&server, &ending, QUICK);
        until("renewal", &server, || {
            calls(&server).contains(&"PUT /paddock/leases/L1".to_owned())
        });
        assert!(!ending.asked());
        assert_eq!(lease.finish(), None);
    }

    #[test]
    fn dropping_a_lease_with_a_renewal_in_flight_releases_it_at_once() {
        let server = StandInEndpoint::start([])
            .like_paddock("pk", &[])
            .hanging_renewals();
        let lease = held(&server, &Ending::default(), QUICK);
        until("renewal", &server, || {
            calls(&server).contains(&"PUT /paddock/leases/L1".to_owned())
        });
        let dropped = Instant::now();
        drop(lease);
        assert!(dropped.elapsed() < Duration::from_secs(1));
        assert_eq!(calls(&server).last().unwrap(), "DELETE /paddock/leases/L1");
    }

    #[test]
    fn a_busy_take_is_asked_again_and_a_refused_one_names_why() {
        let server = StandInEndpoint::start([])
            .like_paddock("pk", &[])
            .with_takes([Take::Status(503, BUSY)]);
        let busy = AtomicUsize::new(0);
        let told = |_| {
            busy.fetch_add(1, Ordering::SeqCst);
        };
        let lease = Lease::take(
            &gateway(&server),
            "qwen",
            "n",
            &|| false,
            &told,
            &Ending::default(),
            QUICK,
        )
        .unwrap();
        assert!(lease.is_some());
        assert_eq!(busy.load(Ordering::SeqCst), 1, "each busy answer is told");
        let takes = calls(&server)
            .iter()
            .filter(|l| l.starts_with("POST"))
            .count();
        assert_eq!(takes, 2);

        let wrong = gateway(&server).with_key(GatewayKey::for_test("other"));
        let err = Lease::take(
            &wrong,
            "qwen",
            "n",
            &|| false,
            &|_| {},
            &Ending::default(),
            QUICK,
        )
        .unwrap_err();
        assert_eq!(
            err,
            "`POST /paddock/leases` answered HTTP 401, unauthorized"
        );
        let bad = r#"{"error":"bad_lease_request","detail":"ttl is at most 1h"}"#;
        let server = StandInEndpoint::start([])
            .like_paddock("pk", &[])
            .with_takes([Take::Status(400, bad)]);
        let err = Lease::take(
            &gateway(&server),
            "qwen",
            "n",
            &|| false,
            &|_| {},
            &Ending::default(),
            QUICK,
        )
        .unwrap_err();
        assert!(
            err.ends_with("bad_lease_request: ttl is at most 1h"),
            "{err}"
        );
    }

    #[test]
    fn a_take_paddock_keeps_queued_or_busy_ends_when_the_turn_gives_up() {
        for take in [Take::Hang, Take::Status(503, BUSY)] {
            let server = StandInEndpoint::start([])
                .like_paddock("pk", &[])
                .with_takes([take.clone(), take]);
            let looks = AtomicUsize::new(0);
            let give_up = || looks.fetch_add(1, Ordering::SeqCst) >= 3;
            let started = Instant::now();
            let lease = Lease::take(
                &gateway(&server),
                "qwen",
                "n",
                &give_up,
                &|_| {},
                &Ending::default(),
                QUICK,
            );
            assert!(matches!(lease, Ok(None)), "{lease:?}");
            assert!(started.elapsed() < Duration::from_secs(5));
            assert!(!calls(&server).iter().any(|l| l.starts_with("DELETE")));
        }
    }

    fn round_calls(server: &StandInEndpoint) -> Vec<String> {
        let all = calls(server).into_iter();
        all.filter(|l| l.starts_with("POST") || l.starts_with("DELETE"))
            .collect()
    }

    fn around<T>(
        server: &StandInEndpoint,
        ending: &Ending,
        give_up: &dyn Fn() -> bool,
        told: &Mutex<Vec<Stand>>,
        call: impl FnOnce() -> T,
    ) -> Result<T, Unheld> {
        let tell = |stand| told.lock().unwrap().push(stand);
        let taking = Taking {
            model: "qwen",
            note: "kelpie local round 1",
            give_up,
            tell: &tell,
            ending,
        };
        let paddock = Paddock { timing: QUICK };
        let port: &dyn ModelGateway = &paddock;
        port.around(&gateway(server), &taking, call)
    }

    #[test]
    fn a_reviewer_round_takes_a_lease_before_the_call_and_returns_it_after() {
        let server = StandInEndpoint::start([]).like_paddock("pk", &[]);
        let told = Mutex::new(Vec::new());
        let during = around(&server, &Ending::default(), &|| false, &told, || {
            round_calls(&server)
        });
        assert_eq!(during, Ok(vec!["POST /paddock/leases".to_owned()]));
        assert_eq!(
            round_calls(&server),
            ["POST /paddock/leases", "DELETE /paddock/leases/L1"]
        );
        assert_eq!(*told.lock().unwrap(), [Stand::Asking, Stand::Granted]);
    }

    #[test]
    fn a_turn_reason_is_told_as_a_turn_and_any_other_busy_answer_as_busy() {
        for (body, wanted) in [(TURN, Stand::Turn), (BUSY, Stand::Busy), ("", Stand::Busy)] {
            let server = StandInEndpoint::start([])
                .like_paddock("pk", &[])
                .with_takes([Take::Status(503, body)]);
            let told = Mutex::new(Vec::new());
            around(&server, &Ending::default(), &|| false, &told, || ()).unwrap();
            let told = told.into_inner().unwrap();
            assert_eq!(told, [Stand::Asking, wanted, Stand::Granted], "{body}");
        }
    }

    #[test]
    fn a_lease_lost_during_the_call_fails_it_whatever_the_call_returned() {
        let server = StandInEndpoint::start([])
            .like_paddock("pk", &[])
            .refusing_renewals(404);
        let ending = Ending::default();
        let lost = around(&server, &ending, &|| false, &Mutex::default(), || {
            until("end", &server, || ending.asked());
            "finished anyway"
        });
        let Err(Unheld::Lost(why)) = lost else {
            panic!("{lost:?}");
        };
        assert!(why.contains("L1") && why.contains("HTTP 404"), "{why}");
    }

    #[test]
    fn a_round_that_gives_up_the_wait_never_runs_and_one_refused_names_why() {
        let server = StandInEndpoint::start([])
            .like_paddock("pk", &[])
            .with_takes([Take::Hang]);
        let ran = AtomicUsize::new(0);
        let call = || ran.fetch_add(1, Ordering::SeqCst);
        let told = Mutex::default();
        let waited = around(&server, &Ending::default(), &|| true, &told, call);
        assert_eq!(waited, Err(Unheld::GaveUp));
        assert_eq!(ran.load(Ordering::SeqCst), 0);

        let bad = r#"{"error":"bad_lease_request","detail":"ttl is at most 1h"}"#;
        let server = StandInEndpoint::start([])
            .like_paddock("pk", &[])
            .with_takes([Take::Status(400, bad)]);
        let refused = around(&server, &Ending::default(), &|| false, &told, call);
        let Err(Unheld::Refused(why)) = refused else {
            panic!("{refused:?}");
        };
        assert!(
            why.ends_with("bad_lease_request: ttl is at most 1h"),
            "{why}"
        );
        assert_eq!(ran.load(Ordering::SeqCst), 0);
    }
}
