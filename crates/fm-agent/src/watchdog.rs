//! A lightweight, cross-platform **watchdog** over the model process: it samples device resources on
//! an interval and **kills the process** the instant a threshold is crossed, a timeout hits, the stop
//! signal is set, or resources cannot be read.
//!
//! **Fail-closed — "better safe than sorry".** Every non-completion path ends in "the model was
//! stopped"; the watchdog would rather kill the model than run the device blind or push its luck. If
//! it cannot read the resources, it stops. The safe failure is always ours, never the device's.
//!
//! **Cross-platform by construction.** Process control is `std::process` (SIGKILL on Unix,
//! TerminateProcess on Windows). The one OS-specific bit — reading the device's resources — sits
//! behind [`ResourceMonitor`]; a Linux `/proc` reader ships, other OSes implement the same trait, and
//! until one does the default monitor *fails closed* so nothing ever runs unmonitored. The whole cost
//! when idle is zero (it only runs while supervising), and while running it is one cheap `/proc` read
//! every couple of seconds.

use std::process::{Child, ExitStatus};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Something went wrong reading the device or supervising the process.
#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct WatchdogError(String);

impl WatchdogError {
    pub fn new(msg: impl Into<String>) -> Self {
        Self(msg.into())
    }
}

/// A device-resource snapshot the watchdog judges. OS-agnostic; each OS fills what it can read.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Resources {
    /// Memory available to start new work without swapping, in bytes (Linux `MemAvailable`).
    pub mem_available_bytes: u64,
    /// 1-minute load average divided by core count — `1.0` means "as busy as it has cores".
    pub load_per_core: f32,
}

/// Reading the device's resources — the one OS-specific seam. General on purpose: a new OS is a new
/// impl and nothing else changes. A fake stands in for it in tests.
pub trait ResourceMonitor {
    fn sample(&self) -> Result<Resources, WatchdogError>;
}

/// The thresholds the watchdog will not let the device cross, plus how often it checks. Pick
/// conservative floors and a hard timeout — the point is to *not push our luck*.
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    /// Stop if available memory drops below this — the device-safety floor.
    pub min_mem_available_bytes: u64,
    /// Stop if load per core climbs above this — keeps the machine responsive.
    pub max_load_per_core: f32,
    /// Stop if the run exceeds this wall-clock, whatever else is true.
    pub max_duration: Duration,
    /// How often to sample. Short enough to react, long enough to stay lightweight.
    pub poll_interval: Duration,
}

impl Limits {
    /// Conservative defaults for a **one-shot** run — a caller tunes them per device (a phone floor is
    /// not a laptop floor). The 300 s bound is a per-*run* backstop; do not use it for a resident
    /// server (see [`resident`](Self::resident)).
    pub fn conservative() -> Self {
        Self {
            min_mem_available_bytes: 1_500_000_000, // keep ~1.5 GB free
            max_load_per_core: 0.9,
            max_duration: Duration::from_secs(300),
            poll_interval: Duration::from_secs(2),
        }
    }

    /// For a model kept **warm for a whole session** (the @name watcher: "on with the app, off with
    /// it"). Keeps the **memory floor** (the real OOM danger) and the off-switch, but drops both the
    /// wall-clock bound and the load-average ceiling:
    /// - no wall-clock — a per-turn cap belongs on the model *call* (`OpenAiStep`'s timeout), not on
    ///   the long-lived server, or the assistant is SIGKILLed mid-session (the 300 s cliff);
    /// - no load ceiling — the model *is* the workload the user asked for; killing it for the CPU its
    ///   own bounded (batch=1, capped-tokens) inference uses is self-defeating, and load-average is a
    ///   poor phone governor anyway (it lags bursts and reads oddly on Android). Memory is the safety;
    ///   thermal headroom is the right *additional* phone signal, a later `ResourceMonitor` refinement.
    pub fn resident() -> Self {
        Self {
            max_duration: Duration::MAX,
            max_load_per_core: f32::INFINITY,
            ..Self::conservative()
        }
    }
}

