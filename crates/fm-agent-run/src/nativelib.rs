//! Find the app's **native library directory** at runtime, in pure Rust — no JVM/Kotlin shim.
//!
//! On Android an app may only `exec` a binary from its `nativeLibraryDir` (W^X/SELinux forbid it from
//! arbitrary app storage), so the bundled `llama-server` rides in `jniLibs/<abi>/` as a `lib*.so` and
//! is launched from there. Tauri's path API doesn't expose that directory, and asking the JVM for
//! `ApplicationInfo.nativeLibraryDir` would mean Kotlin plumbing in the (generated, un-committed)
//! Android project. But *our own* cdylib was loaded from exactly that directory — so `/proc/self/maps`
//! already names it. We read the mapping for our `.so` and take its parent. Pure string parsing, so
//! the logic is unit-tested on the desktop even though it only runs on the phone.

use std::path::{Path, PathBuf};

/// The directory holding the mapped library whose file name ends with `soname`, parsed from the text
/// of `/proc/self/maps`. `None` if no such absolute mapping is present.
///
/// Each `maps` line ends with the backing file's path (when it has one); on Android those paths carry
/// no spaces, so the last whitespace-separated field is the path.
pub fn lib_dir_from_maps(maps: &str, soname: &str) -> Option<PathBuf> {
    for line in maps.lines() {
        let path = line.rsplit(char::is_whitespace).next().unwrap_or("");
        if path.starts_with('/') && path.ends_with(soname) {
            return Path::new(path).parent().map(Path::to_path_buf);
        }
    }
    None
}

/// The app's native library directory, found by locating our own mapped `soname` in
/// `/proc/self/maps`. `None` if the file can't be read or no mapping matches.
///
/// **Android only, and gated so the compiler says so.** `/proc` is a Linux-kernel interface and
/// `nativeLibraryDir` is an Android concept; neither exists on iOS. Ungated, an iOS build of the
/// mobile shell compiled, installed, launched, and then told the user *"the assistant cannot locate
/// this app's own library directory"* — a sentence naming an Android idea, on a platform where the
/// whole approach is forbidden anyway (iOS permits no `exec` at all: `decisions.md#track-m`,
/// 2026-09-02). A compile error at the call site is the honest version of that.
///
/// The parsing half, [`lib_dir_from_maps`], stays `cfg`-free and unit-tested everywhere — the
/// pattern this repo already requires of a platform arm: a thin syscall wrapper over a tested core.
#[cfg(target_os = "android")]
pub fn native_lib_dir(soname: &str) -> Option<PathBuf> {
    let maps = std::fs::read_to_string("/proc/self/maps").ok()?;
    lib_dir_from_maps(&maps, soname)
}

#[cfg(test)]
mod tests {
    use super::*;

    // A representative slice of an Android /proc/self/maps: an anonymous mapping, a system lib, and
    // our own cdylib under the app's per-install lib dir.
    const MAPS: &str = "\
12c00000-12c01000 rw-p 00000000 00:00 0 \n\
7f8a000000-7f8a010000 r-xp 00000000 fd:03 42  /system/lib64/libc.so\n\
7f8b000000-7f8b400000 r-xp 00000000 fd:03 99  /data/app/~~AbC==/dev.formicaria.notes-XyZ==/lib/arm64/libformicaria_mobile_lib.so\n";

    #[test]
    fn finds_our_own_lib_dir() {
        assert_eq!(
            lib_dir_from_maps(MAPS, "libformicaria_mobile_lib.so").unwrap(),
            Path::new("/data/app/~~AbC==/dev.formicaria.notes-XyZ==/lib/arm64"),
        );
    }

    #[test]
    fn ignores_anonymous_and_unmatched_mappings() {
        assert!(lib_dir_from_maps(MAPS, "libnope.so").is_none());
        // A bare soname must not match the system libc line's directory.
        assert_eq!(
            lib_dir_from_maps(MAPS, "libc.so").unwrap(),
            Path::new("/system/lib64"),
        );
    }
}
