//! Herdr, from the app's side: showing a terminal (Open thread). Starting and following agents
//! there are `launch`'s and `sync`'s. Only `herdr-app do agent.focus` runs here, argv only,
//! never through a shell.

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Herdr.app's bundle id, for `open -b`.
pub const BUNDLE_ID: &str = "dev.herdr.app";
/// Open thread's toast when Herdr couldn't show the terminal.
pub const MSG_SHOW_FAILED: &str = "Couldn't show that terminal in Herdr.";
/// How long one `herdr-app` call may take. Both answer in milliseconds when the window is open.
pub const TIMEOUT: Duration = Duration::from_secs(5);

/// `herdr-app`: `~/.local/bin/herdr-app` (where Herdr installs it), then the app's `PATH`.
pub fn find_herdr_app(home: &Path, path: Option<&OsStr>) -> Option<PathBuf> {
    std::iter::once(home.join(".local/bin/herdr-app"))
        .chain(path.into_iter().flat_map(|p| {
            std::env::split_paths(p)
                .map(|d| d.join("herdr-app"))
                .collect::<Vec<_>>()
        }))
        .find(|p| runnable(p))
}

/// An executable file at `p`.
fn runnable(p: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(p).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}

/// `herdr-app`'s arguments to show terminal `terminal`: `do agent.focus {"terminal":…}`, the
/// object as one argument, so the address is never read by a shell.
pub fn show_args(terminal: &str) -> Vec<String> {
    vec![
        "do".into(),
        "agent.focus".into(),
        serde_json::json!({ "terminal": terminal }).to_string(),
    ]
}

/// Runs `herdr-app` with `args`: its stdout, or why it failed (`herdr-app` prints
/// `{"error": …}` and exits 1 when a command fails).
pub fn run(home: &Path, args: &[String]) -> Result<String, String> {
    let app_path = std::env::var_os("PATH");
    let program = find_herdr_app(home, app_path.as_deref())
        .ok_or_else(|| "herdr-app isn't installed".to_string())?;
    let env: Vec<(OsString, OsString)> = airboss_launch::child_env(std::env::vars_os());
    let out = airboss_launch::ProcessRunner::new(program, home.to_path_buf(), env)
        .run(args, None, TIMEOUT)
        .map_err(|e| format!("couldn't run herdr-app: {e}"))?;
    if out.status == Some(0) {
        Ok(out.stdout)
    } else {
        let said = [&out.stdout, &out.stderr]
            .iter()
            .flat_map(|t| t.lines())
            .map(str::trim)
            .find(|l| !l.is_empty())
            .unwrap_or("no output")
            .to_string();
        Err(format!("herdr-app exited with {:?}: {said}", out.status))
    }
}

/// Shows terminal `terminal` in Herdr, then brings Herdr forward (`open -b dev.herdr.app`).
pub fn show_terminal(home: &Path, terminal: &str) -> Result<(), String> {
    run(home, &show_args(terminal))?;
    let out = airboss_launch::ProcessRunner::new(
        PathBuf::from("/usr/bin/open"),
        home.to_path_buf(),
        airboss_launch::child_env(std::env::vars_os()),
    )
    .run(&["-b".into(), BUNDLE_ID.into()], None, TIMEOUT)
    .map_err(|e| format!("couldn't bring Herdr forward: {e}"))?;
    match out.status {
        Some(0) => Ok(()),
        s => Err(format!("open -b {BUNDLE_ID} exited with {s:?}")),
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    #[test]
    fn the_terminal_goes_as_one_json_argument() {
        assert_eq!(
            show_args("t4p"),
            vec!["do", "agent.focus", r#"{"terminal":"t4p"}"#]
        );
        // Whatever the ref holds stays inside the one JSON string.
        let odd = show_args("a\"; rm -rf ~ #");
        assert_eq!(odd.len(), 3);
        let v: serde_json::Value = serde_json::from_str(&odd[2]).unwrap();
        assert_eq!(v["terminal"], "a\"; rm -rf ~ #");
    }

    #[test]
    fn herdr_app_is_found_in_local_bin_first_then_on_path() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("airboss-herdr-find-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let home = dir.join("home");
        let other = dir.join("bin");
        for d in [home.join(".local/bin"), other.clone()] {
            std::fs::create_dir_all(&d).unwrap();
        }
        let path = std::env::join_paths([&other]).unwrap();
        assert_eq!(find_herdr_app(&home, Some(&path)), None);
        let on_path = other.join("herdr-app");
        std::fs::write(&on_path, "#!/bin/sh\n").unwrap();
        std::fs::set_permissions(&on_path, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(find_herdr_app(&home, Some(&path)), Some(on_path));
        let local = home.join(".local/bin/herdr-app");
        std::fs::write(&local, "#!/bin/sh\n").unwrap();
        // Not executable: skipped.
        assert_ne!(find_herdr_app(&home, Some(&path)), Some(local.clone()));
        std::fs::set_permissions(&local, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(find_herdr_app(&home, Some(&path)), Some(local));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
