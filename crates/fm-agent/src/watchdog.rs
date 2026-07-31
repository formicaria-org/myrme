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
        Self { max_duration: Duration::MAX, max_load_per_core: f32::INFINITY, ..Self::conservative() }
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
                    return self
                        .kill(child, Outcome::Failed(WatchdogError::new(format!(
                            "cannot poll the model process: {e}"
                        ))))
                }
            }
            // Stop requested — the easy off-switch.
            if stop.is_stopped() {
                return self.kill(child, Outcome::Stopped("stopped by request".into()));
            }
            // Hard wall-clock bound.
            if start.elapsed() >= self.limits.max_duration {
                let secs = self.limits.max_duration.as_secs();
                return self.kill(child, Outcome::Stopped(format!("exceeded the {secs}s time limit")));
            }
            // Resource check — fail-closed: if we cannot read the device, we do not run blind.
            match self.monitor.sample() {
                Ok(r) => {
                    if let Health::Breach(why) = assess(&r, &self.limits) {
                        return self.kill(child, Outcome::Stopped(format!("device threshold crossed: {why}")));
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

/// The default monitor: reads the host it runs on. Linux reads `/proc`; other OSes **fail closed**
/// until they get an impl, so nothing ever runs unmonitored.
pub struct SystemMonitor;

impl ResourceMonitor for SystemMonitor {
    fn sample(&self) -> Result<Resources, WatchdogError> {
        // Android is a Linux kernel — it has `/proc/meminfo`/`/proc/loadavg` too — so it shares this
        // path (its `target_os` is "android", not "linux", which is why it was fail-closing before).
        #[cfg(any(target_os = "linux", target_os = "android"))]
        {
            proc_sample()
        }
        #[cfg(not(any(target_os = "linux", target_os = "android")))]
        {
            Err(WatchdogError::new(
                "resource monitoring is not implemented on this OS yet — refusing to run the model unmonitored",
            ))
        }
    }
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
    let load1 = loadavg
        .split_whitespace()
        .next()
        .and_then(|v| v.parse::<f32>().ok())
        .unwrap_or(0.0);
    Ok(Resources {
        mem_available_bytes: mem_available_kb * 1024,
        load_per_core: load1 / cores.max(1.0),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn good() -> Resources {
        Resources { mem_available_bytes: 8_000_000_000, load_per_core: 0.2 }
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
