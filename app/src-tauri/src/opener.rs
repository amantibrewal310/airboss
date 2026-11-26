//! The opener's allowlist (SPEC-app "Opening links"). Agents can store any URL through MCP
//! `link`, so a link reaches macOS's `open` only in one of three shapes:
//!
//! - `https://host/…`: the browser.
//! - `t3code://threads/<environment>/<thread>`: T3 Code, with both ids from a short alphabet.
//! - `file:///absolute/path`, with no host, query or fragment: the path is percent-decoded
//!   (and must be UTF-8), resolved (symlinks and `..`), and must exist. Only a plain file of a
//!   document type on [`OPENS`] opens, and only in a fixed system viewer (TextEdit or Preview),
//!   never the app LaunchServices would pick: a Finder alias named like a document, a per-file
//!   "Open With" attribute or a hijacked default handler would make that pick an app.
//!   Everything else, folders, bundles, apps, scripts, executables and files with no
//!   extension, is shown in Finder instead, because opening it could run it.
//!
//! Anything else is refused with Appendix D's "Airboss only opens https links, files and T3
//! threads."

use std::ffi::OsStr;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use crate::errors::{
    AppError, ErrorCode, MSG_FILE_GONE, MSG_FILE_UNREADABLE, MSG_NOT_A_FILE, MSG_URL_NOT_ALLOWED,
};

/// What to do with a link that passed the allowlist.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Opening {
    /// An `https` URL, for the browser.
    Browser(String),
    /// A `t3code://threads/…` URL, rebuilt from its checked parts.
    Thread(String),
    /// A document to open: the resolved path, in a fixed system viewer.
    Open(PathBuf, Viewer),
    /// Anything else on disk, shown in Finder: the resolved path.
    Reveal(PathBuf),
}

/// The document types that open. Each one's default macOS app shows it and runs nothing: no
/// HTML or SVG (a browser would run its scripts), and no script a launcher would run (`.py`
/// opens Python Launcher, `.command` opens Terminal).
#[rustfmt::skip]
pub const OPENS: &[&str] = &[
    // Text and notes.
    "md", "markdown", "txt", "text", "rtf", "log", "csv", "tsv", "json", "jsonl", "yaml", "yml",
    "toml", "xml", "ini", "cfg", "conf", "diff", "patch",
    // Documents and images.
    "pdf", "png", "jpg", "jpeg", "gif", "webp", "heic", "heif", "tif", "tiff", "bmp",
    // Source code.
    "rs", "ts", "tsx", "js", "jsx", "mjs", "cjs", "svelte", "vue", "css", "scss", "go", "java",
    "kt", "swift", "c", "h", "cc", "cpp", "hpp", "sql", "graphql", "proto",
];

/// The system app a document opens in. Both live on the sealed system volume, and are named
/// by path, so `open -a` can't land on a lookalike an agent registered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Viewer {
    /// Text, notes and code, shown as text (RTF as rich text).
    TextEdit,
    /// PDFs and images.
    Preview,
}

impl Viewer {
    /// The viewer for a document extension on [`OPENS`] (any case); `None` for anything else.
    pub fn for_extension(ext: &str) -> Option<Viewer> {
        let ext = ext.to_ascii_lowercase();
        if !OPENS.contains(&ext.as_str()) {
            return None;
        }
        Some(match ext.as_str() {
            "pdf" | "png" | "jpg" | "jpeg" | "gif" | "webp" | "heic" | "heif" | "tif" | "tiff"
            | "bmp" => Viewer::Preview,
            _ => Viewer::TextEdit,
        })
    }

    /// The app's path, for `open -a`.
    pub fn app(self) -> &'static Path {
        Path::new(match self {
            Viewer::TextEdit => "/System/Applications/TextEdit.app",
            Viewer::Preview => "/System/Applications/Preview.app",
        })
    }
}

fn refused() -> AppError {
    AppError::new(ErrorCode::UrlNotAllowed, MSG_URL_NOT_ALLOWED)
}