/// One check's verdict: healthy, or a breach with a human-readable reason.
#[derive(Debug, Clone, PartialEq)]
pub enum Health {
    Ok,
    Breach(String),
}

/// Judge a snapshot against the limits. **Pure** — the heart of the watchdog, tested in isolation.
pub fn assess(r: &Resources, limits: &Limits) -> Health {
    if r.mem_available_bytes < limits.min_mem_available_bytes {
        return Health::Breach(format!(
            "free memory {} MB is below the {} MB floor",
            r.mem_available_bytes / 1_000_000,
            limits.min_mem_available_bytes / 1_000_000
        ));
    }
    if r.load_per_core > limits.max_load_per_core {
        return Health::Breach(format!(
            "load per core {:.2} is above the {:.2} ceiling",
            r.load_per_core, limits.max_load_per_core
        ));
    }
    Health::Ok
}

/// The off-switch: a shared flag anyone can trip to stop the model immediately. Cheap to clone and
/// check — this is what "easily stoppable" means in practice. Hold one, hand a clone to the watchdog.
#[derive(Debug, Clone, Default)]
pub struct StopFlag(Arc<AtomicBool>);

impl StopFlag {
    pub fn new() -> Self {
        Self::default()
    }
    /// Ask the watchdog to stop the model at its next check (within one poll interval).
    pub fn stop(&self) {
        self.0.store(true, Ordering::SeqCst);
    }
    pub fn is_stopped(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

/// Why supervision ended.
#[derive(Debug)]
pub enum Outcome {
    /// The model finished on its own, within limits.
    Completed(ExitStatus),
    /// The watchdog killed it — the reason (a breach, the timeout, or a stop request).
    Stopped(String),
    /// Supervision itself failed; the child was killed to be safe.
    Failed(WatchdogError),
}

/// A watchdog over a [`ResourceMonitor`].
pub struct Watchdog<M: ResourceMonitor> {
    monitor: M,
    limits: Limits,
}

impl<M: ResourceMonitor> Watchdog<M> {
    pub fn new(monitor: M, limits: Limits) -> Self {
        Self { monitor, limits }
    }

    /// Supervise `child` until it finishes, a limit is crossed, `stop` is tripped, or resources
    /// cannot be read (fail-closed → stop). Blocks the calling thread; run it on its own thread and
    /// hold the [`StopFlag`] to stop the model from anywhere. **On every non-`Completed` path the
    /// child is killed and reaped**, so nothing is left running.
    pub fn supervise(&self, child: &mut Child, stop: &StopFlag) -> Outcome {
        let start = Instant::now();
        loop {
            // Finished on its own?
            match child.try_wait() {
                Ok(Some(status)) => return Outcome::Completed(status),
                Ok(None) => {}
                Err(e) => {
                    return self.kill(
                        child,
                        Outcome::Failed(WatchdogError::new(format!(
                            "cannot poll the model process: {e}"
                        ))),
                    )
                }
            }
            // Stop requested — the easy off-switch.
            if stop.is_stopped() {
                return self.kill(child, Outcome::Stopped("stopped by request".into()));
            }
            // Hard wall-clock bound.
            if start.elapsed() >= self.limits.max_duration {
                let secs = self.limits.max_duration.as_secs();
                return self
                    .kill(child, Outcome::Stopped(format!("exceeded the {secs}s time limit")));
            }
            // Resource check — fail-closed: if we cannot read the device, we do not run blind.
            match self.monitor.sample() {
                Ok(r) => {
                    if let Health::Breach(why) = assess(&r, &self.limits) {
                        return self.kill(
                            child,
                            Outcome::Stopped(format!("device threshold crossed: {why}")),
                        );
                    }
                }
                Err(e) => {
                    return self.kill(
                        child,
                        Outcome::Stopped(format!(
                            "cannot read device resources ({e}) — stopping rather than run blind"
                        )),
                    )
                }
            }
            std::thread::sleep(self.limits.poll_interval);
        }
    }

    /// Kill and reap the child, then return `outcome`. Best-effort: it may already be dead.
    fn kill(&self, child: &mut Child, outcome: Outcome) -> Outcome {
        let _ = child.kill();
        let _ = child.wait();
        outcome
    }
}

/// The default monitor: reads the host it runs on. Linux and Android read `/proc`, Windows asks
/// `GlobalMemoryStatusEx`, macOS asks the mach kernel; anything else **fails closed**, so nothing
/// ever runs unmonitored.
pub struct SystemMonitor;

impl ResourceMonitor for SystemMonitor {
    fn sample(&self) -> Result<Resources, WatchdogError> {
        // Android is a Linux kernel — it has `/proc/meminfo`/`/proc/loadavg` too — so it shares this
        // path (its `target_os` is "android", not "linux", which is why it was fail-closing before).
        #[cfg(any(target_os = "linux", target_os = "android"))]
        {
            proc_sample()
        }
        #[cfg(target_os = "windows")]
        {
            windows_sample()
        }
        #[cfg(target_os = "macos")]
        {
            macos_sample()
        }
        #[cfg(target_os = "ios")]
        {
            ios_sample()
        }
        #[cfg(not(any(
            target_os = "linux",
            target_os = "android",
            target_os = "windows",
            target_os = "macos",
            target_os = "ios"
        )))]
        {
            Err(WatchdogError::new(
                "resource monitoring is not implemented on this OS yet — refusing to run the model unmonitored",
            ))
        }
    }
}

