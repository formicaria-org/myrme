//! Which model this installation runs, and whether a newer one should be **offered**.
//!
//! An app update can change the catalogue's default model. It must never change what a person is
//! running behind their back: a model is a download measured in hundreds of megabytes or
//! gigabytes, sometimes over mobile data. So the rule (the owner's, 2026-10-10) is:
//!
//! 1. **Keep running what is already here** for as long as the catalogue still lists it. Every
//!    model speaks the same API to this app (`llama-server` applies each model's own template), so
//!    an older model keeps working; what it needs from the catalogue is its settings.
//! 2. **Offer the new default**, with its size, and download it only when the person says so.
//! 3. **A model the catalogue no longer lists is unsupported**: it has no settings here any more, so
//!    it is not started, and the offer is the way forward.
//!
//! One function decides this for the desktop and the phone. It reads the catalogue and looks at
//! which files exist; it downloads nothing and starts nothing.

use crate::manifest::Manifest;
use std::path::{Path, PathBuf};

/// A model the person can choose to download in place of the one they run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Offer {
    pub name: String,
    /// Weights plus projector, when the catalogue states them: what accepting will download.
    pub bytes: Option<u64>,
    pub license: Option<String>,
    /// The model it would replace, when one is running.
    pub replaces: Option<String>,
}

/// What [`resolve`] found.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Installed {
    /// The model to start, or `None` when nothing usable is on this device.
    pub running: Option<String>,
    /// The catalogue's default, when it is not what runs here and the person has not already
    /// declined it.
    pub offer: Option<Offer>,
    /// Weights are on disk that the catalogue no longer lists, and nothing else can run.
    pub unsupported: bool,
}

/// The speech weights are catalogued beside the assistant's models and are never "the assistant".
fn is_assistant(m: &crate::manifest::Model) -> bool {
    !m.repo.contains("whisper")
}

fn here(manifest: &Manifest, models_dir: &Path, name: &str) -> bool {
    manifest
        .model(name)
        .is_some_and(|m| is_assistant(m) && !m.file.is_empty() && models_dir.join(&m.file).exists())
}

/// Decide what runs and what is offered.
///
/// - `default` is the catalogue's pick **for this kind of device** (the phone's differs).
/// - `chosen` is the model the person last picked or accepted, if this device recorded one.
/// - `default_seen` is the catalogue default at the time of that choice, or when they last said
///   "not now". An offer is made only when the default has **moved since**, so someone who picked a
///   different model on purpose is not asked again every time they open Settings.
pub fn resolve(
    manifest: &Manifest,
    models_dir: &Path,
    default: &str,
    chosen: Option<&str>,
    default_seen: Option<&str>,
) -> Installed {
    let running = chosen
        .filter(|n| here(manifest, models_dir, n))
        .or_else(|| Some(default).filter(|n| here(manifest, models_dir, n)))
        .map(str::to_string)
        // An installation from before choices were recorded: whatever catalogued model is here.
        .or_else(|| {
            manifest
                .models
                .iter()
                .find(|m| here(manifest, models_dir, &m.name))
                .map(|m| m.name.clone())
        });

    let any_weights = std::fs::read_dir(models_dir).is_ok_and(|mut d| {
        d.any(|e| {
            e.is_ok_and(|e| e.path().extension().is_some_and(|x| x.eq_ignore_ascii_case("gguf")))
        })
    });
    let unsupported = running.is_none() && any_weights;

    let wanted = running.as_deref() != Some(default)
        // Nothing here at all is a first enable, which asks its own question.
        && (running.is_some() || unsupported)
        // Declined, or chosen against, this very default already. An unsupported model cannot be
        // kept, so that offer is always shown.
        && (unsupported || default_seen != Some(default));
    let offer = wanted.then(|| manifest.model(default)).flatten().map(|m| Offer {
        name: m.name.clone(),
        bytes: m.bytes.map(|b| b + m.mmproj_bytes.unwrap_or(0)),
        license: m.license.clone(),
        replaces: running.clone(),
    });

    Installed { running, offer, unsupported }
}

/// The files that belong to `names` — weights, projector, and a download of either still in
/// progress — so a sweep of old weights can spare them.
pub fn files_of(manifest: &Manifest, models_dir: &Path, names: &[&str]) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for m in names.iter().filter_map(|n| manifest.model(n)) {
        for f in std::iter::once(&m.file).chain(m.mmproj.as_ref()) {
            out.push(models_dir.join(f));
            out.push(models_dir.join(format!("{f}.part")));
        }
    }
    out
}

