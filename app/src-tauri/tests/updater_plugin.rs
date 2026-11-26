//! `TauriUpdater` on the real updater plugin, in a mock Tauri app, against a small HTTP
//! server on the loopback interface: the timeouts, the plain sentences, and that every
//! install reads the manifest afresh. Nothing leaves this machine, and no download ever
//! passes its signature, so nothing is ever installed.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use airboss_app::native::TauriUpdater;
use airboss_app::updates::{InstallError, Limits, Updater};
use tauri::test::{MockRuntime, mock_builder, mock_context, noop_assets};

// The updater plugin's own throwaway test key, and its signature of the payload `test`
// (`tauri signer sign --app-version 2.0.0`). The server never sends `test`, so no download
// verifies.
const PUBLIC_KEY: &str = "untrusted comment: minisign public key: A49EF4746FA6E6D7\nRWTX5qZvdPSepDGWg2bz8n6ArQnBMwgQK1geVosorcbLzcunD+KJ+cYe";
const SIGNATURE: &str = "untrusted comment: signature from tauri secret key\nRUTX5qZvdPSepPF2zsjvP/L/TQtBLRnWRvaq+4omK1UEugtQp/USrGiI1v762sDy3id3aor7bRC9ETyeot6lMHHrWoZ5hjUM1Qc=\ntrusted comment: timestamp:1791649051\tfile:payload\tversion:2.0.0\nDDq2CLOEl5cQmC2eJRHlnZPVViOZsu1CjpPKQoei4A/XBfkDqGO0B+QMrcrHgEQ5qV3mne75tdFWhJpqUXh9AA==";

