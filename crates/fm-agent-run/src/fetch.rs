//! In-app model download — **resumable + checksum-verified**, so a phone with no `curl` can fetch a
//! ~150 MB GGUF on first enable without re-downloading from scratch on every dropped connection.
//!
//! The bytes stream to a `<dest>.part` sidecar; a re-run resumes it with an HTTP `Range` request, and
//! `dest` only ever appears once the whole file is present and (if a checksum was given) verified — so
//! the runner never finds a half-written model. Behind the `download` feature (agent-only): it pulls a
//! pure-Rust blocking HTTPS client (`ureq`, default `rustls` TLS with bundled roots) so it needs no
//! system TLS and cross-compiles to Android, plus `sha2` to verify. The notes core links neither.

use crate::manifest::Manifest;
use sha2::{Digest, Sha256};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// **Is retrying this worth anything?** — the one question the retry loop needs answered.
///
/// It used to retry everything twelve times with backoff, which is right for a dropped mobile
/// connection and wrong for a full disk: the same failure, forty seconds apart, twelve times, before
/// a message the user could have had at once. Cancelling is the same shape — a user who pressed stop
/// must not wait out a backoff.
#[derive(Debug)]
pub enum FetchError {
    /// Retrying cannot help: the disk is full, the path is unwritable, the user cancelled.
    Fatal(String),
    /// The network did what networks do. Resume and try again.
    Transient(String),
}

impl std::fmt::Display for FetchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FetchError::Fatal(m) | FetchError::Transient(m) => f.write_str(m),
        }
    }
}

/// Connect and **read** deadlines.
///
/// The read one is the load-bearing half and there was none. A half-open connection — the mobile
/// case, where the peer vanishes without a FIN — leaves `read` blocked forever, so the download hung
/// with no error, no progress, and no timeout: `MAX_STALLS` counts *failed attempts*, and an attempt
/// that never returns is never one. A stall the retry loop cannot see is a stall it cannot survive.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(20);
const READ_TIMEOUT: Duration = Duration::from_secs(60);

fn http() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout_connect(CONNECT_TIMEOUT)
        .timeout_read(READ_TIMEOUT)
        .build()
}

/// One download: from `url` to `dest`, optionally verified against `sha256` (hex, case-insensitive).
pub struct Download<'a> {
    pub url: &'a str,
    pub dest: &'a Path,
    pub sha256: Option<&'a str>,
}

