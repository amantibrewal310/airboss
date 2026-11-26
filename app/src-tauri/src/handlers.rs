//! Who opens a link the allowlist passed (SPEC-app "Opening links").
//!
//! A plain `open <url>` lets LaunchServices pick the app, and an agent can register one: an
//! `Evil.app` it wrote, claiming T3 Code's bundle id and the `t3code` scheme, is registered as
//! soon as Finder shows its folder (revealing a sibling file is enough). So before a `t3code`
//! or `https` link opens, the app macOS would use is looked up and its code signature checked
//! against a requirement, and the link is opened with exactly that app (`open -a`):
//!
//! - **T3 links:** T3 Code's own signature, its bundle id and T3 Tools' team id
//!   ([`T3_REQUIREMENT`]). The app's name and folder don't matter ("T3 Code (Beta).app").
//! - **Web links:** an Apple-issued signature (Apple, the App Store or a Developer ID), and the
//!   browser can't live in the home folder, except `~/Applications`.
//!
//! Anything else is refused, with the link offered to copy.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::errors::{AppError, ErrorCode};

/// T3 Code as T3 Tools, Inc. signs it (Developer ID, team ARK85ZXQ4Z).
pub const T3_REQUIREMENT: &str = r#"identifier "com.t3tools.t3code" and anchor apple generic and certificate leaf[subject.OU] = "ARK85ZXQ4Z""#;
/// Any app Apple signed or notarised a Developer ID for: Safari, Chrome, Firefox, Arc …
pub const BROWSER_REQUIREMENT: &str = "anchor apple generic";

pub const MSG_T3_UNVERIFIED: &str =
    "Couldn't verify the app that opens T3 links. Open the thread from T3 Code.";
pub const MSG_BROWSER_UNVERIFIED: &str = "Couldn't verify your browser. Copy the link instead.";

/// Which kind of link is being opened.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Thread,
    Browser,
}

/// The two questions asked of macOS, behind a trait so tests can answer them.
pub trait AppCheck: Send + Sync {
    /// The app macOS would open `url` with, as its resolved bundle path; `None` if none.
    fn handler(&self, url: &str) -> Option<PathBuf>;
    /// Whether the app at `app` is validly signed and meets the code requirement `requirement`.
    fn satisfies(&self, app: &Path, requirement: &str) -> bool;
}

/// The app to open `url` with, once it's verified; otherwise the refusal, with the link as
/// its detail so the UI can offer to copy it.
pub fn verified_app(
    kind: Kind,
    url: &str,
    check: &dyn AppCheck,
    home: &Path,
) -> Result<PathBuf, AppError> {
    let refuse = || {
        let message = match kind {
            Kind::Thread => MSG_T3_UNVERIFIED,
            Kind::Browser => MSG_BROWSER_UNVERIFIED,
        };
        AppError::new(ErrorCode::AppUnverified, message).with_detail(url)
    };
    let app = check.handler(url).ok_or_else(refuse)?;
    let in_home = app.starts_with(home) && !app.starts_with(home.join("Applications"));
    if kind == Kind::Browser && in_home {
        return Err(refuse());
    }
    let requirement = match kind {
        Kind::Thread => T3_REQUIREMENT,
        Kind::Browser => BROWSER_REQUIREMENT,
    };
    if !check.satisfies(&app, requirement) {
        return Err(refuse());
    }
    Ok(app)
}

/// macOS's answers: LaunchServices for the handler, the Security framework for the signature.
#[derive(Debug, Default)]
pub struct MacAppCheck;

impl AppCheck for MacAppCheck {
    fn handler(&self, url: &str) -> Option<PathBuf> {
        crate::macos::handler_for_url(url)?.canonicalize().ok()
    }

    fn satisfies(&self, app: &Path, requirement: &str) -> bool {
        crate::macos::code_satisfies(app, requirement)
    }
}

/// Answers set by a test: a handler per URL scheme, and the (app, requirement) pairs that pass.
#[derive(Debug)]
pub struct FakeAppCheck {
    handlers: Mutex<Vec<(String, PathBuf)>>,
    trusted: Mutex<Vec<(PathBuf, String)>>,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

impl Default for FakeAppCheck {
    /// T3 Code and Safari where they'd be, both genuine.
    fn default() -> FakeAppCheck {
        let t3 = PathBuf::from("/Applications/T3 Code (Beta).app");
        let safari = PathBuf::from("/Applications/Safari.app");
        FakeAppCheck {
            handlers: Mutex::new(vec![
                ("t3code".into(), t3.clone()),
                ("https".into(), safari.clone()),
            ]),
            trusted: Mutex::new(vec![
                (t3, T3_REQUIREMENT.into()),
                (safari, BROWSER_REQUIREMENT.into()),
            ]),
        }
    }
}

impl FakeAppCheck {
    /// The app that opens `scheme`, or none.
    pub fn set_handler(&self, scheme: &str, app: Option<PathBuf>) {
        let mut h = lock(&self.handlers);
        h.retain(|(s, _)| s != scheme);
        if let Some(app) = app {
            h.push((scheme.into(), app));
        }
    }

