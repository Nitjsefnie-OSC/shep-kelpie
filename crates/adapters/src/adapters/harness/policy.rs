//! The sandbox policy every harness starts from

use std::net::{IpAddr, ToSocketAddrs};
use std::path::Path;

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