/// Turn a raw memory reading into [`Resources`], **refusing an implausible one**.
///
/// This is the guard that makes the platform arms below safe to write on a machine that cannot
/// compile them. A wrong struct layout or a misread field does not usually produce a *plausible*
/// wrong number — it produces `0`, or something astronomically large. Either would be acted on:
/// zero refuses every launch (annoying but safe), and a huge value **admits a model onto a machine
/// with no room for it**, which is the failure the whole preflight exists to prevent.
///
/// So the arithmetic and the bounds live here, `cfg`-free, compiled and unit-tested on every
/// platform including the one this was written on. The per-OS code below does one syscall and hands
/// its number to this.
///
/// The ceiling is 1 PiB: far above any machine this will run on for years, far below the values a
/// misread 64-bit field produces.
fn plausible(mem_available_bytes: u64, load_per_core: f32) -> Result<Resources, WatchdogError> {
    const CEILING: u64 = 1 << 50; // 1 PiB
    if mem_available_bytes == 0 {
        return Err(WatchdogError::new(
            "this machine reported no available memory at all — refusing to start the model on a reading that cannot be right",
        ));
    }
    if mem_available_bytes >= CEILING {
        return Err(WatchdogError::new(format!(
            "this machine reported {mem_available_bytes} bytes of available memory, which cannot be right — refusing rather than trusting it"
        )));
    }
    Ok(Resources { mem_available_bytes, load_per_core })
}

/// Windows: `GlobalMemoryStatusEx`, the documented way to ask how much physical memory is free.
///
/// **Hand-declared rather than pulling in `windows-sys`**, which is a very large crate for one
/// call — the same stance that keeps a hand-rolled HTTP client in `fm_agent::http` instead of a
/// framework. The struct is `MEMORYSTATUSEX` verbatim and has been stable since Windows 2000;
/// `dwLength` must be set to its size before the call, which is the one thing that goes wrong.
///
/// No load average: Windows has none, and `Limits::resident()` sets the load ceiling to infinity,
/// so memory is the whole judgment — exactly as on Android, where `/proc/loadavg` is unreadable.
#[cfg(target_os = "windows")]
fn windows_sample() -> Result<Resources, WatchdogError> {
    #[repr(C)]
    struct MemoryStatusEx {
        length: u32,
        memory_load: u32,
        total_phys: u64,
        avail_phys: u64,
        total_page_file: u64,
        avail_page_file: u64,
        total_virtual: u64,
        avail_virtual: u64,
        avail_extended_virtual: u64,
    }
    // SAFETY: `GlobalMemoryStatusEx` writes exactly `length` bytes into the struct we own, and we
    // set `length` to its true size. Every field is a plain integer; there are no pointers or
    // handles to get wrong, and the call cannot fail in a way that leaves the struct partly written
    // while returning non-zero.
    unsafe extern "system" {
        fn GlobalMemoryStatusEx(buffer: *mut MemoryStatusEx) -> i32;
    }
    let mut status = MemoryStatusEx {
        length: std::mem::size_of::<MemoryStatusEx>() as u32,
        memory_load: 0,
        total_phys: 0,
        avail_phys: 0,
        total_page_file: 0,
        avail_page_file: 0,
        total_virtual: 0,
        avail_virtual: 0,
        avail_extended_virtual: 0,
    };
    let ok = unsafe { GlobalMemoryStatusEx(&mut status) };
    if ok == 0 {
        return Err(WatchdogError::new(
            "Windows would not report this machine's memory — refusing to start the model unmonitored",
        ));
    }
    plausible(status.avail_phys, 0.0)
}

