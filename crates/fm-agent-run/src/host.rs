//! **What an app needs to host Myrme on a desktop** — the facade (`docs/context/plan.md`, Phase D's
//! last step; `contract.md`, contract 1.1).
//!
//! An app that offers the assistant has to answer a handful of questions — can it run here, is it
//! installed, what does it take on disk, fetch it, remove it, start it — and until 1.1 it answered
//! them itself, by reaching into the catalogue, the downloader, the installed-model resolver and the
//! watchdog: about twenty items of Myrme's insides. Here they are one type, [`Tools`], over the one
//! folder the app gives Myrme, plus [`unsupported`]. Everything that is the app's own stays the
//! app's: where that folder is, its settings file, the progress it shows, how it cancels, and when it
//! starts the assistant.
//!
//! Moved from formicaria's `fm-serve/src/agent.rs` (2026-10-18) with its messages unchanged, so what
//! a person reads in Settings is the same.

use crate::installed::{self, Installed};
use crate::manifest::Manifest;
use std::path::{Path, PathBuf};
use std::process::Command;

/// The model server's filename on this platform.
pub const SERVER_BIN: &str = if cfg!(windows) { "llama-server.exe" } else { "llama-server" };
/// The speech server's filename on this platform.
pub const WHISPER_BIN: &str = if cfg!(windows) { "whisper-server.exe" } else { "whisper-server" };
/// The port the speech server listens on.
pub const WHISPER_PORT: u16 = 8082;

/// **Why the assistant cannot run on this machine** at all — or `None` when it can.
///
/// One reading of the machine's memory, and the platform is supported exactly when the answer is
/// usable: a claim about the machine, never a list of operating systems. A reason, not a bool,
/// because the app has to print it. Static on purpose: free memory *now* is `preflight`'s question,
/// asked when a model starts.
pub fn unsupported() -> Option<String> {
    use fm_agent::watchdog::ResourceMonitor;
    fm_agent::watchdog::SystemMonitor.sample().err().map(|e| {
        format!(
            "The study assistant cannot run on this computer: {e} Everything else in formicaria \
             works normally — your notes, search, boards and backup are unaffected."
        )
    })
}

/// The catalogue key of this platform's speech-runtime archive, or `None` where upstream publishes
/// none (macOS at whisper.cpp v1.9.1) — a different fact from a missing file, reported differently.
pub fn whisper_runtime_key() -> Option<&'static str> {
    match Manifest::platform_key()? {
        "linux_x64" => Some("whisper_linux_x64"),
        "windows_x64" => Some("whisper_windows_x64"),
        _ => None,
    }
}

/// A step of a download, for the app's progress line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    Runtime,
    Model,
    Projector,
    Speech,
}

impl Stage {
    /// The word the app's progress field has always carried for it.
    pub fn word(self) -> &'static str {
        match self {
            Stage::Runtime => "runtime",
            Stage::Model => "model",
            Stage::Projector => "projector",
            Stage::Speech => "transcribe",
        }
    }
}

/// How the app follows a download: told when a step starts (answer `false` to stop), told its bytes
/// as they come, and asked whether to give up. A cancelled download leaves its `.part` file, so a
/// retry resumes.
pub struct Progress<'a> {
    pub step: &'a dyn Fn(Stage) -> bool,
    pub bytes: &'a dyn Fn(Stage, u64, Option<u64>),
    pub cancelled: &'a dyn Fn() -> bool,
}

/// What [`Tools::launch`] is told by the app.
pub struct Launch<'a> {
    /// The `agent-serve` binary the app ships.
    pub exe: &'a Path,
    /// The app's port, and the instance that answers on it (the assistant leaves when another app
    /// answers there).
    pub serve_port: u16,
    pub serve_instance: &'a str,
    /// The model to run (see [`Tools::installed`]).
    pub model: &'a str,
    /// This device's name for its Myrme, as the person typed it.
    pub device: &'a str,
    /// Whether the person turned speech on, and which speech weights they chose.
    pub speech: bool,
    pub speech_weights: &'a str,
}

/// **The one folder an app gives Myrme**: `models/`, `runtime/` and the catalogue copy live in it.
///
/// `checkout` marks a developer's source checkout (`agents/`), whose files are theirs: nothing is
/// removed or swept in one.
#[derive(Debug, Clone)]
pub struct Tools {
    dir: PathBuf,
    checkout: bool,
}