fn base64(bytes: &[u8]) -> String {
    const ABC: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(ABC[((n >> (18 - 6 * i)) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// What the server answers for the manifest.
#[derive(Clone)]
enum Manifest {
    /// Accept the connection and never answer.
    Stall,
    /// The release `2.0.0`, downloaded from this path.
    Offer(&'static str),
}

/// A loopback HTTP server: the manifest at `/latest.json`, a tampered package at
/// `/tampered`, and 404 for anything else. It counts the requests per path.
struct Server {
    port: u16,
    manifest: Arc<Mutex<Manifest>>,
    hits: Arc<Mutex<HashMap<String, usize>>>,
}

impl Server {
    fn start(manifest: Manifest) -> Server {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let manifest = Arc::new(Mutex::new(manifest));
        let hits = Arc::new(Mutex::new(HashMap::new()));
        let (m, h) = (manifest.clone(), hits.clone());
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let (m, h) = (m.clone(), h.clone());
                std::thread::spawn(move || serve(stream, port, &m, &h));
            }
        });
        Server {
            port,
            manifest,
            hits,
        }
    }

    fn url(&self) -> String {
        format!("http://127.0.0.1:{}/latest.json", self.port)
    }

    fn set(&self, m: Manifest) {
        *self.manifest.lock().unwrap() = m;
    }

    fn hits(&self, path: &str) -> usize {
        self.hits.lock().unwrap().get(path).copied().unwrap_or(0)
    }
}

fn serve(
    mut stream: TcpStream,
    port: u16,
    manifest: &Mutex<Manifest>,
    hits: &Mutex<HashMap<String, usize>>,
) {
    let mut request = Vec::new();
    let mut buf = [0u8; 1024];
    while !request.windows(4).any(|w| w == b"\r\n\r\n") {
        match stream.read(&mut buf) {
            Ok(0) | Err(_) => return,
            Ok(n) => request.extend_from_slice(&buf[..n]),
        }
    }
    let text = String::from_utf8_lossy(&request);
    let path = text.split_whitespace().nth(1).unwrap_or("").to_string();
    *hits.lock().unwrap().entry(path.clone()).or_default() += 1;
    let (status, body) = match (path.as_str(), manifest.lock().unwrap().clone()) {
        ("/latest.json", Manifest::Stall) => {
            std::thread::sleep(Duration::from_secs(30));
            return;
        }
        ("/latest.json", Manifest::Offer(download)) => {
            let platform = serde_json::json!({
                "url": format!("http://127.0.0.1:{port}{download}"),
                "signature": base64(SIGNATURE.as_bytes()),
            });
            let body = serde_json::json!({
                "version": "2.0.0",
                "notes": "",
                "pub_date": "2026-11-26T00:00:00Z",
                "platforms": {
                    "darwin-aarch64": platform.clone(),
                    "darwin-aarch64-app": platform,
                }
            });
            ("200 OK", body.to_string().into_bytes())
        }
        // Not `test`: this never matches the signature.
        ("/tampered", _) => ("200 OK", b"tampered".to_vec()),
        _ => ("404 Not Found", b"not here".to_vec()),
    };
    let head = format!(
        "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(&body);
}

/// A mock app with the updater plugin reading `server`'s manifest. Plain `http` is allowed
/// here only; the app's endpoint is the compiled-in `https` one.
fn updater(
    server: &Server,
    limits: Limits,
) -> (tauri::App<MockRuntime>, TauriUpdater<MockRuntime>) {
    let mut context = mock_context(noop_assets());
    context.config_mut().plugins.0.insert(
        "updater".into(),
        serde_json::json!({
            "pubkey": base64(PUBLIC_KEY.as_bytes()),
            "endpoints": [server.url()],
            "dangerousInsecureTransportProtocol": true,
        }),
    );
    let app = mock_builder()
        .plugin(tauri_plugin_updater::Builder::new().build())
        .build(context)
        .unwrap();
    let updater = TauriUpdater::new(app.handle().clone(), limits);
    (app, updater)
}

const ROOMY: Limits = Limits {
    connect: Duration::from_secs(5),
    read: Duration::from_secs(10),
    check: Duration::from_secs(10),
};

#[test]
fn a_stalled_manifest_fails_on_the_read_timeout_instead_of_hanging() {
    let server = Server::start(Manifest::Stall);
    let (_app, u) = updater(
        &server,
        Limits {
            read: Duration::from_millis(300),
            ..ROOMY
        },
    );
    let started = Instant::now();
    assert_eq!(u.check(), Err("GitHub didn't answer in time.".into()));
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "{:?}",
        started.elapsed()
    );
}

#[test]
fn a_stalled_manifest_fails_on_the_check_timeout_too() {
    let server = Server::start(Manifest::Stall);
    let (_app, u) = updater(
        &server,
        Limits {
            check: Duration::from_millis(300),
            ..ROOMY
        },
    );
    let started = Instant::now();
    assert_eq!(u.check(), Err("GitHub didn't answer in time.".into()));
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "{:?}",
        started.elapsed()
    );
}

#[test]
fn every_install_reads_the_manifest_again() {
    let server = Server::start(Manifest::Offer("/tampered"));
    let (_app, u) = updater(&server, ROOMY);
    let found = u.check().unwrap().unwrap();
    assert_eq!(found.version, "2.0.0");
    assert_eq!(server.hits("/latest.json"), 1);

    // The manifest changes after the check: the install goes by the new one. Reusing the
    // checked release would download `/tampered` and fail on its signature instead.
    server.set(Manifest::Offer("/missing"));
    let mut seen = Vec::new();
    let r = u.install("2.0.0", &mut |p| seen.push(p));
    assert_eq!(
        r,
        Err(InstallError::Failed("The download didn't finish.".into()))
    );
    assert_eq!(server.hits("/latest.json"), 2);
    assert_eq!(server.hits("/missing"), 1);
    assert_eq!(server.hits("/tampered"), 0);

    // A bad signature is refused, in the card's words.
    server.set(Manifest::Offer("/tampered"));
    let r = u.install("2.0.0", &mut |p| seen.push(p));
    assert_eq!(
        r,
        Err(InstallError::Failed(
            "The download's signature didn't match, so it wasn't installed.".into()
        ))
    );
    assert_eq!(server.hits("/latest.json"), 3);
    assert_eq!(server.hits("/tampered"), 1);
    assert_eq!(
        seen.last(),
        Some(&airboss_app::updates::Progress::Installing)
    );

    // A version the manifest no longer offers isn't downloaded at all.
    let r = u.install("1.9.0", &mut |_| {});
    assert_eq!(
        r,
        Err(InstallError::NotOffered(Some(
            airboss_app::updates::Release {
                version: "2.0.0".into()
            }
        )))
    );
    assert_eq!(server.hits("/tampered"), 1);
}
