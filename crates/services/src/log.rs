//! The services' log: one line at a time, to wherever the app sends its own log.
//!
//! The service crates have no logging crate. Each line goes to stderr until the app installs a
//! sink, which it does at start-up so the lines reach `~/Library/Logs/Airboss/app.log`: an app
//! opened from Finder has stderr on `/dev/null`.

use std::sync::OnceLock;

/// Where a line goes: `source` names the service (`"airboss-sync"`), `message` is one line.
pub type Sink = fn(source: &str, message: &str);

static SINK: OnceLock<Sink> = OnceLock::new();

/// Sends every later line to `sink`. The first sink stays; `false` if one was already set.
pub fn set_sink(sink: Sink) -> bool {
    SINK.set(sink).is_ok()
}

/// Whether a sink is installed.
pub fn has_sink() -> bool {
    SINK.get().is_some()
}

/// Logs one line from `source`: to the sink, or to stderr before there is one.
pub fn line(source: &str, message: &str) {
    match SINK.get() {
        Some(sink) => sink(source, message),
        None => eprintln!("{source}: {message}"),
    }
}
