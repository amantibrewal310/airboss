//! The app's settings: `app-settings.json` next to the database, so a temporary `AIRBOSS_DB`
//! isolates a dev run's settings too.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use airboss_core::TaskId;
use airboss_services::{Availability, HerdrAgent, Launcher};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::errors::{AppError, ErrorCode};

/// The default quick-add hotkey: ⌃⌥N (not a macOS default; see SPEC-app "Conflicts resolved").
pub const DEFAULT_HOTKEY: &str = "Control+Alt+N";

/// System, Dark or Light. System (the default) follows macOS.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export_to = "types.ts")]
pub enum Appearance {
    #[default]
    System,
    Dark,
    Light,
}

/// Settings › Agent setup › "Start agents in": where Do it and Plan it start agents.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export_to = "types.ts")]
pub enum StartAgentsIn {
    /// T3 Code when it's Ready, else Herdr when it's Ready, else T3 Code (SPEC-herdr
    /// Decision 1).
    #[default]
    Auto,
    T3,
    Herdr,
}

/// Everything Settings (⌘,) changes, plus the folded parents.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(default)]
#[ts(export_to = "types.ts")]
pub struct Settings {
    /// The quick-add hotkey as an accelerator, e.g. `"Control+Alt+N"`.
    pub hotkey: String,
    pub open_at_login: bool,
    pub appearance: Appearance,
    /// macOS notifications for an agent's question or approval, a failed run and an agent
    /// that stopped. On by default.
    pub notifications: bool,
    /// Parents folded in the tree; saved a second after the last change.
    #[ts(as = "Vec<crate::dto::TaskIdTs>")]
    pub folded: Vec<TaskId>,
    /// Where Do it and Plan it start agents.
    pub start_agents_in: StartAgentsIn,
    /// The agent a Herdr tab runs: Claude Code (the default) or Codex.
    #[ts(as = "crate::dto::HerdrAgentTs")]
    pub herdr_agent: HerdrAgent,
    /// Release the hidden quick-add panel after this many idle minutes (off by default; see
    /// SPEC-app "Memory"). Not shown in the UI.
    #[ts(skip)]
    pub quickadd_release_after_min: Option<u32>,
}

impl Default for Settings {
    fn default() -> Settings {
        Settings {
            hotkey: DEFAULT_HOTKEY.to_string(),
            open_at_login: true,
            appearance: Appearance::System,
            notifications: true,
            folded: Vec::new(),
            start_agents_in: StartAgentsIn::Auto,
            herdr_agent: HerdrAgent::Claude,
            quickadd_release_after_min: None,
        }
    }
}

impl Settings {
    /// The launcher Do it and Plan it use now, from "Start agents in" and what's Ready
    /// (SPEC-herdr Decision 1). Automatic is T3 Code when it's Ready, else Herdr when it's
    /// Ready, else T3 Code, whose error then says what's missing. T3 Code not checked yet
    /// counts as Ready, as the buttons treat it, so a T3 user never starts in Herdr by
    /// pressing Do it in the app's first moment.
    pub fn launcher(&self, t3: &Availability, herdr: &Availability) -> Launcher {
        let herdr_launcher = Launcher::Herdr {
            agent: self.herdr_agent,
        };
        match self.start_agents_in {
            StartAgentsIn::T3 => Launcher::T3,
            StartAgentsIn::Herdr => herdr_launcher,
            StartAgentsIn::Auto => {
                let t3_missing = matches!(t3, Availability::Missing { .. });
                let herdr_ready = matches!(herdr, Availability::Ready { .. });
                if t3_missing && herdr_ready {
                    herdr_launcher
                } else {
                    Launcher::T3
                }
            }
        }
    }
}

/// Whether a first run turns on open at login: only inside an `.app` bundle (never for a dev
/// binary), and only on the default database. The LaunchAgent can't carry `AIRBOSS_DB`, so one
/// registered by a run on another database (QA, `measure.sh`) would open the real database at
/// the next login, from whichever build registered it.
pub fn open_at_login_on_first_run(in_bundle: bool, db_overridden: bool) -> bool {
    in_bundle && !db_overridden
}

/// Where the settings file lives for a database at `db`.
pub fn path_for(db: &Path) -> PathBuf {
    db.with_file_name("app-settings.json")
}