/// Decides what opening `url` does, or refuses it.
pub fn decide(url: &str) -> Result<Opening, AppError> {
    let url = url.trim();
    if url.chars().any(char::is_control) {
        return Err(refused());
    }
    let (scheme, rest) = url.split_once(':').ok_or_else(refused)?;
    let valid_scheme = scheme.starts_with(|c: char| c.is_ascii_alphabetic())
        && scheme
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'));
    if !valid_scheme {
        return Err(refused());
    }
    match scheme.to_ascii_lowercase().as_str() {
        "https" => https(rest).map(Opening::Browser),
        "t3code" => thread(rest).map(Opening::Thread),
        "file" => file(rest),
        _ => Err(refused()),
    }
}

/// `//host[:port]/…`: printable ASCII only, no user info (`https://github.com@evil.com`) and no
/// backslash (which browsers read as `/`).
fn https(rest: &str) -> Result<String, AppError> {
    let after = rest.strip_prefix("//").ok_or_else(refused)?;
    if !after.bytes().all(|b| b.is_ascii_graphic()) || after.contains('\\') {
        return Err(refused());
    }
    let authority = after.split(['/', '?', '#']).next().unwrap_or("");
    // A name or an `[IPv6]` literal, then an optional `:port`.
    let (host_ok, port) = match authority.strip_prefix('[') {
        Some(v6) => match v6.split_once(']') {
            Some((inside, port)) => (
                !inside.is_empty()
                    && inside
                        .bytes()
                        .all(|b| b.is_ascii_hexdigit() || matches!(b, b':' | b'.')),
                port,
            ),
            None => (false, ""),
        },
        None => {
            let (host, port) = authority
                .find(':')
                .map_or((authority, ""), |at| authority.split_at(at));
            (
                !host.is_empty()
                    && host
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-')),
                port,
            )
        }
    };
    let port_ok = port.is_empty()
        || port
            .strip_prefix(':')
            .is_some_and(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()));
    if !(host_ok && port_ok) {
        return Err(refused());
    }
    Ok(format!("https://{after}"))
}

/// One id in a `t3code` URL: letters, digits, `-`, `_` and `%XX` escapes (how `sync` writes
/// them), up to 256 bytes.
fn id_ok(s: &str, escapes: bool) -> bool {
    let b = s.as_bytes();
    if b.is_empty() || b.len() > 256 {
        return false;
    }
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            c if c.is_ascii_alphanumeric() || c == b'-' => i += 1,
            b'_' if escapes => i += 1,
            b'%' if escapes
                && b.get(i + 1).is_some_and(u8::is_ascii_hexdigit)
                && b.get(i + 2).is_some_and(u8::is_ascii_hexdigit) =>
            {
                i += 3
            }
            _ => return false,
        }
    }
    true
}

/// A thread id as T3 will read it, after percent-decoding: UTF-8, with no `/`, `?`, `#` or
/// `\\` (which would make it a path, a query or a fragment), no control byte, and not `.` or
/// `..`. Real ids, such as `mcp:launch:…` escaped by `sync` as `mcp%3Alaunch%3A…`, pass.
fn decoded_id_ok(s: &str) -> bool {
    let Some(bytes) = percent_decode(s) else {
        return false;
    };
    let Ok(id) = std::str::from_utf8(&bytes) else {
        return false;
    };
    !id.contains(['/', '?', '#', '\\']) && id != "." && id != ".."
}

/// `//threads/<environment>/<thread>` and nothing more. The environment is uuid-like
/// (letters, digits and `-`); the thread id may carry `_` and `%XX` escapes, judged again
/// once decoded ([`decoded_id_ok`]).
fn thread(rest: &str) -> Result<String, AppError> {
    let ids = rest.strip_prefix("//threads/").ok_or_else(refused)?;
    match ids.split('/').collect::<Vec<_>>().as_slice() {
        [env, thread] if id_ok(env, false) && id_ok(thread, true) && decoded_id_ok(thread) => {
            Ok(format!("t3code://threads/{env}/{thread}"))
        }
        _ => Err(refused()),
    }
}

/// `%XX` escapes decoded; `None` for a broken escape, or for a control byte (below 0x20, or
/// 0x7F) whether it was escaped or not: a newline or an escape sequence has no place in a
/// path or an id, and would reach `open` or T3 as a different string from the one checked.
fn percent_decode(s: &str) -> Option<Vec<u8>> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' {
            let hex = |at: usize| b.get(at).and_then(|c| (*c as char).to_digit(16));
            let (h, l) = (hex(i + 1)?, hex(i + 2)?);
            out.push((h * 16 + l) as u8);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    (!out.iter().any(|b| *b < 0x20 || *b == 0x7F)).then_some(out)
}

