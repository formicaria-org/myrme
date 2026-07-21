//! The **launcher**: preflight-gate the decision to start, spawn the model server as a child, and
//! hand it to the [`Watchdog`] on a background thread — so "run the model" is *always* supervised and
//! *always* stoppable, never a bare `spawn`.
//!
//! The caller talks to the server over HTTP (an `OpenAiStep` pointed at the port it launched); this
//! layer only owns the *process* and its safety. The returned [`SupervisedModel`] carries the
//! off-switch and joins to the watchdog's [`Outcome`].

use crate::preflight::{admit, Admission, Need};
use crate::watchdog::{Limits, Outcome, ResourceMonitor, StopFlag, Watchdog, WatchdogError};
use std::process::Command;
use std::thread::JoinHandle;

/// A model process running under the watchdog. Dropping it does **not** stop the model (the watchdog
/// thread outlives it); call [`stop`](Self::stop) or [`wait`](Self::wait) to end and reap it.
#[derive(Debug)]
pub struct SupervisedModel {
    stop: StopFlag,
    handle: JoinHandle<Outcome>,
}

impl SupervisedModel {
    /// Preflight, then spawn `cmd` and supervise it from the first instant. `Err` if preflight
    /// **refuses** (nothing is spawned) or the spawn itself fails.
    pub fn launch<M: ResourceMonitor + Send + 'static>(
        mut cmd: Command,
        monitor: M,
        need: &Need,
        limits: Limits,
    ) -> Result<Self, String> {
        if let Admission::Refuse(why) = admit(&monitor, need) {
            return Err(format!("preflight refused to start the model: {why}"));
        }
        let mut child =
            cmd.spawn().map_err(|e| format!("could not start the model server: {e}"))?;
        let stop = StopFlag::new();
        let stop_for_wd = stop.clone();
        let watchdog = Watchdog::new(monitor, limits);
        let handle = std::thread::spawn(move || watchdog.supervise(&mut child, &stop_for_wd));
        Ok(Self { stop, handle })
    }

    /// A clone of the off-switch — trip it from anywhere to stop the model.
    pub fn stopper(&self) -> StopFlag {
        self.stop.clone()
    }

    /// Has supervision already ended (the model finished, or the watchdog stopped it)? Lets a caller
    /// notice the model went away without blocking on [`wait`](Self::wait).
    pub fn finished(&self) -> bool {
        self.handle.is_finished()
    }

    /// Stop the model and wait for the watchdog to finish, returning why it ended.
    pub fn stop(self) -> Outcome {
        self.stop.stop();
        self.join()
    }

    /// Wait for the model to finish on its own (or the watchdog to stop it), without asking it to.
    pub fn wait(self) -> Outcome {
        self.join()
    }

    fn join(self) -> Outcome {
        self.handle
            .join()
            .unwrap_or_else(|_| Outcome::Failed(WatchdogError::new("watchdog thread panicked")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::watchdog::{Resources, WatchdogError};
    use std::time::Duration;

    struct Mon(Result<Resources, ()>);
    impl ResourceMonitor for Mon {
        fn sample(&self) -> Result<Resources, WatchdogError> {
            self.0.map_err(|()| WatchdogError::new("fake read failure"))
        }
    }

    fn plenty() -> Mon {
        Mon(Ok(Resources { mem_available_bytes: 8_000_000_000, load_per_core: 0.1 }))
    }
    fn fast() -> Limits {
        Limits {
            min_mem_available_bytes: 1_000_000_000,
            max_load_per_core: 0.9,
            max_duration: Duration::from_secs(60),
            poll_interval: Duration::from_millis(10),
        }
    }
    fn need() -> Need {
        Need::new(500_000_000, 500_000_000)
    }

    #[test]
    fn preflight_refusal_means_nothing_is_spawned() {
        // A monitor that can't be read ⇒ preflight refuses ⇒ the (bogus) binary is never spawned.
        let cmd = Command::new("definitely-not-a-real-binary-xyz");
        let err = SupervisedModel::launch(cmd, Mon(Err(())), &need(), fast()).unwrap_err();
        assert!(err.contains("preflight refused"), "got: {err}");
    }

    #[cfg(unix)]
    #[test]
    fn it_launches_supervised_and_the_stop_flag_stops_it() {
        let mut cmd = Command::new("sleep");
        cmd.arg("30");
        let model = SupervisedModel::launch(cmd, plenty(), &need(), fast()).unwrap();
        // Stopping ends it promptly with a Stopped outcome.
        assert!(matches!(model.stop(), Outcome::Stopped(_)));
    }

    #[cfg(unix)]
    #[test]
    fn a_starved_device_refuses_before_spawning() {
        let starved = Mon(Ok(Resources { mem_available_bytes: 200_000_000, load_per_core: 0.1 }));
        let mut cmd = Command::new("sleep");
        cmd.arg("30");
        assert!(SupervisedModel::launch(cmd, starved, &need(), fast()).is_err());
    }
}
