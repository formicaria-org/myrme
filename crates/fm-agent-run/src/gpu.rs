//! **Where the laptop model runs under `gpu = "auto"`: a discrete GPU when there is one, else the
//! CPU** (`decisions.md` 2026-10-23, *an integrated GPU is not "a GPU" for `auto`*;
//! `docs/research/2026-10-23-integrated-gpu-fallback.md`).
//!
//! `-ngl 99` alone offloads to whatever Vulkan device llama.cpp finds. On a laptop whose discrete GPU
//! is missing, that is the integrated one, and there the 4B model wrote at 5.7 tok/s against the
//! CPU's 14.4. So before launch the runtime is asked what it sees (`llama-bench --list-devices`
//! prints `| uma: 1 |` for a GPU that shares system memory), and when every device is integrated the
//! server is started with `--device none`.
//!
//! **Every failure keeps today's launch.** No probe binary, a probe that hangs or exits early, output
//! this cannot read: each answers [`Placement::AsConfigured`], so a machine with a working discrete
//! GPU never loses it to a mistake here.

use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// One Vulkan device, as the runtime reports it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Device {
    pub index: u32,
    pub name: String,
    /// `uma: 1` — the GPU shares system memory, i.e. an integrated one.
    pub integrated: bool,
}

/// What the launch does with the `-ngl` it was going to pass.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Placement {
    /// Launch as before.
    AsConfigured,
    /// Every GPU is integrated: `--device none`, so the model runs on the CPU. Carries their names
    /// for the log line.
    CpuOnly { integrated: Vec<String> },
}

/// The decision, kept apart from the probe so it is testable without a runtime. Only `auto` is
/// refined: `on` still offloads to any GPU (the way to force a strong integrated one), `off` is the
/// CPU already.
pub(crate) fn place(policy: &str, devices: Option<&[Device]>) -> Placement {
    if !policy.eq_ignore_ascii_case("auto") {
        return Placement::AsConfigured;
    }
    match devices {
        Some(d) if !d.is_empty() && d.iter().all(|x| x.integrated) => {
            Placement::CpuOnly { integrated: d.iter().map(|x| x.name.clone()).collect() }
        }
        _ => Placement::AsConfigured,
    }
}

/// The Vulkan devices in the runtime's own report, one per line of the form
/// `ggml_vulkan: 0 = <name> (<driver>) | uma: 1 | fp16: 1 | …`. A line this cannot read is skipped;
/// a report with none it can read is an empty list, which [`place`] treats as "no information".
pub(crate) fn parse(text: &str) -> Vec<Device> {
    text.lines()
        .filter_map(|line| {
            let rest = line.trim().strip_prefix("ggml_vulkan: ")?;
            let (index, rest) = rest.split_once(" = ")?;
            let index = index.trim().parse().ok()?;
            let (name, rest) = rest.split_once(" | uma: ")?;
            let integrated = match rest.chars().next()? {
                '0' => false,
                '1' => true,
                _ => return None,
            };
            Some(Device { index, name: name.trim().to_string(), integrated })
        })
        .collect()
}

/// Ask the staged runtime which Vulkan devices it sees. `None` when it cannot say: not a platform
/// this applies to, no `llama-bench` in the runtime, or no answer within a few seconds.
///
/// **macOS is left alone**: its runtime is the Metal build, and Apple's GPUs are integrated but fast.
pub(crate) fn probe(runtime: &Path) -> Option<Vec<Device>> {
    if cfg!(target_os = "macos") {
        return None;
    }
    let bin = runtime.join(format!("llama-bench{}", std::env::consts::EXE_SUFFIX));
    if !bin.exists() {
        return None;
    }
    let mut child = Command::new(&bin)
        .env("LD_LIBRARY_PATH", runtime)
        .arg("--list-devices")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .ok()?;
    // Both streams read while it runs, so a full pipe cannot stall it; the device lines are on stderr.
    let drain = |mut r: Box<dyn Read + Send>| {
        std::thread::spawn(move || {
            let mut s = String::new();
            let _ = r.read_to_string(&mut s);
            s
        })
    };
    let out = drain(Box::new(child.stdout.take()?));
    let err = drain(Box::new(child.stderr.take()?));
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(50)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
    let text = out.join().ok()? + &err.join().ok()?;
    Some(parse(&text))
}

#[cfg(test)]
mod tests {
    use super::*;

