//! **The contract with formicaria, as a compile check.** Every item below is named by formicaria's
//! host code (`crates/fm-serve/src/agent.rs`, `agent_registry.rs`, `mobile/src-tauri/src/*.rs`) at
//! the tag it pins. If a refactor here renames, moves or hides one, this file stops compiling — in
//! Myrme's CI, before formicaria's build ever sees the change. It checks that the names exist, not
//! their signatures; `docs/context/contract.md` is the prose half and says how to refresh the list.
//!
//! An item leaves this file only together with its last use in formicaria.
#![allow(unused_imports, dead_code)]

use fm_agent_run::agent_serve::main as agent_serve_main;
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
use fm_agent_run::{Agent, ImageReader, CATALOGUE, CONTRACT_MAJOR, CONTRACT_MINOR, VERSION};

/// The phone and the desktop's first enable both fetch through these; they exist only with the
/// `download` feature, which formicaria turns on.
#[cfg(feature = "download")]
use fm_agent_run::fetch::{ensure_mmproj, ensure_model, ensure_runtime};

/// **Contract 1.1:** the desktop host's facade — what formicaria's `fm-serve` uses in place of the
/// items above that a desktop host needs (the phone still uses them; they stay until it moves).
#[cfg(feature = "download")]
use fm_agent_run::host::{
    unsupported, whisper_runtime_key, Bundled, Hooks, InProcess, Launch, Progress, Stage, Tools,
    BUNDLED_SERVER, BUNDLED_WHISPER, SERVER_BIN, WHISPER_BIN, WHISPER_PORT,
};

/// **Contract 1.6:** ONNX Runtime's name in the APK, for the OCR (formicaria's packaging stages it
/// under this name).
#[cfg(feature = "download")]
use fm_agent_run::host::BUNDLED_ONNXRUNTIME;

/// The contract this file checks is 1.6 (`CONTRACT_MAJOR`/`CONTRACT_MINOR`). Changing what it names
/// means changing the contract version too — a removal or rename is a new major, an addition a new
/// minor — and the two are compared here so one cannot move without the other.
#[test]
fn this_list_is_contract_1_6() {
    assert_eq!((fm_agent_run::CONTRACT_MAJOR, fm_agent_run::CONTRACT_MINOR), (1, 6));
    assert!(
        fm_agent_run::VERSION.split('.').count() == 3,
        "a release version is MAJOR.MINOR.PATCH"
    );
}

/// Associated functions formicaria calls by path.
#[test]
fn associated_items_formicaria_calls_exist() {
    // Contract 1.2: an app update brings the runtime the new catalogue pins.
    #[cfg(feature = "download")]
    {
        let _ = Tools::runtime_outdated;
        let _ = Tools::refresh_runtime;
        // Contract 1.3: the phone's host — a bundled runtime, weights alone, a run in-process.
        let _ = Tools::provision_weights;
        let _ = Tools::installed_for;
        let _ = Bundled::availability;
    }
    let _ = Manifest::read;
    let _ = Manifest::platform_key;
    // Contract 1.5: the phone's model choice, and per-model settings both launchers apply.
    let _ = Manifest::phone_choices;
    let _ = Manifest::shared_mail_prompt;
    let _ = Manifest::add_template_flags;
    let _ = <SystemMonitor as ResourceMonitor>::sample;
}

/// Contract 1.5: the fields formicaria's phone chooser reads from a catalogued model, and the one
/// `Agent` field a host fills from the catalogue.
#[test]
fn model_fields_formicaria_reads_exist() {
    use fm_agent_run::manifest::Model;
    fn chooser(m: &Model) -> (&str, Option<&str>, Option<u64>, Option<u64>) {
        (&m.name, m.phone_label.as_deref(), m.bytes, m.mmproj_bytes)
    }
    fn settings(m: &Model) -> (Option<bool>, bool) {
        (m.thinking, m.shared_mail_prompt)
    }
    fn agent(a: &Agent<fm_agent_run::fmserve::FmServe>) -> bool {
        a.shared_mail_prompt
    }
    let _ = (chooser, settings, agent);
}

/// Contract 1.6: the OCR. The name its library has in the APK, the catalogue keys formicaria's
/// packaging reads to stage it, and the `Agent` field the launchers fill.
#[test]
fn the_ocr_formicaria_packages_is_named_and_pinned() {
    #[cfg(feature = "download")]
    assert_eq!(BUNDLED_ONNXRUNTIME, "libonnxruntime.so");
    let m = Manifest::parse(fm_agent_run::CATALOGUE);
    let aar = m.runtime("onnxruntime_android_arm64").expect("the phone's ONNX Runtime is pinned");
    assert!(aar.url.ends_with(".aar") && aar.sha256.len() == 64, "{aar:?}");
    assert_eq!(m.ocr_files().len(), 3, "detector, recogniser, dictionary");
    fn agent(a: &Agent<fm_agent_run::fmserve::FmServe>) -> bool {
        a.ocr.is_some()
    }
    let _ = agent;
}

/// The catalogue formicaria embeds on the phone and writes beside the desktop program parses.
#[test]
fn the_shipped_catalogue_parses() {
    let _ = Manifest::parse(fm_agent_run::CATALOGUE);
}

/// Contract 1.4: what formicaria reads from files rather than Rust items. Its packaging stages the
/// phone's runtimes from the catalogue, and its manual renders `docs/manual/assistant.md`, linking
/// into it by heading.
#[test]
fn the_files_formicaria_reads_are_where_it_reads_them() {
    let m = fm_agent_run::manifest::Manifest::parse(CATALOGUE);
    assert!(m.runtime("android_arm64").is_some(), "the phone's runtime archive and its hash");
    assert!(
        CATALOGUE.lines().any(|l| l.starts_with("whisper_android_source = \"v")),
        "the whisper.cpp tag the phone build compiles"
    );
    let manual = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/manual/assistant.md"),
    )
    .expect("docs/manual/assistant.md, formicaria's assistant chapter");
    // `<ManualLink page="user/assistant.html" section="turn-it-on">` in formicaria's Settings.
    assert!(manual.lines().any(|l| l.trim_start_matches('#').trim() == "Turn it on"));
}