/// macOS: the mach kernel's own VM statistics, through `libc` rather than hand-declared bindings —
/// the struct is large and getting one field's offset wrong is exactly the silent-wrong-number
/// failure [`plausible`] exists to catch, so it is not written out by hand here.
///
/// **Available, not free.** macOS keeps very little memory "free": pages that a new process could
/// have are counted `inactive` (evictable file cache) and `purgeable`. Counting only `free_count`
/// would under-report by gigabytes on a healthy machine and refuse launches that would have been
/// perfectly fine. Under-reporting is the safe direction, but not when it makes the feature
/// unusable, so the three are summed — which is what every memory tool on the platform does.
///
/// Speculative pages are deliberately **not** counted: they are read-ahead the kernel expects to
/// use, and claiming them would tip the estimate optimistic, which is the one direction that hurts.
#[cfg(target_os = "macos")]
fn macos_sample() -> Result<Resources, WatchdogError> {
    let mut stats: libc::vm_statistics64 = unsafe { std::mem::zeroed() };
    let mut count = (std::mem::size_of::<libc::vm_statistics64>() / std::mem::size_of::<u32>())
        as libc::mach_msg_type_number_t;
    // SAFETY: `host_statistics64` fills `count` 32-bit words into a struct we own and sized from
    // that same type. `mach_host_self()` returns a port that does not need releasing here.
    //
    // `mach_host_self` is deprecated in `libc` in favour of the `mach2` crate. Kept as-is: it is
    // deprecated, not removed, and taking a whole crate for one call is the trade this project
    // declines elsewhere too (a hand-rolled HTTP client rather than a framework, hand-declared
    // Windows FFI rather than `windows-sys`). If `libc` ever removes it, `pixi run -e cross
    // check-cross` fails on the spot — which is the arrangement that makes keeping it defensible.
    #[allow(deprecated)]
    let rc = unsafe {
        libc::host_statistics64(
            libc::mach_host_self(),
            libc::HOST_VM_INFO64,
            &mut stats as *mut _ as *mut libc::integer_t,
            &mut count,
        )
    };
    if rc != libc::KERN_SUCCESS {
        return Err(WatchdogError::new(
            "macOS would not report this machine's memory — refusing to start the model unmonitored",
        ));
    }
    // `vm_page_size` is the kernel's own page size for this machine, not a compile-time guess.
    let page = unsafe { libc::vm_page_size } as u64;
    let usable = (stats.free_count as u64)
        .saturating_add(stats.inactive_count as u64)
        .saturating_add(stats.purgeable_count as u64);
    plausible(usable.saturating_mul(page), 0.0)
}

