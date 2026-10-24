//! The browser proof of a web bump: its dev server, and its routes opened in the local browser
//!
//! The server starts from `main`'s launch file, as a worker turn's does, but in
//! the bump's checkout, which holds code a bot changed. So it runs in a sandbox
//! that writes only that checkout and its build folder, and reaches only the
//! project's `browser.domains`. The plan is made on the runner's thread, where
//! the settings are, and finished on the proof's thread once the checkout is
//! there. The browser is headless whatever `headed` says.
//!
//! A route passes when the page loads, no request to the dev server's origin
//! answers 5xx, and the console logs no error. What fails says the route and
//! the status, and never the URL or path of a request.

use std::collections::BTreeSet;
use std::path::Path;
use std::sync::Arc;

use super::{Runner, command_of, launch};
use crate::ports::{
    ConsoleError, DevPort, DevRun, DevServers, PageSession, Pages, Policy, READY_WITHIN, Visit,
};

/// What the browser proof of one head came to
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::runner) enum Browsed {
    /// Every route passed
    Passed,
    /// A route failed, a line for each reason
    Failed(Vec<String>),
    /// The head cannot be proved in the browser, now or on another try
    Unavailable(String),
    /// The proof did not happen and is worth asking for again
    NotRun(String),
}

/// A browser proof planned, to run once the head is checked out
pub(in crate::runner) struct BrowserProof {
    configuration: launch::Configuration,
    run: DevRun,
    session: PageSession,
    routes: Vec<String>,
    servers: Arc<dyn DevServers>,
    pages: Arc<dyn Pages>,
}

impl Runner {
    /// Whether this project proves a web bump in the local browser: it turned
    /// the browser on, this is macOS, and the tools are installed
    fn bump_has_browser(&self) -> bool {
        // Chromium outside the sandbox is a macOS measurement; Linux waits on #135.
        let here = cfg!(any(target_os = "macos", test));
        self.settings.browser.enabled && here && self.paths.tools.has_browser()
    }

    /// The browser proof of bump `pull_request`, planned, if the project has the browser
    /// and `main` has a launch file to start a server from
    ///
    /// `Ok(None)` when it has no browser to prove with. The port it picked is held until
    /// [`Runner::release_proof_port`], so no other server takes it.
    ///
    /// # Errors
    ///
    /// Why the project has the browser but the proof cannot be planned, which leaves the
    /// bump open until it can.
    pub(in crate::runner) fn browser_proof(
        &self,
        pull_request: u64,
    ) -> Result<Option<BrowserProof>, String> {
        if !self.bump_has_browser() {
            return Ok(None);
        }
        let browser = &self.settings.browser;
        let text = match launch::read(&self.settings.git.checkout, self.settings.git.base())? {
            Some(text) => text,
            None => return Ok(None),
        };
        let named = browser.configuration.as_ref();
        let Some(configuration) =
            launch::select(&text, named.map(|n| n.as_str()), self.settings.git.base())?
        else {
            return Ok(None);
        };
        let (port, picked) = self
            .port_for(&configuration)
            .map_err(|why| format!("a web bump, and its dev server cannot be had: {why}"))?;
        self.planned.hold_proof(pull_request, Some(port));
        let worktree = self.paths.bump_worktree(pull_request);
        let build = self.paths.bump_build(pull_request);
        let (args, env) = command_of(&configuration, port, picked, &build.join(super::TEMP));
        let hosts: Vec<String> = (browser.domains.iter())
            .map(|d| d.as_str().to_owned())
            .collect();
        let gate = DevPort::default();
        let worktree_read = worktree.clone();
        let run = DevRun {
            program: configuration.runtime_executable.clone(),
            args,
            cwd: worktree.clone(),
            port,
            env,
            policy: Policy {
                write: vec![worktree, build.clone()],
                no_read: self.proof_no_read(),
                // Both sit under kelpie's home, which `no_read` fences.
                read: vec![worktree_read, build.clone()],
                hosts: hosts.clone(),
                verify_tls: true,
                no_keychain: self.app_covers(),
                listen: true,
                services: vec![super::FS_EVENTS.to_owned()],
                ..Policy::default()
            },
            settings: (self.paths.worker).join(format!("dev-server-bump-{pull_request}.srt.json")),
            log: build.join("dev-server.log"),
            ready_within: READY_WITHIN,
            gate: gate.clone(),
        };
        let tools = &self.paths.tools;
        let session = PageSession {
            playwright: tools.playwright_mcp(),
            browsers: tools.browsers(),
            folder: self.paths.bump_browser(pull_request),
            sockets: self.paths.worker.clone(),
            domains: hosts,
            gate,
        };
        Ok(Some(BrowserProof {
            configuration,
            run,
            session,
            routes: browser.routes.clone(),
            servers: Arc::clone(&self.ports.dev_servers),
            pages: Arc::clone(&self.ports.pages),
        }))
    }

