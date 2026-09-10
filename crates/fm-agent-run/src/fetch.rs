//! Model-shaped wrappers over the generic downloader in [`fm_fetch`].
//!
//! The engine — resume, checksum, the `.part` sidecar, the `Fatal`/`Transient` split — lives in
//! `fm-fetch` and is re-exported here so every existing caller keeps working. What stays is what
//! knows about *models*: the catalogue-driven `ensure_*` trio, and the runtime-archive extractors
//! with their keep-list, which are deliberately **not** general-purpose (they flatten entries and
//! drop everything but the binary, its libraries and the licence).

use crate::manifest::Manifest;
// The engine, re-exported: `fm_agent_run::fetch::{fetch, Download, FetchError}` is still the
// path every caller in this crate and in the mobile shell already uses.
pub use fm_fetch::{ensure_parent, fetch, part_path, verify, Download, FetchError};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Fetch the multimodal projector beside the weights, when the model has one and it is wanted.
///
/// Separate from [`ensure_model`] because it is a **separate choice**: it is another 836 MB for the
/// desktop pick, it buys exactly one capability (reading images), and a text-only model is a
/// perfectly good assistant without it. `Ok(None)` means "this model has no projector" — not a
/// failure; `serve.rs` sets `vision` from whether the file actually reached the command line, so a
/// missing projector disables image reading rather than producing an invented reading of a picture.
///
/// **Not checksum-pinned, and that is inherited, not chosen.** The catalogue pins a `sha256` for
/// the weights only; the projector rides the same pinned Hugging Face commit, which fixes the bytes
/// without a second hash to keep in step.
#[cfg(feature = "download")]
pub fn ensure_mmproj(
    models_dir: &Path,
    manifest: &Manifest,
    name: &str,
    on_progress: &dyn Fn(u64, Option<u64>),
    cancel: &dyn Fn() -> bool,
) -> Result<Option<PathBuf>, String> {
    let Some(model) = manifest.model(name) else {
        return Err(format!("model '{name}' is not in the manifest"));
    };
    let Some(file) = model.mmproj.as_deref() else {
        return Ok(None); // text-only: nothing to fetch, and nothing wrong
    };
    let dest = models_dir.join(file);
    if dest.exists() {
        return Ok(Some(dest));
    }
    let Some(url) = manifest.mmproj_url(name) else {
        return Ok(None);
    };
    let dl = Download { url: &url, dest: &dest, sha256: None };
    let mut stalls: u32 = 0;
    loop {
        match fetch(&dl, on_progress, cancel) {
            Ok(()) => return Ok(Some(dest)),
            Err(FetchError::Fatal(e)) => return Err(e),
            Err(FetchError::Transient(e)) => {
                stalls += 1;
                if stalls >= 6 {
                    return Err(format!("could not fetch the projector: {e}"));
                }
                std::thread::sleep(std::time::Duration::from_secs(2u64.pow(stalls.min(3))));
                if cancel() {
                    return Err("cancelled".into());
                }
            }
        }
    }
}