/// iOS: `os_proc_available_memory()`, which is a different question from every other arm here —
/// and the right one.
///
/// **Not "how much memory does this machine have free".** iOS kills a process that exceeds a
/// per-process resident limit (jetsam); that limit is per-device, undocumented, and unrelated to
/// free physical memory, so a `host_statistics64` reading — the macOS arm right above — would be
/// confidently wrong here in the one direction that hurts: optimistic. `os_proc_available_memory`
/// answers "how many more bytes may *this app* allocate before it is killed", which is exactly
/// what [`crate::preflight::admit`] needs and the only honest number iOS offers.
///
/// **Hand-declared, deliberately.** It lives in `<os/proc.h>` (iOS 13+) and `libc` does not expose
/// it. The same trade as the Windows arm below, whose comment states the stance: a hand-declared
/// symbol beats a large crate for one call. `cargo check --target aarch64-apple-ios` type-checks
/// the declaration on this machine, which is the arrangement that makes an unverifiable arm
/// defensible — the same one `check-cross` was built for after `die_with_supervisor` shipped
/// calling Linux's `prctl` under `#[cfg(unix)]`.
///
/// **It returns 0 when it cannot answer**, and [`plausible`] refuses zero — so an iOS build with no
/// usable reading declines to start the model rather than guessing, which is the behaviour every
/// other arm here already has.
///
/// Load is `0.0` for the same reason as macOS and Android: there is no accessible load average, and
/// on a battery device memory is the governor anyway (see `Limits::resident`).
///
/// **Never widen the macOS arm to cover iOS — and note the compiler will not stop you.** It looks
/// like the obvious tidy-up, and `libc` *does* expose `host_statistics64` and `mach_host_self` for
/// iOS targets, so `#[cfg(any(target_os = "macos", target_os = "ios"))]` compiles clean (measured:
/// one dead-code warning, nothing else). That is worse than a build error. It would hand iOS the
/// machine's free *physical* memory in place of this process's jetsam headroom — a plausible number,
/// far too large, in the one direction [`plausible`] cannot catch and `admit` acts on. A wrong
/// reading that refuses is safe; a wrong reading that admits is the failure preflight exists for.
#[cfg(target_os = "ios")]
fn ios_sample() -> Result<Resources, WatchdogError> {
    extern "C" {
        /// <os/proc.h>, iOS 13.0+. Bytes this process may still allocate before jetsam; 0 when
        /// the OS declines to answer.
        fn os_proc_available_memory() -> libc::size_t;
    }
    // SAFETY: a nullary call into libSystem returning an integer. No pointers, no ownership.
    let available = unsafe { os_proc_available_memory() } as u64;
    plausible(available, 0.0)
}

/// Read `/proc` for available memory + load. Shared by Linux and Android (both expose these).
#[cfg(any(target_os = "linux", target_os = "android"))]
fn proc_sample() -> Result<Resources, WatchdogError> {
    let meminfo = std::fs::read_to_string("/proc/meminfo")
        .map_err(|e| WatchdogError::new(format!("cannot read /proc/meminfo: {e}")))?;
    // `/proc/loadavg` is **denied to apps on Android** (SELinux) — and load is not the governor on a
    // phone anyway; memory is (see `Limits::resident`). So an unreadable loadavg is tolerated as "load
    // unknown", and only unreadable MEMORY fails closed — that is what actually prevents an OOM/crash.
    // On the desktop loadavg is always readable, so nothing changes there.
    //
    // **On Android we do not even attempt the read**, which is not the same as tolerating its
    // failure: the kernel audits every *denied* open, so a tolerated read at the poll interval —
    // two models × one sample each, every 2 s — wrote ~1 `avc: denied` line per second into logcat
    // forever. Measured on the owner's phone 2026-07-31: the app's own log buffer was ~90 %
    // `name="loadavg"` denials, which is both noise in the one diagnostic channel a phone has and a
    // syscall per poll that can never succeed. A denial you have decided to live with should not be
    // re-provoked on a timer.
    #[cfg(target_os = "android")]
    let loadavg = String::new();
    #[cfg(not(target_os = "android"))]
    let loadavg = std::fs::read_to_string("/proc/loadavg").unwrap_or_default();
    parse_proc(&meminfo, &loadavg, proc_cores())
}

