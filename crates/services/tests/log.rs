//! The services' one log line: to stderr until the app installs its sink, then to the sink.
#![allow(clippy::unwrap_used)]

use std::sync::Mutex;

use airboss_services::log;

static SEEN: Mutex<Vec<(String, String)>> = Mutex::new(Vec::new());

fn capture(source: &str, message: &str) {
    SEEN.lock().unwrap().push((source.into(), message.into()));
}

#[test]
fn a_line_goes_to_the_installed_sink_and_the_first_sink_stays() {
    // Before a sink: stderr, and nothing captured.
    log::line("airboss-sync", "before the sink");
    assert!(SEEN.lock().unwrap().is_empty());
    assert!(!log::has_sink());

    assert!(log::set_sink(capture));
    assert!(log::has_sink());
    fn other(_: &str, _: &str) {
        panic!("a second sink must not replace the first");
    }
    assert!(!log::set_sink(other));

    log::line("airboss-sync", "Claude sync broken: ps failed");
    log::line("airboss-launch", "pending launch 3 recovered");
    assert_eq!(
        *SEEN.lock().unwrap(),
        vec![
            (
                "airboss-sync".into(),
                "Claude sync broken: ps failed".into()
            ),
            ("airboss-launch".into(), "pending launch 3 recovered".into()),
        ]
    );
}