impl Tools {
    pub fn new(dir: impl Into<PathBuf>, checkout: bool) -> Self {
        Self { dir: dir.into(), checkout }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn models(&self) -> PathBuf {
        self.dir.join("models")
    }

    /// The catalogue in this folder (`models.toml`), else the one this Myrme ships with.
    pub fn catalogue(&self) -> Manifest {
        Manifest::read(&self.dir.join("models.toml"))
            .unwrap_or_else(|_| Manifest::parse(crate::CATALOGUE))
    }

    /// Which model runs and which, if any, to offer — given the person's choice and the default they
    /// last saw (see [`installed::resolve`]).
    pub fn installed(
        &self,
        manifest: &Manifest,
        chosen: Option<&str>,
        seen: Option<&str>,
    ) -> Installed {
        installed::resolve(manifest, &self.models(), &manifest.default, chosen, seen)
    }

    /// [`Tools::installed`] against another default — a phone's (`Manifest::mobile_default`).
    pub fn installed_for(
        &self,
        manifest: &Manifest,
        default: &str,
        chosen: Option<&str>,
        seen: Option<&str>,
    ) -> Installed {
        installed::resolve(manifest, &self.models(), default, chosen, seen)
    }

    /// Is the assistant's stack here — the runtime **and** a model the catalogue still lists? With no
    /// catalogue in the folder, any `.gguf` counts: a moved default never costs a download.
    pub fn is_provisioned(&self, chosen: Option<&str>, seen: Option<&str>) -> bool {
        if !self.dir.join("runtime").join(SERVER_BIN).exists() {
            return false;
        }
        if let Ok(manifest) = Manifest::read(&self.dir.join("models.toml")) {
            return self.installed(&manifest, chosen, seen).running.is_some();
        }
        std::fs::read_dir(self.models()).is_ok_and(|mut d| {
            d.any(|e| {
                e.is_ok_and(|e| {
                    e.path().extension().is_some_and(|x| x.eq_ignore_ascii_case("gguf"))
                })
            })
        })
    }

    /// The disk the model and the runtime use, in bytes — so a removal can say what it frees.
    pub fn bytes(&self) -> u64 {
        ["models", "runtime"]
            .iter()
            .filter_map(|sub| std::fs::read_dir(self.dir.join(sub)).ok())
            .flatten()
            .filter_map(|e| e.ok())
            .filter_map(|e| e.metadata().ok())
            .filter(|m| m.is_file())
            .map(|m| m.len())
            .sum()
    }

    /// Delete the model and the runtime — those two subfolders and nothing else — and say how much
    /// was freed. Refused in a checkout, whose `agents/` is a developer's.
    pub fn remove(&self) -> Result<u64, String> {
        if self.checkout {
            return Err(
                "this installation runs from a source checkout, so the model in `agents/` is yours to \
                 manage — formicaria will not delete it for you"
                    .into(),
            );
        }
        let freed = self.bytes();
        for sub in ["models", "runtime"] {
            let p = self.dir.join(sub);
            if p.exists() {
                std::fs::remove_dir_all(&p)
                    .map_err(|e| format!("could not remove {}: {e}", p.display()))?;
            }
        }
        Ok(freed)
    }

    /// Fetch what `model` needs — the runtime, its weights, and its projector when `vision` — with
    /// checksums, resumable. The runtime is skipped when present; a platform with no verified archive
    /// pinned is a reason given, never something fetched unverified.
    pub fn provision(
        &self,
        manifest: &Manifest,
        model: &str,
        vision: bool,
        p: &Progress,
    ) -> Result<(), String> {
        std::fs::create_dir_all(self.models())
            .map_err(|e| format!("cannot create {}: {e}", self.dir.display()))?;
        let cancelled = p.cancelled;
        if !self.dir.join("runtime").join(SERVER_BIN).exists() {
            let key = Manifest::platform_key().ok_or(
                "no model runtime has been published for this kind of computer yet".to_string(),
            )?;
            let rt = manifest
                .runtime(key)
                .ok_or_else(|| format!("no verified model runtime is pinned for {key} yet"))?;
            if !(p.step)(Stage::Runtime) {
                return Err(CANCELLED.into());
            }
            crate::fetch::ensure_runtime(
                &self.dir.join("runtime"),
                rt,
                SERVER_BIN,
                &|d, t| (p.bytes)(Stage::Runtime, d, t),
                cancelled,
            )?;
            self.stamp_runtime(&rt.sha256);
        }
        self.provision_weights(manifest, model, vision, p)
    }

    /// The weights of `model`, and its projector when `vision` — no runtime. What a phone fetches
    /// (its runtime is bundled), and the second half of [`Tools::provision`].
    pub fn provision_weights(
        &self,
        manifest: &Manifest,
        model: &str,
        vision: bool,
        p: &Progress,
    ) -> Result<(), String> {
        std::fs::create_dir_all(self.models())
            .map_err(|e| format!("cannot create {}: {e}", self.dir.display()))?;
        let cancelled = p.cancelled;
        if !(p.step)(Stage::Model) {
            return Err(CANCELLED.into());
        }
        crate::fetch::ensure_model(
            &self.models(),
            manifest,
            model,
            &|d, t| (p.bytes)(Stage::Model, d, t),
            cancelled,
        )?;
        if vision {
            if !(p.step)(Stage::Projector) {
                return Err(CANCELLED.into());
            }
            crate::fetch::ensure_mmproj(
                &self.models(),
                manifest,
                model,
                &|d, t| (p.bytes)(Stage::Projector, d, t),
                cancelled,
            )?;
        }
        Ok(())
    }

    /// Where the record of which archive the model runtime came from is kept.
    fn runtime_stamp(&self) -> PathBuf {
        self.dir.join("runtime").join(".runtime-sha256")
    }

    fn stamp_runtime(&self, sha256: &str) {
        let _ = std::fs::write(self.runtime_stamp(), format!("{sha256}\n"));
    }

    /// **Is the model runtime here older than the one the catalogue pins?** A runtime with no record
    /// of where it came from (every install before Myrme 0.4.0) counts as older, once. `false` when
    /// there is no runtime yet (that is provisioning's job), when this platform has no pinned
    /// archive, or in a checkout, whose runtime is a developer's.
    ///
    /// Why it matters: an app update brings a new catalogue, and a runtime already on disk was never
    /// fetched again — so the faster runtime of 2026-10-18 (llguidance) would never have reached
    /// anyone who had the assistant before it.
    pub fn runtime_outdated(&self, manifest: &Manifest) -> bool {
        if self.checkout || !self.dir.join("runtime").join(SERVER_BIN).exists() {
            return false;
        }
        let Some(rt) = Manifest::platform_key().and_then(|k| manifest.runtime(k)) else {
            return false;
        };
        std::fs::read_to_string(self.runtime_stamp()).map(|s| s.trim().to_string()).ok().as_deref()
            != Some(rt.sha256.as_str())
    }

    /// **Bring the model runtime up to the catalogue's pin**, without ever leaving none: the archive
    /// is fetched and checked into `runtime-next/`, and only when that is complete are its files
    /// copied over the old ones and the record written. A failed or cancelled fetch leaves the old
    /// runtime as it was, and working. Call it while the assistant is not running.
    pub fn refresh_runtime(&self, manifest: &Manifest, p: &Progress) -> Result<(), String> {
        let key = Manifest::platform_key().ok_or(
            "no model runtime has been published for this kind of computer yet".to_string(),
        )?;
        let rt = manifest
            .runtime(key)
            .ok_or_else(|| format!("no verified model runtime is pinned for {key} yet"))?;
        if !(p.step)(Stage::Runtime) {
            return Err(CANCELLED.into());
        }
        let next = self.dir.join("runtime-next");
        let _ = std::fs::remove_dir_all(&next);
        crate::fetch::ensure_runtime(
            &next,
            rt,
            SERVER_BIN,
            &|d, t| (p.bytes)(Stage::Runtime, d, t),
            p.cancelled,
        )?;
        if (p.cancelled)() {
            let _ = std::fs::remove_dir_all(&next);
            return Err(CANCELLED.into());
        }
        let runtime = self.dir.join("runtime");
        std::fs::create_dir_all(&runtime).map_err(|e| e.to_string())?;
        for entry in std::fs::read_dir(&next).map_err(|e| e.to_string())? {
            let from = entry.map_err(|e| e.to_string())?.path();
            if from.is_file() {
                let to = runtime.join(from.file_name().unwrap_or_default());
                std::fs::copy(&from, &to)
                    .map_err(|e| format!("could not update {}: {e}", to.display()))?;
            }
        }
        let _ = std::fs::remove_dir_all(&next);
        self.stamp_runtime(&rt.sha256);
        Ok(())
    }

    /// Fetch the speech runtime and the chosen speech `weights`, then remove speech weights nobody
    /// uses any more (never in a checkout).
    pub fn provision_speech(
        &self,
        manifest: &Manifest,
        weights: &str,
        p: &Progress,
    ) -> Result<(), String> {
        let rt = whisper_runtime_key().and_then(|k| manifest.runtime(k)).ok_or(
            "no speech-to-text runtime has been published for this kind of computer yet"
                .to_string(),
        )?;
        if !(p.step)(Stage::Speech) {
            return Err(CANCELLED.into());
        }
        let bytes = |d, t| (p.bytes)(Stage::Speech, d, t);
        crate::fetch::ensure_runtime(
            &self.dir.join("runtime"),
            rt,
            WHISPER_BIN,
            &bytes,
            p.cancelled,
        )?;
        crate::fetch::ensure_model(&self.models(), manifest, weights, &bytes, p.cancelled)?;
        if (p.cancelled)() {
            return Err(CANCELLED.into());
        }
        if !self.checkout {
            for gone in installed::sweep_whisper(manifest, &self.models(), weights) {
                println!("study agent: removed unused speech model {}", gone.display());
            }
        }
        Ok(())
    }

    /// Is speech here — its runtime and any catalogued speech weights?
    pub fn speech_staged(&self) -> bool {
        let catalogue = Manifest::read(&self.dir.join("models.toml")).ok();
        executable(&self.dir.join("runtime").join(WHISPER_BIN))
            && (catalogue.is_some_and(|m| {
                installed::whisper_on_disk(&m, &self.models(), "ggml-base").is_some()
            }) || self.models().join("ggml-base.bin").is_file())
    }

    /// Could speech be fetched here, if the person asked for it?
    pub fn speech_fetchable(manifest: &Manifest) -> bool {
        whisper_runtime_key().is_some_and(|k| manifest.runtime(k).is_some())
    }

    /// Do the chosen speech `weights` still have to be downloaded?
    pub fn speech_wanted(&self, manifest: &Manifest, weights: &str) -> bool {
        manifest.model(weights).is_some_and(|w| !self.models().join(&w.file).is_file())
    }

    /// The `agent-serve` command for this folder: the model, this app's port and instance, the
    /// device's name, in-process web search, and speech when it is on and staged (the chosen
    /// weights, or the earlier ones while the chosen download). Model files that belong to neither
    /// the model about to run nor the catalogue's default are removed first (never in a checkout).
    /// Output goes to a rolling `runtime/agent.log`. The app spawns the command.
    pub fn launch(&self, manifest: &Manifest, l: &Launch) -> Command {
        if !self.checkout {
            for gone in installed::sweep(manifest, &self.models(), &[l.model, &manifest.default]) {
                println!("study agent: removed unused model file {}", gone.display());
            }
        }
        let mut cmd = Command::new(l.exe);
        cmd.arg("--agents-dir").arg(&self.dir).arg("--serve-port").arg(l.serve_port.to_string());
        cmd.arg("--serve-instance").arg(l.serve_instance);
        cmd.arg("--model").arg(l.model);
        cmd.arg("--device")
            .arg(fm_agent::convo::device_word(l.device).unwrap_or_else(|| "laptop".into()));
        cmd.arg("--web-direct");
        if l.speech && self.speech_staged() {
            if let Some(ears) =
                installed::whisper_on_disk(manifest, &self.models(), l.speech_weights)
            {
                cmd.arg("--whisper-port").arg(WHISPER_PORT.to_string());
                cmd.arg("--whisper-model").arg(ears);
            }
        }
        match self.log_file() {
            Some(f) => {
                if let Ok(err) = f.try_clone() {
                    cmd.stdout(std::process::Stdio::from(f)).stderr(std::process::Stdio::from(err));
                }
            }
            None => eprintln!("study agent: could not open its log — running without one"),
        }
        cmd
    }

    /// `runtime/agent.log`, rolled at 5 MB.
    fn log_file(&self) -> Option<std::fs::File> {
        let runtime = self.dir.join("runtime");
        std::fs::create_dir_all(&runtime).ok()?;
        let log = runtime.join("agent.log");
        if std::fs::metadata(&log).is_ok_and(|m| m.len() > 5_000_000) {
            let _ = std::fs::rename(&log, runtime.join("agent.log.1"));
        }
        std::fs::OpenOptions::new().create(true).append(true).open(&log).ok()
    }
}

/// **A runtime an app bundles** — on Android, the APK's `jniLibs`, which Android extracts to the
/// app's native-library directory, the one place an app may execute from. The servers ride there
/// renamed as libraries ([`BUNDLED_SERVER`], [`BUNDLED_WHISPER`]).
#[derive(Debug, Clone)]
pub struct Bundled {
    dir: PathBuf,
}

/// The model server, as an APK carries it.
pub const BUNDLED_SERVER: &str = "libllama-server.so";
/// The speech server, as an APK carries it.
pub const BUNDLED_WHISPER: &str = "libwhisper-server.so";

impl Bundled {
    /// The native-library directory, found from where `own_lib` (the app's own library) is mapped.
    #[cfg(target_os = "android")]
    pub fn locate(own_lib: &str) -> Option<Self> {
        crate::nativelib::native_lib_dir(own_lib).map(|dir| Self { dir })
    }