/// Ensure the GGUF for `name` is present under `models_dir`, fetching it from Hugging Face on first
/// enable, and return its local path. `on_progress(done, total)` fires while downloading. Idempotent:
/// an already-present file (checksum-valid, if the manifest pins one) returns with no network — so a
/// **sideloaded** model, or one with no `repo`, is used as-is. This is the one call the phone's
/// first-run provisioning makes; the desktop still has `fetch.sh`.
pub fn ensure_model(
    models_dir: &Path,
    manifest: &Manifest,
    name: &str,
    on_progress: &dyn Fn(u64, Option<u64>),
    cancel: &dyn Fn() -> bool,
) -> Result<PathBuf, String> {
    let model = manifest
        .model(name)
        .ok_or_else(|| format!("model '{name}' is not in the manifest"))?;
    let dest = models_dir.join(&model.file);

    // Already provisioned (a prior fetch, or sideloaded)? Then no URL is even needed.
    if dest.exists() {
        match model.sha256.as_deref() {
            Some(want) if verify(&dest, want)? => return Ok(dest),
            None => return Ok(dest),
            Some(_) => {} // present but wrong bytes — fall through to refetch
        }
    }

    let url = manifest
        .download_url(name)
        .ok_or_else(|| format!("model '{name}' is absent and has no `repo` to fetch it from"))?;
    let dl = Download { url: &url, dest: &dest, sha256: model.sha256.as_deref() };

    // Mobile networks drop large transfers mid-stream and hit intermittent DNS failures (both seen
    // on-device: HF aborts every ~10 MB, and the resolver blips). Each `fetch` resumes from
    // `<dest>.part`, so keep retrying — a download must survive a flaky link. The stall counter only
    // rises on an attempt that moved **zero** bytes; any progress resets it, so a normal drop-and-
    // resume never counts. We give up only after MAX_STALLS truly-dead attempts in a row, with a
    // capped exponential backoff so a longer outage is tolerated without spinning hot.
    const MAX_STALLS: u32 = 12;
    let part = part_path(&dest);
    let part_len = || fs::metadata(&part).map(|m| m.len()).unwrap_or(0);
    let mut stalls = 0u32;
    loop {
        let before = part_len();
        match fetch(&dl, on_progress, cancel) {
            Ok(()) => return Ok(dest),
            // **Nothing a second attempt can change.** A full disk, an unwritable path, or a user
            // who pressed stop: retrying spends up to ten minutes of backoff to arrive at the same
            // sentence. Report it now, with the `.part` left in place so a later run resumes.
            Err(e @ FetchError::Fatal(_)) => return Err(e.to_string()),
            Err(e) => {
                if part_len() > before {
                    stalls = 0; // made headway; a dropped connection is expected, keep going
                } else {
                    stalls += 1;
                    if stalls >= MAX_STALLS {
                        return Err(format!("gave up after {MAX_STALLS} stalled attempts: {e}"));
                    }
                }
                let backoff = 2u64.saturating_pow(stalls.min(4)).min(16); // 2,4,8,16,16… seconds
                std::thread::sleep(Duration::from_secs(backoff));
                if cancel() {
                    return Err("download cancelled".to_string());
                }
            }
        }
    }
}

