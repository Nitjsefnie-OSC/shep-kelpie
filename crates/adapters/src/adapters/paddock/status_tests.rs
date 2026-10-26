//! Paddock's shared lease-turn availability

use super::queued_in;

const FULL: &str = r#"{
    "models": [{"model":"coder","turns":{"lease":2,"request":1}}],
    "leases": [
        {"model":"coder","revoked":null,"reclaimable":false},
        {"model":"coder","revoked":null,"reclaimable":false}
    ],
    "waiters": [{"model":"coder","kind":"lease"}]
}"#;

#[test]
fn two_held_lease_turns_and_a_waiter_are_queued_despite_the_extra_request_turn() {
    assert!(queued_in(FULL, "coder"));
}
