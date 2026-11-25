//! The `PATH` the user's own shell sets, for "~/.local/bin isn't on your PATH yet".
//!
//! The app's own `PATH` is launchd's, so it never has `~/.local/bin`. A login shell (`-l`)
//! reads `~/.zprofile` but not `~/.zshrc`, which is where the docs say to add the line, so the
//! check runs an interactive login shell (`-ilc`), as Terminal does. Interactive startup files
//! can print anything and can hang, so the `PATH` is read after a marker, the read has a
//! deadline, and the shell is killed if it's still running then and always reaped.

use std::ffi::OsStr;
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

/// How long the user's startup files get.
pub const DEADLINE: Duration = Duration::from_secs(3);

const MARKER: &str = "__airboss_path__=";

/// The `PATH` that `shell -ilc` ends up with, or `None` if it didn't say within `deadline`.
/// `envs` are set for the shell (tests point `ZDOTDIR` at a throwaway folder).
pub fn login_shell_path(
    shell: &Path,
    envs: &[(&str, &OsStr)],
    deadline: Duration,
) -> Option<String> {
    let mut child = Command::new(shell)
        .args(["-ilc", &format!("printf '\\n{MARKER}%s\\n' \"$PATH\"")])
        .envs(envs.iter().copied())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let stdout = child.stdout.take()?;
    // Lines arrive on a thread of their own, so the deadline holds even if a startup file
    // never ends, or leaves something running that keeps the pipe open.
    let (tx, rx) = mpsc::channel();
    let reader = std::thread::Builder::new()
        .name("airboss-login-path".into())
        .spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { return };
                if let Some(path) = line.strip_prefix(MARKER) {
                    let _ = tx.send(path.to_string());
                    return;
                }
            }
        });
    let found = match reader {
        Ok(_) => {
            let until = Instant::now() + deadline;
            rx.recv_timeout(until.saturating_duration_since(Instant::now()))
                .ok()
        }
        Err(_) => None,
    };
    // Reap the shell: kill it if it's still going (a hung startup file), then wait for it.
    if !matches!(child.try_wait(), Ok(Some(_))) {
        let _ = child.kill();
    }
    let _ = child.wait();
    found
}

/// Whether `dir` is on the `PATH` of the user's shell (`$SHELL`, or zsh).
pub fn on_login_path(shell: &Path, dir: &Path) -> bool {
    login_shell_path(shell, &[], DEADLINE)
        .is_some_and(|path| std::env::split_paths(&path).any(|p| p == dir))
}

/// The user's shell: `$SHELL`, or zsh, macOS's default.
pub fn user_shell() -> std::path::PathBuf {
    std::env::var_os("SHELL")
        .filter(|s| !s.is_empty())
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| "/bin/zsh".into())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    /// A throwaway `ZDOTDIR` with one startup file, so the user's own are never read.
    fn zdotdir(name: &str, file: &str, text: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("airboss-zdot-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(file), text).unwrap();
        dir
    }

    fn zsh_path(zdot: &Path, deadline: Duration) -> Option<String> {
        login_shell_path(
            Path::new("/bin/zsh"),
            &[("ZDOTDIR", zdot.as_os_str())],
            deadline,
        )
    }

    /// The final audit's should-fix 10: the docs say to add the line to ~/.zshrc, which a
    /// plain login shell (`-lc`) never reads.
    #[test]
    fn a_path_added_in_zshrc_counts() {
        let zdot = zdotdir(
            "rc",
            ".zshrc",
            "echo 'Welcome back!'\nexport PATH=\"$PATH:/opt/airboss-test/bin\"\n",
        );
        let path = zsh_path(&zdot, Duration::from_secs(10)).unwrap();
        assert!(
            std::env::split_paths(&path).any(|p| p == Path::new("/opt/airboss-test/bin")),
            "{path}"
        );
        let _ = std::fs::remove_dir_all(&zdot);
    }

    #[test]
    fn a_path_added_in_zprofile_counts_too() {
        let zdot = zdotdir(
            "profile",
            ".zprofile",
            "export PATH=\"/opt/airboss-profile/bin:$PATH\"\n",
        );
        let path = zsh_path(&zdot, Duration::from_secs(10)).unwrap();
        assert!(path.split(':').any(|p| p == "/opt/airboss-profile/bin"));
        let _ = std::fs::remove_dir_all(&zdot);
    }

    #[test]
    fn a_startup_file_that_hangs_gives_up_at_the_deadline() {
        let zdot = zdotdir("hang", ".zshrc", "sleep 30\n");
        let started = Instant::now();
        assert_eq!(zsh_path(&zdot, Duration::from_millis(600)), None);
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "{:?}",
            started.elapsed()
        );
        let _ = std::fs::remove_dir_all(&zdot);
    }

    #[test]
    fn a_shell_that_isnt_there_is_not_on_the_path() {
        assert!(!on_login_path(
            Path::new("/nonexistent/airboss-shell"),
            Path::new("/usr/bin")
        ));
    }
}
