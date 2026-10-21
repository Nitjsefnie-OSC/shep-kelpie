//! An image from an issue's text, fetched with the maintainer's `gh` login
//!
//! GitHub serves an attachment from `github.com/user-attachments/assets/` and
//! `user-images.githubusercontent.com`, and sends an image of a repo that needs a login to a signed
//! address on another host. The login rides in a header curl reads from its
//! stdin, so no process listing shows it, and curl drops it at a redirect to
//! another host.

use std::ffi::OsString;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Stdio;

use super::gh;
use crate::adapters::curl::render;
use crate::ports::{ForgeError, is_github_attachment};

/// Seconds curl gets to connect, and to finish the whole download
const CONNECT_TIMEOUT: u32 = 10;
const MAX_TIME: u32 = 60;

/// The file extension of a `Content-Type` that is an image a model reads, and
/// `None` for any other type
fn extension(content_type: &str) -> Option<&'static str> {
    let kind = content_type.split(';').next()?.trim().to_ascii_lowercase();
    match kind.as_str() {
        "image/png" => Some("png"),
        "image/jpeg" => Some("jpg"),
        "image/gif" => Some("gif"),
        "image/webp" => Some("webp"),
        _ => None,
    }
}

/// Downloads the image at `url` to `stem` plus its extension, at most `most`
/// bytes of it
///
/// # Errors
///
/// [`ForgeError`] when `url` is not a GitHub attachment link, `gh` has no login
/// to send, curl cannot fetch it within the size and time allowed, or what came
/// is not a PNG, JPEG, GIF or WebP image. The file is removed on an error.
pub(super) fn fetch_image(url: &str, stem: &Path, most: u64) -> Result<PathBuf, ForgeError> {
    if !is_github_attachment(url) {
        return Err(ForgeError::Failed(
            "not a link to a GitHub attachment".to_owned(),
        ));
    }
    let token = String::from_utf8_lossy(&gh(&["auth", "token"])?)
        .trim()
        .to_owned();
    let mut part = OsString::from(stem);
    part.push(".part");
    let part = PathBuf::from(part);
    let fetched = download(url, &token, &part, most).and_then(|content_type| {
        let extension = extension(&content_type).ok_or_else(|| {
            ForgeError::Failed(format!("it is not an image: {}", content_type.trim()))
        })?;
        let size = fs::metadata(&part)
            .map_err(|e| ForgeError::Failed(format!("cannot read what came: {e}")))?
            .len();
        if size > most {
            return Err(ForgeError::Failed(format!(
                "it is {size} bytes, past the {most} allowed"
            )));
        }
        let mut named = OsString::from(stem);
        named.push(".");
        named.push(extension);
        let named = PathBuf::from(named);
        fs::rename(&part, &named)
            .map_err(|e| ForgeError::Failed(format!("cannot keep what came: {e}")))?;
        Ok(named)
    });
    if fetched.is_err() {
        let _ = fs::remove_file(&part);
    }
    fetched
}

// Runs curl for `url` into `part` and returns the content type it was served as
fn download(url: &str, token: &str, part: &Path, most: u64) -> Result<String, ForgeError> {
    let config = render(&[
        ("url", url.to_owned()),
        ("proto", "=https".to_owned()),
        ("proto-redir", "=https".to_owned()),
        ("location", String::new()),
        ("max-redirs", "3".to_owned()),
        ("fail", String::new()),
        ("connect-timeout", CONNECT_TIMEOUT.to_string()),
        ("max-time", MAX_TIME.to_string()),
        ("max-filesize", most.to_string()),
        ("output", part.display().to_string()),
        ("write-out", "%{content_type}".to_owned()),
        ("header", format!("Authorization: Bearer {token}")),
    ]);
    let mut child = crate::spawn::command("curl")
        .args(["--config", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| ForgeError::Spawn(e.to_string()))?;
    let mut stdin = child.stdin.take().expect("stdin was piped");
    let written = stdin.write_all(config.as_bytes());
    drop(stdin);
    let output = child
        .wait_with_output()
        .map_err(|e| ForgeError::Spawn(e.to_string()))?;
    if written.is_err() || !output.status.success() {
        let code = output.status.code().map_or(-1, i64::from);
        return Err(ForgeError::Failed(format!("curl exited {code}")));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

#[cfg(test)]
mod tests {
    use super::{extension, fetch_image};
    use crate::ports::ForgeError;
    use std::path::Path;

    #[test]
    fn only_the_image_types_a_model_reads_have_an_extension() {
        assert_eq!(extension("image/png"), Some("png"));
        assert_eq!(extension("image/jpeg"), Some("jpg"));
        assert_eq!(extension("Image/GIF; charset=binary"), Some("gif"));
        assert_eq!(extension("image/webp"), Some("webp"));
        for other in [
            "image/svg+xml",
            "text/html",
            "video/mp4",
            "application/zip",
            "",
        ] {
            assert_eq!(extension(other), None, "{other}");
        }
    }

    #[test]
    fn a_link_to_another_host_is_refused_before_any_login_is_read() {
        let result = fetch_image("https://example.com/a.png", Path::new("/nowhere/1"), 10);
        assert_eq!(
            result,
            Err(ForgeError::Failed(
                "not a link to a GitHub attachment".to_owned()
            ))
        );
    }
}