    const INTEGRATED: &str = "ggml_vulkan: Found 1 Vulkan devices:\nggml_vulkan: 0 = Example iGPU (Example open-source driver) | uma: 1 | fp16: 1 | bf16: 0 | fp4: 0 | warp size: 32 | shared memory: 49152 | int dot: 1 | matrix cores: none\nload_backend: loaded Vulkan backend from /x/libggml-vulkan.so\n";

    fn dev(index: u32, integrated: bool) -> Device {
        Device { index, name: format!("gpu {index}"), integrated }
    }

    #[test]
    fn it_reads_the_runtimes_device_lines() {
        assert_eq!(
            parse(INTEGRATED),
            vec![Device {
                index: 0,
                name: "Example iGPU (Example open-source driver)".into(),
                integrated: true
            }]
        );
        let two = "ggml_vulkan: 0 = A dGPU (driver) | uma: 0 | fp16: 1\nggml_vulkan: 1 = An iGPU (driver) | uma: 1 | fp16: 1\n";
        assert_eq!(parse(two).iter().map(|d| d.integrated).collect::<Vec<_>>(), [false, true]);
    }

    /// The "Found N devices" header, other backends' lines and a changed format are not devices.
    #[test]
    fn lines_it_cannot_read_are_not_devices() {
        assert!(parse("ggml_vulkan: Found 1 Vulkan devices:\nload_backend: loaded CPU backend\n")
            .is_empty());
        assert!(parse("ggml_vulkan: 0 = Example iGPU | unified: yes\n").is_empty());
        assert!(parse("ggml_vulkan: 0 = Example iGPU | uma: ? | fp16: 1\n").is_empty());
    }

    /// The whole policy: only `auto`, and only when every GPU is integrated.
    #[test]
    fn auto_avoids_an_integrated_gpu_only_when_it_is_the_only_kind() {
        assert_eq!(
            place("auto", Some(&[dev(0, true)])),
            Placement::CpuOnly { integrated: vec!["gpu 0".into()] }
        );
        assert_eq!(
            place("Auto", Some(&[dev(0, true), dev(1, true)])),
            place("auto", Some(&[dev(0, true), dev(1, true)]))
        );
        assert_eq!(
            place("auto", Some(&[dev(0, true), dev(1, false)])),
            Placement::AsConfigured,
            "a discrete GPU is there"
        );
        assert_eq!(place("auto", Some(&[dev(0, false)])), Placement::AsConfigured);
    }

    /// No information is never a reason to move off a GPU.
    #[test]
    fn no_answer_keeps_the_launch_as_configured() {
        assert_eq!(place("auto", None), Placement::AsConfigured, "the probe could not run");
        assert_eq!(place("auto", Some(&[])), Placement::AsConfigured, "no device it could read");
    }

    /// `on` forces any GPU, a strong integrated one included; `off` is already the CPU.
    #[test]
    fn on_and_off_are_unchanged() {
        assert_eq!(place("on", Some(&[dev(0, true)])), Placement::AsConfigured);
        assert_eq!(place("off", Some(&[dev(0, true)])), Placement::AsConfigured);
    }

    /// **The line's shape, against a real runtime.** The flag is in no structured output of the pinned
    /// build, so a runtime bump that changes the line must fail here rather than quietly keep the
    /// integrated GPU. Runs where a desktop runtime is staged (`MYRME_RUNTIME`, else `agents/runtime`);
    /// skipped elsewhere, as the gate's runners have none.
    #[test]
    fn the_staged_runtime_still_reports_devices_in_this_shape() {
        let runtime = std::env::var_os("MYRME_RUNTIME")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../agents/runtime"));
        if cfg!(target_os = "macos") || !runtime.join("llama-bench").exists() {
            eprintln!("skipped: no desktop runtime staged at {}", runtime.display());
            return;
        }
        let out = Command::new(runtime.join("llama-bench"))
            .env("LD_LIBRARY_PATH", &runtime)
            .arg("--list-devices")
            .output()
            .unwrap();
        let text = String::from_utf8_lossy(&out.stdout).into_owned()
            + &String::from_utf8_lossy(&out.stderr);
        let lines = text
            .lines()
            .filter(|l| {
                l.trim()
                    .strip_prefix("ggml_vulkan: ")
                    .and_then(|r| r.split_once(" = "))
                    .is_some_and(|(i, _)| i.trim().parse::<u32>().is_ok())
            })
            .count();
        assert_eq!(parse(&text).len(), lines, "every device line read:\n{text}");
        assert_eq!(probe(&runtime).map(|d| d.len()), Some(lines), "the probe agrees");
    }
}
