//! In-app model download — **resumable + checksum-verified**, so a phone with no `curl` can fetch a
//! ~150 MB GGUF on first enable without re-downloading from scratch on every dropped connection.
//!
//! The bytes stream to a `<dest>.part` sidecar; a re-run resumes it with an HTTP `Range` request, and
//! `dest` only ever appears once the whole file is present and (if a checksum was given) verified — so
//! the runner never finds a half-written model. Behind the `download` feature (agent-only): it pulls a
//! pure-Rust blocking HTTPS client (`ureq`, default `rustls` TLS with bundled roots) so it needs no
//! system TLS and cross-compiles to Android, plus `sha2` to verify. The notes core links neither.

use sha2::{Digest, Sha256};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

/// One download: from `url` to `dest`, optionally verified against `sha256` (hex, case-insensitive).
pub struct Download<'a> {
    pub url: &'a str,
    pub dest: &'a Path,
    pub sha256: Option<&'a str>,
}

/// Fetch `d`, resuming a partial `<dest>.part` if present. `on_progress(done, total)` fires as bytes
/// arrive (`total` is `None` when the server sends no length). **Idempotent:** an existing `dest`
/// returns `Ok` without touching the network — verifying it first if a checksum was given.
pub fn fetch(d: &Download, on_progress: &dyn Fn(u64, Option<u64>)) -> Result<(), String> {
    // Already in place? Trust it if we can verify it; with no checksum to check against, a present
    // dest is taken as done (the caller chose not to pin one). A wrong-bytes dest is removed + refetched.
    if d.dest.exists() {
        match d.sha256 {
            Some(want) if verify(d.dest, want)? => return Ok(()),
            Some(_) => {
                let _ = fs::remove_file(d.dest);
            }
            None => return Ok(()),
        }
    }

    let part = part_path(d.dest);
    let have = fs::metadata(&part).map(|m| m.len()).unwrap_or(0);

    let mut req = ureq::get(d.url);
    if have > 0 {
        req = req.set("Range", &format!("bytes={have}-"));
    }
    let resp = req.call().map_err(|e| format!("GET {}: {e}", d.url))?;
    let status = resp.status();

    // 206 = the server honoured the range → append; 200 = it ignored it → start over; 416 = the range
    // is past the end, i.e. `.part` is already the whole file → straight to verify.
    let (mut file, mut done, read_body) = match status {
        206 => (open_append(&part)?, have, true),
        200 => (open_truncate(&part)?, 0, true),
        416 => (open_append(&part)?, have, false),
        s => return Err(format!("GET {}: unexpected status {s}", d.url)),
    };

    let total = resp
        .header("Content-Length")
        .and_then(|s| s.trim().parse::<u64>().ok())
        .map(|len| done + len);
    on_progress(done, total);

    if read_body {
        let mut reader = resp.into_reader();
        let mut buf = [0u8; 64 * 1024];
        loop {
            let n = reader.read(&mut buf).map_err(|e| format!("read {}: {e}", d.url))?;
            if n == 0 {
                break;
            }
            file.write_all(&buf[..n]).map_err(|e| format!("write {}: {e}", part.display()))?;
            done += n as u64;
            on_progress(done, total);
        }
    }
    file.flush().map_err(|e| e.to_string())?;
    drop(file);

    if let Some(want) = d.sha256 {
        if !verify(&part, want)? {
            // Corrupt: drop the partial so the next attempt starts clean rather than resuming garbage.
            let _ = fs::remove_file(&part);
            return Err(format!("checksum mismatch for {} — deleted the partial, retry", d.dest.display()));
        }
    }
    fs::rename(&part, d.dest)
        .map_err(|e| format!("rename {} -> {}: {e}", part.display(), d.dest.display()))?;
    Ok(())
}

/// `<dest>.part` — the resume sidecar beside the final file.
fn part_path(dest: &Path) -> PathBuf {
    let mut s = dest.as_os_str().to_owned();
    s.push(".part");
    PathBuf::from(s)
}

fn open_append(p: &Path) -> Result<File, String> {
    ensure_parent(p)?;
    OpenOptions::new()
        .create(true)
        .append(true)
        .open(p)
        .map_err(|e| format!("open {}: {e}", p.display()))
}

fn open_truncate(p: &Path) -> Result<File, String> {
    ensure_parent(p)?;
    OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(p)
        .map_err(|e| format!("open {}: {e}", p.display()))
}

fn ensure_parent(p: &Path) -> Result<(), String> {
    if let Some(dir) = p.parent() {
        fs::create_dir_all(dir).map_err(|e| format!("mkdir {}: {e}", dir.display()))?;
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
    use std::io::Read as _;
    use std::net::TcpListener;
    use std::thread;

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
        fetch(&Download { url: &url, dest: &dest, sha256: Some(&want) }, &|d, _| last.set(d)).unwrap();

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

        fetch(&Download { url: &url, dest: &dest, sha256: Some(&want) }, &|_, _| {}).unwrap();

        assert_eq!(fs::read(&dest).unwrap(), body, "resumed file equals the whole body");
        let _ = fs::remove_dir_all(&dir);
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
        )
        .unwrap_err();
        assert!(err.contains("checksum mismatch"), "got: {err}");
        assert!(!dest.exists(), "no dest is left on a bad checksum");
        assert!(!part_path(&dest).exists(), "the corrupt partial is deleted");
        let _ = fs::remove_dir_all(&dir);
    }
}
