//! The keychain probes, run under the real runtime where the machine allows it
//!
//! A test needs macOS, `shep kelpie tools install` run for the keychain-free
//! copy, and a keychain that takes a new login without asking. Without them it
//! says why on stderr and passes: a sandbox cannot start inside another, so
//! the runner's own calls skip it too. The logins it probes are its own, put
//! in the keychain with the `security` command as `gh` and git put theirs, and
//! taken out again.

use std::process::{Command, Output, Stdio};

use super::*;

const SECRET: &str = "kelpie-probe-secret";
const GIT_HOST: &str = "kelpie-probe.invalid";

// What a worker reaches: GitHub's API and git host.
const HOSTS: [&str; 2] = ["github.com", "api.github.com"];

fn sh(script: &str) -> Output {
    Command::new("/bin/sh")
        .args(["-c", script])
        .stdin(Stdio::null())
        .output()
        .expect("/bin/sh runs")
}

// A login in the macOS keychain, stored the way `gh` and git store theirs
struct Logins {
    service: String,
}

impl Logins {
    fn store() -> Option<Self> {
        let service = format!("kelpie-probe-{}", std::process::id());
        let generic =
            format!("security add-generic-password -s {service} -a kelpie -w {SECRET} -U");
        let git = format!(
            "printf 'protocol=https\\nhost={GIT_HOST}\\nusername=kelpie\\npassword={SECRET}\\n\\n' \
             | git credential-osxkeychain store"
        );
        // Dropped on the way out, so a half-stored pair is cleaned up too.
        let logins = Self { service };
        (sh(&generic).status.success() && sh(&git).status.success()).then_some(logins)
    }
}

impl Drop for Logins {
    fn drop(&mut self) {
        sh(&format!(
            "security delete-generic-password -s {}",
            self.service
        ));
        sh(&format!(
            "printf 'protocol=https\\nhost={GIT_HOST}\\n\\n' | git credential-osxkeychain erase"
        ));
    }
}

// `script` under the runtime, as a covered call (`blocked`) or an uncovered one runs it.
fn probe(tools: &Tools, blocked: bool, script: &str) -> Output {
    let (settings, work) = (crate::test::short_tempdir(), crate::test::short_tempdir());
    let policy = Policy {
        write: vec![work.path().canonicalize().unwrap()],
        hosts: HOSTS.map(str::to_owned).into(),
        verify_tls: true,
        no_keychain: blocked,
        ..Policy::default()
    };
    let mut inner = Command::new("/bin/sh");
    inner.args(["-c", script]).current_dir(work.path());
    let mut wrapped = SandboxRuntime::new(tools.clone())
        .wrap(&policy, &settings.path().join("srt.json"), &inner)
        .expect("the runtime wraps the probe");
    wrapped.stdin(Stdio::null());
    wrapped.output().expect("the runtime runs")
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

// The installed runtime with its keychain-free copy, or why there is none.
fn runtime() -> Result<Tools, String> {
    if !cfg!(target_os = "macos") {
        return Err("there is no keychain off macOS".into());
    }
    let home = crate::home::kelpie_home().map_err(|e| e.to_string())?;
    let tools = Tools::under(&home);
    match tools.sandbox().is_file() && tools.keychain_blocked() {
        true => Ok(tools),
        false => Err(format!(
            "run `shep kelpie tools install` for {}",
            home.display()
        )),
    }
}

fn online() -> bool {
    sh("curl -sS -f -o /dev/null --max-time 10 https://api.github.com")
        .status
        .success()
}

fn skipped(why: &str) {
    eprintln!("skipped: {why}");
}

#[test]
fn a_covered_call_cannot_ask_the_keychain_for_a_login_and_an_uncovered_one_can() {
    let tools = match runtime() {
        Ok(tools) => tools,
        Err(why) => return skipped(&why),
    };
    let Some(logins) = Logins::store() else {
        return skipped("the keychain did not take a login without asking");
    };
    let find = format!("security find-generic-password -s {} -w", logins.service);
    let git =
        format!("printf 'protocol=https\\nhost={GIT_HOST}\\n\\n' | git credential-osxkeychain get");

    // As shipped, the premise: the command's logins are one call away.
    let open = probe(&tools, false, &find);
    assert!(stdout(&open).contains(SECRET), "{open:?}");
    let open = probe(&tools, false, &git);
    assert!(stdout(&open).contains(SECRET), "{open:?}");

    // Blocked: refused, and no secret in what it prints.
    for script in [&find, &git] {
        let shut = probe(&tools, true, script);
        assert!(!stdout(&shut).contains(SECRET), "{script}: {shut:?}");
    }
    assert!(!probe(&tools, true, &find).status.success());
}

#[test]
fn a_covered_call_still_reaches_github_signs_and_runs_a_program() {
    let tools = match runtime() {
        Ok(tools) => tools,
        Err(why) => return skipped(&why),
    };
    if !online() {
        return skipped("GitHub is out of reach");
    }
    let work: [(&str, &str); 4] = [
        ("curl", "curl -sS -f -o /dev/null https://api.github.com"),
        (
            "git ls-remote",
            "git ls-remote https://github.com/shep-pm/shep-kelpie HEAD",
        ),
        (
            "python urllib",
            "python3 -c \"import urllib.request as u; u.urlopen('https://api.github.com')\"",
        ),
        (
            "codesign",
            "cp /usr/bin/true ./true && codesign --force --sign - ./true && ./true",
        ),
    ];
    for (name, script) in work {
        let done = probe(&tools, true, script);
        assert!(done.status.success(), "{name}: {done:?}");
    }
}