/// `///absolute/path` (an empty host): decoded, resolved, and sorted into open or reveal.
fn file(rest: &str) -> Result<Opening, AppError> {
    let path = rest.strip_prefix("//").ok_or_else(refused)?;
    if !path.starts_with('/') || path.contains(['?', '#']) {
        return Err(refused());
    }
    let bytes = percent_decode(path).ok_or_else(refused)?;
    // Not UTF-8: the opener would pass the path on changed (lossily), not as checked here.
    let text = std::str::from_utf8(&bytes).map_err(|_| refused())?;
    let path = Path::new(text);
    let real = path.canonicalize().map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => AppError::new(ErrorCode::NotFound, MSG_FILE_GONE),
        _ => AppError::new(ErrorCode::NotFound, MSG_FILE_UNREADABLE),
    })?;
    // A FIFO, a socket or a device (`/dev/stdin`) is neither opened nor shown.
    let kind = std::fs::metadata(&real)
        .map_err(|_| AppError::new(ErrorCode::NotFound, MSG_FILE_UNREADABLE))?
        .file_type();
    if !kind.is_file() && !kind.is_dir() {
        return Err(AppError::new(ErrorCode::NotFound, MSG_NOT_A_FILE));
    }
    Ok(match viewer(&real) {
        Some(v) => Opening::Open(real, v),
        None => Opening::Reveal(real),
    })
}