/// Ensure a prebuilt runtime is unpacked under `runtime_dir`, fetching and extracting its archive
/// on first enable. Returns the path of `want` (e.g. `llama-server`) inside that directory.
///
/// **The desktop half of what `jniLibs` does for the phone.** Android bundles `llama-server` in the
/// APK; a desktop download cannot, because the archive would then carry a runtime for one platform
/// and the wrong one for the others — so the binary is fetched, verified and unpacked on the
/// machine that will run it.
///
/// Three properties this must have, each learned from something already in this tree:
///
/// - **Verified before it is unpacked, never after.** The archive's `sha256` is checked by
///   [`fetch`] before a single entry is read, because unpacking an unverified archive has already
///   written attacker-chosen paths to disk by the time you notice.
/// - **The notice travels with the binary.** llama.cpp and whisper.cpp are MIT, and their licence
///   must accompany a redistributed build. `agents/fetch.sh` gets this right only by accident — it
///   copies the whole tarball — so anything named `LICENSE*` is extracted deliberately here.
/// - **No path escapes the destination.** `tar` will happily write `../../..` if an archive says
///   so; every entry is resolved and refused unless it lands under `runtime_dir`.
///
/// Idempotent: `want` already present means no network, so a sideloaded runtime is used as-is.
#[cfg(feature = "download")]
pub fn ensure_runtime(
    runtime_dir: &Path,
    rt: &crate::manifest::Runtime,
    want: &str,
    on_progress: &dyn Fn(u64, Option<u64>),
    cancel: &dyn Fn() -> bool,
) -> Result<PathBuf, String> {
    let target = runtime_dir.join(want);
    if target.exists() {
        return Ok(target);
    }
    ensure_parent(&target).map_err(|e| e.to_string())?;

    // Beside the destination, not in a temp dir: the same filesystem, so nothing later has to move
    // across a device boundary, and a cancelled fetch leaves its `.part` where a retry resumes it.
    // The name carries the format because the extractor dispatches on it — llama.cpp publishes
    // `.tar.gz` for Linux and macOS and `.zip` for Windows.
    let archive = runtime_dir.join(if rt.url.ends_with(".zip") {
        "runtime-archive.zip"
    } else {
        "runtime-archive.tar.gz"
    });
    let dl = Download { url: &rt.url, dest: &archive, sha256: Some(&rt.sha256) };
    let mut stalls: u32 = 0;
    loop {
        match fetch(&dl, on_progress, cancel) {
            Ok(()) => break,
            Err(FetchError::Fatal(e)) => return Err(e),
            Err(FetchError::Transient(e)) => {
                stalls += 1;
                if stalls >= 6 {
                    return Err(format!("could not fetch the runtime: {e}"));
                }
                std::thread::sleep(std::time::Duration::from_secs(2u64.pow(stalls.min(3))));
                if cancel() {
                    return Err("cancelled".into());
                }
            }
        }
    }

    let unpacked = if rt.url.ends_with(".zip") {
        unpack_zip(&archive, runtime_dir, want)
    } else {
        unpack_tar_gz(&archive, runtime_dir, want)
    };
    // The archive is large and single-use; keep the disk it borrowed either way, but never let its
    // removal mask the extraction's own error.
    let _ = std::fs::remove_file(&archive);
    unpacked?;

    if !target.exists() {
        return Err(format!(
            "the runtime archive did not contain '{want}' — it may have moved between releases"
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        // tar carries the mode, but an archive built elsewhere may not; a runtime that cannot be
        // executed fails much later and far less clearly.
        let _ = std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o755));
    }
    Ok(target)
}

/// Extract `want`, every shared library beside it, and any licence, into `dest`.
///
/// Flattened deliberately: these archives nest everything under `build/bin/`, and the runtime is
/// spawned with `dest` on the library search path, so a preserved tree would put the `.so` files
/// somewhere the loader is not looking.
#[cfg(feature = "download")]
fn unpack_tar_gz(archive: &Path, dest: &Path, want: &str) -> Result<(), String> {
    let f = std::fs::File::open(archive)
        .map_err(|e| format!("cannot open the runtime archive: {e}"))?;
    let mut ar = tar::Archive::new(flate2::read::GzDecoder::new(f));
    let entries = ar.entries().map_err(|e| format!("unreadable runtime archive: {e}"))?;
    for entry in entries {
        let mut entry =
            entry.map_err(|e| format!("unreadable entry in the runtime archive: {e}"))?;
        let path = entry.path().map_err(|e| format!("bad path in the runtime archive: {e}"))?;
        let Some(name) = path.file_name().and_then(|n| n.to_str()).map(str::to_string) else {
            continue;
        };
        let keep = name == want
            || name.ends_with(".so")
            || name.contains(".so.")
            || name.ends_with(".dylib")
            || name.ends_with(".dll")
            || name.to_ascii_uppercase().starts_with("LICENSE");
        if !keep {
            continue;
        }
        // **The escape check.** `name` is a single component by construction (`file_name`), so the
        // join cannot climb — this asserts that rather than trusting it, because the cost of being
        // wrong is a write outside the directory.
        let out = dest.join(&name);
        if out.parent() != Some(dest) {
            return Err(format!("refusing an archive entry that escapes its directory: {name}"));
        }
        entry.unpack(&out).map_err(|e| format!("could not unpack {name}: {e}"))?;
    }
    Ok(())
}

