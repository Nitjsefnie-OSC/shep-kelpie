//! A stand-in GitHub for kelpie's App, and a signer that needs no key

use std::collections::{BTreeMap, VecDeque};
use std::path::Path;
use std::sync::{Arc, Mutex};

use super::FakeClock;
use crate::github::tokens::Jwt;
use crate::github::{
    ApiError, AppTokens, Apps, Call, Conversion, GithubApi, Grant, InstallationToken, IssuedToken,
    Keychain, Pem, Signer,
};
use crate::ports::{Clock, Timestamp};
use crate::settings::ForgeSlug;

/// A key no test would print by chance, so finding it anywhere else is a leak
pub(crate) const PEM: &str =
    "-----BEGIN RSA PRIVATE KEY-----\nkelpie-test-app-key-s3cr3t\n-----END RSA PRIVATE KEY-----\n";

/// What the stand-in was asked, in order
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Asked {
    Convert(String),
    Installation(String),
    /// An installation's token, and the repo it was asked to reach
    AccessToken(u64, String),
    /// A write, and the token it was made with
    Write(String, Call),
}

#[derive(Debug, Default)]
struct Hub {
    installed: BTreeMap<String, u64>,
    // What the next calls answer in place of their success, in turn
    failures: VecDeque<ApiError>,
    // The owner the next conversion hands back in place of the stand-in's
    convert_owner: Option<String>,
    asked: Vec<Asked>,
    jwts: Vec<String>,
    minted: u32,
    issues: u64,
    comment_ids: u64,
    // Why every comment is refused, while it is
    comments_refused: Option<ApiError>,
    // Why every worker's token is refused, while it is
    workers_refused: Option<ApiError>,
    // A permission GitHub grants a worker's token beyond the one asked for
    extra_grant: Option<(String, String)>,
    // What the keychain says about the maintainer's `gh` login
    keychain: Option<Result<bool, String>>,
}

/// GitHub as kelpie's App sees it: one code converts to an App owned by
/// `owner`, and each token lasts an hour from the clock's now
#[derive(Debug, Clone)]
pub(crate) struct FakeGithub {
    hub: Arc<Mutex<Hub>>,
    clock: FakeClock,
    owner: String,
}

impl FakeGithub {
    /// How long a token it mints lasts, as GitHub's do
    pub(crate) const LIFETIME: u64 = 3600;

    pub(crate) fn new(clock: FakeClock, owner: &str) -> Self {
        Self {
            hub: Arc::default(),
            clock,
            owner: owner.to_owned(),
        }
    }

    /// The App the stand-in hands back for a code
    pub(crate) fn conversion(&self) -> Conversion {
        Conversion {
            id: 42,
            slug: format!("kelpie-{}", self.owner.to_ascii_lowercase()),
            client_id: "Iv23liTEST".to_owned(),
            owner: self.owner.clone(),
            pem: Pem::new(PEM.to_owned()),
        }
    }

    /// Installs the App on `repo` as installation `id`
    pub(crate) fn install(&self, repo: &str, id: u64) {
        let repo = repo.to_ascii_lowercase();
        self.hub.lock().unwrap().installed.insert(repo, id);
    }

    /// Makes the next call answer `error`, after any failures already set
    pub(crate) fn fail_next(&self, error: ApiError) {
        self.hub.lock().unwrap().failures.push_back(error);
    }

    /// Makes every comment answer `error` from now on, whatever else is asked
    pub(crate) fn refuse_comments(&self, error: ApiError) {
        self.hub.lock().unwrap().comments_refused = Some(error);
    }

    /// Makes every worker's token answer `error` from now on, as an App that
    /// was never given the Contents permission does
    pub(crate) fn refuse_workers(&self, error: ApiError) {
        self.hub.lock().unwrap().workers_refused = Some(error);
    }

    /// Makes GitHub grant every worker's token `permission` at `level` besides
    /// what was asked for
    pub(crate) fn grants_too(&self, permission: &str, level: &str) {
        let grant = (permission.to_owned(), level.to_owned());
        self.hub.lock().unwrap().extra_grant = Some(grant);
    }

    /// Makes the keychain hold the maintainer's `gh` login, or not
    pub(crate) fn keychain_holds_login(&self, holds: bool) {
        self.hub.lock().unwrap().keychain = Some(Ok(holds));
    }

    /// Makes the keychain unable to say, with `why`
    pub(crate) fn keychain_cannot_be_asked(&self, why: &str) {
        self.hub.lock().unwrap().keychain = Some(Err(why.to_owned()));
    }

    /// Takes comments again after [`Self::refuse_comments`]
    pub(crate) fn accept_comments(&self) {
        self.hub.lock().unwrap().comments_refused = None;
    }

    /// Makes the next conversion hand back an App owned by `owner`
    pub(crate) fn converts_for(&self, owner: &str) {
        self.hub.lock().unwrap().convert_owner = Some(owner.to_owned());
    }

    pub(crate) fn asked(&self) -> Vec<Asked> {
        self.hub.lock().unwrap().asked.clone()
    }

    /// Every write made as the App, in order, with the token it was made with
    pub(crate) fn writes(&self) -> Vec<(String, Call)> {
        (self.asked().into_iter())
            .filter_map(|asked| match asked {
                Asked::Write(token, call) => Some((token, call)),
                _ => None,
            })
            .collect()
    }

    /// The text of each comment the App posted, with the issue or pull
    /// request it went to, in order
    pub(crate) fn comments(&self) -> Vec<(u64, String)> {
        (self.writes().into_iter())
            .filter_map(|(_, call)| {
                let thread = call.path.strip_suffix("/comments")?.rsplit('/').next()?;
                let body: serde_json::Value = serde_json::from_str(call.body.as_deref()?).ok()?;
                Some((thread.parse().ok()?, body["body"].as_str()?.to_owned()))
            })
            .collect()
    }

