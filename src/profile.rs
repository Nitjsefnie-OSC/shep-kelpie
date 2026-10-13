//! The worker's profile: the sandbox and instructions kelpie starts it with
//!
//! The worker runs with no one to ask, so its sandbox is its whole fence.
//! Writes go to its worktree, its build folder and what a commit and a push
//! need, and never to agents' own files in the worktree. Credential
//! paths are unreadable, hosts are GitHub's and the project's, and what only
//! the project manager does is a command it may not run. Each harness's
//! adapter enforces the fence its own way.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::fence;
use crate::github::InstallationToken;
use crate::hooks::Hooks;
use crate::lease::door;
use crate::ports::{Fence, Guard, Reach};
use crate::settings::{BuildDir, EnvName, GuardHook, NonBlank};
use crate::worktree::BASE;

mod housekeeping;
mod login;

pub use login::write as write_login;

/// Kelpie's instructions to every worker, appended to its system prompt
pub const INSTRUCTIONS: &str = include_str!("worker-instructions.md");

// How a worker is told to get its pull request opened: by itself, as the
// maintainer's login, or by writing its text where kelpie opens it from.
const OPEN_YOURSELF: &str = "When the work is done, push the branch with `git push origin HEAD`. Unless the branch already has a pull request, open a draft one with `gh pr create --draft`, giving it your own body and a `--title` that is a conventional commit subject. End the body with the line `Resolves #<issue>`, naming your work item's issue. When a later turn changes the branch, change the open pull request's body with `gh pr edit --body`.";

const HAND_OVER: &str = "When the work is done, push the branch with `git push origin HEAD`. Do not open or edit the pull request yourself: your credential cannot, and kelpie does it. Write its title and body in `{file}`. The first line is the title, a conventional commit subject. The rest is the body, which ends with the line `Resolves #<issue>`, naming your work item's issue. Write the file before you end your turn, and write it again to change the title or body; kelpie opens the draft when a turn ends with the branch pushed, and sets the title and body to what the file says.";

/// Kelpie's own worker instructions, naming `kelpie`, the binary a worker
/// runs its tests under the machine's test lease with, and `deferred_file`,
/// where it leaves the findings it defers
///
/// With `pull_request_file`, the worker holds a credential that opens no
/// pull request, and is told to write its text there for kelpie to open.
pub fn instructions(
    kelpie: &Path,
    pull_request_file: Option<&Path>,
    deferred_file: &Path,
) -> String {
    let pull_request = match pull_request_file {
        Some(file) => HAND_OVER.replace("{file}", &file.display().to_string()),
        None => OPEN_YOURSELF.to_owned(),
    };
    INSTRUCTIONS
        .replace("{pull_request}", &pull_request)
        .replace("{kelpie}", &kelpie.display().to_string())
        .replace("{deferred}", &deferred_file.display().to_string())
}

// Claude Code opens a worktree's whole common git dir to sandboxed writes.
// These are the parts a commit and a push do not need, and whose change
// would run code outside the sandbox. `refs/remotes` stays open for the
// push's tracking ref; kelpie fetches `origin/main` before cutting from it.
const GIT_DENY: [&str; 8] = [
    "config",
    "hooks",
    "info",
    "modules",
    "HEAD",
    "index",
    "packed-refs",
    "refs/tags",
];

// Read and written by no worker. `~/.config/gh` stays readable for a worker
// that runs as the maintainer: `gh` will not start without its config, and
// the worker opens its own pull request. A worker holding the App's token has
// a config folder of its own.
pub(crate) const CREDENTIALS: [&str; 12] = [
    "~/.ssh/**",
    "~/.aws/**",
    "~/.gnupg/**",
    "~/.docker/**",
    "~/.netrc",
    "~/.git-credentials",
    "~/.npmrc",
    "~/.cargo/credentials",
    "~/.cargo/credentials.toml",
    // Kelpie's old home. The maintainer's disk still holds a project's state
    // and settings file, the webhook URL's old file and the authenticator
    // secret there, so these rules still protect real files. Kelpie's home
    // and the shepherd's are the profile's own.
    "~/.kelpie/projects/**",
    "~/.kelpie/settings.toml",
    "~/.kelpie/totp/**",
];

// What only the project manager does: merge, mark ready, and summon. `kelpie
// guard` refuses these in any shape; these match a command's prefix only.
const PM_ONLY: [&str; 5] = [
    "gh pr merge",
    "gh pr merge *",
    "gh pr ready",
    "gh pr ready *",
    "gh *review please*",
];

// `gh api` could merge or relabel around the rules above, and `gh auth token`
// prints the maintainer's token. No worker needs either.
const GH_DENY: [&str; 4] = ["gh api", "gh api *", "gh auth", "gh auth *"];

