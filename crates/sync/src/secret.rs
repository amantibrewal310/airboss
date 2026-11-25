//! A `gh` token, held in memory only.

use std::fmt;

/// A token. It has no `Display`, and `Debug` prints `Secret(…)`, so it can't reach a log, an
/// error or a table by accident. Only `gh.rs` calls [`expose`](Secret::expose), to set a
/// child process's environment.
#[derive(Clone, PartialEq, Eq)]
pub struct Secret(String);

impl Secret {
    /// Wraps a token, trimmed of the newline `gh auth token` prints.
    pub fn new(token: impl Into<String>) -> Secret {
        Secret(token.into().trim().to_string())
    }

    /// The token itself, for a child's environment only.
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret(…)")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_never_shows_the_token() {
        let s = Secret::new("gho_abc123\n");
        assert_eq!(format!("{s:?}"), "Secret(…)");
        assert_eq!(format!("{:?}", Some(&s)), "Some(Secret(…))");
        assert_eq!(s.expose(), "gho_abc123");
    }
}
