//! **The contract with formicaria, as a compile check.** Every item below is named by formicaria's
//! host code (`crates/fm-serve/src/agent.rs`, `agent_registry.rs`, `mobile/src-tauri/src/*.rs`) at
//! the tag it pins. If a refactor here renames, moves or hides one, this file stops compiling — in
//! Myrme's CI, before formicaria's build ever sees the change. It checks that the names exist, not
//! their signatures; `docs/context/contract.md` is the prose half and says how to refresh the list.
//!
//! An item leaves this file only together with its last use in formicaria.
#![allow(unused_imports, dead_code)]

use fm_agent_run::fm_agent::convo::{device_word, MYRME};
use fm_agent_run::fm_agent::launch::SupervisedModel;
use fm_agent_run::fm_agent::preflight::Need;
use fm_agent_run::fm_agent::transcribe;
use fm_agent_run::fm_agent::watchdog::{Limits, ResourceMonitor, StopFlag, SystemMonitor};
use fm_agent_run::fmserve::{Origin, VaultAccess};
use fm_agent_run::installed::{resolve, sweep, sweep_whisper, whisper_on_disk, Installed};
use fm_agent_run::label::{read, ready_json};
use fm_agent_run::manifest::{whisper_language, Manifest};
use fm_agent_run::nativelib::lib_dir_from_maps;
/// Android-only (`cfg(target_os = "android")`): the phone's host locates the bundled runtime with
/// it. Its parsing half is checked everywhere.
#[cfg(target_os = "android")]
use fm_agent_run::nativelib::native_lib_dir;
use fm_agent_run::watch::{serve_loop, wait_ready};
use fm_agent_run::{Agent, ImageReader};

/// The phone and the desktop's first enable both fetch through these; they exist only with the
/// `download` feature, which formicaria turns on.
#[cfg(feature = "download")]
use fm_agent_run::fetch::{ensure_mmproj, ensure_model, ensure_runtime};

/// Associated functions formicaria calls by path.
#[test]
fn associated_items_formicaria_calls_exist() {
    let _ = Manifest::read;
    let _ = Manifest::platform_key;
    let _ = <SystemMonitor as ResourceMonitor>::sample;
}

/// The catalogue formicaria embeds on the phone and writes beside the desktop program parses.
#[test]
fn the_shipped_catalogue_parses() {
    let _ = Manifest::parse(fm_agent_run::CATALOGUE);
}
