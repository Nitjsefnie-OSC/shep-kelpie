// A Codex call that blocks the keychain verifies certificates against the
// system's bundle, since it cannot ask the keychain.

use std::ffi::OsStr;

use super::tests::{WORKER_ID, World, id};
use super::*;

#[test]
fn a_call_that_blocks_the_keychain_trusts_the_systems_certificates_on_macos() {
    let w = World::new();
    let codex = CodexCli::on(&Base::default(), w.path("login"));
    let headless = Launch {
        turn: Turn::Headless,
        served: None,
    };
    let ca = |call: &AgentCall| {
        let command = codex.cli.command(call, &None, &headless).unwrap();
        (command.get_envs())
            .find(|(name, _)| *name == OsStr::new("CODEX_CA_CERTIFICATE"))
            .and_then(|(_, value)| value.map(OsStr::to_owned))
    };
    let mut call = w.fenced(Path::new("/k/kelpie"), Session::New(id(WORKER_ID)));

    assert_eq!(ca(&call), None);
    call.reach.block_keychain = true;
    let system = cfg!(target_os = "macos").then(|| OsStr::new(SYSTEM_CERTIFICATES).to_owned());
    assert_eq!(ca(&call), system);
}
