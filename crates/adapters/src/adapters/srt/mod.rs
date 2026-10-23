//! The sandbox runtime a call runs under
//!
//! On macOS it is crabpot, linked into kelpie's own binary: a wrapped command
//! is `shep-kelpie --crabpot --settings <file> -- prog args`, which
//! `crabpot::run_from_args` takes over first thing in `main`. On Linux it is
//! `srt` from kelpie's own tools, the engine under Claude Code's own sandbox:
//! bubblewrap, and a proxy that holds the network to a host list. Kelpie pins
//! its version in [`crate::tools::PACKAGE`]. A sandbox cannot start inside
//! another, so nothing it runs may start one of its own.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{Value, json};

use crate::ports::{Policy, Sandbox, SandboxError};
use crate::tools::Tools;

// The Mach services a command asks the keychain through, which a covered call may not look up
const KEYCHAIN_SERVICES: [&str; 2] = ["com.apple.securityd.xpc", "com.apple.SecurityServer"];

/// The sandbox runtime installed under kelpie's tools
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SandboxRuntime {
    tools: Tools,
    macos: bool,
    // The program that is crabpot on macOS: kelpie itself, unless a test names the built one.
    kelpie: Option<PathBuf>,
}

impl SandboxRuntime {
    /// The runtime `shep kelpie tools install` put in `tools`
    pub fn new(tools: Tools) -> Self {
        Self {
            tools,
            macos: cfg!(target_os = "macos"),
            kelpie: None,
        }
    }

    /// The same, as on macOS or not, which a test runs on either
    #[cfg(test)]
    pub(crate) fn on_macos(self, macos: bool) -> Self {
        Self { macos, ..self }
    }

    /// The same, with `kelpie` as crabpot on macOS, since a test binary has no
    /// `run_from_args`
    #[cfg(test)]
    pub(crate) fn run_by(self, kelpie: PathBuf) -> Self {
        Self {
            kelpie: Some(kelpie),
            ..self
        }
    }

    // The program that runs a command in crabpot's sandbox
    fn crabpot(&self) -> Result<PathBuf, SandboxError> {
        match &self.kelpie {
            Some(kelpie) => Ok(kelpie.clone()),
            None => std::env::current_exe().map_err(|e| SandboxError::Executable(e.to_string())),
        }
    }

    // The command that runs a program under `--settings`: crabpot on macOS, node `srt` elsewhere
    fn runtime(&self) -> Result<Command, SandboxError> {
        if self.macos {
            let mut crabpot = crate::spawn::command(self.crabpot()?);
            crabpot.arg("--crabpot");
            return Ok(crabpot);
        }
        let cli = self.tools.sandbox();
        if !cli.is_file() {
            return Err(SandboxError::Missing(cli));
        }
        let mut node = crate::spawn::command("node");
        node.arg(cli);
        Ok(node)
    }
}

impl Sandbox for SandboxRuntime {
    fn wrap(
        &self,
        policy: &Policy,
        settings: &Path,
        command: &Command,
    ) -> Result<Command, SandboxError> {
        let mut wrapped = self.runtime()?;
        let text = serde_json::to_string_pretty(&settings_on(policy, self.macos))
            .expect("settings are JSON");
        let folder = settings.parent().unwrap_or(Path::new("/"));
        std::fs::create_dir_all(folder)
            .and_then(|()| std::fs::write(settings, text))
            .map_err(|e| SandboxError::Settings(settings.to_owned(), e.kind().to_string()))?;
        wrapped
            .arg("--settings")
            .arg(settings)
            .arg("--")
            .arg(command.get_program())
            .args(command.get_args());
        if let Some(dir) = command.get_current_dir() {
            wrapped.current_dir(dir);
        }
        for (name, value) in command.get_envs() {
            match value {
                Some(value) => wrapped.env(name, value),
                None => wrapped.env_remove(name),
            };
        }
        Ok(wrapped)
    }
}

/// `srt`'s settings file for `policy`, on this machine
#[cfg(any(test, feature = "test-support"))]
pub fn srt_settings(policy: &Policy) -> Value {
    settings_on(policy, cfg!(target_os = "macos"))
}

