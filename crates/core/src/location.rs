//! Where the database lives, worked out once for the CLI and the app.

use std::ffi::OsString;
use std::path::PathBuf;

use crate::{Error, Result};

/// The variable that points every Airboss process at another database file.
pub const DB_VAR: &str = "AIRBOSS_DB";

/// The database file: `AIRBOSS_DB` if it's set and not blank, otherwise
/// `~/Library/Application Support/Airboss/airboss.db`.
///
/// Without either, there's no sensible place: a relative path would put the tasks wherever
/// the process happened to start, so it's an error that says to set `AIRBOSS_DB`.
pub fn default_db_path() -> Result<PathBuf> {
    db_path_from(|name| std::env::var_os(name))
}

/// [`default_db_path`] over any environment, so tests don't touch the real one.
fn db_path_from(var: impl Fn(&str) -> Option<OsString>) -> Result<PathBuf> {
    let set = |name: &str| var(name).filter(|v| !v.to_string_lossy().trim().is_empty());
    if let Some(path) = set(DB_VAR) {
        return Ok(PathBuf::from(path));
    }
    let home = set("HOME").ok_or_else(|| {
        Error::cant_open(format!(
            "HOME isn't set, so there's no default database; set {DB_VAR}"
        ))
    })?;
    Ok(PathBuf::from(home).join("Library/Application Support/Airboss/airboss.db"))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    fn env(pairs: &'static [(&'static str, &'static str)]) -> impl Fn(&str) -> Option<OsString> {
        move |name| {
            pairs
                .iter()
                .find(|(k, _)| *k == name)
                .map(|(_, v)| OsString::from(v))
        }
    }

    #[test]
    fn airboss_db_overrides_the_default_path() {
        let path = db_path_from(env(&[(DB_VAR, "/tmp/x/airboss.db"), ("HOME", "/Users/me")]));
        assert_eq!(path.unwrap(), PathBuf::from("/tmp/x/airboss.db"));
    }

    #[test]
    fn the_default_path_is_in_application_support() {
        let path = db_path_from(env(&[("HOME", "/Users/me")]));
        assert_eq!(
            path.unwrap(),
            PathBuf::from("/Users/me/Library/Application Support/Airboss/airboss.db")
        );
    }

    #[test]
    fn a_blank_override_is_ignored() {
        let path = db_path_from(env(&[(DB_VAR, "  "), ("HOME", "/Users/me")]));
        assert!(path.unwrap().starts_with("/Users/me"));
    }

    #[test]
    fn no_home_and_no_override_is_an_error_never_a_relative_path() {
        for vars in [&[][..], &[("HOME", "")][..]] {
            let err = db_path_from(env(vars)).unwrap_err();
            assert!(err.to_string().contains(DB_VAR), "{err}");
            assert!(!err.is_newer_database());
        }
    }
}
