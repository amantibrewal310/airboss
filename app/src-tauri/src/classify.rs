//! Sorting pasted links by their form (SPEC-app "Pasting"), and the opener's allowlist.

use std::path::Path;

use airboss_core::{LinkKind, NewLink};
use airboss_services::PrRef;

use crate::errors::{AppError, ErrorCode, MSG_NOT_A_LINK, MSG_URL_NOT_ALLOWED};

const T3_THREADS: &str = "t3code://threads/";

/// What a pasted link or path is, ready to give `core`.
pub fn classify(text: &str, title: Option<&str>, home: &Path) -> Result<NewLink, AppError> {
    let text = text.trim();
    let lower = text.to_ascii_lowercase();
    let mut link = if let Some(rest) = strip_github(&lower) {
        let parts: Vec<&str> = rest
            .split(['?', '#'])
            .next()
            .unwrap_or("")
            .split('/')
            .collect();
        match parts.as_slice() {
            [_, _, "pull", n, ..] if is_number(n) => {
                let pr = PrRef::parse(text).ok_or_else(not_a_link)?;
                NewLink::pr(pr.as_str()).with_url(text)
            }
            [owner, repo, "issues", n, ..] if is_number(n) => {
                NewLink::new(LinkKind::GithubIssue, format!("{owner}/{repo}#{n}")).with_url(text)
            }
            _ => NewLink::new(LinkKind::Doc, text).with_url(text),
        }
    } else if lower.starts_with(T3_THREADS) {
        // t3code://threads/<environment>/<thread>, keeping the thread id's case as pasted.
        let thread = text[T3_THREADS.len()..]
            .split(['?', '#'])
            .next()
            .unwrap_or("")
            .split('/')
            .nth(1)
            .filter(|t| !t.is_empty())
            .ok_or_else(not_a_link)?;
        NewLink::new(LinkKind::T3Thread, thread).with_url(text)
    } else if is_slack(&lower) {
        let path = text
            .split_once("://")
            .map(|(_, rest)| rest)
            .and_then(|rest| rest.split_once('/'))
            .map(|(_, path)| format!("/{path}"))
            .unwrap_or_else(|| text.to_string());
        NewLink::new(LinkKind::Slack, path).with_url(text)
    } else if lower.starts_with("http://") || lower.starts_with("https://") {
        NewLink::new(LinkKind::Doc, text).with_url(text)
    } else if text.starts_with('/') || text.starts_with("~/") || text == "~" {
        let path = expand_home(text, home);
        NewLink::new(LinkKind::Doc, text).with_url(file_url(&path))
    } else if let Some(pr) = PrRef::parse(text) {
        NewLink::pr(pr.as_str())
    } else {
        return Err(not_a_link());
    };
    if let Some(t) = title.map(str::trim).filter(|t| !t.is_empty()) {
        link = link.with_title(t);
    } else if link.kind == LinkKind::Doc
        && link
            .url
            .as_deref()
            .is_some_and(|u| u.starts_with("file://"))
    {
        // A path shows its file name.
        if let Some(name) = Path::new(text).file_name().and_then(|n| n.to_str()) {
            link = link.with_title(name);
        }
    }
    Ok(link)
}

fn not_a_link() -> AppError {
    AppError::new(ErrorCode::NotALink, MSG_NOT_A_LINK)
}

fn is_number(n: &str) -> bool {
    !n.is_empty() && n.chars().all(|c| c.is_ascii_digit())
}

fn strip_github(lower: &str) -> Option<&str> {
    [
        "https://github.com/",
        "http://github.com/",
        "https://www.github.com/",
        "github.com/",
    ]
    .iter()
    .find_map(|p| lower.strip_prefix(p))
}

fn is_slack(lower: &str) -> bool {
    let Some(rest) = lower
        .strip_prefix("https://")
        .or_else(|| lower.strip_prefix("http://"))
    else {
        return false;
    };
    let (host, path) = rest.split_once('/').unwrap_or((rest, ""));
    (host == "slack.com" || host.ends_with(".slack.com")) && path.starts_with("archives/")
}

/// `~/x` → `<home>/x`.
pub fn expand_home(path: &str, home: &Path) -> std::path::PathBuf {
    match path.strip_prefix("~") {
        Some(rest) => home.join(rest.trim_start_matches('/')),
        None => std::path::PathBuf::from(path),
    }
}