// `srt`'s settings file for `policy`, on macOS or not: `open`, `scratch` and
// `socket_folders` reach srt on macOS alone.
fn settings_on(policy: &Policy, macos: bool) -> Value {
    // Without `strictAllowlist`, a host off the list is asked about rather than refused.
    let mut network = json!({
        "allowedDomains": policy.hosts,
        "deniedDomains": policy.denied_hosts,
        "strictAllowlist": true,
    });
    if !policy.denied_addresses.is_empty() {
        network["deniedResolvedAddresses"] = json!(policy.denied_addresses);
    }
    if let Some(forward) = &policy.forward {
        let mut hosts = policy.hosts.clone();
        hosts.push(forward.host.clone());
        network["allowedDomains"] = json!(hosts);
        network["mitmProxy"] = json!({ "socketPath": forward.socket, "domains": [&forward.host] });
    }
    let folders = policy.socket_folders.iter().filter(|_| macos);
    let sockets: Vec<&PathBuf> = policy.sockets.iter().chain(folders).collect();
    if !sockets.is_empty() {
        network["allowUnixSockets"] = json!(sockets);
    }
    if policy.listen {
        network["allowLocalBinding"] = json!(true);
    }
    if !policy.services.is_empty() {
        network["allowMachLookup"] = json!(policy.services);
    }
    // Only macOS has a keychain to ask. crabpot denies these last, over any allow.
    if policy.no_keychain && macos {
        network["denyMachLookup"] = json!(KEYCHAIN_SERVICES);
    }
    let mut read = json!(policy.read);
    let mut write = json!(policy.write);
    if macos {
        let (read, write) = (array(&mut read), array(&mut write));
        read.extend(policy.open.iter().map(|folder| alone(folder).into()));
        for glob in &policy.scratch {
            write.extend([json!(glob), json!(format!("{glob}/*"))]);
        }
    }
    json!({
        "network": network,
        "filesystem": {
            "denyRead": policy.no_read,
            "allowRead": read,
            "allowWrite": write,
            "denyWrite": policy.no_write,
        },
        "enableWeakerNetworkIsolation": policy.verify_tls,
    })
}

fn array(value: &mut Value) -> &mut Vec<Value> {
    value
        .as_array_mut()
        .expect("a list of paths is a JSON array")
}

// `folder` as a glob that matches it and nothing under it. srt reads a path
// with no glob character as the folder and all it holds, and on macOS turns a
// glob into a regex of the whole path, so one bracketed character is enough.
fn alone(folder: &Path) -> String {
    let text = folder.to_string_lossy();
    let mut glob = String::with_capacity(text.len() + 2);
    for c in text.chars() {
        match c {
            '*' | '?' | '[' | ']' => glob.extend(['[', c, ']']),
            c => glob.push(c),
        }
    }
    if glob.len() == text.len()
        && let Some(last) = glob.pop()
    {
        glob.extend(['[', last, ']']);
    }
    glob
}

/// The `shep-kelpie` binary built beside this test binary, which has the
/// `crabpot::run_from_args` a test binary lacks, or why there is none
#[cfg(test)]
pub(crate) fn built_kelpie() -> Result<PathBuf, String> {
    let test = std::env::current_exe().map_err(|e| e.to_string())?;
    // `<target>/debug/deps/<test>` beside `<target>/debug/shep-kelpie`
    let kelpie = test
        .parent()
        .and_then(Path::parent)
        .map(|debug| debug.join(format!("shep-kelpie{}", std::env::consts::EXE_SUFFIX)))
        .ok_or_else(|| format!("{} has no folder above it", test.display()))?;
    match kelpie.is_file() {
        true => Ok(kelpie),
        false => Err(format!("run `cargo build` for {}", kelpie.display())),
    }
}