    /// A bundle at `dir` (tests; a platform that bundles elsewhere).
    pub fn at(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// What the app's Settings needs: `(installed, why, speech available)`, in the shape a desktop
    /// answers. `None` means the app could not find its own library directory.
    pub fn availability(bundle: Option<&Bundled>) -> (bool, String, bool) {
        let Some(b) = bundle else {
            return (
                false,
                "The assistant cannot locate this app's own library directory, so it cannot start a \
                 model. Everything else in formicaria works normally."
                    .into(),
                false,
            );
        };
        let installed = b.dir.join(BUNDLED_SERVER).exists();
        let why = if installed {
            String::new()
        } else {
            "This build of the app does not carry the model runtime, so the assistant cannot be turned \
             on. Everything else in formicaria works normally."
                .to_string()
        };
        (installed, why, installed && b.dir.join(BUNDLED_WHISPER).exists())
    }
}

/// What an app asks [`Tools::run_in_process`] to run.
pub struct InProcess<V: crate::fmserve::VaultAccess> {
    /// The app's way into its notes.
    pub vault: V,
    /// The model to run (from [`Tools::installed`], or the catalogue's default on a first enable).
    pub model: String,
    /// The model on offer, whose weights are kept while their download may be half done.
    pub offered: String,
    /// The chosen speech weights, when the person turned speech on.
    pub speech: Option<String>,
    /// This device's name for its Myrme.
    pub device: String,
}

/// How the app follows an in-process run. Each `*_up` registers a server's stop switch; answering
/// `false` means the app retired this launch meanwhile, and the server is stopped at once.
pub struct Hooks<'a> {
    pub cancelled: &'a dyn Fn() -> bool,
    pub model_up: &'a dyn Fn(fm_agent::watchdog::StopFlag) -> bool,
    pub speech_up: &'a dyn Fn(fm_agent::watchdog::StopFlag, u16) -> bool,
    /// Myrme is answering (`true`) or has stopped (`false`) — for the app's `@`-mention picker.
    pub present: &'a dyn Fn(bool),
    pub log: &'a dyn Fn(&str),
}

impl Tools {
    /// **Run Myrme inside the app**, on a bundled runtime, until it is stopped — what a phone does,
    /// having no separate server process to host it. Fetches the weights when missing (resumable;
    /// offline is "not yet"), the projector best-effort (without it the model starts text-only),
    /// sweeps unused weights, starts the model under the memory watchdog, starts speech beside it
    /// when asked and when it fits, then answers `@Myrme` until stopped.
    ///
    /// `Err` only for "nothing was started" (no weights yet, the model refused or failed to start);
    /// everything after that is logged. Moved from formicaria's `mobile/src-tauri/src/agent.rs`
    /// (2026-10-18) with its log lines unchanged.
    pub fn run_in_process<V: crate::fmserve::VaultAccess>(
        &self,
        bundle: &Bundled,
        manifest: &Manifest,
        job: InProcess<V>,
        h: &Hooks,
    ) -> Result<(), String> {
        use fm_agent::launch::SupervisedModel;
        use fm_agent::preflight::Need;
        use fm_agent::watchdog::{Limits, SystemMonitor};
        let models_dir = self.models();
        let logs_dir = self.dir.join("runtime");
        let _ = std::fs::create_dir_all(&logs_dir);
        let server_bin = bundle.dir.join(BUNDLED_SERVER);
        let (port, ctx, threads) = (manifest.port, manifest.ctx, manifest.mobile_threads());
        let model_name = job.model.clone();

        // The weights (a first enable fetches them), logged once per MB.
        let last_mb = std::cell::Cell::new(u64::MAX);
        let progress = |done: u64, total: Option<u64>| {
            let mb = done / 1_000_000;
            if last_mb.replace(mb) != mb {
                match total {
                    Some(t) => (h.log)(&format!(
                        "study agent: fetching {model_name} {mb}/{} MB",
                        t / 1_000_000
                    )),
                    None => (h.log)(&format!("study agent: fetching {model_name} {mb} MB")),
                }
            }
        };
        let model_gguf =
            crate::fetch::ensure_model(&models_dir, manifest, &model_name, &progress, h.cancelled)
                .map_err(|e| format!("no model yet ({e})"))?;
        if let Err(e) =
            crate::fetch::ensure_mmproj(&models_dir, manifest, &model_name, &progress, h.cancelled)
        {
            (h.log)(&format!("study agent: no image projector yet ({e}) — starting text-only"));
        }
        for gone in installed::sweep(manifest, &models_dir, &[&model_name, &job.offered]) {
            (h.log)(&format!("study agent: removed unused model {}", gone.display()));
        }
        if (h.cancelled)() {
            (h.log)("study agent: cancelled before the model was started");
            return Ok(());
        }

        let mut cmd = Command::new(&server_bin);
        cmd.env("LD_LIBRARY_PATH", &bundle.dir).arg("-m").arg(&model_gguf).args([
            "--host",
            "127.0.0.1",
            "--port",
            &port.to_string(),
            "-c",
            &ctx.to_string(),
            "-t",
            &threads.to_string(),
            "--no-warmup",
        ]);
        let sees = manifest.add_projector(&mut cmd, &model_name, &models_dir);
        let model_bytes = std::fs::metadata(&model_gguf).map(|m| m.len()).unwrap_or(500_000_000)
            + sees.as_ref().and_then(|p| std::fs::metadata(p).ok()).map(|m| m.len()).unwrap_or(0);
        let model = SupervisedModel::launch(
            cmd,
            SystemMonitor,
            &Need::new(model_bytes, 1_000_000_000),
            Limits::resident(),
        )
        .map_err(|e| format!("model failed to launch: {e}"))?;
        if !(h.model_up)(model.stopper()) {
            (h.log)("study agent: cancelled as the model came up — stopping it");
            model.stopper().stop();
            let _ = model.wait();
            return Ok(());
        }

        // Speech beside the text model, when on, bundled and fitting.
        let mut whisper_port = None;
        let mut heard_with = job.speech.clone().unwrap_or_else(|| "ggml-base".into());
        let _whisper = match &job.speech {
            None => None,
            Some(_) if !bundle.dir.join(BUNDLED_WHISPER).exists() => {
                (h.log)("study agent: transcription runtime not bundled — skipping");
                None
            }
            Some(chosen) => {
                let ears = match crate::fetch::ensure_model(
                    &models_dir,
                    manifest,
                    chosen,
                    &|_, _| {},
                    h.cancelled,
                ) {
                    Ok(path) => {
                        for gone in installed::sweep_whisper(manifest, &models_dir, chosen) {
                            (h.log)(&format!(
                                "study agent: removed unused speech model {}",
                                gone.display()
                            ));
                        }
                        Ok((path, chosen.clone()))
                    }
                    Err(e) => match installed::whisper_on_disk(manifest, &models_dir, chosen)
                        .and_then(|n| Some((models_dir.join(&manifest.model(&n)?.file), n)))
                    {
                        Some(found) => {
                            (h.log)(&format!(
                                "study agent: {chosen} not here yet ({e}) — using {}",
                                found.1
                            ));
                            Ok(found)
                        }
                        None => Err(e),
                    },
                };
                match ears {
                    Ok((wmodel, wname)) => {
                        heard_with = wname;
                        let wport = port + 1;
                        let mut wcmd = Command::new(bundle.dir.join(BUNDLED_WHISPER));
                        wcmd.arg("-m").arg(&wmodel).args([
                            "--host",
                            "127.0.0.1",
                            "--port",
                            &wport.to_string(),
                            "-t",
                            &threads.to_string(),
                            "-l",
                            crate::manifest::whisper_language(&heard_with),
                        ]);
                        let wbytes =
                            std::fs::metadata(&wmodel).map(|m| m.len()).unwrap_or(80_000_000);
                        match SupervisedModel::launch(
                            wcmd,
                            SystemMonitor,
                            &Need::new(wbytes, 300_000_000),
                            Limits::resident(),
                        ) {
                            Ok(wm) => {
                                if (h.speech_up)(wm.stopper(), wport) {
                                    whisper_port = Some(wport);
                                    (h.log)(&format!(
                                        "study agent: audio transcription on (whisper :{wport}, {heard_with})"
                                    ));
                                } else {
                                    (h.log)("study agent: cancelled — stopping whisper");
                                    wm.stopper().stop();
                                }
                                Some(wm)
                            }
                            Err(e) => {
                                (h.log)(&format!("study agent: transcription off — {e}"));
                                None
                            }
                        }
                    }
                    Err(e) => {
                        (h.log)(&format!(
                            "study agent: transcription off — no whisper model yet ({e})"
                        ));
                        None
                    }
                }
            }
        };

        let agent = crate::Agent {
            fm: job.vault,
            model_port: port,
            model: model_name.clone(),
            searxng_port: None,
            web_direct: true,
            whisper_port,
            whisper_model: heard_with,
            vision: sees.is_some(),
            read_prompt: manifest.read_prompt(&model_name),
            reads_labels: manifest.reads_labels(&model_name),
            image_specialist: None,
            max_reply_chars: manifest.max_reply_chars,
            history_budget: 4000,
            device: job.device,
        };
        if !crate::watch::ready_unless_gone(port, &|| model.finished()) && model.finished() {
            let _ = model.wait();
            if let Some(w) = _whisper {
                w.stopper().stop();
                let _ = w.wait();
            }
            return Err(format!("the model server for @{model_name} exited before it was ready"));
        }
        (h.log)(&format!("study agent listening in-process — @{model_name}"));
        (h.present)(true);
        let stopper = model.stopper();
        crate::watch::serve_loop(&agent, &model_name, &logs_dir, 1, &|| model.finished(), &|| {
            stopper.stop()
        });
        (h.present)(false);
        let _ = model.wait();
        (h.log)("study agent stopped");
        Ok(())
    }
}

/// What a provisioning stopped by the app answers with. The app tells a stop from a failure by its
/// own cancel state, not by this text.
const CANCELLED: &str = "cancelled";

#[cfg(unix)]
fn executable(p: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(p).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn executable(p: &Path) -> bool {
    p.is_file()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("myrme-host-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("models")).unwrap();
        std::fs::create_dir_all(d.join("runtime")).unwrap();
        d
    }

    #[test]
    fn provisioned_means_the_runtime_and_a_listed_model() {
        let d = temp("prov");
        let tools = Tools::new(&d, false);
        assert!(!tools.is_provisioned(None, None), "nothing here yet");
        std::fs::write(d.join("runtime").join(SERVER_BIN), b"x").unwrap();
        assert!(!tools.is_provisioned(None, None), "a runtime alone is not the stack");
        std::fs::write(d.join("models").join("anything.gguf"), b"x").unwrap();
        assert!(tools.is_provisioned(None, None), "no catalogue here: any .gguf counts");
        std::fs::write(d.join("models.toml"), crate::CATALOGUE).unwrap();
        assert!(!tools.is_provisioned(None, None), "with a catalogue, an unlisted file does not");
        let _ = std::fs::remove_dir_all(&d);
    }

    /// Moved with the predicate from formicaria (2026-10-18): the weights are any `.gguf`, whatever
    /// its case, when the folder has no catalogue — so a moved default never costs a download.
    #[test]
    fn any_gguf_counts_whatever_its_case() {
        let d = temp("case");
        std::fs::write(d.join("runtime").join(SERVER_BIN), b"x").unwrap();
        std::fs::write(d.join("models").join("Some-Model.GGUF"), b"x").unwrap();
        assert!(Tools::new(&d, false).is_provisioned(None, None));
        let _ = std::fs::remove_dir_all(&d);
    }

    /// Speech needs its runtime **and** its weights; either alone is not speech. Moved with the
    /// predicate from formicaria (2026-10-18).
    #[test]
    fn speech_needs_the_runtime_and_the_weights_together() {
        let d = temp("speech");
        let tools = Tools::new(&d, false);
        assert!(!tools.speech_staged(), "nothing staged");
        let bin = d.join("runtime").join(WHISPER_BIN);
        std::fs::write(&bin, b"#!/bin/true").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        assert!(!tools.speech_staged(), "a runtime but no weights");
        std::fs::write(d.join("models").join("ggml-base.bin"), b"x").unwrap();
        assert!(tools.speech_staged(), "both staged");
        let _ = std::fs::remove_dir_all(&d);
    }

    /// The tools folder holds a copy of the catalogue once anything was provisioned, and that does
    /// not make it a checkout: removal works there (formicaria refused, 2026-10-18).
    #[test]
    fn a_tools_folder_with_a_catalogue_copy_can_still_be_cleared() {
        let d = temp("rmcat");
        std::fs::write(d.join("models.toml"), crate::CATALOGUE).unwrap();
        std::fs::write(d.join("models").join("m.gguf"), b"x").unwrap();
        assert_eq!(Tools::new(&d, false).remove(), Ok(1));
        let _ = std::fs::remove_dir_all(&d);
    }

    /// **The dev script and this code decide speech the same way.** `agents/agent-serve.sh` starts
    /// speech when its runtime and weights are staged; [`Tools::speech_staged`] predicts it for the
    /// app's settings row. A shell script cannot share a predicate, so the names are compared here —
    /// a guard formicaria's `ci/checks.sh` kept until this code moved (2026-10-18).
    #[test]
    fn the_dev_script_and_this_code_name_the_same_speech_files() {
        let script = include_str!("../../../agents/agent-serve.sh");
        let here = include_str!("host.rs");
        for name in ["whisper-server", "ggml-base.bin"] {
            assert!(
                script.contains(name) && here.contains(name),
                "{name} is named on one side only"
            );
        }
    }

    /// A runtime with no record of its archive is outdated once; a recorded pin is current; a
    /// checkout is never refreshed; and no runtime at all is provisioning's business, not this.
    #[test]
    fn a_runtime_without_a_record_or_with_another_pin_is_outdated() {
        let d = temp("rt");
        let m = Tools::new(&d, false).catalogue();
        let Some(rt) = Manifest::platform_key().and_then(|k| m.runtime(k)) else {
            return; // a platform with no pinned runtime has nothing to compare
        };
        let tools = Tools::new(&d, false);
        assert!(!tools.runtime_outdated(&m), "no runtime yet: provisioning's job");
        std::fs::write(d.join("runtime").join(SERVER_BIN), b"x").unwrap();
        assert!(tools.runtime_outdated(&m), "an install from before records were kept");
        tools.stamp_runtime(&rt.sha256);
        assert!(!tools.runtime_outdated(&m), "the pinned archive");
        tools.stamp_runtime("an-older-archive");
        assert!(tools.runtime_outdated(&m), "another archive");
        assert!(!Tools::new(&d, true).runtime_outdated(&m), "a checkout is a developer's");
        let _ = std::fs::remove_dir_all(&d);
    }

    /// **Live:** an old runtime with no record is brought up to the pinned archive, downloaded for
    /// real (tens of MB). `cargo test -p fm-agent-run --features download -- --ignored refresh_live`
    #[test]
    #[ignore = "downloads the pinned runtime"]
    fn refresh_live() {
        let d = temp("refresh-live");
        std::fs::write(d.join("runtime").join(SERVER_BIN), b"an old server").unwrap();
        std::fs::write(d.join("runtime").join("agent.log"), b"kept").unwrap();
        let tools = Tools::new(&d, false);
        let m = tools.catalogue();
        assert!(tools.runtime_outdated(&m));
        let p = Progress { step: &|_| true, bytes: &|_, _, _| {}, cancelled: &|| false };
        tools.refresh_runtime(&m, &p).unwrap();
        assert!(!tools.runtime_outdated(&m), "the record now names the pinned archive");
        let server = std::fs::read(d.join("runtime").join(SERVER_BIN)).unwrap();
        assert!(server.len() > 1000, "the server is the downloaded one");
        assert_eq!(std::fs::read(d.join("runtime").join("agent.log")).unwrap(), b"kept");
        assert!(!d.join("runtime-next").exists(), "the side folder is gone");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_bundle_without_its_server_says_why_and_offers_no_switch() {
        let d = temp("bundle");
        assert!(!Bundled::availability(None).0, "no library directory: nothing to offer");
        let (on, why, speech) = Bundled::availability(Some(&Bundled::at(&d)));
        assert!(!on && !speech && why.contains("does not carry the model runtime"));
        std::fs::write(d.join(BUNDLED_SERVER), b"x").unwrap();
        std::fs::write(d.join(BUNDLED_WHISPER), b"x").unwrap();
        assert_eq!(Bundled::availability(Some(&Bundled::at(&d))), (true, String::new(), true));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn removal_takes_two_folders_says_what_it_freed_and_never_touches_a_checkout() {
        let d = temp("rm");
        std::fs::write(d.join("models").join("m.gguf"), vec![0u8; 1000]).unwrap();
        std::fs::write(d.join("keep.txt"), b"x").unwrap();
        assert!(Tools::new(&d, true).remove().is_err(), "a checkout is a developer's");
        assert_eq!(Tools::new(&d, false).remove().unwrap(), 1000);
        assert!(!d.join("models").exists() && d.join("keep.txt").exists());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn launch_names_the_model_the_app_and_the_device() {
        let d = temp("launch");
        let tools = Tools::new(&d, true);
        let m = tools.catalogue();
        let cmd = tools.launch(
            &m,
            &Launch {
                exe: Path::new("agent-serve"),
                serve_port: 8765,
                serve_instance: "abc",
                model: "qwen3-vl-4b",
                device: "Office desktop",
                speech: false,
                speech_weights: "ggml-base",
            },
        );
        let args: Vec<String> = cmd.get_args().map(|a| a.to_string_lossy().into_owned()).collect();
        for want in
            ["--model", "qwen3-vl-4b", "--serve-instance", "abc", "office-desktop", "--web-direct"]
        {
            assert!(args.iter().any(|a| a == want), "{want} missing from {args:?}");
        }
        assert!(!args.iter().any(|a| a == "--whisper-port"), "speech off: no speech server");
        let _ = std::fs::remove_dir_all(&d);
    }
}
