//! The links to a GitHub issue's attachments that kelpie may fetch
//!
//! GitHub serves what a person drops into an issue from two hosts of its own.
//! A link to any other host is somebody else's server, and the forge login is
//! never sent to it.

/// Whether `url` is a link to an attachment on GitHub's own hosts:
/// `https://github.com/user-attachments/assets/...` or
/// `https://user-images.githubusercontent.com/...`
///
/// The host must be the whole authority, so a port, a login in front of it or
/// a longer name that merely ends in it is no match.
#[must_use]
pub fn is_github_attachment(url: &str) -> bool {
    let Some(rest) = url.strip_prefix("https://") else {
        return false;
    };
    let (host, path) = match rest.find(['/', '?', '#']) {
        Some(at) => rest.split_at(at),
        None => (rest, ""),
    };
    let path = path.split(['?', '#']).next().unwrap_or_default();
    let segments_ok = path.split('/').skip(1).all(|s| !matches!(s, ".." | "."));
    if !segments_ok {
        return false;
    }
    match host {
        "github.com" => path
            .strip_prefix("/user-attachments/assets/")
            .is_some_and(|name| !name.is_empty()),
        "user-images.githubusercontent.com" => path.len() > 1,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::is_github_attachment;

    #[test]
    fn github_s_own_attachment_links_are_fetchable() {
        for url in [
            "https://github.com/user-attachments/assets/3f2a9c1e-0b7d-4c55-9a53-1d2e4f6a8b90",
            "https://user-images.githubusercontent.com/1234/5678-abcd.png",
            "https://user-images.githubusercontent.com/1234/5678-abcd.png?raw=true",
        ] {
            assert!(is_github_attachment(url), "{url}");
        }
    }

    #[test]
    fn a_link_to_any_other_host_or_shape_is_not() {
        for url in [
            "https://example.com/user-attachments/assets/3f2a",
            "https://github.com.evil.example/user-attachments/assets/3f2a",
            "https://github.com@evil.example/user-attachments/assets/3f2a",
            "https://evil.example/https://github.com/user-attachments/assets/3f2a",
            "https://github.com:8443/user-attachments/assets/3f2a",
            "https://github.com/user-attachments/assets/",
            "https://github.com/user-attachments/files/123/log.zip",
            "https://github.com/someone/repo/issues/1",
            "https://github.com/user-attachments/assets/../../login",
            "https://evil-user-images.githubusercontent.com/1/a.png",
            "https://user-images.githubusercontent.com/",
            "https://user-images.githubusercontent.com",
            "http://github.com/user-attachments/assets/3f2a",
            "ftp://github.com/user-attachments/assets/3f2a",
            "github.com/user-attachments/assets/3f2a",
            "",
        ] {
            assert!(!is_github_attachment(url), "{url}");
        }
    }
}