#[cfg(test)]
mod live;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_runtime_refuses_to_run_anything() {
        let dir = tempfile::tempdir().unwrap();
        let srt = SandboxRuntime::new(Tools::at(dir.path().to_owned())).on_macos(false);
        let settings = dir.path().join("call.srt.json");
        let err = srt
            .wrap(&Policy::default(), &settings, &Command::new("claude"))
            .unwrap_err();
        assert!(matches!(err, SandboxError::Missing(_)), "{err:?}");
        assert!(
            err.to_string().contains("shep kelpie tools install"),
            "{err}"
        );
        assert!(!settings.exists());
    }

    #[test]
    fn a_forwarded_host_is_the_only_one_allowed_and_its_traffic_goes_to_the_socket() {
        let policy = Policy {
            forward: Some(crate::ports::Forward {
                host: "model.kelpie.test".into(),
                socket: PathBuf::from("/k/worker/model.sock"),
            }),
            ..Policy::default()
        };
        let network = &srt_settings(&policy)["network"];
        assert_eq!(network["allowedDomains"], json!(["model.kelpie.test"]));
        assert_eq!(network["deniedDomains"], json!([]));
        assert!(network.get("deniedResolvedAddresses").is_none());
        assert_eq!(
            network["mitmProxy"],
            json!({ "socketPath": "/k/worker/model.sock", "domains": ["model.kelpie.test"] })
        );
        assert!(network.get("allowUnixSockets").is_none());
        let denying = Policy {
            denied_hosts: vec!["192.0.2.9".into(), "localhost".into()],
            denied_addresses: vec!["192.0.2.9".into()],
            ..policy.clone()
        };
        let network = &srt_settings(&denying)["network"];
        assert_eq!(network["deniedDomains"], json!(["192.0.2.9", "localhost"]));
        assert_eq!(network["deniedResolvedAddresses"], json!(["192.0.2.9"]));
        let plain = &srt_settings(&Policy::default())["network"];
        assert!(plain.get("mitmProxy").is_none());
    }

    #[test]
    fn a_policy_that_listens_binds_a_local_port_and_looks_up_its_services() {
        let listening = Policy {
            listen: true,
            services: vec!["com.apple.FSEvents".into()],
            ..Policy::default()
        };
        let network = &srt_settings(&listening)["network"];
        assert_eq!(network["allowLocalBinding"], json!(true));
        assert_eq!(network["allowMachLookup"], json!(["com.apple.FSEvents"]));
        let plain = &srt_settings(&Policy::default())["network"];
        assert!(plain.get("allowLocalBinding").is_none(), "{plain}");
        assert!(plain.get("allowMachLookup").is_none(), "{plain}");
    }

    // From 0.0.80 srt blocks the user namespaces a command makes itself unless
    // this is set, and setting it gives up the write denies the fence rests on
    // (`docs/facts/worker-profile-and-sandbox.md`).
    #[test]
    fn a_command_may_not_make_user_namespaces_of_its_own() {
        for macos in [true, false] {
            let settings = settings_on(&Policy::default(), macos);
            assert!(
                settings.get("allowNestedUserNamespaces").is_none(),
                "{settings}"
            );
        }
    }

    #[test]
    fn on_macos_an_opened_folder_is_read_alone_and_a_scratch_glob_is_written_one_level_down() {
        let policy = Policy {
            read: vec![PathBuf::from("/k/wt/7")],
            write: vec![PathBuf::from("/k/wt/7")],
            open: vec![PathBuf::from("/k/graphs"), PathBuf::from("/k/odd[1]*?")],
            scratch: vec!["/private/tmp/cbm-search-*".into()],
            ..Policy::default()
        };
        let files = &settings_on(&policy, true)["filesystem"];
        assert_eq!(
            files["allowRead"],
            json!(["/k/wt/7", "/k/graph[s]", "/k/odd[[]1[]][*][?]"])
        );
        assert_eq!(
            files["allowWrite"],
            json!([
                "/k/wt/7",
                "/private/tmp/cbm-search-*",
                "/private/tmp/cbm-search-*/*"
            ])
        );
        let files = &settings_on(&policy, false)["filesystem"];
        assert_eq!(files["allowRead"], json!(["/k/wt/7"]));
        assert_eq!(files["allowWrite"], json!(["/k/wt/7"]));
    }

    #[test]
    fn on_macos_a_socket_folder_is_listed_after_the_sockets_and_off_macos_it_is_not() {
        let socket = PathBuf::from("/k/dog/lease.sock");
        let folder = PathBuf::from("/private/k/call.tmp");
        let policy = |sockets: Vec<PathBuf>, socket_folders: Vec<PathBuf>| Policy {
            sockets,
            socket_folders,
            ..Policy::default()
        };
        let both = policy(vec![socket.clone()], vec![folder.clone()]);
        let folder_only = policy(vec![], vec![folder.clone()]);
        let socket_only = policy(vec![socket.clone()], vec![]);
        let neither = policy(vec![], vec![]);

        let listed = |policy: &Policy, macos: bool| {
            settings_on(policy, macos)["network"]
                .get("allowUnixSockets")
                .cloned()
        };
        assert_eq!(
            listed(&both, true),
            Some(json!(["/k/dog/lease.sock", "/private/k/call.tmp"]))
        );
        assert_eq!(
            listed(&folder_only, true),
            Some(json!(["/private/k/call.tmp"]))
        );
        assert_eq!(
            listed(&socket_only, true),
            Some(json!(["/k/dog/lease.sock"]))
        );
        assert_eq!(listed(&neither, true), None);

        assert_eq!(listed(&both, false), Some(json!(["/k/dog/lease.sock"])));
        assert_eq!(listed(&folder_only, false), None);
        assert_eq!(
            listed(&socket_only, false),
            Some(json!(["/k/dog/lease.sock"]))
        );
        assert_eq!(listed(&neither, false), None);
    }

    // A tools folder with the node runtime installed
    fn installed(dir: &Path) -> Tools {
        let tools = Tools::at(dir.to_owned());
        std::fs::create_dir_all(tools.sandbox().parent().unwrap()).unwrap();
        std::fs::write(tools.sandbox(), "").unwrap();
        tools
    }

    // The call every wrap test makes, and the settings file it writes
    fn inner() -> Command {
        let mut inner = Command::new("claude");
        inner
            .args(["-p", "it's"])
            .current_dir("/k/wt/7")
            .env("CARGO_TARGET_DIR", "/k/targets/7")
            .env_remove("CLAUDECODE");
        inner
    }

    fn policy() -> Policy {
        Policy {
            write: vec![PathBuf::from("/k/wt/7")],
            hosts: vec!["api.anthropic.com".into()],
            ..Policy::default()
        }
    }

    fn written(settings: &Path) -> Value {
        serde_json::from_str(&std::fs::read_to_string(settings).unwrap()).unwrap()
    }

    fn assert_carries_the_call(wrapped: &Command) {
        assert_eq!(wrapped.get_current_dir(), Some(Path::new("/k/wt/7")));
        let envs: Vec<_> = wrapped.get_envs().collect();
        assert!(envs.contains(&("CARGO_TARGET_DIR".as_ref(), Some("/k/targets/7".as_ref()))));
        assert!(envs.contains(&("CLAUDECODE".as_ref(), None)));
    }

    #[test]
    fn on_macos_the_wrapped_command_is_current_exe_crabpot_settings() {
        // Nothing is installed: crabpot is in kelpie, so no node `srt` is needed.
        let dir = tempfile::tempdir().unwrap();
        let settings = dir.path().join("worker").join("call.srt.json");
        let wrapped = SandboxRuntime::new(Tools::at(dir.path().to_owned()))
            .on_macos(true)
            .wrap(&policy(), &settings, &inner())
            .unwrap();
        assert_eq!(
            wrapped.get_program(),
            std::env::current_exe().unwrap().as_os_str()
        );
        let args: Vec<_> = wrapped.get_args().collect();
        assert_eq!(
            args,
            [
                "--crabpot".as_ref(),
                "--settings".as_ref(),
                settings.as_os_str(),
                "--".as_ref(),
                "claude".as_ref(),
                "-p".as_ref(),
                "it's".as_ref(),
            ]
        );
        assert_carries_the_call(&wrapped);
        let written = written(&settings);
        assert_eq!(written, settings_on(&policy(), true));
        assert_eq!(written["network"]["strictAllowlist"], true);
        assert_eq!(written["filesystem"]["allowWrite"], json!(["/k/wt/7"]));
    }

    #[test]
    fn on_linux_the_wrapped_command_is_still_node() {
        let dir = tempfile::tempdir().unwrap();
        let tools = installed(dir.path());
        let settings = dir.path().join("worker").join("call.srt.json");
        let wrapped = SandboxRuntime::new(tools.clone())
            .on_macos(false)
            .wrap(&policy(), &settings, &inner())
            .unwrap();
        assert_eq!(wrapped.get_program(), "node");
        let args: Vec<_> = wrapped.get_args().collect();
        let cli = tools.sandbox();
        assert_eq!(
            args,
            [
                cli.as_os_str(),
                "--settings".as_ref(),
                settings.as_os_str(),
                "--".as_ref(),
                "claude".as_ref(),
                "-p".as_ref(),
                "it's".as_ref(),
            ]
        );
        assert_carries_the_call(&wrapped);
        let written = written(&settings);
        assert_eq!(written, settings_on(&policy(), false));
        assert_eq!(written["network"]["strictAllowlist"], true);
        assert_eq!(written["filesystem"]["allowWrite"], json!(["/k/wt/7"]));
    }

    #[test]
    fn no_keychain_adds_deny_mach_lookup_for_both_services() {
        let covered = Policy {
            no_keychain: true,
            services: vec!["com.apple.FSEvents".into()],
            ..Policy::default()
        };
        let network = &settings_on(&covered, true)["network"];
        assert_eq!(
            network["denyMachLookup"],
            json!(["com.apple.securityd.xpc", "com.apple.SecurityServer"])
        );
        // The allow is kept: crabpot denies last, over every allow.
        assert_eq!(network["allowMachLookup"], json!(["com.apple.FSEvents"]));
        // A call that leaves the keychain reachable denies nothing.
        let open = &settings_on(&Policy::default(), true)["network"];
        assert!(open.get("denyMachLookup").is_none(), "{open}");
        // There is no keychain to ask off macOS.
        let linux = &settings_on(&covered, false)["network"];
        assert!(linux.get("denyMachLookup").is_none(), "{linux}");
    }

    // The settings are read by crabpot's own parser, which refuses a key it
    // cannot apply yet rather than drop it.
    #[test]
    fn crabpot_reads_every_setting_a_policy_can_write_on_macos() {
        let policy = Policy {
            read: vec![PathBuf::from("/k/wt/7")],
            write: vec![PathBuf::from("/k/wt/7")],
            no_read: vec!["/Users/me/**".into()],
            no_write: vec![PathBuf::from("/k/wt/7/.git/hooks")],
            open: vec![PathBuf::from("/k/graphs")],
            scratch: vec!["/private/tmp/cbm-search-*".into()],
            hosts: vec!["github.com".into()],
            denied_hosts: vec!["localhost".into()],
            denied_addresses: vec!["192.0.2.9".into()],
            forward: Some(crate::ports::Forward {
                host: "model.kelpie.test".into(),
                socket: PathBuf::from("/k/worker/model.sock"),
            }),
            sockets: vec![PathBuf::from("/k/dog/lease.sock")],
            socket_folders: vec![PathBuf::from("/private/k/call.tmp")],
            listen: true,
            services: vec!["com.apple.FSEvents".into()],
            verify_tls: true,
            no_keychain: true,
        };
        let text = settings_on(&policy, true).to_string();
        let config = crabpot::Config::from_json(&text).unwrap_or_else(|e| panic!("{e}: {text}"));
        assert_eq!(
            config.network.deny_mach_lookup,
            Some(vec![
                "com.apple.securityd.xpc".to_owned(),
                "com.apple.SecurityServer".to_owned()
            ])
        );
    }
}
