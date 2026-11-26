//! The version, everywhere it's written down, agrees with the workspace's `Cargo.toml`, so a
//! release can't ship with two numbers (CONTRIBUTING.md, Releasing).

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::Path;

const VERSION: &str = env!("CARGO_PKG_VERSION");

fn read(path: &str) -> String {
    std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join(path)).unwrap()
}

fn json(path: &str) -> serde_json::Value {
    serde_json::from_str(&read(path)).unwrap()
}

/// `0.1.0` from `0.1.0-alpha.3`.
fn release() -> String {
    format!(
        "{}.{}.{}",
        env!("CARGO_PKG_VERSION_MAJOR"),
        env!("CARGO_PKG_VERSION_MINOR"),
        env!("CARGO_PKG_VERSION_PATCH")
    )
}

#[test]
fn the_tauri_config_and_package_json_carry_the_crate_version() {
    assert_eq!(json("tauri.conf.json")["version"], VERSION);
    assert_eq!(json("../package.json")["version"], VERSION);
}

#[test]
fn info_plist_gives_apple_three_numbers() {
    // Apple: CFBundleShortVersionString is three period-separated integers.
    let plist = read("Info.plist");
    let squashed: String = plist.split_whitespace().collect();
    let want = format!(
        "<key>CFBundleShortVersionString</key><string>{}</string>",
        release()
    );
    assert!(
        squashed.contains(&want),
        "Info.plist's CFBundleShortVersionString should be {}",
        release()
    );
}

#[test]
fn the_build_number_is_a_whole_number_and_an_alpha_uses_its_own() {
    let conf = json("tauri.conf.json");
    let build = conf["bundle"]["macOS"]["bundleVersion"].as_str().unwrap();
    let n: u32 = build.parse().expect("CFBundleVersion is a whole number");
    assert!(n > 0);
    if let Some(alpha) = env!("CARGO_PKG_VERSION_PRE").strip_prefix("alpha.") {
        assert_eq!(build, alpha, "alpha N is build N");
    }
}

#[test]
fn the_changelog_has_this_version() {
    let log = read("../../CHANGELOG.md");
    assert!(
        log.contains(&format!("## [{VERSION}] - ")),
        "CHANGELOG.md has no section for {VERSION}"
    );
    assert!(log.contains(&format!("[{VERSION}]: https://github.com/")));
}

/// Standard base64, for the updater's public key.
fn base64(text: &str) -> Vec<u8> {
    const ABC: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut bits = 0u32;
    let mut n = 0;
    let mut out = Vec::new();
    for c in text.bytes().filter(|c| *c != b'=') {
        let v = ABC.iter().position(|a| *a == c).expect("base64") as u32;
        bits = (bits << 6) | v;
        n += 6;
        if n >= 8 {
            n -= 8;
            out.push((bits >> n) as u8);
        }
    }
    out
}

#[test]
fn the_updater_reads_the_manifest_on_main_and_checks_airboss_own_key() {
    // SPEC-updates decisions 2 and 3: one compiled-in https endpoint, and a minisign key.
    let updater = &json("tauri.conf.json")["plugins"]["updater"];
    assert_eq!(
        updater["endpoints"],
        serde_json::json!([
            "https://raw.githubusercontent.com/amantibrewal310/airboss/main/updates/latest.json"
        ])
    );
    let key = String::from_utf8(base64(updater["pubkey"].as_str().unwrap())).unwrap();
    let lines: Vec<&str> = key.lines().collect();
    assert_eq!(lines.len(), 2, "{key}");
    assert!(
        lines[0].starts_with("untrusted comment: minisign public key: "),
        "{key}"
    );
    // An Ed25519 minisign key: "Ed", an 8-byte key id and 32 bytes of key.
    let raw = base64(lines[1]);
    assert_eq!(raw.len(), 42);
    assert_eq!(&raw[..2], b"Ed");
    // The release build's config doesn't move the endpoint or the key.
    assert!(json("tauri.bundle.conf.json").get("plugins").is_none());
}
