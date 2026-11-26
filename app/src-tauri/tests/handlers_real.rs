//! The handler check on this Mac, for real: LaunchServices and the Security framework. Ignored
//! by default, because it needs T3 Code installed; run with
//! `cargo test -p airboss-app --test handlers_real -- --ignored`.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use airboss_app::handlers::{AppCheck, BROWSER_REQUIREMENT, MacAppCheck, T3_REQUIREMENT};

/// A bundle like the audit's `Evil.app`: T3 Code's bundle id and the `t3code` scheme, with a
/// shell script for its executable. Never shown in Finder, so never registered.
fn fake_t3(dir: &Path, sign: bool) -> std::path::PathBuf {
    let app = dir.join("Evil.app");
    std::fs::create_dir_all(app.join("Contents/MacOS")).unwrap();
    std::fs::write(
        app.join("Contents/Info.plist"),
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleIdentifier</key><string>com.t3tools.t3code</string>
<key>CFBundleExecutable</key><string>evil</string>
<key>CFBundleVersion</key><string>999</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>CFBundleURLTypes</key><array><dict><key>CFBundleURLSchemes</key><array><string>t3code</string></array></dict></array>
</dict></plist>"#,
    )
    .unwrap();
    let exe = app.join("Contents/MacOS/evil");
    std::fs::write(&exe, "#!/bin/sh\nopen -a Calculator\n").unwrap();
    std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).unwrap();
    if sign {
        let signed = std::process::Command::new("/usr/bin/codesign")
            .args(["-s", "-", "--force"])
            .arg(&app)
            .output()
            .unwrap();
        assert!(signed.status.success(), "{signed:?}");
    }
    app
}

#[test]
#[ignore = "needs T3 Code installed; checks this Mac's real signatures"]
fn the_installed_t3_code_passes_and_a_fresh_lookalike_fails() {
    let check = MacAppCheck;
    let t3 = check
        .handler("t3code://threads/env/abc")
        .expect("T3 Code handles t3code links");
    assert!(t3.to_string_lossy().contains("T3 Code"), "{}", t3.display());
    assert!(check.satisfies(&t3, T3_REQUIREMENT), "{}", t3.display());

    for sign in [false, true] {
        let dir = common::temp_dir(if sign { "evil-adhoc" } else { "evil-unsigned" });
        let evil = fake_t3(&dir, sign);
        assert!(
            !check.satisfies(&evil, T3_REQUIREMENT),
            "signed ad hoc: {sign}"
        );
        assert!(
            !check.satisfies(&evil, BROWSER_REQUIREMENT),
            "signed ad hoc: {sign}"
        );
    }
    // A requirement that doesn't parse never passes.
    assert!(!check.satisfies(&t3, "identifier and and"));
}

#[test]
#[ignore = "checks this Mac's real default browser"]
fn the_default_browser_is_apple_signed() {
    let check = MacAppCheck;
    let browser = check.handler("https://example.com/").expect("a browser");
    assert!(
        check.satisfies(&browser, BROWSER_REQUIREMENT),
        "{}",
        browser.display()
    );
}
