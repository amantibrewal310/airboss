//! PR refs in `core`'s canonical form.

use std::fmt;

use serde::{Deserialize, Serialize};

/// A PR ref in `core`'s canonical form, `"owner/repo#n"` in lowercase (`core` rule 29).
///
/// The only ways to make one are [`parse`](PrRef::parse) and [`new`](PrRef::new), so every
/// `PrRef` is valid. It serialises as that plain string, and deserialising checks it.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(into = "String", try_from = "String")]
pub struct PrRef(String);

impl PrRef {
    /// `"Owner/Repo#12"` in any case, or `"https://github.com/Owner/Repo/pull/12"` with an
    /// optional trailing path, query or fragment. `None` for anything `core` would refuse.
    pub fn parse(input: &str) -> Option<PrRef> {
        canonical(input).map(PrRef)
    }

    /// From GitHub's `nameWithOwner` and a number: `("Owner/Repo", 12)`.
    pub fn new(owner_repo: &str, number: u64) -> Option<PrRef> {
        PrRef::parse(&format!("{owner_repo}#{number}"))
    }

    /// `"owner/repo#n"`.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The owner, the repo and the number, for `gh`'s query variables.
    pub fn parts(&self) -> (&str, &str, u64) {
        // Valid by construction, so the fallbacks never apply.
        let (path, number) = self.0.split_once('#').unwrap_or((&self.0, "0"));
        let (owner, repo) = path.split_once('/').unwrap_or((path, ""));
        (owner, repo, number.parse().unwrap_or(0))
    }
}

impl fmt::Display for PrRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<PrRef> for String {
    fn from(pr: PrRef) -> String {
        pr.0
    }
}

impl TryFrom<String> for PrRef {
    type Error = String;

    fn try_from(text: String) -> Result<PrRef, String> {
        PrRef::parse(&text).ok_or_else(|| format!("not a PR ref: {text:?}"))
    }
}

/// `core` v3's rule, step for step (`canonical_pr_ref` in `crates/core/src/links.rs`). A
/// contract test runs the same inputs through `core` and checks both agree.
fn canonical(input: &str) -> Option<String> {
    const GITHUB: &str = "https://github.com/";
    let input = input.trim();
    let url_path = input
        .get(..GITHUB.len())
        .filter(|start| start.eq_ignore_ascii_case(GITHUB))
        .map(|_| &input[GITHUB.len()..]);
    let (owner, repo, number) = match url_path {
        Some(rest) => {
            let path = rest.split(['?', '#']).next().unwrap_or_default();
            let mut parts = path.split('/');
            match (parts.next(), parts.next(), parts.next(), parts.next()) {
                (Some(owner), Some(repo), Some(pull), Some(n))
                    if pull.eq_ignore_ascii_case("pull") =>
                {
                    (owner, repo, n)
                }
                _ => return None,
            }
        }
        None => {
            let (path, n) = input.split_once('#')?;
            let (owner, repo) = path.split_once('/')?;
            (owner, repo, n)
        }
    };
    let name_ok = |name: &str| {
        !name.is_empty()
            && name != "."
            && name != ".."
            && name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
    };
    if number.is_empty() || !number.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let number: u64 = number.parse().ok()?;
    if !name_ok(owner) || !name_ok(repo) || number == 0 {
        return None;
    }
    Some(format!(
        "{}/{}#{number}",
        owner.to_ascii_lowercase(),
        repo.to_ascii_lowercase()
    ))
}
