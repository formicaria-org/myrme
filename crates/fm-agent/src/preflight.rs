//! The **before-launch admission gate** — the watchdog's one-shot twin. It answers "is it safe to
//! *start* the model right now?" and **refuses** if not. Fail-closed: if it cannot read the device,
//! it refuses. The watchdog then guards the run; preflight guards the decision to begin.
//!
//! Reuses the watchdog's [`ResourceMonitor`](crate::watchdog::ResourceMonitor), so the same OS reader
//! serves both, and the same "unknown ⇒ refuse" stance holds at both gates.

use crate::watchdog::ResourceMonitor;

/// How much memory the model needs to load and run without pushing the device: the model's own
/// footprint plus headroom for the KV cache, the host app, and a safety margin.
#[derive(Debug, Clone, Copy)]
pub struct Need {
    pub model_bytes: u64,
    pub headroom_bytes: u64,
}

impl Need {
    pub fn new(model_bytes: u64, headroom_bytes: u64) -> Self {
        Self { model_bytes, headroom_bytes }
    }
    fn required(&self) -> u64 {
        self.model_bytes.saturating_add(self.headroom_bytes)
    }
}

/// The verdict: go, or refuse with a reason.
#[derive(Debug, Clone, PartialEq)]
pub enum Admission {
    Go,
    Refuse(String),
}

/// Decide whether to start. Refuses when free memory is below `model + headroom`, or when the device
/// cannot be read at all — never starts the model blind.
pub fn admit(monitor: &impl ResourceMonitor, need: &Need) -> Admission {
    match monitor.sample() {
        Ok(r) => {
            let required = need.required();
            if r.mem_available_bytes < required {
                Admission::Refuse(format!(
                    "need ~{} MB (model {} MB + {} MB headroom) but only {} MB is free — not starting",
                    required / 1_000_000,
                    need.model_bytes / 1_000_000,
                    need.headroom_bytes / 1_000_000,
                    r.mem_available_bytes / 1_000_000
                ))
            } else {
                Admission::Go
            }
        }
        Err(e) => Admission::Refuse(format!(
            "cannot read device memory ({e}) — refusing to start the model blind"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::watchdog::{Resources, WatchdogError};

    struct Mon(Result<Resources, ()>);
    impl ResourceMonitor for Mon {
        fn sample(&self) -> Result<Resources, WatchdogError> {
            self.0.map_err(|()| WatchdogError::new("fake read failure"))
        }
    }

    fn res(mem: u64) -> Resources {
        Resources { mem_available_bytes: mem, load_per_core: 0.1 }
    }

    #[test]
    fn it_goes_when_there_is_room_and_refuses_when_there_is_not() {
        let need = Need::new(1_000_000_000, 1_500_000_000); // 1 GB model + 1.5 GB headroom = 2.5 GB
        assert_eq!(admit(&Mon(Ok(res(4_000_000_000))), &need), Admission::Go);
        assert!(matches!(admit(&Mon(Ok(res(2_000_000_000))), &need), Admission::Refuse(_)));
    }

    #[test]
    fn it_refuses_fail_closed_when_it_cannot_read_memory() {
        let need = Need::new(500_000_000, 500_000_000);
        assert!(matches!(admit(&Mon(Err(())), &need), Admission::Refuse(_)));
    }
}
