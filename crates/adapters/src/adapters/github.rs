//! The GitHub App's calls over `curl`, and its signer over `openssl`
//!
//! Each call reaches curl as a config on its stdin, as the webhook's do, so
//! no process listing shows a JWT. `openssl` reads the key from its file and
//! the JWT's header and claims from its stdin, so signing never loads the key
//! into kelpie. Setup holds the key, from the conversion's answer to its
//! file; it never goes in a process argument or an environment variable.

use std::io::Write;
use std::path::Path;
use std::process::Stdio;

use super::curl;
use crate::github::api::{
    self, ApiError, Call, Conversion, GithubApi, Grant, Installation, IssuedToken, Signer, Verb,
};
use crate::github::tokens::Jwt;
use crate::ports::{AlertError, Cooldown, InstallationToken};
use crate::settings::ForgeSlug;

const API: &str = "https://api.github.com";

/// Seconds curl gets to connect, and to finish the whole call
const CONNECT_TIMEOUT: u32 = 5;
const MAX_TIME: u32 = 30;

/// The App's calls, made with the system's `curl`
#[derive(Debug, Clone, Copy, Default)]
pub struct CurlGithub;

impl GithubApi for CurlGithub {
    fn convert(&self, code: &str) -> Result<Conversion, ApiError> {
        let url = format!("{API}/app-manifests/{code}/conversions");
        match call(&url, None, Some(""))? {
            (body, 201, _) => api::parse_conversion(&body),
            (body, status, cooldown) => Err(api::refusal(status, &body, cooldown)),
        }
    }

    fn installation(&self, jwt: &Jwt, repo: &ForgeSlug) -> Result<Option<Installation>, ApiError> {
        let url = format!("{API}/repos/{}/installation", repo.as_str());
        match call(&url, Some(jwt), None)? {
            (body, 200, _) => api::parse_installation(&body).map(Some),
            (_, 404, _) => Ok(None),
            (body, status, cooldown) => Err(api::refusal(status, &body, cooldown)),
        }
    }

    fn access_token(
        &self,
        jwt: &Jwt,
        installation: u64,
        repo: &ForgeSlug,
        grant: Grant,
    ) -> Result<IssuedToken, ApiError> {
        let url = format!("{API}/app/installations/{installation}/access_tokens");
        match call(&url, Some(jwt), Some(&api::token_request(repo, grant)))? {
            (body, 201, _) => api::parse_token(&body),
            (body, status, cooldown) => Err(api::refusal(status, &body, cooldown)),
        }
    }

    fn write(&self, token: &InstallationToken, write: &Call) -> Result<String, ApiError> {
        let url = format!("{API}{}", write.path);
        let bearer = Some(token.expose());
        let sent = match write.verb {
            Verb::Post => request(&url, bearer, None, write.body.as_deref())?,
            Verb::Patch => request(&url, bearer, Some("PATCH"), write.body.as_deref())?,
            Verb::Delete => request(&url, bearer, Some("DELETE"), None)?,
        };
        match sent {
            (body, 200..=299, _) => Ok(body),
            (body, status, cooldown) => Err(api::refusal(status, &body, cooldown)),
        }
    }
}

// One call to `url`, a `POST` of `post` when there is one, and what it
// answered with its status and when its headers say to ask again. A redirect
// is never followed, so a JWT goes nowhere but GitHub's API.
fn call(
    url: &str,
    jwt: Option<&Jwt>,
    post: Option<&str>,
) -> Result<(String, u16, Cooldown), ApiError> {
    request(url, jwt.map(Jwt::expose), None, post)
}