/// Delete weights that belong to neither the running model nor the offered one. Returns what was
/// removed. Only `.gguf` and `.part` files directly inside `models_dir`; the speech weights are
/// `.bin` and are never touched.
pub fn sweep(manifest: &Manifest, models_dir: &Path, keep: &[&str]) -> Vec<PathBuf> {
    let spare = files_of(manifest, models_dir, keep);
    let mut gone = Vec::new();
    let Ok(entries) = std::fs::read_dir(models_dir) else { return gone };
    for p in entries.flatten().map(|e| e.path()) {
        let ext = p.extension().and_then(|x| x.to_str()).unwrap_or("");
        if (ext.eq_ignore_ascii_case("gguf") || ext == "part")
            && !spare.contains(&p)
            && std::fs::remove_file(&p).is_ok()
        {
            gone.push(p);
        }
    }
    gone
}

#[cfg(test)]
mod tests {
    use super::*;

    const CATALOGUE: &str = "default = \"new\"\n\
        [[models]]\nname = \"new\"\nfile = \"new.gguf\"\nbytes = 100\nmmproj = \"new-proj.gguf\"\nmmproj_bytes = 20\nlicense = \"L\"\n\
        [[models]]\nname = \"old\"\nfile = \"old.gguf\"\n\
        [[models]]\nname = \"ears\"\nrepo = \"x/whisper.cpp\"\nfile = \"ears.gguf\"\n";

    fn dir(tag: &str, files: &[&str]) -> PathBuf {
        let d = std::env::temp_dir().join(format!("fm-installed-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        for f in files {
            std::fs::write(d.join(f), b"w").unwrap();
        }
        d
    }

    #[test]
    fn nothing_here_is_a_first_enable_and_offers_nothing() {
        let m = Manifest::parse(CATALOGUE);
        let d = dir("empty", &[]);
        assert_eq!(resolve(&m, &d, "new", None, None), Installed::default());
    }

    /// The case this exists for: an update moved the default, the old model is still on disk.
    #[test]
    fn the_old_model_keeps_running_and_the_new_default_is_offered_with_its_size() {
        let m = Manifest::parse(CATALOGUE);
        let d = dir("moved", &["old.gguf"]);
        let got = resolve(&m, &d, "new", None, None);
        assert_eq!(got.running.as_deref(), Some("old"), "what is here keeps running");
        let offer = got.offer.expect("the new default is offered");
        assert_eq!((offer.name.as_str(), offer.bytes), ("new", Some(120)), "weights + projector");
        assert_eq!(offer.replaces.as_deref(), Some("old"));
        assert!(!got.unsupported);
    }

    #[test]
    fn an_offer_declined_is_not_made_again_until_the_default_moves_again() {
        let m = Manifest::parse(CATALOGUE);
        let d = dir("declined", &["old.gguf"]);
        assert_eq!(resolve(&m, &d, "new", Some("old"), Some("new")).offer, None, "said not now");
        assert!(resolve(&m, &d, "new", Some("old"), Some("older")).offer.is_some(), "moved since");
    }

    #[test]
    fn once_the_default_is_here_it_runs_and_nothing_is_offered() {
        let m = Manifest::parse(CATALOGUE);
        let d = dir("both", &["old.gguf", "new.gguf"]);
        let got = resolve(&m, &d, "new", Some("new"), Some("new"));
        assert_eq!((got.running.as_deref(), got.offer), (Some("new"), None));
        // A choice whose file has gone falls back to what is here rather than to nothing.
        let d = dir("lost", &["old.gguf"]);
        assert_eq!(
            resolve(&m, &d, "new", Some("new"), Some("new")).running.as_deref(),
            Some("old")
        );
    }

    /// A model the catalogue dropped has no settings here, so it is not started — and that offer
    /// is shown even if an earlier one was declined, because there is nothing to keep.
    #[test]
    fn weights_the_catalogue_no_longer_lists_are_unsupported_and_always_offered_a_way_forward() {
        let m = Manifest::parse(CATALOGUE);
        let d = dir("dropped", &["retired.gguf"]);
        let got = resolve(&m, &d, "new", Some("retired"), Some("new"));
        assert_eq!(got.running, None);
        assert!(got.unsupported);
        assert_eq!(got.offer.map(|o| o.name), Some("new".into()));
    }

    #[test]
    fn the_speech_weights_are_never_mistaken_for_the_assistant() {
        let m = Manifest::parse(CATALOGUE);
        let d = dir("ears", &["ears.gguf"]);
        assert_eq!(resolve(&m, &d, "new", None, None).running, None);
    }

    #[test]
    fn a_sweep_spares_the_running_model_the_offered_one_and_a_download_in_progress() {
        let m = Manifest::parse(CATALOGUE);
        let d = dir(
            "sweep",
            &["old.gguf", "new.gguf.part", "new-proj.gguf", "stale.gguf", "x.part", "speech.bin"],
        );
        let mut gone: Vec<String> = sweep(&m, &d, &["old", "new"])
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        gone.sort();
        assert_eq!(gone, ["stale.gguf", "x.part"]);
        for kept in ["old.gguf", "new.gguf.part", "new-proj.gguf", "speech.bin"] {
            assert!(d.join(kept).exists(), "{kept} must survive");
        }
    }
}