/// The same extraction, for the `.zip` Windows builds are published as.
///
/// Deliberately a mirror of [`unpack_tar_gz`]: the same keep-list, the same flattening, the same
/// refusal of any entry that would land outside the directory. Two formats, one policy — if these
/// two ever disagree about what is safe to write, the disagreement is the bug.
#[cfg(feature = "download")]
fn unpack_zip(archive: &Path, dest: &Path, want: &str) -> Result<(), String> {
    let f = std::fs::File::open(archive)
        .map_err(|e| format!("cannot open the runtime archive: {e}"))?;
    let mut ar = zip::ZipArchive::new(f).map_err(|e| format!("unreadable runtime archive: {e}"))?;
    for i in 0..ar.len() {
        let mut entry = ar.by_index(i).map_err(|e| format!("unreadable entry: {e}"))?;
        // `enclosed_name` is `None` for a path that tries to escape — the crate's own version of
        // the check the tar side makes, and the reason this does not re-implement it.
        let Some(path) = entry.enclosed_name() else { continue };
        let Some(name) = path.file_name().and_then(|n| n.to_str()).map(str::to_string) else {
            continue;
        };
        let keep = name == want
            || name.ends_with(".dll")
            || name.ends_with(".so")
            || name.contains(".so.")
            || name.ends_with(".dylib")
            || name.to_ascii_uppercase().starts_with("LICENSE");
        if !keep || entry.is_dir() {
            continue;
        }
        let out = dest.join(&name);
        if out.parent() != Some(dest) {
            return Err(format!("refusing an archive entry that escapes its directory: {name}"));
        }
        let mut w = std::fs::File::create(&out).map_err(|e| format!("cannot write {name}: {e}"))?;
        std::io::copy(&mut entry, &mut w).map_err(|e| format!("could not unpack {name}: {e}"))?;
    }
    Ok(())
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
    let model =
        manifest.model(name).ok_or_else(|| format!("model '{name}' is not in the manifest"))?;
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

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn ensure_model_uses_a_present_file_without_network() {
        // No server is started: if ensure_model touched the network this would hang/fail. A present,
        // unpinned file must be returned as-is (the sideloaded / already-fetched case).
        let m = Manifest::parse("[[models]]\nname = \"x\"\nrepo = \"r/x\"\nfile = \"x.gguf\"\n");
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

    /// Build a `.tar.gz` in memory the way the real runtime archives are shaped: everything nested
    /// under `build/bin/`, a licence at the root, and a file we do not want.
    fn runtime_archive(dest: &Path, extra: &[(&str, &[u8])]) {
        let f = fs::File::create(dest).unwrap();
        let enc = flate2::write::GzEncoder::new(f, flate2::Compression::fast());
        let mut ar = tar::Builder::new(enc);
        let mut add = |name: &str, body: &[u8]| {
            let mut h = tar::Header::new_gnu();
            h.set_size(body.len() as u64);
            h.set_mode(0o644);
            h.set_cksum();
            ar.append_data(&mut h, name, body).unwrap();
        };
        add("build/bin/llama-server", b"#!/bin/true\n");
        add("build/bin/libggml.so", b"not really a library");
        add("LICENSE", b"MIT");
        add("build/bin/README.md", b"docs we do not want");
        for (n, b) in extra {
            add(n, b);
        }
        ar.into_inner().unwrap().finish().unwrap();
    }

    #[test]
    #[cfg(feature = "download")]
    fn unpacking_keeps_the_binary_its_libraries_and_the_licence_and_drops_the_rest() {
        let dir = scratch("unpack-keeps");
        let archive = dir.join("rt.tar.gz");
        runtime_archive(&archive, &[]);

        unpack_tar_gz(&archive, &dir, "llama-server").expect("a well-formed archive unpacks");

        assert!(dir.join("llama-server").exists(), "the binary we asked for");
        assert!(dir.join("libggml.so").exists(), "and the library it loads, flattened beside it");
        assert!(dir.join("LICENSE").exists(), "the notice must travel with the binary");
        assert!(!dir.join("README.md").exists(), "everything else stays out");
        // Flattened, not nested: the runtime is spawned with this directory on the library path.
        assert!(!dir.join("build").exists(), "no tree is preserved");
        let _ = fs::remove_dir_all(&dir);
    }

    /// One tar entry, written **by hand**, so the name can be hostile.
    ///
    /// `tar::Builder` refuses to serialise a path containing `..` — which is a good default and
    /// exactly why it cannot produce the archive this test needs. A real attacker writes the bytes,
    /// so the test does too: 512-byte header, name at offset 0, checksum computed with the checksum
    /// field held as spaces.
    #[cfg(feature = "download")]
    fn hostile_entry(name: &str, body: &[u8]) -> Vec<u8> {
        let mut h = [0u8; 512];
        h[..name.len()].copy_from_slice(name.as_bytes());
        h[100..107].copy_from_slice(b"0000644"); // mode
        h[108..115].copy_from_slice(b"0000000"); // uid
        h[116..123].copy_from_slice(b"0000000"); // gid
        let size = format!("{:011o}", body.len());
        h[124..135].copy_from_slice(size.as_bytes());
        h[136..147].copy_from_slice(b"00000000000"); // mtime
        h[148..156].copy_from_slice(b"        "); // checksum field, as spaces, while summing
        h[156] = b'0'; // typeflag: regular file
        let sum: u32 = h.iter().map(|&b| b as u32).sum();
        let chk = format!("{sum:06o}\0 ");
        h[148..156].copy_from_slice(chk.as_bytes());

        let mut out = h.to_vec();
        out.extend_from_slice(body);
        out.resize(out.len().div_ceil(512) * 512, 0); // pad the data to a block
        out
    }

    /// **What this pins, and what it does not.** It asserts the *outcome* — a hostile entry never
    /// lands outside the runtime directory — not any particular layer. `tar`'s own `unpack` also
    /// refuses a path containing `..`, so removing our `file_name()` reduction alone does **not**
    /// fail this test; it was tried. The value is that it fails if the extractor is ever swapped
    /// for one without that protection, which is the realistic regression, and it documents the
    /// property a reader needs to trust. Attribution to a single layer would be a claim the test
    /// cannot support.
    #[test]
    #[cfg(feature = "download")]
    fn an_archive_entry_never_lands_outside_the_runtime_directory() {
        let dir = scratch("unpack-escape");
        fs::create_dir_all(&dir).unwrap();
        let archive = dir.join("rt.tar.gz");

        // A hostile archive: one entry climbing out of the destination, then the binary we asked
        // for, then the end-of-archive blocks.
        let mut raw = hostile_entry("../../escaped.so", b"pwned");
        raw.extend_from_slice(&hostile_entry("build/bin/llama-server", b"#!/bin/true\n"));
        raw.extend_from_slice(&[0u8; 1024]);
        let f = fs::File::create(&archive).unwrap();
        let mut enc = flate2::write::GzEncoder::new(f, flate2::Compression::fast());
        std::io::Write::write_all(&mut enc, &raw).unwrap();
        enc.finish().unwrap();

        let _ = unpack_tar_gz(&archive, &dir, "llama-server");

        // The one thing that must never happen.
        let escaped = dir.parent().unwrap().join("escaped.so");
        assert!(!escaped.exists(), "an entry wrote outside the runtime directory: {escaped:?}");
        // And the archive was genuinely processed — otherwise this passes for the wrong reason.
        assert!(dir.join("llama-server").exists(), "the legitimate entry still extracted");
        let _ = fs::remove_dir_all(&dir);
    }

    /// The zip path keeps the same policy as the tar one — proved on a real archive rather than
    /// assumed from the fact that the code looks similar.
    #[test]
    #[cfg(feature = "download")]
    fn the_zip_extractor_keeps_and_drops_exactly_what_the_tar_one_does() {
        let dir = scratch("unpack-zip");
        fs::create_dir_all(&dir).unwrap();
        let archive = dir.join("rt.zip");
        {
            let f = fs::File::create(&archive).unwrap();
            let mut z = zip::ZipWriter::new(f);
            let opts: zip::write::FileOptions<()> = zip::write::FileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated);
            for (name, body) in [
                ("build/bin/llama-server.exe", &b"MZ"[..]),
                ("build/bin/ggml.dll", &b"dll"[..]),
                ("LICENSE", &b"MIT"[..]),
                ("build/bin/README.md", &b"docs"[..]),
            ] {
                z.start_file(name, opts).unwrap();
                std::io::Write::write_all(&mut z, body).unwrap();
            }
            z.finish().unwrap();
        }

        unpack_zip(&archive, &dir, "llama-server.exe").expect("a well-formed zip unpacks");

        assert!(dir.join("llama-server.exe").exists(), "the binary we asked for");
        assert!(dir.join("ggml.dll").exists(), "and the library beside it, flattened");
        assert!(dir.join("LICENSE").exists(), "the notice travels with the binary");
        assert!(!dir.join("README.md").exists(), "everything else stays out");
        assert!(!dir.join("build").exists(), "no tree is preserved");
        let _ = fs::remove_dir_all(&dir);
    }

    /// **The two extractors are mirrors, so they must agree.** They are separate functions only
    /// because the formats differ; if they ever disagree about what is safe to write, the
    /// disagreement *is* the bug, and neither format's own test would notice.
    #[test]
    #[cfg(feature = "download")]
    fn tar_and_zip_keep_and_drop_the_same_things() {
        let dir = scratch("parity");
        fs::create_dir_all(&dir).unwrap();
        let entries: [(&str, &[u8]); 4] = [
            ("build/bin/llama-server", b"bin"),
            ("build/bin/libggml.so", b"lib"),
            ("LICENSE", b"MIT"),
            ("build/bin/README.md", b"docs"),
        ];

        let tar_dir = dir.join("tar");
        fs::create_dir_all(&tar_dir).unwrap();
        {
            let f = fs::File::create(dir.join("a.tar.gz")).unwrap();
            let enc = flate2::write::GzEncoder::new(f, flate2::Compression::fast());
            let mut ar = tar::Builder::new(enc);
            for (n, b) in entries {
                let mut h = tar::Header::new_gnu();
                h.set_size(b.len() as u64);
                h.set_mode(0o644);
                h.set_cksum();
                ar.append_data(&mut h, n, b).unwrap();
            }
            ar.into_inner().unwrap().finish().unwrap();
        }
        unpack_tar_gz(&dir.join("a.tar.gz"), &tar_dir, "llama-server").unwrap();

        let zip_dir = dir.join("zip");
        fs::create_dir_all(&zip_dir).unwrap();
        {
            let f = fs::File::create(dir.join("a.zip")).unwrap();
            let mut z = zip::ZipWriter::new(f);
            let opts: zip::write::FileOptions<()> = zip::write::FileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated);
            for (n, b) in entries {
                z.start_file(n, opts).unwrap();
                std::io::Write::write_all(&mut z, b).unwrap();
            }
            z.finish().unwrap();
        }
        unpack_zip(&dir.join("a.zip"), &zip_dir, "llama-server").unwrap();

        let listing = |d: &Path| {
            let mut v: Vec<String> = fs::read_dir(d)
                .unwrap()
                .filter_map(|e| e.ok().map(|e| e.file_name().to_string_lossy().into_owned()))
                .collect();
            v.sort();
            v
        };
        assert_eq!(
            listing(&tar_dir),
            listing(&zip_dir),
            "one policy, two formats — these must not drift apart"
        );
        assert_eq!(listing(&tar_dir), vec!["LICENSE", "libggml.so", "llama-server"]);
        let _ = fs::remove_dir_all(&dir);
    }

    /// **Are the pinned runtimes still the bytes we recorded?**
    ///
    /// An upstream release asset can be re-uploaded, and then every new install fails its checksum
    /// with nothing here having noticed first. `#[ignore]`d because it downloads ~110 MB across
    /// five archives — run it with `pixi run check-pins`, and before cutting a release.
    ///
    /// Skips rather than fails when the network is unreachable, the shape `fm-app`'s acquire tests
    /// already use: a gate that fails on a train is a gate people learn to ignore.
    #[test]
    #[ignore = "downloads ~110 MB; run via `pixi run check-pins`"]
    #[cfg(feature = "download")]
    fn every_pinned_runtime_still_hashes_to_what_the_catalogue_records() {
        let manifest =
            crate::manifest::Manifest::parse(include_str!("../../../agents/models.toml"));
        let dir = scratch("pins");
        fs::create_dir_all(&dir).unwrap();
        let mut checked = 0;
        // Walk the catalogue, never a list beside it: a pin the list forgot is a pin nobody checks.
        let keys = manifest.runtime_keys();
        assert!(!keys.is_empty(), "the catalogue pins no runtimes at all");
        for key in &keys {
            let Some(rt) = manifest.runtime(key) else { continue };
            let dest = dir.join(key);
            let dl = Download { url: &rt.url, dest: &dest, sha256: Some(&rt.sha256) };
            match fetch(&dl, &|_, _| {}, &never) {
                Ok(()) => checked += 1,
                Err(FetchError::Transient(e)) => {
                    eprintln!("skipping {key}: network unreachable ({e})");
                }
                Err(FetchError::Fatal(e)) => {
                    panic!("{key} no longer matches its pinned sha256, or cannot be fetched: {e}")
                }
            }
            let _ = fs::remove_file(&dest);
        }
        eprintln!("verified {checked} of {} pinned runtime archive(s)", keys.len());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    #[cfg(feature = "download")]
    fn a_runtime_already_present_costs_no_network() {
        let dir = scratch("runtime-present");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("llama-server"), b"already here").unwrap();
        // An unreachable URL and a checksum that could never match: if either were consulted this
        // would fail, which is the point — a sideloaded runtime is used as-is.
        let rt = crate::manifest::Runtime {
            url: "http://127.0.0.1:1/never".into(),
            sha256: "0".repeat(64),
        };
        let got = ensure_runtime(&dir, &rt, "llama-server", &|_, _| {}, &never)
            .expect("a present runtime returns without fetching");
        assert_eq!(got, dir.join("llama-server"));
        let _ = fs::remove_dir_all(&dir);
    }
}