/// Fetch `d`, resuming a partial `<dest>.part` if present. `on_progress(done, total)` fires as bytes
/// arrive (`total` is `None` when the server sends no length). **Idempotent:** an existing `dest`
/// returns `Ok` without touching the network — verifying it first if a checksum was given.
pub fn fetch(
    d: &Download,
    on_progress: &dyn Fn(u64, Option<u64>),
    cancel: &dyn Fn() -> bool,
) -> Result<(), FetchError> {
    use FetchError::{Fatal, Transient};

    // Already in place? Trust it if we can verify it; with no checksum to check against, a present
    // dest is taken as done (the caller chose not to pin one). A wrong-bytes dest is removed + refetched.
    if d.dest.exists() {
        match d.sha256 {
            Some(want) if verify(d.dest, want).map_err(Fatal)? => return Ok(()),
            Some(_) => {
                let _ = fs::remove_file(d.dest);
            }
            None => return Ok(()),
        }
    }
    if cancel() {
        return Err(Fatal("download cancelled".into()));
    }

    let part = part_path(d.dest);
    let have = fs::metadata(&part).map(|m| m.len()).unwrap_or(0);

    let mut req = http().get(d.url);
    // **Never compressed, ranged or not.** `ureq`'s default `Accept-Encoding: gzip` is transparently
    // decoded, so `Content-Length` describes the *encoded* body while the bytes written are the
    // decoded ones — the byte accounting below would be comparing two different numbers. Worse with
    // a `Range`, which the server applies to the encoded stream: resuming at the decoded offset
    // appends the wrong bytes and the file is silently wrong until the checksum catches it (and if
    // no checksum is pinned, never). A GGUF is already compressed; this costs nothing.
    req = req.set("Accept-Encoding", "identity");
    if have > 0 {
        req = req.set("Range", &format!("bytes={have}-"));
    }
    let resp = req.call().map_err(|e| Transient(format!("GET {}: {e}", d.url)))?;
    let status = resp.status();

    // 206 = the server honoured the range → append; 200 = it ignored it → start over; 416 = the range
    // is past the end, i.e. `.part` is already the whole file → straight to verify.
    let (mut file, mut done, read_body) = match status {
        206 => (open_append(&part)?, have, true),
        200 => (open_truncate(&part)?, 0, true),
        416 => (open_append(&part)?, have, false),
        s => return Err(Transient(format!("GET {}: unexpected status {s}", d.url))),
    };

    // How many bytes this file should end up with. **Not read from a 416's `Content-Length`** — that
    // describes the error page, not the file, so the old code reported a total of "what we already
    // have plus the length of an apology" and the completeness check below would have failed a file
    // that was already whole.
    let total = if read_body {
        resp.header("Content-Length")
            .and_then(|s| s.trim().parse::<u64>().ok())
            .map(|len| done + len)
    } else {
        Some(have)
    };
    on_progress(done, total);

    if read_body {
        let mut reader = resp.into_reader();
        let mut buf = [0u8; 64 * 1024];
        loop {
            if cancel() {
                // The `.part` stays: a cancelled download is a paused one, and the next run resumes
                // from exactly here rather than re-fetching the gigabytes already on disk.
                return Err(Fatal("download cancelled".into()));
            }
            let n = reader.read(&mut buf).map_err(|e| Transient(format!("read {}: {e}", d.url)))?;
            if n == 0 {
                break;
            }
            file.write_all(&buf[..n]).map_err(|e| write_failed(&part, e))?;
            done += n as u64;
            on_progress(done, total);
        }
    }
    file.flush().map_err(|e| write_failed(&part, e))?;
    drop(file);

    // **A body that stopped early is not a download.** Without this, a connection cut at 80 % was
    // renamed over `dest` and `ensure_model` returned that truncated file forever — nothing in the
    // tree ever deleted a bad `.gguf`, so the only self-heal was uninstalling the app, which also
    // destroys the vault (`known-issues.md`). Transient on purpose: the `.part` is genuinely that
    // far along, and the next attempt resumes from it.
    //
    // `ureq` happens to catch the declared-length case first, in its own reader. This is the
    // backstop that does not depend on it: the invariant is ours, the check belongs at our level,
    // and a body delimited by connection close (no `Content-Length`) reaches this line and nothing
    // else. Same reasoning as the transport refusing to delegate a check it can make itself.
    if let Some(want) = total {
        if done < want {
            return Err(Transient(format!(
                "{} ended early: {done} of {want} bytes — resuming",
                d.url
            )));
        }
    }

    if let Some(want) = d.sha256 {
        if !verify(&part, want).map_err(Fatal)? {
            // Corrupt: drop the partial so the next attempt starts clean rather than resuming garbage.
            let _ = fs::remove_file(&part);
            return Err(Transient(format!(
                "checksum mismatch for {} — deleted the partial, retry",
                d.dest.display()
            )));
        }
    }
    fs::rename(&part, d.dest).map_err(|e| {
        Fatal(format!("rename {} -> {}: {e}", part.display(), d.dest.display()))
    })?;
    Ok(())
}

/// Classify a write failure. **A full disk is the one that must not be retried**: twelve attempts
/// with backoff is up to ten minutes spent arriving at the same sentence, and the disk does not
/// empty itself in the meantime. `StorageFull` is `std`'s portable name for `ENOSPC` and Windows'
/// `ERROR_DISK_FULL`, so this needs no platform code and no new dependency.
fn write_failed(part: &Path, e: std::io::Error) -> FetchError {
    let msg = format!("write {}: {e}", part.display());
    match e.kind() {
        std::io::ErrorKind::StorageFull => FetchError::Fatal(format!(
            "{msg} — there is not enough free space for this download"
        )),
        std::io::ErrorKind::PermissionDenied => FetchError::Fatal(msg),
        _ => FetchError::Transient(msg),
    }
}

/// `<dest>.part` — the resume sidecar beside the final file.
fn part_path(dest: &Path) -> PathBuf {
    let mut s = dest.as_os_str().to_owned();
    s.push(".part");
    PathBuf::from(s)
}

fn open_append(p: &Path) -> Result<File, FetchError> {
    ensure_parent(p)?;
    OpenOptions::new().create(true).append(true).open(p).map_err(|e| write_failed(p, e))
}