/// Reads the settings. `first_run` is true when there was no file yet; a file that can't be
/// read is replaced by the defaults (and logged), never a reason not to start.
pub fn load(path: &Path) -> (Settings, bool) {
    match fs::read(path) {
        Ok(bytes) => match serde_json::from_slice(&bytes) {
            Ok(s) => (s, false),
            Err(e) => {
                tauri_plugin_log::log::warn!("settings unreadable, using defaults: {e}");
                (Settings::default(), false)
            }
        },
        Err(e) if e.kind() == io::ErrorKind::NotFound => (Settings::default(), true),
        Err(e) => {
            tauri_plugin_log::log::warn!("settings unreadable, using defaults: {e}");
            (Settings::default(), false)
        }
    }
}

/// Writes the settings through a temporary file, so a crash never leaves half a file.
pub fn save(path: &Path, s: &Settings) -> io::Result<()> {
    let text = serde_json::to_vec_pretty(s).map_err(io::Error::other)?;
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, text)?;
    fs::rename(&tmp, path)
}

/// The modifiers and key of an accelerator, in a fixed form for comparing.
fn parts(accel: &str) -> (Vec<&'static str>, String) {
    let mut mods = Vec::new();
    let mut key = String::new();
    for part in accel.split('+').map(str::trim) {
        match part.to_ascii_lowercase().as_str() {
            "control" | "ctrl" => mods.push("ctrl"),
            "alt" | "option" => mods.push("alt"),
            "shift" => mods.push("shift"),
            "super" | "cmd" | "command" | "commandorcontrol" | "cmdorctrl" | "meta" => {
                mods.push("cmd")
            }
            other => {
                key = other
                    .strip_prefix("key")
                    .filter(|k| k.len() == 1)
                    .unwrap_or(other)
                    .to_string()
            }
        }
    }
    mods.sort_unstable();
    mods.dedup();
    (mods, key)
}

/// Refuses the keys macOS uses by default (SPEC-app Settings): ⌃Space, ⌃⌥Space, ⌘Space,
/// ⌥⌘Space and ⌃⌘Space. A key with no modifier is refused too: it would swallow typing.
pub fn check_hotkey(accel: &str) -> Result<(), AppError> {
    let (mods, key) = parts(accel);
    if key.is_empty() || mods.is_empty() || mods == ["shift"] {
        return Err(AppError::new(
            ErrorCode::HotkeyReserved,
            "A hotkey needs ⌃, ⌥ or ⌘ and one other key.",
        ));
    }
    let reserved: [&[&str]; 5] = [
        &["ctrl"],
        &["alt", "ctrl"],
        &["cmd"],
        &["alt", "cmd"],
        &["cmd", "ctrl"],
    ];
    if key == "space" && reserved.contains(&mods.as_slice()) {
        return Err(AppError::new(
            ErrorCode::HotkeyReserved,
            "macOS uses that key for Spotlight or input sources. Pick another.",
        ));
    }
    Ok(())
}

