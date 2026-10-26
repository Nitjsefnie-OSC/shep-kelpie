// A Codex call's variables: the fence's set by `env` inside the sandbox, and
// the system's certificates for a call that blocks the keychain.

use std::ffi::OsStr;
use std::process::Command;

use super::tests::{WORKER_ID, World, id};
use super::*;

fn command_of(w: &World, call: &AgentCall) -> Command {
    let codex = CodexCli::on(&Base::default(), w.path("login"));
    let headless = Launch {
        turn: Turn::Headless,
        served: None,
    };
    codex.cli.command(call, &None, &headless).unwrap()
}

fn env_of(command: &Command, name: &str) -> Option<OsString> {
    (command.get_envs())
        .find(|(n, _)| *n == OsStr::new(name))
        .and_then(|(_, value)| value.map(OsStr::to_owned))
}

#[test]
fn the_fences_variables_are_set_by_env_so_the_sandbox_cannot_replace_them() {
    let w = World::new();
    let mut call = w.fenced(Path::new("/k/kelpie"), Session::New(id(WORKER_ID)));
    let fence = call.reach.fence.as_mut().unwrap();
    fence.env.insert(
        "GIT_CONFIG_PARAMETERS".into(),
        "'credential.helper'=''".into(),
    );

    let command = command_of(&w, &call);

    assert_eq!(command.get_program(), "env");
    let args: Vec<&OsStr> = command.get_args().collect();
    let codex = args.iter().position(|a| *a == "codex").unwrap();
    let set = OsStr::new("GIT_CONFIG_PARAMETERS='credential.helper'=''");
    assert!(args[..codex].contains(&set), "{args:?}");
    assert_eq!(env_of(&command, "GIT_CONFIG_PARAMETERS"), None);
}

#[test]
fn a_call_that_blocks_the_keychain_trusts_the_systems_certificates_on_macos() {
    let w = World::new();
    let ca = |call: &AgentCall| env_of(&command_of(&w, call), "CODEX_CA_CERTIFICATE");
    let mut call = w.fenced(Path::new("/k/kelpie"), Session::New(id(WORKER_ID)));

    assert_eq!(ca(&call), None);
    call.reach.block_keychain = true;
    let system = cfg!(target_os = "macos").then(|| OsStr::new(SYSTEM_CERTIFICATES).to_owned());
    assert_eq!(ca(&call), system);
}