// One request to `url` as `verb`, curl's own choice when `None`, bearing
// `bearer` when there is one.
fn request(
    url: &str,
    bearer: Option<&str>,
    verb: Option<&str>,
    post: Option<&str>,
) -> Result<(String, u16, Cooldown), ApiError> {
    let mut lines = vec![
        ("url", url.to_owned()),
        ("proto", "=https".to_owned()),
        ("connect-timeout", CONNECT_TIMEOUT.to_string()),
        ("max-time", MAX_TIME.to_string()),
        ("header", "Accept: application/vnd.github+json".to_owned()),
        ("header", "X-GitHub-Api-Version: 2022-11-28".to_owned()),
        ("user-agent", "shep-kelpie".to_owned()),
        ("write-out", WRITE_OUT.to_owned()),
    ];
    if let Some(bearer) = bearer {
        lines.push(("header", format!("Authorization: Bearer {bearer}")));
    }
    if let Some(verb) = verb {
        lines.push(("request", verb.to_owned()));
    }
    if let Some(body) = post {
        lines.push(("header", "Content-Type: application/json".to_owned()));
        lines.push(("data-raw", body.to_owned()));
    }
    let (printed, status) = curl::run(&curl::render(&lines)).map_err(|e| match e {
        AlertError::Unreachable(code) => ApiError::Unreachable(format!("curl exited {code}")),
        e => ApiError::Unreachable(e.to_string()),
    })?;
    let (body, cooldown) = split_cooldown(&printed);
    Ok((body.to_owned(), status, cooldown))
}

// What curl prints after the body: the two headers a rate limit answers
// with, on a line of their own, then the status, which `curl::run` takes.
const WRITE_OUT: &str = "\n%header{retry-after}|%header{x-ratelimit-reset}\n%{http_code}";

// `printed` without its last line, which is `WRITE_OUT`'s headers, and what
// those say. A curl too old to know `%header` prints the line as it was
// written, which reads as no cooldown.
fn split_cooldown(printed: &str) -> (&str, Cooldown) {
    let (body, headers) = printed.rsplit_once('\n').unwrap_or(("", printed));
    let (retry_after, reset) = headers.split_once('|').unwrap_or_default();
    (body, Cooldown::read(retry_after, reset))
}

/// Signs with the system's `openssl`, which reads the key from its file
#[derive(Debug, Clone, Copy, Default)]
pub struct Openssl;

impl Signer for Openssl {
    fn sign(&self, key: &Path, input: &[u8]) -> Result<Vec<u8>, String> {
        let mut child = crate::spawn::command("openssl")
            .args(["dgst", "-sha256", "-sign"])
            .arg(key)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("cannot run openssl: {e}"))?;
        let mut stdin = child.stdin.take().expect("stdin was piped");
        let written = stdin.write_all(input);
        drop(stdin);
        let output = child
            .wait_with_output()
            .map_err(|e| format!("cannot run openssl: {e}"))?;
        match written {
            Ok(()) if output.status.success() && !output.stdout.is_empty() => Ok(output.stdout),
            _ => Err(format!(
                "openssl could not sign with {}: {}",
                key.display(),
                String::from_utf8_lossy(&output.stderr).trim()
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::Timestamp;

    #[test]
    fn the_headers_a_rate_limit_answers_with_are_read_after_the_body() {
        let (body, cooldown) = split_cooldown("{\"message\":\"slow\"}\n30|");
        assert_eq!(body, "{\"message\":\"slow\"}");
        assert_eq!(cooldown.retry_after, Some(30));
        assert_eq!(cooldown.reset, None);

        let (_, cooldown) = split_cooldown("{}\n|1790003600");
        assert_eq!(cooldown.retry_after, None);
        assert_eq!(cooldown.reset, Some(Timestamp(1_790_003_600)));
    }

    #[test]
    fn a_body_over_several_lines_keeps_them_and_no_headers_read_as_no_cooldown() {
        let (body, cooldown) = split_cooldown("a\nb\n|");
        assert_eq!(body, "a\nb");
        assert_eq!(cooldown, Cooldown::default());

        // A curl that does not know `%header` prints the line as written.
        let (body, cooldown) =
            split_cooldown("{}\n%header{retry-after}|%header{x-ratelimit-reset}");
        assert_eq!(body, "{}");
        assert_eq!(cooldown, Cooldown::default());
    }

    #[test]
    fn an_answer_with_no_body_has_no_cooldown_either() {
        let (body, cooldown) = split_cooldown("\n|");
        assert_eq!((body, cooldown), ("", Cooldown::default()));
    }
}