/// The hotkey as menus show it: "⌃⌥N".
pub fn hotkey_label(accel: &str) -> String {
    let (mods, key) = parts(accel);
    let mut out = String::new();
    // macOS order: ⌃ ⌥ ⇧ ⌘.
    for (m, sym) in [("ctrl", "⌃"), ("alt", "⌥"), ("shift", "⇧"), ("cmd", "⌘")] {
        if mods.contains(&m) {
            out.push_str(sym);
        }
    }
    let key = match key.as_str() {
        "space" => "Space".to_string(),
        "enter" | "return" => "↩".to_string(),
        k => k.to_ascii_uppercase(),
    };
    out.push_str(&key);
    out
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    #[test]
    fn a_first_run_turns_on_open_at_login_only_for_the_installed_app_on_its_own_database() {
        assert!(open_at_login_on_first_run(true, false));
        // A LaunchAgent can't carry AIRBOSS_DB: one registered by a run on a temporary
        // database (QA, measure.sh) would open the real database at the next login.
        assert!(!open_at_login_on_first_run(true, true));
        assert!(!open_at_login_on_first_run(false, false));
        assert!(!open_at_login_on_first_run(false, true));
    }

    #[test]
    fn defaults_are_ctrl_alt_n_open_at_login_and_system() {
        let s = Settings::default();
        assert_eq!(s.hotkey, "Control+Alt+N");
        assert!(s.open_at_login);
        assert_eq!(s.appearance, Appearance::System);
        assert!(s.notifications, "notifications are on by default");
    }

    #[test]
    fn a_missing_file_is_a_first_run_and_a_saved_one_round_trips() {
        let dir = std::env::temp_dir().join(format!("airboss-settings-{}", std::process::id()));
        let path = dir.join("app-settings.json");
        let _ = fs::remove_file(&path);
        assert_eq!(load(&path), (Settings::default(), true));
        let s = Settings {
            hotkey: "Control+Alt+K".into(),
            open_at_login: false,
            appearance: Appearance::Dark,
            notifications: false,
            folded: vec![TaskId(3)],
            start_agents_in: StartAgentsIn::Herdr,
            herdr_agent: HerdrAgent::Codex,
            quickadd_release_after_min: Some(10),
        };
        save(&path, &s).unwrap();
        assert_eq!(load(&path), (s, false));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_partial_file_fills_in_defaults() {
        let s: Settings = serde_json::from_str(r#"{"appearance":"light"}"#).unwrap();
        assert_eq!(s.appearance, Appearance::Light);
        assert_eq!(s.hotkey, DEFAULT_HOTKEY);
        assert!(
            s.notifications,
            "a file from before notifications turns them on"
        );
    }

    #[test]
    fn a_file_from_before_herdr_starts_agents_automatically_with_claude_code() {
        let old = r#"{"hotkey":"Control+Alt+K","open_at_login":false,"appearance":"dark","notifications":true,"folded":[3]}"#;
        let s: Settings = serde_json::from_str(old).unwrap();
        assert_eq!(s.hotkey, "Control+Alt+K");
        assert_eq!(s.start_agents_in, StartAgentsIn::Auto);
        assert_eq!(s.herdr_agent, HerdrAgent::Claude);
        let text = serde_json::to_value(Settings::default()).unwrap();
        assert_eq!(text["start_agents_in"], "auto");
        assert_eq!(text["herdr_agent"], "claude");
        let s: Settings =
            serde_json::from_str(r#"{"start_agents_in":"herdr","herdr_agent":"codex"}"#).unwrap();
        assert_eq!(
            (s.start_agents_in, s.herdr_agent),
            (StartAgentsIn::Herdr, HerdrAgent::Codex)
        );
    }

    #[test]
    fn automatic_picks_t3_when_ready_else_herdr_when_ready_else_t3() {
        let ready = || Availability::Ready {
            version: "1".into(),
        };
        let missing = || Availability::Missing {
            reason: "gone".into(),
        };
        let unknown = || Availability::Unknown;
        let claude = Launcher::Herdr {
            agent: HerdrAgent::Claude,
        };
        let auto = Settings::default();
        for (t3, herdr, want) in [
            (ready(), ready(), Launcher::T3),
            (ready(), missing(), Launcher::T3),
            (missing(), ready(), claude),
            (missing(), missing(), Launcher::T3),
            (missing(), unknown(), Launcher::T3),
            // Not checked yet counts as Ready, as the buttons treat it.
            (unknown(), ready(), Launcher::T3),
        ] {
            assert_eq!(
                auto.launcher(&t3, &herdr),
                want,
                "T3 {t3:?}, Herdr {herdr:?}"
            );
        }
        // A choice is kept whatever is Ready: its own error says what's missing.
        let t3 = Settings {
            start_agents_in: StartAgentsIn::T3,
            ..Settings::default()
        };
        assert_eq!(t3.launcher(&missing(), &ready()), Launcher::T3);
        let codex = Settings {
            start_agents_in: StartAgentsIn::Herdr,
            herdr_agent: HerdrAgent::Codex,
            ..Settings::default()
        };
        assert_eq!(
            codex.launcher(&ready(), &missing()),
            Launcher::Herdr {
                agent: HerdrAgent::Codex
            }
        );
    }

    #[test]
    fn macos_defaults_and_bare_keys_are_refused() {
        for k in [
            "Control+Space",
            "Control+Alt+Space",
            "Super+Space",
            "Alt+Super+Space",
            "Control+Super+Space",
            "N",
            "Shift+N",
        ] {
            assert!(check_hotkey(k).is_err(), "{k}");
        }
        for k in ["Control+Alt+N", "Alt+Space", "Shift+Super+K"] {
            assert!(check_hotkey(k).is_ok(), "{k}");
        }
    }

    #[test]
    fn labels_use_mac_symbols_in_mac_order() {
        assert_eq!(hotkey_label("Control+Alt+N"), "⌃⌥N");
        assert_eq!(hotkey_label("Super+Shift+KeyK"), "⇧⌘K");
        assert_eq!(hotkey_label("Alt+Space"), "⌥Space");
    }
}