    /// Makes `app` meet `requirement`.
    pub fn trust(&self, app: PathBuf, requirement: &str) {
        lock(&self.trusted).push((app, requirement.into()));
    }
}

impl AppCheck for FakeAppCheck {
    fn handler(&self, url: &str) -> Option<PathBuf> {
        let scheme = url.split(':').next()?.to_ascii_lowercase();
        lock(&self.handlers)
            .iter()
            .find(|(s, _)| *s == scheme)
            .map(|(_, app)| app.clone())
    }

    fn satisfies(&self, app: &Path, requirement: &str) -> bool {
        lock(&self.trusted)
            .iter()
            .any(|(a, r)| a == app && r == requirement)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    const T3: &str = "t3code://threads/env/abc";
    const WEB: &str = "https://github.com/acme/web/pull/1";

    fn home() -> PathBuf {
        PathBuf::from("/Users/me")
    }

    #[test]
    fn the_genuine_apps_open_their_links() {
        let check = FakeAppCheck::default();
        assert_eq!(
            verified_app(Kind::Thread, T3, &check, &home()).unwrap(),
            PathBuf::from("/Applications/T3 Code (Beta).app")
        );
        assert_eq!(
            verified_app(Kind::Browser, WEB, &check, &home()).unwrap(),
            PathBuf::from("/Applications/Safari.app")
        );
    }

    /// The audit's chain: an agent's Evil.app claims the `t3code` scheme with T3's bundle id.
    #[test]
    fn a_t3_handler_without_t3s_signature_is_refused_with_the_link() {
        let check = FakeAppCheck::default();
        let evil = PathBuf::from("/Users/me/Code/web/.worktree/Evil.app");
        check.set_handler("t3code", Some(evil.clone()));
        // Even a valid Apple-anchored signature isn't enough: it must be T3 Tools'.
        check.trust(evil, BROWSER_REQUIREMENT);
        let e = verified_app(Kind::Thread, T3, &check, &home()).unwrap_err();
        assert_eq!(e.code, ErrorCode::AppUnverified);
        assert_eq!(
            e.message,
            "Couldn't verify the app that opens T3 links. Open the thread from T3 Code."
        );
        assert_eq!(e.detail.as_deref(), Some(T3));
    }

    #[test]
    fn t3_code_is_trusted_wherever_it_lives_and_whatever_its_name() {
        let check = FakeAppCheck::default();
        let moved = PathBuf::from("/Users/me/Applications/T3 Code.app");
        check.set_handler("t3code", Some(moved.clone()));
        check.trust(moved.clone(), T3_REQUIREMENT);
        assert_eq!(
            verified_app(Kind::Thread, T3, &check, &home()).unwrap(),
            moved
        );
    }

    #[test]
    fn a_browser_must_be_apple_signed_and_not_in_the_home_folder_but_applications() {
        let check = FakeAppCheck::default();
        for (app, signed, ok) in [
            ("/Applications/Google Chrome.app", true, true),
            ("/Users/me/Applications/Arc.app", true, true),
            ("/Applications/Lookalike.app", false, false),
            // Signed, but somewhere an agent can write.
            ("/Users/me/Downloads/Browser.app", true, false),
            ("/Users/me/Applications.evil/Browser.app", true, false),
            ("/Users/me/Code/web/.worktree/Evil.app", true, false),
        ] {
            let app = PathBuf::from(app);
            check.set_handler("https", Some(app.clone()));
            if signed {
                check.trust(app.clone(), BROWSER_REQUIREMENT);
            }
            let got = verified_app(Kind::Browser, WEB, &check, &home());
            assert_eq!(got.is_ok(), ok, "{}", app.display());
            if let Err(e) = got {
                assert_eq!(
                    e.message,
                    "Couldn't verify your browser. Copy the link instead."
                );
                assert_eq!(e.detail.as_deref(), Some(WEB));
            }
        }
    }

    #[test]
    fn no_handler_at_all_is_refused() {
        let check = FakeAppCheck::default();
        check.set_handler("t3code", None);
        check.set_handler("https", None);
        assert!(verified_app(Kind::Thread, T3, &check, &home()).is_err());
        assert!(verified_app(Kind::Browser, WEB, &check, &home()).is_err());
    }
}