    /// The id of each comment the App reacted to, in order
    pub(crate) fn reactions(&self) -> Vec<u64> {
        (self.writes().into_iter())
            .filter_map(|(_, call)| {
                let path = call.path.strip_suffix("/reactions")?;
                path.rsplit('/').next()?.parse().ok()
            })
            .collect()
    }

    /// Every JWT a call was made with
    pub(crate) fn jwts(&self) -> Vec<String> {
        self.hub.lock().unwrap().jwts.clone()
    }

    /// Kelpie's App tokens over this stand-in and [`FakeSigner`], kept in `kelpie_home`
    pub(crate) fn tokens(&self, kelpie_home: &Path) -> AppTokens {
        AppTokens::new(
            Apps::under(kelpie_home),
            Box::new(self.clone()),
            Box::new(FakeSigner),
            Box::new(self.clock.clone()),
            Box::new(self.clone()),
        )
    }

    /// Registers the App in `kelpie_home` as setup would, and installs it on `repo`
    pub(crate) fn registered(&self, kelpie_home: &Path, repo: &str) {
        Apps::under(kelpie_home).save(&self.conversion()).unwrap();
        self.install(repo, 7);
    }
}

impl GithubApi for FakeGithub {
    fn convert(&self, code: &str) -> Result<Conversion, ApiError> {
        let mut hub = self.hub.lock().unwrap();
        hub.asked.push(Asked::Convert(code.to_owned()));
        if let Some(error) = hub.failures.pop_front() {
            return Err(error);
        }
        let owner = hub
            .convert_owner
            .take()
            .unwrap_or_else(|| self.owner.clone());
        Ok(Conversion {
            owner,
            ..self.conversion()
        })
    }

    fn installation(&self, jwt: &Jwt, repo: &ForgeSlug) -> Result<Option<u64>, ApiError> {
        let mut hub = self.hub.lock().unwrap();
        hub.asked
            .push(Asked::Installation(repo.as_str().to_owned()));
        hub.jwts.push(jwt.expose().to_owned());
        if let Some(error) = hub.failures.pop_front() {
            return Err(error);
        }
        let repo = repo.as_str().to_ascii_lowercase();
        Ok(hub.installed.get(&repo).copied())
    }

    fn access_token(
        &self,
        jwt: &Jwt,
        installation: u64,
        repo: &ForgeSlug,
        grant: Grant,
    ) -> Result<IssuedToken, ApiError> {
        let mut hub = self.hub.lock().unwrap();
        let body = crate::github::api::token_request(repo, grant);
        hub.asked.push(Asked::AccessToken(installation, body));
        hub.jwts.push(jwt.expose().to_owned());
        if let Some(error) = hub.failures.pop_front() {
            return Err(error);
        }
        if let Some(error) = hub
            .workers_refused
            .clone()
            .filter(|_| grant == Grant::Worker)
        {
            return Err(error);
        }
        hub.minted += 1;
        let ask = |pairs: &[(&str, &str)]| {
            (pairs.iter())
                .map(|&(name, level)| (name.to_owned(), level.to_owned()))
                .collect::<BTreeMap<_, _>>()
        };
        let mut permissions = match grant {
            Grant::Kelpie => ask(&[
                ("issues", "write"),
                ("metadata", "read"),
                ("pull_requests", "write"),
            ]),
            Grant::Worker => ask(&[("contents", "write"), ("metadata", "read")]),
        };
        if let Some((name, level)) = hub.extra_grant.clone().filter(|_| grant == Grant::Worker) {
            permissions.insert(name, level);
        }
        Ok(IssuedToken {
            permissions,
            token: InstallationToken::new(format!("ghs_test{}", hub.minted)),
            expires_at: Timestamp(self.clock.now().0 + Self::LIFETIME),
        })
    }

    // A new issue answers with the next number from 900, clear of any a test opens by hand.
    fn write(&self, token: &InstallationToken, call: &Call) -> Result<String, ApiError> {
        let mut hub = self.hub.lock().unwrap();
        hub.asked
            .push(Asked::Write(token.expose().to_owned(), call.clone()));
        if let Some(error) = hub.failures.pop_front() {
            return Err(error);
        }
        if let Some(error) = hub
            .comments_refused
            .clone()
            .filter(|_| call.path.ends_with("/comments"))
        {
            return Err(error);
        }
        // A comment answers with its id, numbered from 100 and so below any a
        // person writes on the fake forge, and the time the clock says.
        if call.path.ends_with("/comments") {
            hub.comment_ids += 1;
            let created = jiff::Timestamp::from_second(self.clock.now().0 as i64).unwrap();
            return Ok(format!(
                r#"{{"id":{},"created_at":"{created}"}}"#,
                99 + hub.comment_ids
            ));
        }
        if call.path.ends_with("/issues") {
            hub.issues += 1;
            return Ok(format!(r#"{{"number":{}}}"#, 899 + hub.issues));
        }
        Ok("{}".to_owned())
    }
}

// The keychain holds no login unless a test says it does.
impl Keychain for FakeGithub {
    fn holds_github_login(&self) -> Result<bool, String> {
        (self.hub.lock().unwrap().keychain.clone()).unwrap_or(Ok(false))
    }
}

/// Signs anything with one fixed signature
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct FakeSigner;

impl FakeSigner {
    /// What every signature is
    pub(crate) const SIGNATURE: &[u8] = b"signed";
}

impl Signer for FakeSigner {
    fn sign(&self, _key: &Path, _input: &[u8]) -> Result<Vec<u8>, String> {
        Ok(Self::SIGNATURE.to_vec())
    }
}