// Pushes that rewrite or delete branches on the remote whatever they name:
// `--mirror` and `--all` push every local branch, and a `+` refspec forces.
// A rule's `*` needs something to match, so a flag straight after `push`
// takes a rule of its own.
const PUSH_FLAGS: [&str; 9] = [
    "git push *--mirror*",
    "git push *--all*",
    "git push *--delete*",
    "git push -d*",
    "git push * -d*",
    "git push *--force*",
    "git push -f*",
    "git push * -f*",
    "git push * +*",
];

// What a worker holding the App's token may not read: `gh`'s own config
// folder, which can hold the maintainer's token, and the keychain files. `gh`
// runs on its own config folder then, so it needs neither.
const MAINTAINERS_LOGIN: [&str; 2] = ["~/.config/gh/**", "~/Library/Keychains/**"];

// `security` reads the keychain `gh` and git's osxkeychain helper keep the
// maintainer's token in, and so does the helper itself, run by hand or with
// `git credential`. Nothing a worker holding the App's token needs any of
// them, git asks its helpers itself. These are a guardrail against a mistake,
// not the boundary: the sandbox closes the keychain itself (#429).
const KEYCHAIN_COMMANDS: [&str; 6] = [
    "security",
    "security *",
    "git credential",
    "git credential *",
    "git credential-*",
    "git -c credential*",
];

// What `git push` and `gh` reach. A project adds its own, such as a registry.
const GITHUB: [&str; 2] = ["github.com", "api.github.com"];

// Where GitHub redirects a download of a run's logs, a job's logs or an
// artifact to. Listed by name, since `*.blob.core.windows.net` would reach any
// Azure storage account. The storage hosts are the range GitHub's docs list.
const GITHUB_DOWNLOADS: [&str; 21] = [
    "results-receiver.actions.githubusercontent.com",
    "productionresultssa0.blob.core.windows.net",
    "productionresultssa1.blob.core.windows.net",
    "productionresultssa2.blob.core.windows.net",
    "productionresultssa3.blob.core.windows.net",
    "productionresultssa4.blob.core.windows.net",
    "productionresultssa5.blob.core.windows.net",
    "productionresultssa6.blob.core.windows.net",
    "productionresultssa7.blob.core.windows.net",
    "productionresultssa8.blob.core.windows.net",
    "productionresultssa9.blob.core.windows.net",
    "productionresultssa10.blob.core.windows.net",
    "productionresultssa11.blob.core.windows.net",
    "productionresultssa12.blob.core.windows.net",
    "productionresultssa13.blob.core.windows.net",
    "productionresultssa14.blob.core.windows.net",
    "productionresultssa15.blob.core.windows.net",
    "productionresultssa16.blob.core.windows.net",
    "productionresultssa17.blob.core.windows.net",
    "productionresultssa18.blob.core.windows.net",
    "productionresultssa19.blob.core.windows.net",
];

/// Where one worker may write, and what it runs under
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkerProfile<'a> {
    /// The work item's worktree
    pub worktree: &'a Path,
    /// The worker's build folder, its `CARGO_TARGET_DIR`
    pub build: &'a Path,
    /// The project repo's common git dir, which holds objects and refs
    pub git_common_dir: &'a Path,
    /// The worktree's own git dir, inside the common one
    pub git_dir: &'a Path,
    /// The work item's branch
    pub branch: &'a str,
    /// The kelpie binary, which runs kelpie's own checks
    pub kelpie: &'a Path,
    /// Kelpie's home, whose path the guard keeps off the forge
    pub kelpie_home: &'a Path,
    /// The project's checkout, whose path the guard keeps off the forge
    pub repo: &'a Path,
    /// The project's own guard hooks, which run after kelpie's
    pub guard_hooks: &'a [GuardHook],
    /// The domains the project's settings add to GitHub's
    pub allowed_domains: &'a [NonBlank],
    /// Variables the project's settings point into the build folder
    pub build_env: &'a BTreeMap<EnvName, BuildDir>,
    /// The shepherd's home, whose `dogs.toml` holds the webhook's URL. The
    /// worker reads none of it but its own folders in kelpie's.
    pub shep_home: &'a Path,
    /// The dog's door, the one Unix socket the worker may connect to
    pub door: &'a Path,
    /// The token of kelpie's App for the project's repo, on a repo the App
    /// covers: the worker's only credential for GitHub, and the
    /// maintainer's login is then out of its reach. Without one the worker
    /// runs as the maintainer's login.
    pub token: Option<&'a InstallationToken>,
}