/// The viewer for a resolved path that's a plain, non-executable file of a type on [`OPENS`];
/// `None` for anything else, which is revealed instead.
pub fn viewer(real: &Path) -> Option<Viewer> {
    let meta = std::fs::metadata(real).ok()?;
    if !meta.is_file() || meta.permissions().mode() & 0o111 != 0 {
        return None;
    }
    Viewer::for_extension(real.extension().and_then(OsStr::to_str)?)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use std::os::unix::fs::{PermissionsExt, symlink};

    use super::*;
    use crate::classify::file_url;

    fn refused_code(url: &str) -> ErrorCode {
        decide(url).unwrap_err().code
    }

    #[test]
    fn https_opens_in_the_browser() {
        for (url, opened) in [
            (
                "https://github.com/acme/web/pull/1",
                "https://github.com/acme/web/pull/1",
            ),
            (
                "HTTPS://Example.com/Path?q=1#top",
                "https://Example.com/Path?q=1#top",
            ),
            ("https://localhost:8080/x", "https://localhost:8080/x"),
            ("https://[::1]:8080/", "https://[::1]:8080/"),
            (
                "  https://acme.slack.com/archives/C01\n",
                "https://acme.slack.com/archives/C01",
            ),
        ] {
            assert_eq!(
                decide(url).unwrap(),
                Opening::Browser(opened.into()),
                "{url}"
            );
        }
    }

    #[test]
    fn t3_links_must_be_a_thread_with_two_plain_ids() {
        for (url, opened) in [
            (
                "t3code://threads/fake-environment/abc",
                "t3code://threads/fake-environment/abc",
            ),
            (
                "T3Code://threads/0b8a1f9e-1d2c-4b3a/mcp%3Alaunch%3Aabc",
                "t3code://threads/0b8a1f9e-1d2c-4b3a/mcp%3Alaunch%3Aabc",
            ),
            (
                "t3code://threads/e/a%3Ab%20c_d",
                "t3code://threads/e/a%3Ab%20c_d",
            ),
        ] {
            assert_eq!(
                decide(url).unwrap(),
                Opening::Thread(opened.into()),
                "{url}"
            );
        }
    }

    /// Every form that never reaches `open`, including both tricks the audit proved.
    #[test]
    fn everything_else_is_refused() {
        for url in [
            // The audit's two: an executable behind a query, and a one-slash form whose
            // query holds the `://` the old check looked for.
            "file:///tmp/x.command?a",
            "file:/System/Applications/Calculator.app?://",
            // File URLs with a host, a query, a fragment or the wrong number of slashes.
            "file:/tmp/plan.md",
            "file:tmp/plan.md",
            "file://localhost/tmp/plan.md",
            "FILE://LOCALHOST/tmp/plan.md",
            "file://evil.example/tmp/plan.md",
            "file:///tmp/plan.md#top",
            "file:///tmp/plan.md?x=1",
            "file:///tmp/%ZZ.md",
            "file:///tmp/a%00.md",
            "file:///tmp/pl\nan.md",
            // Other schemes, plain http included.
            "http://example.com",
            "javascript:alert(1)",
            "data:text/html,<script>alert(1)</script>",
            "ssh://host",
            "smb://host/share",
            "vnc://host",
            "ftp://host/x",
            "x-apple.systempreferences:com.apple.preference.security",
            "applescript://com.apple.scripteditor?action=new&script=beep",
            "shortcuts://run-shortcut?name=x",
            "x-man-page://ls",
            "mailto:a@b.example",
            "/etc/passwd",
            "~/notes.md",
            "",
            "://x",
            // https that isn't a plain host, or isn't plain ASCII.
            "https:example.com",
            "https://",
            "https:///path",
            "https://github.com@evil.example/",
            "https://user:pw@evil.example/",
            "https://evil.example\\@github.com/",
            "https://exa mple.com/",
            "https://example.com:80x/",
            // T3 links that aren't exactly a thread.
            "t3code://threads/e/t?x=1",
            "t3code://threads/e/t#x",
            "t3code://threads/e/t/more",
            "t3code://threads/e",
            "t3code://threads//t",
            "t3code://threads/e/",
            "t3code://projects/e/t",
            "t3code:threads/e/t",
            "t3code://threads/e/t%2",
            "t3code://threads/e!/t",
            "t3code://threads/e_%41/t",
            "t3code://threads/../t",
            // Lookalikes: a Cyrillic і, о and ԁ, and a fullwidth ｈ.
            "fіle:///tmp/plan.md",
            "t3cоde://threads/e/t",
            "ｈttps://github.com/",
            "https://gіthub.com/",
            "https://github.com/acme/wеb",
        ] {
            assert_eq!(refused_code(url), ErrorCode::UrlNotAllowed, "{url:?}");
        }
        assert_eq!(
            decide("ssh://x").unwrap_err().message,
            "Airboss only opens https links, files and T3 threads."
        );
    }

    fn mode(path: &Path, m: u32) {
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(m)).unwrap();
    }

    /// A temporary folder of tricky files; each test gets its own.
    fn fixtures(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("airboss-opener-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        for f in [
            "plan.md",
            "README.MD",
            "booking flow.md",
            "photo.PNG",
            "spec.pdf",
            "main.rs",
            "x.command",
            "run.sh",
            "script.py",
            "page.html",
            "art.svg",
            "x.terminal",
            "noext",
            ".md",
            "exec.txt",
            "tool",
            "notes.m\u{0501}",
            "evil\u{202E}dm.command",
            "x\u{FF0E}md",
        ] {
            std::fs::write(dir.join(f), "x").unwrap();
        }
        mode(&dir.join("exec.txt"), 0o755);
        mode(&dir.join("tool"), 0o755);
        mode(&dir.join("x.command"), 0o755);
        for bundle in ["Thing.app", "Flow.workflow", "Notes.rtfd", "folder.md"] {
            std::fs::create_dir_all(dir.join(bundle)).unwrap();
        }
        symlink(dir.join("x.command"), dir.join("link.md")).unwrap();
        symlink("/bin/sh", dir.join("shell.txt")).unwrap();
        symlink(dir.join("plan.md"), dir.join("alias.md")).unwrap();
        dir
    }

    #[test]
    fn a_file_opens_only_when_its_real_self_is_a_plain_document() {
        let dir = fixtures("sort");
        let real = |p: &str| dir.join(p).canonicalize().unwrap();
        for name in [
            "plan.md",
            "README.MD",
            "booking flow.md",
            "photo.PNG",
            "spec.pdf",
            "main.rs",
        ] {
            let url = file_url(&dir.join(name));
            let viewer = if name.ends_with(".rs") || name.ends_with(".md") || name.ends_with(".MD")
            {
                Viewer::TextEdit
            } else {
                Viewer::Preview
            };
            assert_eq!(
                decide(&url).unwrap(),
                Opening::Open(real(name), viewer),
                "{name}"
            );
        }
        // A symlink to a document opens the document.
        assert_eq!(
            decide(&file_url(&dir.join("alias.md"))).unwrap(),
            Opening::Open(real("plan.md"), Viewer::TextEdit)
        );
        // A mixed-case scheme is still a file URL.
        let upper = file_url(&dir.join("plan.md")).replacen("file", "FiLe", 1);
        assert_eq!(
            decide(&upper).unwrap(),
            Opening::Open(real("plan.md"), Viewer::TextEdit)
        );
        for name in [
            "x.command",
            "run.sh",
            "script.py",
            "page.html",
            "art.svg",
            "x.terminal",
            "noext",
            ".md",
            "exec.txt",
            "tool",
            "Thing.app",
            "Flow.workflow",
            "Notes.rtfd",
            "folder.md",
            "sub",
            // Lookalike extensions: a Cyrillic ԁ, a right-to-left override that makes
            // ".command" read as "dm", and a fullwidth dot.
            "notes.m\u{0501}",
            "evil\u{202E}dm.command",
            "x\u{FF0E}md",
        ] {
            let url = file_url(&dir.join(name));
            assert_eq!(decide(&url).unwrap(), Opening::Reveal(real(name)), "{name}");
        }
        // Symlinks are judged by what they point at.
        assert_eq!(
            decide(&file_url(&dir.join("link.md"))).unwrap(),
            Opening::Reveal(real("x.command"))
        );
        assert_eq!(
            decide(&file_url(&dir.join("shell.txt"))).unwrap(),
            Opening::Reveal(Path::new("/bin/sh").canonicalize().unwrap())
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn dot_dot_and_escaped_segments_resolve_before_the_type_is_judged() {
        let dir = fixtures("dots");
        let base = file_url(&dir);
        let real = |p: &str| dir.join(p).canonicalize().unwrap();
        assert_eq!(
            decide(&format!("{base}/sub/../x.command")).unwrap(),
            Opening::Reveal(real("x.command"))
        );
        assert_eq!(
            decide(&format!("{base}/sub/%2E%2E/x.command")).unwrap(),
            Opening::Reveal(real("x.command"))
        );
        assert_eq!(
            decide(&format!("{base}/sub/../plan.md")).unwrap(),
            Opening::Open(real("plan.md"), Viewer::TextEdit)
        );
        assert_eq!(
            decide(&format!("{base}%2Fplan.md")).unwrap(),
            Opening::Open(real("plan.md"), Viewer::TextEdit)
        );
        // The audit's first trick, with the file really there: still refused, never opened.
        assert_eq!(
            refused_code(&format!("{base}/x.command?a")),
            ErrorCode::UrlNotAllowed
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The final review: LaunchServices picks the app for a plain `open`, and a Finder alias
    /// named `plan.md`, a per-file "Open With" attribute or a hijacked default handler can make
    /// that pick an app. So a document opens in a fixed system viewer, never in whatever
    /// claims its type.
    #[test]
    fn documents_open_only_in_a_fixed_system_viewer() {
        for ext in OPENS {
            let v = Viewer::for_extension(ext).unwrap();
            assert!(v.app().starts_with("/System/Applications/"), "{ext}");
        }
        assert_eq!(Viewer::for_extension("pdf"), Some(Viewer::Preview));
        assert_eq!(Viewer::for_extension("PNG"), Some(Viewer::Preview));
        assert_eq!(Viewer::for_extension("md"), Some(Viewer::TextEdit));
        assert_eq!(Viewer::for_extension("command"), None);
        assert_eq!(
            Viewer::TextEdit.app(),
            Path::new("/System/Applications/TextEdit.app")
        );
        assert_eq!(
            Viewer::Preview.app(),
            Path::new("/System/Applications/Preview.app")
        );
    }

    /// Bytes that aren't UTF-8 would reach `open` changed (lossily), not as checked.
    #[test]
    fn a_path_that_isnt_utf8_is_refused() {
        assert_eq!(
            refused_code("file:///tmp/a%FF.md"),
            ErrorCode::UrlNotAllowed
        );
        assert_eq!(
            refused_code("file:///tmp/%C3%28.md"),
            ErrorCode::UrlNotAllowed
        );
    }

    /// The second audit, items 2 and 3: a `t3code` id is judged after percent-decoding, so
    /// `%2F`, `%2E%2E`, `%3F`, `%23`, `%5C`, `%00` and control bytes can't smuggle a path, a
    /// query or a fragment into T3; real ids, escaped by `sync`, still pass.
    #[test]
    fn t3_ids_are_judged_after_decoding() {
        for url in [
            "t3code://threads/env/%2E%2E%2F%2E%2E%2Fsettings",
            "t3code://threads/env/a%2Fb",
            "t3code://threads/env/a%00b",
            "t3code://threads/env/a%3Fx%3D1%23y",
            "t3code://threads/env/a%23b",
            "t3code://threads/env/a%5Cb",
            "t3code://threads/env/a%0Ab",
            "t3code://threads/env/a%1Bb",
            "t3code://threads/env/a%7Fb",
            "t3code://threads/env/%2E%2E",
            "t3code://threads/env/%2E",
            "t3code://threads/env/%FF",
            "t3code://threads/env/t/",
        ] {
            assert_eq!(refused_code(url), ErrorCode::UrlNotAllowed, "{url}");
        }
        for (url, opened) in [
            (
                "t3code://threads/0b8a1f9e-1d2c-4b3a-9f8e-7d6c5b4a3f21/mcp%3Alaunch%3A9f8e7d6c",
                "t3code://threads/0b8a1f9e-1d2c-4b3a-9f8e-7d6c5b4a3f21/mcp%3Alaunch%3A9f8e7d6c",
            ),
            (
                "t3code://threads/0b8a1f9e/thread_01HZX8Q2%2Eb",
                "t3code://threads/0b8a1f9e/thread_01HZX8Q2%2Eb",
            ),
            ("T3CODE://threads/env/t", "t3code://threads/env/t"),
        ] {
            assert_eq!(
                decide(url).unwrap(),
                Opening::Thread(opened.into()),
                "{url}"
            );
        }
    }

    /// The second audit's whole list, on files laid out as the auditor laid them out.
    #[test]
    fn the_second_audits_cases() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("airboss-audit2-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let fx = dir.join("fx");
        std::fs::create_dir_all(fx.join("dir.md")).unwrap();
        std::fs::create_dir_all(fx.join("Evil.app/Contents/MacOS")).unwrap();
        for f in [
            "plan.md",
            "plan.md ",
            "plan.md.",
            "-rf.md",
            "café.md",
            "nl\nx.md",
            "esc\u{1b}x.md",
            "a.command",
            "a.fileloc",
            "a.html",
            "a.inetloc",
            "a.svg",
            "a.terminal",
            "a.textClipping",
            "a.webloc",
            "t.json",
            "t.rtf",
            "t.xml",
            "t.yaml",
            "exec.md",
        ] {
            std::fs::write(fx.join(f), "x").unwrap();
        }
        for f in ["a.command", "exec.md"] {
            std::fs::set_permissions(fx.join(f), std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        std::fs::hard_link(fx.join("a.command"), fx.join("hard.md")).unwrap();
        symlink(fx.join("a.command"), fx.join("link.md")).unwrap();
        symlink("/System/Applications/Calculator.app", fx.join("calc.md")).unwrap();
        symlink(fx.join("a.html"), fx.join("swap.md")).unwrap();
        let fifo = std::process::Command::new("/usr/bin/mkfifo")
            .arg(fx.join("fifo.md"))
            .status()
            .unwrap();
        assert!(fifo.success());
        let base = file_url(&fx);
        let real = |n: &str| fx.join(n).canonicalize().unwrap();
        let text = |o: Result<Opening, AppError>| match o {
            Ok(Opening::Open(_, Viewer::TextEdit)) => "textedit".to_string(),
            Ok(Opening::Open(_, Viewer::Preview)) => "preview".to_string(),
            Ok(Opening::Reveal(p)) => {
                format!("reveal {}", p.file_name().unwrap().to_string_lossy())
            }
            Ok(other) => format!("{other:?}"),
            Err(e) => format!("{:?}", e.code),
        };
        let tail = base.trim_start_matches("file://");
        for (url, want) in [
            (format!("{base}/plan.md"), "textedit"),
            (format!("{base}/PLAN.MD"), "textedit"),
            (format!("file:///{tail}/plan.md"), "textedit"),
            (format!("{base}/./plan.md"), "textedit"),
            (format!("{base}/plan.md%3Fx"), "NotFound"),
            (format!("{base}/plan.md%23"), "NotFound"),
            (format!("file://%2F{tail}/plan.md"), "UrlNotAllowed"),
            (format!("{}%2Fplan.md", base), "textedit"),
            (format!("{base}/plan.md."), "reveal plan.md."),
            (format!("{base}/plan.md%20"), "reveal plan.md "),
            (format!("{base}/-rf.md"), "textedit"),
            (format!("{base}/caf%C3%A9.md"), "textedit"),
            (format!("{base}/cafe%CC%81.md"), "textedit"),
            (format!("{base}/nl%0Ax.md"), "UrlNotAllowed"),
            (format!("{base}/esc%1Bx.md"), "UrlNotAllowed"),
            (format!("{base}/del%7Fx.md"), "UrlNotAllowed"),
            (format!("{base}/exec.md"), "reveal exec.md"),
            (format!("{base}/hard.md"), "reveal hard.md"),
            (format!("{base}/link.md"), "reveal a.command"),
            (format!("{base}/calc.md"), "reveal Calculator.app"),
            (format!("{base}/swap.md"), "reveal a.html"),
            (format!("{base}/dir.md"), "reveal dir.md"),
            (format!("{base}/fifo.md"), "NotFound"),
            (format!("{base}/Evil.app"), "reveal Evil.app"),
            (format!("{base}/Evil.app/"), "reveal Evil.app"),
            (format!("{base}/a.webloc"), "reveal a.webloc"),
            (format!("{base}/a.fileloc"), "reveal a.fileloc"),
            (format!("{base}/a.inetloc"), "reveal a.inetloc"),
            (format!("{base}/a.terminal"), "reveal a.terminal"),
            (format!("{base}/a.textClipping"), "reveal a.textClipping"),
            (format!("{base}/a.html"), "reveal a.html"),
            (format!("{base}/a.svg"), "reveal a.svg"),
            (format!("{base}/t.json"), "textedit"),
            (format!("{base}/t.rtf"), "textedit"),
            (format!("{base}/t.xml"), "textedit"),
            (format!("{base}/t.yaml"), "textedit"),
            (format!("file://localhost{tail}/plan.md"), "UrlNotAllowed"),
            (format!("file://[::1]{tail}/plan.md"), "UrlNotAllowed"),
            (format!("{base}/plan.md;x"), "NotFound"),
            (format!("file:{tail}/plan.md"), "UrlNotAllowed"),
            ("file:///dev/stdin".to_string(), "NotFound"),
            ("file:///dev/fd/0".to_string(), "NotFound"),
            ("file:///dev/null".to_string(), "NotFound"),
            (
                "file:///System/Applications/Calculator.app".to_string(),
                "reveal Calculator.app",
            ),
            (
                "https://github.com/".to_string(),
                "Browser(\"https://github.com/\")",
            ),
            ("https://github.com:/".to_string(), "UrlNotAllowed"),
            (
                "https://github.com:443@evil.com/".to_string(),
                "UrlNotAllowed",
            ),
            (
                "https://xn--gthub-n4a.com/".to_string(),
                "Browser(\"https://xn--gthub-n4a.com/\")",
            ),
            (
                "https://github.com%2F@evil.com".to_string(),
                "UrlNotAllowed",
            ),
            (
                "https://[::ffff:127.0.0.1]/".to_string(),
                "Browser(\"https://[::ffff:127.0.0.1]/\")",
            ),
            (
                "https://a.b/%0a".to_string(),
                "Browser(\"https://a.b/%0a\")",
            ),
            (
                "https://www.icloud.com/shortcuts/abc".to_string(),
                "Browser(\"https://www.icloud.com/shortcuts/abc\")",
            ),
            (
                "https://github.com./".to_string(),
                "Browser(\"https://github.com./\")",
            ),
        ] {
            assert_eq!(text(decide(&url)), want, "{url:?}");
        }
        // What's opened is the real file, not the spelling in the URL.
        assert!(
            matches!(decide(&format!("{base}/./plan.md")), Ok(Opening::Open(p, _)) if p == real("plan.md"))
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_missing_file_says_so() {
        let e = decide("file:///nowhere/airboss-missing.md").unwrap_err();
        assert_eq!(e.code, ErrorCode::NotFound);
        assert_eq!(e.message, "That file no longer exists.");
    }
}