/// A `file://` URL for an absolute path, with spaces and other unsafe bytes escaped.
pub fn file_url(path: &Path) -> String {
    let mut out = String::from("file://");
    for b in path.to_string_lossy().bytes() {
        if b.is_ascii_alphanumeric() || b"/-._~".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// The opener's allowlist: only `http`, `https`, `file` and `t3code`.
pub fn check_url(url: &str) -> Result<(), AppError> {
    let scheme = url.split_once(':').map(|(s, _)| s.to_ascii_lowercase());
    match scheme.as_deref() {
        Some("http" | "https" | "file" | "t3code") if url.contains("://") => Ok(()),
        _ => Err(AppError::new(ErrorCode::UrlNotAllowed, MSG_URL_NOT_ALLOWED)),
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    fn c(text: &str) -> NewLink {
        classify(text, None, Path::new("/Users/me")).unwrap()
    }

    #[test]
    fn every_row_of_the_paste_table() {
        let pr = c("https://github.com/Acme/Web/pull/58");
        assert_eq!((pr.kind, pr.r#ref.as_str()), (LinkKind::Pr, "acme/web#58"));
        assert_eq!(
            pr.url.as_deref(),
            Some("https://github.com/Acme/Web/pull/58")
        );

        let issue = c("https://github.com/acme/web/issues/7");
        assert_eq!(
            (issue.kind, issue.r#ref.as_str()),
            (LinkKind::GithubIssue, "acme/web#7")
        );

        let slack = c("https://acme.slack.com/archives/C01/p1700000000");
        assert_eq!(slack.kind, LinkKind::Slack);
        assert_eq!(slack.r#ref, "/archives/C01/p1700000000");

        let t3 = c("t3code://threads/env-1/Thread-AbC");
        assert_eq!(
            (t3.kind, t3.r#ref.as_str()),
            (LinkKind::T3Thread, "Thread-AbC")
        );

        let doc = c("~/notes/booking flow.md");
        assert_eq!(doc.kind, LinkKind::Doc);
        assert_eq!(
            doc.url.as_deref(),
            Some("file:///Users/me/notes/booking%20flow.md")
        );
        assert_eq!(doc.title.as_deref(), Some("booking flow.md"));

        let abs = c("/tmp/plan.md");
        assert_eq!(abs.url.as_deref(), Some("file:///tmp/plan.md"));

        let web = c("https://example.com/spec");
        assert_eq!(
            (web.kind, web.url.as_deref()),
            (LinkKind::Doc, Some("https://example.com/spec"))
        );

        let bare = c("Acme/Web#12");
        assert_eq!(
            (bare.kind, bare.r#ref.as_str()),
            (LinkKind::Pr, "acme/web#12")
        );
    }

    #[test]
    fn anything_else_is_not_a_link() {
        for t in ["hello", "", "ftp://x/y", "github.com/acme/web/pull/x"] {
            let r = classify(t, None, Path::new("/"));
            if t.starts_with("github.com") {
                // A GitHub page that isn't a PR or issue is still a web link.
                assert_eq!(r.unwrap().kind, LinkKind::Doc);
            } else {
                assert_eq!(r.unwrap_err().code, ErrorCode::NotALink, "{t}");
            }
        }
    }

    #[test]
    fn a_title_overrides_the_default() {
        let l = classify("/tmp/a.md", Some("Plan"), Path::new("/")).unwrap();
        assert_eq!(l.title.as_deref(), Some("Plan"));
    }

    #[test]
    fn the_opener_allows_only_four_schemes() {
        for ok in [
            "https://x.y",
            "http://x",
            "file:///tmp/a",
            "t3code://threads/e/t",
        ] {
            assert!(check_url(ok).is_ok(), "{ok}");
        }
        for bad in [
            "javascript:alert(1)",
            "ssh://x",
            "x-apple.systempreferences:",
            "/etc/passwd",
            "mailto:a@b",
        ] {
            assert_eq!(
                check_url(bad).unwrap_err().code,
                ErrorCode::UrlNotAllowed,
                "{bad}"
            );
        }
    }
}