impl WorkerProfile<'_> {
    /// The worker's sandbox
    pub fn reach(&self) -> Reach {
        let git = |p: &str| self.git_common_dir.join(p);
        let branch_ref = git("refs/heads").join(self.branch);
        let tracking_ref = git("refs/remotes/origin").join(self.branch);
        let write = vec![
            self.worktree.to_owned(),
            self.build.to_owned(),
            git("objects"),
            self.git_dir.to_owned(),
            git("logs/refs/heads").join(self.branch),
            with_suffix(&branch_ref, ".lock"),
            branch_ref,
            git("logs/refs/remotes/origin").join(self.branch),
            with_suffix(&tracking_ref, ".lock"),
            tracking_ref,
            // A commit locks `packed-refs` before it finds nothing to delete from
            // it. The lock is all it needs then, and the file stays denied
            // below, so a write that needs the file still fails. One a killed
            // git leaves is removed by `worktree::prepare`.
            git("packed-refs.lock"),
        ];
        // The clone's own base branch, which a push of every branch would carry.
        let no_write: Vec<PathBuf> = GIT_DENY
            .iter()
            .map(|p| git(p))
            .chain([git("refs/heads").join(BASE)])
            .chain(fence::deny_write(self.worktree))
            .collect();
        // Kelpie's home is the shepherd's too unless `KELPIE_HOME` puts it elsewhere.
        let homes = [self.shep_home, self.kelpie_home]
            .into_iter()
            .enumerate()
            .filter(|&(i, home)| i == 0 || !home.starts_with(self.shep_home))
            .map(|(_, home)| format!("{}/**", home.display()));
        let no_read = CREDENTIALS
            .iter()
            .chain(self.token.iter().flat_map(|_| &MAINTAINERS_LOGIN))
            .map(|&p| p.to_owned())
            .chain(homes)
            .collect();
        let read = [self.worktree, self.build, self.kelpie]
            .into_iter()
            .map(Path::to_owned)
            .collect();
        let no_commands = PM_ONLY
            .iter()
            .chain(&GH_DENY)
            .chain(self.token.iter().flat_map(|_| &KEYCHAIN_COMMANDS))
            .map(|&c| c.to_owned())
            .chain(push_to_base())
            .chain(PUSH_FLAGS.iter().map(|&c| c.to_owned()))
            .collect();
        let hosts = GITHUB
            .into_iter()
            .chain(GITHUB_DOWNLOADS)
            .chain(self.allowed_domains.iter().map(NonBlank::as_str))
            .map(str::to_owned)
            .collect();
        let fence = Fence {
            write,
            no_write,
            no_read,
            read,
            hosts,
            no_commands,
            env: self.env(),
            sockets: vec![self.door.to_owned()],
            guard: Guard {
                kelpie: self.kelpie.to_owned(),
                worktree: self.worktree.to_owned(),
                build: self.build.to_owned(),
                git_common_dir: self.git_common_dir.to_owned(),
                folders: vec![self.kelpie_home.to_owned(), self.repo.to_owned()],
                issues: None,
                hooks: Hooks::default(),
            },
            hooks: self.guard_hooks.to_vec(),
        };
        Reach {
            read: Vec::new(),
            fence: Some(Box::new(fence)),
            block_keychain: false,
            code_graph: None,
        }
    }

    fn env(&self) -> BTreeMap<String, String> {
        let path = |p: &Path| p.to_string_lossy().into_owned();
        let cargo = ("CARGO_TARGET_DIR".to_owned(), path(self.build));
        let door = (door::SOCKET_VAR.to_owned(), path(self.door));
        let project = self.build_env.iter().map(|(name, dir)| {
            (
                name.as_str().to_owned(),
                path(&self.build.join(dir.as_path())),
            )
        });
        let github = self.token.into_iter().flat_map(|_| login::env(self.build));
        // One variable, so a covered worker's value carries both lists.
        let git_config = [housekeeping::PARAMETERS]
            .into_iter()
            .chain(self.token.map(|_| login::GIT_CONFIG))
            .collect::<Vec<_>>()
            .join(" ");
        [
            cargo,
            door,
            ("GIT_CONFIG_PARAMETERS".to_owned(), git_config),
        ]
        .into_iter()
        .chain(project)
        .chain(github)
        .collect()
    }
}

// Pushes that name the branch kelpie cuts from, as `origin main`,
// `HEAD:main` or `refs/heads/main`. A repo without branch protection
// would otherwise take them. The guard reads every spelling; these are its backup.
fn push_to_base() -> impl Iterator<Item = String> {
    [" {b}", " {b} *", ":{b}*", "/{b}*"]
        .into_iter()
        .map(|p| format!("git push *{}", p.replace("{b}", BASE)))
}

fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut s = path.as_os_str().to_owned();
    s.push(suffix);
    s.into()
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::*;
    use crate::ports::Tools;
    use crate::settings::{HookEvent, NonBlank};

    impl WorkerProfile<'_> {
        // The profile as Claude Code's settings file carries it, with the
        // sandbox kelpie runs the call in under `sandbox`, as srt reads it
        fn settings(&self) -> Value {
            let reach = self.reach();
            let mut s = crate::adapters::claude_settings(Tools::Work, &reach);
            assert_eq!(s["sandbox"], json!({ "enabled": false }));
            let fence = reach.fence.as_deref().expect("a worker is fenced");
            s["sandbox"] = crate::adapters::srt_settings(&crate::adapters::fence_policy(fence));
            s
        }
    }

    fn guard(event: HookEvent, matcher: Option<&str>, command: &str) -> GuardHook {
        GuardHook {
            event,
            matcher: matcher.map(|m| NonBlank::try_from(m.to_owned()).unwrap()),
            command: NonBlank::try_from(command.to_owned()).unwrap(),
        }
    }

    fn settings(hooks: &[GuardHook]) -> Value {
        with_domains(hooks, &[])
    }

    fn with_domains(hooks: &[GuardHook], domains: &[NonBlank]) -> Value {
        profile(hooks, domains).settings()
    }

    static NO_BUILD_ENV: BTreeMap<EnvName, BuildDir> = BTreeMap::new();

    fn profile<'a>(hooks: &'a [GuardHook], domains: &'a [NonBlank]) -> WorkerProfile<'a> {
        WorkerProfile {
            worktree: Path::new("/k/wt/shep/7"),
            build: Path::new("/k/targets/shep/7"),
            git_common_dir: Path::new("/k/repos/shep/.git"),
            git_dir: Path::new("/k/repos/shep/.git/worktrees/7"),
            branch: "kelpie/7",
            kelpie: Path::new("/opt/kelpie's bin/kelpie"),
            kelpie_home: Path::new("/k"),
            repo: Path::new("/k/repos/shep"),
            guard_hooks: hooks,
            allowed_domains: domains,
            build_env: &NO_BUILD_ENV,
            shep_home: Path::new("/srv/shep"),
            door: Path::new("/k/dog/lease.sock"),
            token: None,
        }
    }

    fn covered_fence(token: &InstallationToken) -> Fence {
        let profile = WorkerProfile {
            token: Some(token),
            ..profile(&[], &[])
        };
        *profile.reach().fence.expect("a worker is fenced")
    }

    // On a repo kelpie's App covers, the worker's credential is a token for
    // that repo alone, and the maintainer's login, `gh`'s config folder and
    // the keychain are out of its reach. The guard parses commands and can
    // miss one, so this is the boundary.
    #[test]
    fn on_a_repo_the_app_covers_a_worker_holds_the_apps_token_and_never_the_maintainers_login() {
        let fence = covered_fence(&InstallationToken::new("ghs_scoped".to_owned()));

        assert_eq!(
            fence.env,
            BTreeMap::from([
                ("CARGO_TARGET_DIR".to_owned(), "/k/targets/shep/7".to_owned()),
                ("KELPIE_LEASE_SOCKET".to_owned(), "/k/dog/lease.sock".to_owned()),
                ("GH_CONFIG_DIR".to_owned(), "/k/targets/shep/7/gh".to_owned()),
                ("GH_PROMPT_DISABLED".to_owned(), "1".to_owned()),
                ("GIT_TERMINAL_PROMPT".to_owned(), "0".to_owned()),
                (
                    // Git's helpers are emptied, which drops the keychain's, and
                    // github.com's is gh's, which answers from the config folder.
                    // The sandbox's own proxy setting, which this replaces, stays.
                    "GIT_CONFIG_PARAMETERS".to_owned(),
                    "'http.proxyAuthMethod'='basic' 'gc.auto'='0' 'maintenance.auto'='false' 'fetch.prune'='false' 'credential.helper'='' 'credential.https://github.com.helper'='!gh auth git-credential'"
                        .to_owned()
                ),
            ])
        );
        // Codex drops a variable named for a key, a secret or a token.
        let names = fence.env.keys().map(|n| n.to_ascii_uppercase());
        for name in names {
            assert!(
                !["KEY", "SECRET", "TOKEN"].iter().any(|w| name.contains(w)),
                "{name}"
            );
        }
        assert!(
            !format!("{fence:?}").contains("ghs_scoped"),
            "the token is in no variable"
        );
        for path in ["~/.config/gh/**", "~/Library/Keychains/**"] {
            assert!(fence.no_read.contains(&path.to_owned()), "{path}");
        }
        for command in [
            "security",
            "security *",
            "git credential *",
            "git credential-*",
            "git -c credential*",
        ] {
            assert!(fence.no_commands.contains(&command.to_owned()), "{command}");
        }
    }

    #[test]
    fn with_no_app_a_worker_runs_as_the_maintainer_and_its_git_still_skips_housekeeping() {
        let fence = profile(&[], &[]).reach().fence.expect("a worker is fenced");
        let names: Vec<&str> = fence.env.keys().map(String::as_str).collect();
        assert_eq!(
            names,
            [
                "CARGO_TARGET_DIR",
                "GIT_CONFIG_PARAMETERS",
                "KELPIE_LEASE_SOCKET"
            ]
        );
        // The sandbox's own proxy setting, which this replaces, stays.
        assert_eq!(
            fence.env["GIT_CONFIG_PARAMETERS"],
            "'http.proxyAuthMethod'='basic' 'gc.auto'='0' 'maintenance.auto'='false' 'fetch.prune'='false'"
        );
        assert!(!fence.no_read.iter().any(|p| p.contains(".config/gh")));
        assert!(!fence.no_commands.contains(&"security".to_owned()));
    }

    // A `KELPIE_HOME` set outside the shepherd's home holds the TOTP secret
    // and other projects' state just the same.
    #[test]
    fn a_kelpie_home_outside_the_shepherds_is_denied_around_the_workers_own() {
        let dir = tempfile::tempdir().unwrap();
        let (kelpie, shep) = (dir.path().join("k"), dir.path().join("shep"));
        let (worktree, build) = (
            kelpie.join("koji/worktrees/7"),
            kelpie.join("koji/builds/7"),
        );
        for folder in [
            &worktree,
            &build,
            &kelpie.join("totp"),
            &kelpie.join("rotom"),
            &shep,
        ] {
            std::fs::create_dir_all(folder).unwrap();
        }
        let s = WorkerProfile {
            worktree: &worktree,
            build: &build,
            git_common_dir: Path::new("/r/.git"),
            git_dir: Path::new("/r/.git/worktrees/7"),
            branch: "kelpie/7",
            kelpie: Path::new("/opt/kelpie"),
            kelpie_home: &kelpie,
            repo: Path::new("/r"),
            guard_hooks: &[],
            allowed_domains: &[],
            build_env: &BTreeMap::new(),
            shep_home: &shep,
            door: Path::new("/s/kelpie/dog/lease.sock"),
            token: None,
        }
        .settings();

        let files = &s["sandbox"]["filesystem"];
        let denied = strings(&files["denyRead"]);
        for home in [&kelpie, &shep] {
            assert!(
                denied.contains(&format!("{}/**", home.display()).as_str()),
                "{denied:?}"
            );
        }
        assert!(strings(&files["allowRead"]).contains(&worktree.to_str().unwrap()));
        let deny = strings(&s["permissions"]["deny"]);
        for rule in ["totp/**", "rotom/**"] {
            let rule = format!("Read(/{}/{rule})", kelpie.display());
            assert!(deny.contains(&rule.as_str()), "{rule} not in {deny:?}");
        }
        let kelpie_rule = format!("Read(/{}/**)", kelpie.display());
        assert!(
            !deny.contains(&kelpie_rule.as_str()),
            "it would hide the worktree"
        );
    }

    fn strings(v: &Value) -> Vec<&str> {
        v.as_array()
            .unwrap()
            .iter()
            .map(|s| s.as_str().unwrap())
            .collect()
    }

    // The whole process runs in kelpie's sandbox, which has no exceptions to
    // ask for: no command runs outside it, and a missing one runs nothing.
    #[test]
    fn the_sandbox_has_no_way_out() {
        let s = settings(&[]);
        for key in [
            "excludedCommands",
            "allowUnsandboxedCommands",
            "allowAllUnixSockets",
        ] {
            assert_eq!(s["sandbox"][key], Value::Null, "{key}");
            assert_eq!(s["sandbox"]["network"][key], Value::Null, "{key}");
        }
    }

    #[test]
    fn gh_can_verify_tls_inside_the_sandbox() {
        assert_eq!(
            settings(&[])["sandbox"]["enableWeakerNetworkIsolation"],
            true
        );
    }

    #[test]
    fn writes_go_only_to_the_worktree_the_build_folder_and_what_a_commit_and_push_need() {
        let s = settings(&[]);
        assert_eq!(
            strings(&s["sandbox"]["filesystem"]["allowWrite"]),
            [
                "/k/wt/shep/7",
                "/k/targets/shep/7",
                "/k/repos/shep/.git/objects",
                "/k/repos/shep/.git/worktrees/7",
                "/k/repos/shep/.git/logs/refs/heads/kelpie/7",
                "/k/repos/shep/.git/refs/heads/kelpie/7.lock",
                "/k/repos/shep/.git/refs/heads/kelpie/7",
                "/k/repos/shep/.git/logs/refs/remotes/origin/kelpie/7",
                "/k/repos/shep/.git/refs/remotes/origin/kelpie/7.lock",
                "/k/repos/shep/.git/refs/remotes/origin/kelpie/7",
                "/k/repos/shep/.git/packed-refs.lock",
            ]
        );
        assert_eq!(
            strings(&s["sandbox"]["filesystem"]["denyWrite"]),
            [
                "/k/repos/shep/.git/config",
                "/k/repos/shep/.git/hooks",
                "/k/repos/shep/.git/info",
                "/k/repos/shep/.git/modules",
                "/k/repos/shep/.git/HEAD",
                "/k/repos/shep/.git/index",
                "/k/repos/shep/.git/packed-refs",
                "/k/repos/shep/.git/refs/tags",
                "/k/repos/shep/.git/refs/heads/main",
                "/k/wt/shep/7/.claude",
                "/k/wt/shep/7/**/.claude",
                "/k/wt/shep/7/.mcp.json",
                "/k/wt/shep/7/.codex",
                "/k/wt/shep/7/**/.codex",
                "/k/wt/shep/7/.agents",
                "/k/wt/shep/7/**/.agents",
                "/k/wt/shep/7/.pi",
                "/k/wt/shep/7/**/.pi",
            ]
        );
        assert_eq!(s["env"]["CARGO_TARGET_DIR"], "/k/targets/shep/7");
    }

    #[test]
    fn build_env_points_into_the_build_folder_beside_cargo() {
        let build_env = BTreeMap::from([(
            EnvName::try_from("BUN_INSTALL_CACHE_DIR".to_owned()).unwrap(),
            BuildDir::try_from("bun".to_owned()).unwrap(),
        )]);
        let s = WorkerProfile {
            worktree: Path::new("/k/wt/lab/7"),
            build: Path::new("/k/targets/lab/7"),
            git_common_dir: Path::new("/k/repos/lab/.git"),
            git_dir: Path::new("/k/repos/lab/.git/worktrees/7"),
            branch: "kelpie/7",
            kelpie: Path::new("/opt/kelpie"),
            kelpie_home: Path::new("/k"),
            repo: Path::new("/k/repos/shep"),
            guard_hooks: &[],
            allowed_domains: &[],
            build_env: &build_env,
            shep_home: Path::new("/srv/shep"),
            door: Path::new("/k/dog/lease.sock"),
            token: None,
        }
        .settings();
        assert_eq!(
            s["env"],
            json!({
                "CARGO_TARGET_DIR": "/k/targets/lab/7",
                "BUN_INSTALL_CACHE_DIR": "/k/targets/lab/7/bun",
                "GIT_CONFIG_PARAMETERS": housekeeping::PARAMETERS,
                "KELPIE_LEASE_SOCKET": "/k/dog/lease.sock",
            })
        );
    }

    #[test]
    fn the_network_is_github_and_the_projects_own_domains_only() {
        let plain = settings(&[]);
        let hosts = strings(&plain["sandbox"]["network"]["allowedDomains"]);
        assert_eq!(hosts[..2], ["github.com", "api.github.com"]);
        let npm = [NonBlank::try_from("registry.npmjs.org".to_owned()).unwrap()];
        let extended = with_domains(&[], &npm);
        let with_npm = strings(&extended["sandbox"]["network"]["allowedDomains"]);
        assert_eq!(with_npm[..hosts.len()], hosts[..]);
        assert_eq!(with_npm[hosts.len()..], ["registry.npmjs.org"]);
    }

    #[test]
    fn a_fresh_profile_allows_the_hosts_a_log_download_redirects_to() {
        let settings = settings(&[]);
        let hosts = strings(&settings["sandbox"]["network"]["allowedDomains"]);
        let storage = (0..20).map(|n| format!("productionresultssa{n}.blob.core.windows.net"));
        for host in ["results-receiver.actions.githubusercontent.com".to_owned()]
            .into_iter()
            .chain(storage)
        {
            assert!(
                hosts.contains(&host.as_str()),
                "{host} missing from {hosts:?}"
            );
        }
    }

    #[test]
    fn a_fresh_profile_allows_no_other_blob_host() {
        let settings = settings(&[]);
        let hosts = strings(&settings["sandbox"]["network"]["allowedDomains"]);
        let blobs: Vec<_> = hosts.iter().filter(|h| h.contains("windows.net")).collect();
        assert_eq!(blobs.len(), 20, "{blobs:?}");
        assert!(!hosts.iter().any(|h| h.contains('*')), "{hosts:?}");
        assert!(!hosts.contains(&"evil.blob.core.windows.net"));
        assert!(!hosts.contains(&"productionresultssa20.blob.core.windows.net"));
    }

    #[test]
    fn a_host_outside_the_list_is_refused_not_asked_about() {
        assert_eq!(settings(&[])["sandbox"]["network"]["strictAllowlist"], true);
    }

    #[test]
    fn a_worker_binds_no_port() {
        let s = settings(&[]);
        let network = s["sandbox"]["network"].as_object().unwrap();
        assert!(!network.contains_key("allowLocalBinding"), "{network:?}");
        assert!(!network.contains_key("allowMachLookup"), "{network:?}");
        assert_eq!(s["env"]["NODE_USE_ENV_PROXY"], Value::Null);
    }

    #[test]
    fn file_tools_are_held_to_the_same_folders_by_kelpie() {
        let s = settings(&[]);
        assert_eq!(
            s["hooks"]["PreToolUse"][0],
            json!({
                "matcher": "Edit|Write|MultiEdit|NotebookEdit",
                "hooks": [{
                    "type": "command",
                    "command": r"'/opt/kelpie'\''s bin/kelpie' 'confine' '/k/wt/shep/7' '/k/targets/shep/7'",
                }],
            })
        );
    }

    #[test]
    fn what_only_the_project_manager_does_is_denied() {
        let deny = settings(&[])["permissions"]["deny"].clone();
        let deny = strings(&deny);
        for rule in [
            "Bash(gh pr merge)",
            "Bash(gh pr merge *)",
            "Bash(gh pr ready)",
            "Bash(gh pr ready *)",
            "Bash(gh *review please*)",
        ] {
            assert!(deny.contains(&rule), "{rule}");
        }
    }

    #[test]
    fn a_projects_settings_cannot_switch_the_hooks_off() {
        assert_eq!(settings(&[])["disableAllHooks"], false);
    }

    #[test]
    fn tools_that_reach_past_the_fence_are_denied() {
        let deny = settings(&[])["permissions"]["deny"].clone();
        for tool in [
            "Monitor",
            "RemoteTrigger",
            "Workflow",
            "EnterWorktree",
            "ExitWorktree",
        ] {
            assert!(strings(&deny).contains(&tool), "{tool}: {deny}");
        }
    }

    #[test]
    fn a_worker_drops_the_features_it_never_uses() {
        let s = settings(&[]);
        for key in [
            "disableBundledSkills",
            "disableWorkflows",
            "disableClaudeAiConnectors",
            "disableArtifact",
            "disableRemoteControl",
        ] {
            assert_eq!(s[key], true, "{key}");
        }
        let deny = strings(&s["permissions"]["deny"]).join(" ");
        for tool in [
            "EnterPlanMode",
            "CronCreate",
            "DesignSync",
            "ScheduleWakeup",
        ] {
            assert!(deny.contains(tool), "{tool}");
        }
    }

    #[test]
    fn gh_api_and_gh_auth_are_denied() {
        let deny = settings(&[])["permissions"]["deny"].clone();
        let deny = strings(&deny);
        for rule in [
            "Bash(gh api)",
            "Bash(gh api *)",
            "Bash(gh auth)",
            "Bash(gh auth *)",
        ] {
            assert!(deny.contains(&rule), "{rule}");
        }
    }

    #[test]
    fn a_push_to_the_base_branch_or_with_a_forcing_flag_is_denied() {
        let deny = settings(&[])["permissions"]["deny"].clone();
        assert_eq!(
            strings(&deny)
                .into_iter()
                .filter(|r| r.starts_with("Bash(git push"))
                .collect::<Vec<_>>(),
            [
                "Bash(git push * main)",
                "Bash(git push * main *)",
                "Bash(git push *:main*)",
                "Bash(git push */main*)",
                "Bash(git push *--mirror*)",
                "Bash(git push *--all*)",
                "Bash(git push *--delete*)",
                "Bash(git push -d*)",
                "Bash(git push * -d*)",
                "Bash(git push *--force*)",
                "Bash(git push -f*)",
                "Bash(git push * -f*)",
                "Bash(git push * +*)",
            ]
        );
    }

    #[test]
    fn the_clones_base_branch_is_not_writable() {
        let s = settings(&[]);
        let deny = strings(&s["sandbox"]["filesystem"]["denyWrite"]);
        assert!(
            deny.contains(&"/k/repos/shep/.git/refs/heads/main"),
            "{deny:?}"
        );
        let allow = strings(&s["sandbox"]["filesystem"]["allowWrite"]);
        assert!(!allow.iter().any(|p| p.ends_with("/main")), "{allow:?}");
    }

    // Every other Unix socket stays blocked, kelpie's shepherd socket
    // included, so a worker cannot `shep trigger` its own ruling's answer.
    #[test]
    fn a_worker_reaches_the_dogs_door_and_no_other_unix_socket() {
        let s = settings(&[]);
        let network = s["sandbox"]["network"].as_object().unwrap();
        assert_eq!(network["allowUnixSockets"], json!(["/k/dog/lease.sock"]));
        assert_eq!(
            s["env"]["KELPIE_LEASE_SOCKET"], network["allowUnixSockets"][0],
            "the worker asks at the one socket its sandbox lets it reach"
        );
        assert!(!network.contains_key("allowAllUnixSockets"), "{network:?}");
    }

    #[test]
    fn credential_paths_are_unreadable() {
        let deny = settings(&[])["permissions"]["deny"].clone();
        let deny = strings(&deny);
        for rule in [
            "Read(~/.ssh/**)",
            "Read(~/.kelpie/projects/**)",
            "Read(~/.kelpie/settings.toml)",
            "Read(~/.kelpie/totp/**)",
            "Read(//srv/shep/**)",
        ] {
            assert!(deny.contains(&rule), "{rule}");
        }
        assert!(
            !deny.iter().any(|r| r.contains(".config/gh")),
            "gh cannot start without its config"
        );
        assert!(
            !deny.contains(&"Read(~/.kelpie/**)"),
            "a worker's worktree and build folder are under ~/.kelpie"
        );
    }

    #[test]
    fn every_worker_has_kelpies_guard_on_bash_and_agents_with_no_project_hooks() {
        let s = settings(&[]);
        assert_eq!(
            s["hooks"]["PreToolUse"][1],
            json!({
                "matcher": "Bash|Agent|Task",
                "hooks": [{
                    "type": "command",
                    "command": r"'/opt/kelpie'\''s bin/kelpie' 'guard' '/k/repos/shep/.git' '/k/wt/shep/7' '--folder=/k' '--folder=/k/repos/shep'",
                }],
            })
        );
        assert_eq!(s["hooks"]["PreToolUse"].as_array().unwrap().len(), 2);
        assert_eq!(s["hooks"]["PostToolUse"], Value::Null);
    }

    #[test]
    fn the_projects_guard_hooks_are_carried_after_kelpies_own() {
        let s = settings(&[
            guard(HookEvent::PreToolUse, Some("Bash"), "~/bin/before"),
            guard(HookEvent::PostToolUse, None, "~/bin/after"),
        ]);
        assert_eq!(
            s["hooks"]["PreToolUse"][2],
            json!({
                "matcher": "Bash",
                "hooks": [{ "type": "command", "command": "~/bin/before" }],
            })
        );
        assert_eq!(
            s["hooks"]["PostToolUse"],
            json!([{ "hooks": [{ "type": "command", "command": "~/bin/after" }] }])
        );
    }

    // A commit locks `packed-refs` on its own, the fence denies the lock, and a
    // worker that took the message for a failure spent turns checking `git log`.
    #[test]
    fn the_instructions_say_the_packed_refs_lock_message_is_no_failure() {
        assert!(INSTRUCTIONS.contains("packed-refs.lock"), "{INSTRUCTIONS}");
    }

    // The first-turn skill names no docs, so a worker read the domain docs in
    // about a third of its sessions.
    #[test]
    fn the_instructions_send_a_worker_to_the_domain_docs_before_code() {
        assert!(
            INSTRUCTIONS.contains("`docs/agents/domain.md`"),
            "{INSTRUCTIONS}"
        );
    }

    #[test]
    fn the_worker_instructions_name_the_deferred_findings_files_path() {
        let deferred = Path::new("/kelpie/shep/build-7/deferred-findings.md");
        let text = instructions(Path::new("/opt/kelpie"), None, deferred);
        assert!(
            text.contains("`/kelpie/shep/build-7/deferred-findings.md`"),
            "{text}"
        );
        assert!(!text.contains("{deferred}"), "{text}");
    }

    #[test]
    fn the_instructions_never_mention_money_or_limits() {
        let text = INSTRUCTIONS.to_lowercase();
        for word in ["budget", "cost", "spend", "token", "usage", "$", "limit"] {
            assert!(!text.contains(word), "the instructions mention {word:?}");
        }
    }
}