/// The system's core count for the load-per-core denominator. `available_parallelism()` returns 1 on
/// some Android builds (affinity/bionic quirk), which would inflate load-per-core ~8× and make the
/// watchdog kill the model for the load its *own* inference creates. Count `/proc/cpuinfo` instead,
/// which lists every online core, and fall back to `available_parallelism`.
#[cfg(any(target_os = "linux", target_os = "android"))]
fn proc_cores() -> f32 {
    let counted = std::fs::read_to_string("/proc/cpuinfo")
        .map(|s| s.lines().filter(|l| l.starts_with("processor")).count())
        .unwrap_or(0);
    let n = if counted > 0 {
        counted
    } else {
        std::thread::available_parallelism().map(|c| c.get()).unwrap_or(1)
    };
    n.max(1) as f32
}

/// Parse the two `/proc` texts into [`Resources`]. Pure, so it is tested on captured samples. Uses
/// `MemAvailable` (not `MemFree`) — the kernel's own honest "how much can I use" figure.
#[cfg(any(target_os = "linux", target_os = "android"))]
fn parse_proc(meminfo: &str, loadavg: &str, cores: f32) -> Result<Resources, WatchdogError> {
    let mem_available_kb = meminfo
        .lines()
        .find_map(|l| l.strip_prefix("MemAvailable:"))
        .and_then(|v| v.trim().trim_end_matches("kB").trim().parse::<u64>().ok())
        .ok_or_else(|| WatchdogError::new("no MemAvailable in /proc/meminfo"))?;
    // Unreadable/absent loadavg (Android, or a `/proc` without it) ⇒ load unknown = 0.0: memory does
    // the gating. We never fail the whole sample for a missing *load* figure.
    let load1 =
        loadavg.split_whitespace().next().and_then(|v| v.parse::<f32>().ok()).unwrap_or(0.0);
    // Through the same guard as every other arm — four of four, rather than three of four.
    // `/proc` is text, so it cannot misread a struct layout the way an FFI arm can; but a
    // `MemAvailable` of 0 should refuse here exactly as it does on Windows, and `* 1024` on a
    // garbage value is an overflow panic in debug and a wrap in release. `saturating_mul` then
    // lands on the ceiling, which `plausible` rejects — the failure mode this guard is for.
    plausible(mem_available_kb.saturating_mul(1024), load1 / cores.max(1.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn good() -> Resources {
        Resources { mem_available_bytes: 8_000_000_000, load_per_core: 0.2 }
    }

    /// **The guard that makes the Windows, macOS and iOS arms safe to write blind.**
    ///
    /// Those three call a syscall this machine cannot compile, let alone run. What they hand back
    /// goes through `plausible`, which is compiled and tested everywhere — so the dangerous
    /// outcome, a wrong-but-believable number admitting a model onto a machine with no room, is
    /// guarded by code that *is* exercised. A misread field yields 0 or something astronomical;
    /// both must refuse.
    #[test]
    fn an_implausible_memory_reading_is_refused_rather_than_acted_on() {
        assert!(plausible(0, 0.0).is_err(), "zero cannot be right, and admitting on it is worse");
        assert!(plausible(1 << 50, 0.0).is_err(), "a petabyte is a misread field, not a machine");
        assert!(plausible(u64::MAX, 0.0).is_err(), "the classic all-ones misread");

        let ok = plausible(8_000_000_000, 0.0).expect("8 GB is an ordinary reading");
        assert_eq!(ok.mem_available_bytes, 8_000_000_000);
        assert_eq!(ok.load_per_core, 0.0, "no load average off Linux — memory is the judgment");
    }

    /// A refusal must say something a person can act on, like every other capability message here.
    #[test]
    fn an_implausible_reading_says_what_it_saw() {
        let why = plausible(u64::MAX, 0.0).unwrap_err().to_string();
        assert!(why.contains(&u64::MAX.to_string()), "name the number: {why}");
        assert!(why.len() > 20, "and explain it: {why}");
    }

    /// Fast limits for tests: a 10 ms poll so the loop reacts quickly.
    fn fast_limits() -> Limits {
        Limits {
            min_mem_available_bytes: 1_000_000_000,
            max_load_per_core: 0.9,
            max_duration: Duration::from_secs(60),
            poll_interval: Duration::from_millis(10),
        }
    }

    #[test]
    fn assess_flags_low_memory_and_high_load_but_passes_a_healthy_device() {
        let l = Limits::conservative();
        assert_eq!(assess(&good(), &l), Health::Ok);
        assert!(matches!(
            assess(&Resources { mem_available_bytes: 100_000_000, ..good() }, &l),
            Health::Breach(_)
        ));
        assert!(matches!(
            assess(&Resources { load_per_core: 3.0, ..good() }, &l),
            Health::Breach(_)
        ));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn parse_proc_reads_memavailable_and_load_per_core() {
        let mem = "MemTotal:  16000000 kB\nMemFree: 500000 kB\nMemAvailable:  2000000 kB\n";
        let load = "0.50 0.40 0.30 1/500 12345\n";
        let r = parse_proc(mem, load, 4.0).unwrap();
        assert_eq!(r.mem_available_bytes, 2_000_000 * 1024);
        assert!((r.load_per_core - 0.125).abs() < 1e-6, "load per core = {}", r.load_per_core);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn parse_proc_errs_without_memavailable() {
        assert!(parse_proc("MemTotal: 1 kB\n", "0.1 0.1 0.1 1/1 1", 1.0).is_err());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn parse_proc_tolerates_an_unreadable_loadavg() {
        // Android denies apps /proc/loadavg — an empty string must NOT fail the sample; memory still
        // parses and load falls back to 0 (memory is the governor on a phone).
        let mem = "MemAvailable:  3000000 kB\n";
        let r = parse_proc(mem, "", 4.0).unwrap();
        assert_eq!(r.mem_available_bytes, 3_000_000 * 1024);
        assert_eq!(r.load_per_core, 0.0);
    }

    /// A monitor that returns a canned reading (or a failure), for supervision tests.
    struct FixedMon(Result<Resources, ()>);
    impl ResourceMonitor for FixedMon {
        fn sample(&self) -> Result<Resources, WatchdogError> {
            self.0.map_err(|()| WatchdogError::new("fake read failure"))
        }
    }

    #[cfg(unix)]
    fn long_child() -> Child {
        std::process::Command::new("sleep").arg("30").spawn().expect("spawn sleep")
    }

    #[cfg(unix)]
    #[test]
    fn it_stops_the_model_when_a_threshold_is_crossed() {
        let mut child = long_child();
        let starved = Resources { mem_available_bytes: 100_000_000, load_per_core: 0.1 };
        let wd = Watchdog::new(FixedMon(Ok(starved)), fast_limits());
        assert!(matches!(wd.supervise(&mut child, &StopFlag::new()), Outcome::Stopped(_)));
    }

    #[cfg(unix)]
    #[test]
    fn the_stop_flag_stops_the_model() {
        let mut child = long_child();
        let wd = Watchdog::new(FixedMon(Ok(good())), fast_limits());
        let stop = StopFlag::new();
        stop.stop();
        assert!(matches!(wd.supervise(&mut child, &stop), Outcome::Stopped(_)));
    }

    #[cfg(unix)]
    #[test]
    fn it_fails_closed_when_it_cannot_read_the_device() {
        let mut child = long_child();
        let wd = Watchdog::new(FixedMon(Err(())), fast_limits());
        assert!(matches!(wd.supervise(&mut child, &StopFlag::new()), Outcome::Stopped(_)));
    }

    #[cfg(unix)]
    #[test]
    fn a_model_that_finishes_on_its_own_is_completed() {
        let mut child = std::process::Command::new("sleep").arg("0").spawn().unwrap();
        std::thread::sleep(Duration::from_millis(80)); // let it exit
        let wd = Watchdog::new(FixedMon(Ok(good())), fast_limits());
        assert!(matches!(wd.supervise(&mut child, &StopFlag::new()), Outcome::Completed(_)));
    }
}
