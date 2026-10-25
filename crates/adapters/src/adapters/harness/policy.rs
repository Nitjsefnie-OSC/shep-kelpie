//! The sandbox policy every harness starts from

use std::env;
use std::net::{IpAddr, ToSocketAddrs};
use std::path::{Path, PathBuf};

use crate::dep_sources::Env;
use crate::forwarder::{Upstream, WORKER_HOST};
use crate::ports::{AgentCall, Fence, Forward, Policy, Tools};
use crate::profile::CREDENTIALS;

/// What a fenced session may do, from its fence alone
pub fn fence_policy(fence: &Fence) -> Policy {
    Policy {
        write: fence.write.clone(),
        no_write: fence.no_write.clone(),
        no_read: fence.no_read.clone(),
        read: fence.read.clone(),
        open: Vec::new(),
        scratch: Vec::new(),
        hosts: fence.hosts.clone(),
        sockets: fence.sockets.clone(),
        socket_folders: Vec::new(),
        forward: None,
        denied_hosts: Vec::new(),
        denied_addresses: Vec::new(),
        // Without it, `gh` fails TLS verification on macOS: x509 OSStatus -26276.
        verify_tls: true,
        no_keychain: false,
        listen: false,
        services: Vec::new(),
    }
}

/// What a call is held to before its harness adds its own needs: its fence's
/// policy, or with no fence the credentials and `unfenced` unread
///
/// Every harness starts here, so a call with no fence is denied `unfenced`
/// whichever one runs it. Its own folder stays readable, since a worktree
/// can sit inside the shepherd's home.
pub(crate) fn call_policy(call: &AgentCall, unfenced: &[String]) -> Policy {
    match &call.reach.fence {
        Some(fence) => fence_policy(fence),
        None => {
            let no_read = CREDENTIALS.iter().map(|&p| p.to_owned());
            let mut policy = Policy {
                no_read: no_read.chain(unfenced.iter().cloned()).collect(),
                ..Policy::default()
            };
            if !unfenced.is_empty() {
                policy.read.push(call.cwd.clone());
            }
            policy
        }
    }
}

/// Where `program` runs, as the files the sandbox must let through
///
/// A call whose sandbox denies the home folder whole must allow back the harness
/// it starts, which may sit under it: a bare name is looked up on `PATH`, and a
/// link is followed to what it runs, since the sandbox judges the real file but
/// the shell still has to read the link it ran.
pub(crate) fn program_folder(program: &Path, env: &dyn Env) -> Vec<PathBuf> {
    let on_path = || {
        env.get("PATH")
            .iter()
            .flat_map(|paths| env::split_paths(paths))
            .map(|folder| folder.join(program))
            .find(|candidate| candidate.is_file())
    };
    // `Path::parent` answers an empty folder for a bare name rather than none,
    // and an empty folder is no folder: the name is the shell's to find.
    let bare = program
        .parent()
        .is_none_or(|parent| parent.as_os_str().is_empty());
    let Some(found) = (if bare {
        on_path()
    } else {
        program.is_file().then(|| program.to_owned())
    }) else {
        return Vec::new();
    };
    let real = found.canonicalize().unwrap_or_else(|_| found.clone());
    [found, real]
        .into_iter()
        .collect::<std::collections::BTreeSet<PathBuf>>()
        .into_iter()
        .collect()
}

/// Lets a call that runs a work item's tests bind Unix sockets in `scratch`,
/// the folder its `TMPDIR` names, where the tests make theirs
///
/// srt matches the real path, so `scratch` is resolved where it exists.
pub(crate) fn bind_in_scratch(policy: &mut Policy, call: &AgentCall, scratch: &Path) {
    if call.tools == Tools::Work {
        let real = scratch.canonicalize();
        policy
            .socket_folders
            .push(real.unwrap_or_else(|_| scratch.to_owned()));
    }
}

/// Holds a call on a model server to the one host it may dial: the forwarder at `socket`
pub(crate) fn dial_the_forwarder(policy: &mut Policy, upstream: &Upstream, socket: &Path) {
    deny_the_server(policy, upstream);
    policy.forward = Some(Forward {
        host: WORKER_HOST.to_owned(),
        socket: socket.to_owned(),
    });
}

// Whatever `allowed_domains` says, the sandbox never reaches the model server
// or this machine's loopback by name or by the address the server resolves to.
pub(crate) fn deny_the_server(policy: &mut Policy, upstream: &Upstream) {
    let bracketed = |host: &str| match host.contains(':') {
        true => format!("[{host}]"),
        false => host.to_owned(),
    };
    for host in [upstream.host(), "localhost", "127.0.0.1", "::1"] {
        let host = bracketed(host);
        if !policy.denied_hosts.contains(&host) {
            policy.denied_hosts.push(host);
        }
    }
    let resolved = upstream.address().to_socket_addrs().into_iter().flatten();
    for address in resolved {
        let address = match address.ip() {
            IpAddr::V6(v6) => v6.to_ipv4_mapped().map_or(address.ip(), IpAddr::V4),
            v4 => v4,
        };
        let address = address.to_string();
        if !policy.denied_addresses.contains(&address) {
            policy.denied_addresses.push(address);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::program_folder;
    use crate::dep_sources::Env;
    use crate::test::{NO_PROGRAM, write_script};
    use std::path::{Path, PathBuf};

    // The environment a test names: `PATH` alone, which is all the lookup reads
    struct Fixed(Option<String>);

    impl Env for Fixed {
        fn get(&self, name: &str) -> Option<String> {
            (name == "PATH").then(|| self.0.clone()).flatten()
        }
    }

    // A folder on the `PATH` that holds a `claude` of its own
    fn on_path() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("bin");
        std::fs::create_dir(&bin).unwrap();
        let program = bin.join("claude");
        write_script(&program, "#!/bin/sh\nexit 0\n");
        (dir, program)
    }

    #[test]
    fn a_bare_program_name_is_found_on_the_path_that_holds_it() {
        let (_dir, program) = on_path();
        let env = Fixed(Some(program.parent().unwrap().display().to_string()));
        let grant = program_folder(Path::new("claude"), &env);
        assert!(grant.contains(&program), "{grant:?}");
    }

    #[test]
    fn a_bare_name_no_path_holds_gets_no_grant() {
        let env = Fixed(Some("/nowhere/bin".to_owned()));
        assert!(program_folder(Path::new("claude"), &env).is_empty());
    }

    #[test]
    fn a_program_that_is_no_file_gets_no_grant() {
        assert!(program_folder(&NO_PROGRAM, &Fixed(None)).is_empty());
    }

    #[test]
    fn a_folder_named_as_the_program_gets_no_grant() {
        let (_dir, program) = on_path();
        let folder = program.parent().unwrap();
        assert!(program_folder(folder, &Fixed(None)).is_empty());
    }
}