    /// Lets go of the port held for bump `pull_request`'s proof
    pub(in crate::runner) fn release_proof_port(&self, pull_request: u64) {
        self.planned.hold_proof(pull_request, None);
    }

    // What a server running a bot's code may not read: the logins and the homes of
    // shep and kelpie, as a worker's own fence has it.
    fn proof_no_read(&self) -> Vec<String> {
        let (shep, kelpie) = (&self.paths.shep_home, &self.paths.kelpie_home);
        let homes = [shep.clone()]
            .into_iter()
            .chain((!kelpie.starts_with(shep)).then(|| kelpie.clone()))
            .map(|home| format!("{}/**", home.display()));
        (crate::profile::CREDENTIALS.iter())
            .map(|&p| p.to_owned())
            .chain(homes)
            .collect()
    }
}

impl BrowserProof {
    /// Starts the server in the checkout at `worktree`, opens each route, and judges them
    ///
    /// The server stops, and its port shuts to the browser, before this returns.
    pub(in crate::runner) fn run(mut self, worktree: &Path) -> Browsed {
        match launch::folder(worktree, &self.configuration) {
            Ok(cwd) => self.run.cwd = cwd,
            Err(why) => return Browsed::Unavailable(format!("its dev server cannot start: {why}")),
        }
        if let Some(build) = self.run.log.parent().map(Path::to_owned) {
            let temp = build.join(super::TEMP);
            if let Err(e) = std::fs::create_dir_all(&temp) {
                return Browsed::NotRun(format!("cannot make {}: {e}", temp.display()));
            }
            super::install::before(&mut self.run, worktree, &build);
        }
        let started = match self.servers.start(&self.run) {
            Ok(started) => started,
            Err(why) => {
                return Browsed::Unavailable(format!("its dev server cannot start: {why}"));
            }
        };
        let _shut = Shut(self.run.gate.clone());
        if !started.answered {
            return Browsed::Unavailable(format!(
                "its dev server had not answered after {} seconds",
                self.run.ready_within.as_secs()
            ));
        }
        self.run.gate.open(self.run.port);
        let urls: Vec<String> = (self.routes.iter())
            .map(|route| format!("http://localhost:{}{route}", self.run.port))
            .collect();
        let visits = match self.pages.visit(&self.session, &urls) {
            Ok(visits) => visits,
            Err(why) => return Browsed::NotRun(format!("the browser could not be driven: {why}")),
        };
        let failed: Vec<String> = (self.routes.iter().zip(&visits))
            .flat_map(|(route, visit)| judge(route, self.run.port, visit))
            .collect();
        match failed.is_empty() && visits.len() == self.routes.len() {
            true => Browsed::Passed,
            false if failed.is_empty() => {
                Browsed::NotRun("the browser opened fewer pages than it was asked".to_owned())
            }
            false => Browsed::Failed(failed),
        }
    }
}

// Shuts the browser's right to the dev server's port when dropped, before the server stops.
struct Shut(DevPort);

impl Drop for Shut {
    fn drop(&mut self) {
        self.0.shut();
    }
}

// Why `visit` of `route` fails, naming the route and a status and nothing of any URL.
fn judge(route: &str, port: u16, visit: &Visit) -> Vec<String> {
    if visit.loaded.is_err() {
        return vec![format!("route `{route}` did not load")];
    }
    let statuses: BTreeSet<u16> = (visit.requests.iter())
        .filter(|request| from_dev_server(&request.url, port))
        .filter_map(|request| request.status.filter(|status| (500..600).contains(status)))
        .collect();
    let mut why: Vec<String> = (statuses.iter())
        .map(|status| format!("route `{route}` had a request answer {status}"))
        .collect();
    // Chromium logs every resource that fails to load as a console error. One from another
    // origin is that origin's failure, which this proof does not judge.
    let errors = (visit.console_errors.iter())
        .filter(|error| !foreign_load_failure(error, port))
        .count();
    match errors {
        0 => {}
        1 => why.push(format!("route `{route}` logged a console error")),
        n => why.push(format!("route `{route}` logged {n} console errors")),
    }
    why
}

// Whether `error` is Chromium's log of a resource at another origin that failed to load.
fn foreign_load_failure(error: &ConsoleError, port: u16) -> bool {
    error.text.starts_with("Failed to load resource")
        && (error.url.as_deref()).is_some_and(|url| !from_dev_server(url, port))
}

// Whether `url` is on the dev server's origin: either loopback name, on its port. A URL
// the browser reports with no port is on its scheme's default one.
fn from_dev_server(url: &str, port: u16) -> bool {
    let (scheme, rest) = url.split_once("://").unwrap_or(("http", url));
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    let default = match scheme.to_ascii_lowercase().as_str() {
        "https" | "wss" => 443,
        _ => 80,
    };
    ["localhost", "127.0.0.1", "[::1]"].iter().any(|host| {
        let named = format!("{host}:{port}");
        authority.eq_ignore_ascii_case(&named)
            || (port == default && authority.eq_ignore_ascii_case(host))
    })
}

#[cfg(test)]
mod tests;