fn open_truncate(p: &Path) -> Result<File, FetchError> {
    ensure_parent(p)?;
    OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(p)
        .map_err(|e| write_failed(p, e))
}

fn ensure_parent(p: &Path) -> Result<(), FetchError> {
    if let Some(dir) = p.parent() {
        fs::create_dir_all(dir).map_err(|e| write_failed(dir, e))?;
    }
    Ok(())
}

/// Stream `path` through SHA-256 and compare to `want` (case-insensitive hex).
fn verify(path: &Path, want: &str) -> Result<bool, String> {
    let mut f = File::open(path).map_err(|e| format!("open {}: {e}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = f.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    let got: String = hasher.finalize().iter().map(|b| format!("{b:02x}")).collect();
    Ok(got.eq_ignore_ascii_case(want.trim()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;
    use std::thread;

    /// The caller never cancels — the ordinary case, spelled once.
    fn never() -> bool {
        false
    }

    /// A throwaway directory. `std::env::temp_dir` rather than a `tempfile` dev-dependency, matching
    /// the tests already here: this crate is deliberately dependency-free so it cross-compiles to
    /// Android, and a test is a poor reason to be the first exception.
    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("fm-fetch-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn sha_hex(bytes: &[u8]) -> String {
        let mut h = Sha256::new();
        h.update(bytes);
        h.finalize().iter().map(|b| format!("{b:02x}")).collect()
    }

    /// A one-shot localhost HTTP/1.1 server that serves `body`, honouring a single `Range: bytes=N-`
    /// with a 206. Hermetic — no external network. Returns the bound `http://127.0.0.1:PORT/model`.
    fn serve_once(body: Vec<u8>) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/model", listener.local_addr().unwrap());
        thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut req = Vec::new();
            let mut tmp = [0u8; 1024];
            loop {
                let n = stream.read(&mut tmp).unwrap();
                req.extend_from_slice(&tmp[..n]);
                if n == 0 || req.windows(4).any(|w| w == b"\r\n\r\n") {
                    break;
                }
            }
            let text = String::from_utf8_lossy(&req);
            let start = text
                .lines()
                .find_map(|l| l.strip_prefix("Range: bytes="))
                .and_then(|r| r.split('-').next())
                .and_then(|n| n.parse::<usize>().ok());
            match start {
                Some(s) => {
                    let part = &body[s..];
                    let head = format!(
                        "HTTP/1.1 206 Partial Content\r\nContent-Length: {}\r\nContent-Range: bytes {}-{}/{}\r\nConnection: close\r\n\r\n",
                        part.len(), s, body.len() - 1, body.len()
                    );
                    stream.write_all(head.as_bytes()).unwrap();
                    stream.write_all(part).unwrap();
                }
                None => {
                    let head = format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    );
                    stream.write_all(head.as_bytes()).unwrap();
                    stream.write_all(&body).unwrap();
                }
            }
        });
        url
    }

    #[test]
    fn downloads_whole_file_and_verifies_checksum() {
        let body: Vec<u8> = (0..50_000u32).map(|i| i as u8).collect();
        let want = sha_hex(&body);
        let url = serve_once(body.clone());
        let dir = std::env::temp_dir().join(format!("fm-fetch-{}", std::process::id()));
        let _ = fs::create_dir_all(&dir);
        let dest = dir.join("m.gguf");
        let _ = fs::remove_file(&dest);

        let last = std::cell::Cell::new(0u64);
        fetch(&Download { url: &url, dest: &dest, sha256: Some(&want) }, &|d, _| last.set(d), &never)
            .unwrap();

        assert_eq!(fs::read(&dest).unwrap(), body, "downloaded bytes match");
        assert_eq!(last.get(), body.len() as u64, "progress reached the full size");
        assert!(!part_path(&dest).exists(), ".part is renamed away on success");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn resumes_from_a_partial_and_lands_the_whole_file() {
        let body: Vec<u8> = (0..40_000u32).map(|i| (i * 7) as u8).collect();
        let want = sha_hex(&body);
        let url = serve_once(body.clone());
        let dir = std::env::temp_dir().join(format!("fm-fetch-resume-{}", std::process::id()));
        let _ = fs::create_dir_all(&dir);
        let dest = dir.join("m.gguf");
        let _ = fs::remove_file(&dest);
        // Pre-seed a partial: the first 25 000 bytes already on disk. The server must Range-serve the rest.
        fs::write(part_path(&dest), &body[..25_000]).unwrap();

        fetch(&Download { url: &url, dest: &dest, sha256: Some(&want) }, &|_, _| {}, &never).unwrap();

        assert_eq!(fs::read(&dest).unwrap(), body, "resumed file equals the whole body");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn ensure_model_uses_a_present_file_without_network() {
        // No server is started: if ensure_model touched the network this would hang/fail. A present,
        // unpinned file must be returned as-is (the sideloaded / already-fetched case).
        let m = Manifest::parse(
            "[[models]]\nname = \"x\"\nrepo = \"r/x\"\nfile = \"x.gguf\"\n",
        );
        let dir = std::env::temp_dir().join(format!("fm-ensure-{}", std::process::id()));
        let _ = fs::create_dir_all(&dir);
        fs::write(dir.join("x.gguf"), b"already here").unwrap();

        let got = ensure_model(&dir, &m, "x", &|_, _| {}, &never).unwrap();
        assert_eq!(got, dir.join("x.gguf"));
        assert_eq!(fs::read(&got).unwrap(), b"already here");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn ensure_model_errors_when_absent_and_unfetchable() {
        // Absent + no repo ⇒ a clear error, not a panic or a bogus URL fetch.
        let m = Manifest::parse("[[models]]\nname = \"x\"\nfile = \"x.gguf\"\n");
        let dir = std::env::temp_dir().join(format!("fm-ensure-none-{}", std::process::id()));
        let _ = fs::create_dir_all(&dir);
        let err = ensure_model(&dir, &m, "x", &|_, _| {}, &never).unwrap_err();
        assert!(err.contains("no `repo`"), "got: {err}");
        let _ = fs::remove_dir_all(&dir);
    }

    /// A one-shot server that answers with `declared` as its `Content-Length` but sends only
    /// `send.len()` bytes and hangs up — the shape of a connection cut mid-transfer. It also hands
    /// back the request text it received, so a test can assert what we asked for.
    fn serve_truncated(declared: usize, send: Vec<u8>) -> (String, std::sync::mpsc::Receiver<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/model", listener.local_addr().unwrap());
        let (tx, rx) = std::sync::mpsc::channel();
        thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut req = Vec::new();
            let mut tmp = [0u8; 1024];
            loop {
                let n = stream.read(&mut tmp).unwrap();
                req.extend_from_slice(&tmp[..n]);
                if n == 0 || req.windows(4).any(|w| w == b"\r\n\r\n") {
                    break;
                }
            }
            let _ = tx.send(String::from_utf8_lossy(&req).into_owned());
            let head = format!("HTTP/1.1 200 OK\r\nContent-Length: {declared}\r\nConnection: close\r\n\r\n");
            let _ = stream.write_all(head.as_bytes());
            let _ = stream.write_all(&send);
        });
        (url, rx)
    }

    /// **The bug the checksums were pinned to close.** A body that stops early used to be renamed
    /// over `dest` and returned forever — nothing in the tree ever deleted a bad `.gguf`, so the only
    /// self-heal was uninstalling the app, which also destroys the vault. Transient, so the retry
    /// resumes; never a finished file.
    #[test]
    fn a_body_that_ends_early_is_not_a_finished_download() {
        let (url, _rx) = serve_truncated(50_000, vec![7u8; 20_000]);
        let dir = scratch("early");
        let dest = dir.join("m.gguf");

        let err = fetch(&Download { url: &url, dest: &dest, sha256: None }, &|_, _| {}, &never)
            .unwrap_err();

        assert!(matches!(err, FetchError::Transient(_)), "an early close is worth retrying: {err}");
        assert!(!dest.exists(), "a truncated body must never be renamed over dest");
        assert_eq!(
            fs::metadata(part_path(&dest)).unwrap().len(),
            20_000,
            "the bytes that did arrive stay, so the retry resumes rather than restarts"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    /// Compression and byte accounting cannot both be right: `Content-Length` would describe the
    /// encoded body while the bytes written are the decoded ones, and with a `Range` the server
    /// applies the offset to the encoded stream — so a resume appends the wrong bytes.
    #[test]
    fn the_request_never_asks_for_a_compressed_body() {
        let (url, rx) = serve_truncated(10, vec![0u8; 10]);
        let dir = scratch("encoding");
        let dest = dir.join("m.gguf");
        let _ = fetch(&Download { url: &url, dest: &dest, sha256: None }, &|_, _| {}, &never);

        let req = rx.recv().unwrap().to_ascii_lowercase();
        assert!(req.contains("accept-encoding: identity"), "asked for: {req}");
        assert!(!req.contains("gzip"), "gzip was still offered: {req}");
        let _ = fs::remove_dir_all(&dir);
    }

    /// Stop must mean stop *now*: a user who turns the assistant off during a 1.4 GB first-run
    /// download must not keep downloading, and must not wait out a retry backoff either. What is
    /// already on disk stays, so turning it back on resumes.
    #[test]
    fn cancelling_stops_mid_download_and_keeps_what_arrived() {
        let body: Vec<u8> = (0..400_000u32).map(|i| i as u8).collect();
        let url = serve_once(body);
        let dir = scratch("cancel");
        let dest = dir.join("m.gguf");

        let seen = std::cell::Cell::new(0u64);
        let err = fetch(
            &Download { url: &url, dest: &dest, sha256: None },
            &|done, _| seen.set(done),
            &|| seen.get() > 100_000,
        )
        .unwrap_err();

        assert!(matches!(err, FetchError::Fatal(_)), "cancelling must not be retried: {err}");
        assert!(!dest.exists(), "a cancelled download is not a finished one");
        let kept = fs::metadata(part_path(&dest)).unwrap().len();
        assert!(kept > 0 && kept < 400_000, "kept a resumable partial, got {kept} bytes");
        let _ = fs::remove_dir_all(&dir);
    }

    /// A full disk does not empty itself while we wait. Retried like a dropped connection it costs
    /// twelve attempts and ~90 s of backoff to arrive at the same sentence the first attempt knew.
    #[test]
    fn a_full_disk_is_fatal_and_a_dropped_write_is_not() {
        let full = std::io::Error::from(std::io::ErrorKind::StorageFull);
        assert!(matches!(write_failed(Path::new("m.part"), full), FetchError::Fatal(_)));

        let denied = std::io::Error::from(std::io::ErrorKind::PermissionDenied);
        assert!(matches!(write_failed(Path::new("m.part"), denied), FetchError::Fatal(_)));

        let flaky = std::io::Error::from(std::io::ErrorKind::Interrupted);
        assert!(matches!(write_failed(Path::new("m.part"), flaky), FetchError::Transient(_)));
    }

    #[test]
    fn rejects_a_bad_checksum() {
        let body: Vec<u8> = (0..10_000u32).map(|i| i as u8).collect();
        let url = serve_once(body);
        let dir = std::env::temp_dir().join(format!("fm-fetch-bad-{}", std::process::id()));
        let _ = fs::create_dir_all(&dir);
        let dest = dir.join("m.gguf");
        let _ = fs::remove_file(&dest);

        let err = fetch(
            &Download { url: &url, dest: &dest, sha256: Some(&"00".repeat(32)) },
            &|_, _| {},
            &never,
        )
        .unwrap_err();
        assert!(err.to_string().contains("checksum mismatch"), "got: {err}");
        assert!(!dest.exists(), "no dest is left on a bad checksum");
        assert!(!part_path(&dest).exists(), "the corrupt partial is deleted");
        let _ = fs::remove_dir_all(&dir);
    }
}
