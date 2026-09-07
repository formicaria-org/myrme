//! The **one** reader of `agents/models.toml` — the model catalogue + runtime defaults. It replaces
//! the three divergent `awk` copies (`fetch.sh`, `serve.sh`, `agent-serve.sh`) and the hardcoded clap
//! defaults, so "the single place that says which model to run" is actually single.
//!
//! A tiny hand parser for this fixed, simple shape (top-level `key = value` + `[[models]]` blocks with
//! `name`/`file`) — no `toml` dep, keeping the runner dep-free for Android, in the same std-only,
//! minimal-deps stance as the hand-rolled HTTP client. Unknown keys and comments are ignored.

use std::path::Path;

/// One catalogued model: enough to fetch it (`repo` + `file`, optionally checksum-pinned) and to run
/// it (`file` is the local GGUF name).
pub struct Model {
    /// The key a user passes to fetch/run (and `@name`-mentions).
    pub name: String,
    /// The GGUF filename — both the Hugging Face asset and the local file under `models/`.
    pub file: String,
    /// The Hugging Face repo the file lives in, e.g. `LiquidAI/LFM2.5-230M-GGUF`. Empty if unset.
    pub repo: String,
    /// Optional SHA-256 (hex) to verify a download against. `None` ⇒ fetch without verification.
    pub sha256: Option<String>,
    /// The exact Hugging Face commit the [`sha256`](Self::sha256) was taken from. Unset ⇒ `main`.
    ///
    /// **A checksum without one of these is a bomb on a timer.** `resolve/main` is a moving pointer:
    /// the day the repo re-quantises or fixes a template, every download starts failing its checksum
    /// — permanently, on every device, with no way for the user to fix it and no way for us to reach
    /// an already-shipped release. Pinning the commit makes the URL name the same bytes forever, so
    /// the checksum can only ever fail on a *corrupt transfer*, which is what it is for.
    pub revision: Option<String>,
    /// The **multimodal projector** filename, for a vision-language model. `None` — the default —
    /// means text-only, which is what every model here was until images arrived.
    ///
    /// A separate file from the weights, in the same repo, and genuinely optional: the same GGUF
    /// serves text-only without it. That is why it is a field rather than a second model entry —
    /// it is a *capability* of one model, not a model.
    pub mmproj: Option<String>,
    /// Per-model context-window override; falls back to the manifest's `ctx` when unset.
    pub ctx: Option<u32>,
    /// Per-model thread-count override; falls back to the manifest's `threads` when unset.
    pub threads: Option<u32>,
    /// The weights' licence, e.g. `Apache-2.0` — shown before anything is downloaded, because the
    /// terms someone is accepting should not be discoverable only by reading a TOML comment.
    pub license: Option<String>,
    /// The download size in bytes, and the projector's separately. A first enable can cost several
    /// gigabytes; a screen that asks first has to be able to say how many.
    pub bytes: Option<u64>,
    pub mmproj_bytes: Option<u64>,
}

/// The runtime defaults and the model list from `models.toml`.
pub struct Manifest {
    /// The model run when none is named (the laptop/desktop default).
    pub default: String,
    /// The phone's default model, when it differs (a phone can't hold the laptop model). Falls back
    /// to [`default`](Self::default) — read it through [`mobile_default`](Self::mobile_default).
    pub default_mobile: Option<String>,
    /// The model server port.
    pub port: u16,
    /// Context window (bounds the KV cache).
    pub ctx: u32,
    /// Inference threads (the laptop value).
    pub threads: u32,
    /// The phone's thread count, when it differs (its big-core count). Falls back to
    /// [`threads`](Self::threads) — read it through [`mobile_threads`](Self::mobile_threads).
    pub threads_mobile: Option<u32>,
    /// GPU policy for the desktop runtime: `"auto"` (use the best GPU when one is present, else CPU),
    /// `"on"` (always offload), or `"off"` (CPU only). Default `"auto"`. Read via [`gpu_layers`].
    ///
    /// [`gpu_layers`]: Self::gpu_layers
    pub gpu: String,
    /// The longest reply the agent posts, in characters — a "never berserk" bound and a brevity nudge
    /// for a small model. Configurable so a formula + explanation isn't clipped. Default 2000.
    pub max_reply_chars: usize,
    /// The phone's whisper model name (a `[[models]]` entry), fetched on demand when Audio transcription
    /// is on. `None` ⇒ no phone whisper. Read via [`mobile_whisper`](Self::mobile_whisper).
    pub whisper_mobile: Option<String>,
    /// Every catalogued model.
    models: Vec<Model>,
    /// The prebuilt runtime archives, keyed by the suffix that names a platform — `linux_x64`,
    /// `linux_x64_gpu`, `whisper_linux_x64`, and in time `macos_arm64` / `windows_x64`.
    ///
    /// **Flat suffixed keys rather than a `[runtime.…]` table**, because this parser has no concept
    /// of a named table: a `[…]` line that is not `[[models]]` is skipped and its keys are then read
    /// as *top-level* ones — silently. A shape the parser cannot see is worse than an ugly one it
    /// can, and the suffix is also what `agents/fetch.sh`'s `awk` can still read.
    ///
    /// Read through [`runtime`](Self::runtime), which picks the suffix for the running platform.
    runtimes: std::collections::BTreeMap<String, Runtime>,
}

/// One prebuilt runtime archive: where to get it, and what it must hash to.
///
/// **The checksum is not optional here, unlike a model's.** A model is pinned by a Hugging Face
/// commit as well, so `sha256` is a second belt; a runtime archive is a URL to an executable and
/// the checksum is the only thing standing between a redirect and running someone else's binary.
/// A platform whose checksum we have not verified therefore has no entry at all, rather than an
/// entry we cannot check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Runtime {
    pub url: String,
    pub sha256: String,
}

impl Manifest {
    /// Read and parse `<path>` (e.g. `agents/models.toml`).
    pub fn read(path: &Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
        Ok(Self::parse(&text))
    }

    /// Parse the manifest text. Fixed-shape and forgiving: a malformed number keeps the built-in
    /// default, unknown keys are skipped. (Kept `pub` so the parse is unit-tested without a file.)
    pub fn parse(text: &str) -> Self {
        let (mut default, mut port, mut ctx, mut threads) = (String::new(), 8081u16, 2048u32, 4u32);
        let (mut default_mobile, mut threads_mobile): (Option<String>, Option<u32>) = (None, None);
        let mut gpu = String::from("auto");
        let mut max_reply_chars = 2000usize;
        let mut whisper_mobile: Option<String> = None;
        let mut models: Vec<Model> = Vec::new();
        // `runtime_url_<platform>` and `runtime_sha256_<platform>` arrive in either order, so they
        // are gathered by suffix and paired at the end. A URL without a checksum is **dropped**,
        // not kept — see [`Runtime`].
        let mut rt_url: std::collections::BTreeMap<String, String> = Default::default();
        let mut rt_sha: std::collections::BTreeMap<String, String> = Default::default();
        // The block currently being parsed. `Some` ⇒ we are inside a `[[models]]` block (so top-level
        // keys no longer apply); a block is committed on the next `[[models]]` or at EOF, when it has
        // at least a name + file. Keys within a block are order-free.
        let mut cur: Option<Model> = None;
        let flush = |cur: &mut Option<Model>, models: &mut Vec<Model>| {
            if let Some(m) = cur.take() {
                if !m.name.is_empty() && !m.file.is_empty() {
                    models.push(m);
                }
            }
        };

        for raw in text.lines() {
            let line = raw.split('#').next().unwrap_or("").trim();
            if line.is_empty() {
                continue;
            }
            if line == "[[models]]" {
                flush(&mut cur, &mut models);
                cur = Some(Model {
                    name: String::new(),
                    file: String::new(),
                    repo: String::new(),
                    sha256: None,
                    revision: None,
                    mmproj: None,
                    ctx: None,
                    threads: None,
                    license: None,
                    bytes: None,
                    mmproj_bytes: None,
                });
                continue;
            }
            let Some((key, val)) = line.split_once('=') else {
                continue;
            };
            let key = key.trim();
            let val = val.trim().trim_matches('"').trim();
            match cur.as_mut() {
                // Inside a [[models]] block: fill the current model, ignore unknown keys.
                Some(m) => match key {
                    "name" => m.name = val.to_string(),
                    "file" => m.file = val.to_string(),
                    "repo" => m.repo = val.to_string(),
                    "sha256" => m.sha256 = Some(val.to_string()),
                    "revision" => m.revision = Some(val.to_string()),
                    "mmproj" => m.mmproj = Some(val.to_string()),
                    "ctx" => m.ctx = val.parse().ok(),
                    "threads" => m.threads = val.parse().ok(),
                    "license" => m.license = Some(val.to_string()),
                    "bytes" => m.bytes = val.parse().ok(),
                    "mmproj_bytes" => m.mmproj_bytes = val.parse().ok(),
                    _ => {}
                },
                // Top-level (before any [[models]]).
                None => match key {
                    "default" => default = val.to_string(),
                    "default_mobile" => default_mobile = Some(val.to_string()),
                    "port" => port = val.parse().unwrap_or(port),
                    "ctx" => ctx = val.parse().unwrap_or(ctx),
                    "threads" => threads = val.parse().unwrap_or(threads),
                    "threads_mobile" => threads_mobile = val.parse().ok(),
                    "gpu" => gpu = val.to_string(),
                    "max_reply_chars" => max_reply_chars = val.parse().unwrap_or(max_reply_chars),
                    "whisper_mobile" => whisper_mobile = Some(val.to_string()),
                    k if k.starts_with("runtime_url_") => {
                        rt_url.insert(k["runtime_url_".len()..].to_string(), val.to_string());
                    }
                    k if k.starts_with("runtime_sha256_") => {
                        rt_sha.insert(k["runtime_sha256_".len()..].to_string(), val.to_string());
                    }
                    _ => {}
                },
            }
        }
        flush(&mut cur, &mut models); // the last block has no trailing [[models]] to flush it
                                      // Pair them. A URL whose checksum is missing is discarded here rather than downstream: the
                                      // fetch must never be handed an archive it cannot verify, and a half-entry that reaches it
                                      // would be a decision made by omission.
        let runtimes = rt_url
            .into_iter()
            .filter_map(|(k, url)| {
                let sha256 = rt_sha.get(&k)?.clone();
                (!url.is_empty() && !sha256.is_empty()).then_some((k, Runtime { url, sha256 }))
            })
            .collect();
        Manifest {
            default,
            default_mobile,
            port,
            ctx,
            threads,
            threads_mobile,
            gpu,
            max_reply_chars,
            whisper_mobile,
            models,
            runtimes,
        }
    }

    /// Every catalogued model's name, in file order — the order the catalogue's author chose,
    /// which is the order a chooser should offer them in.
    pub fn names(&self) -> Vec<String> {
        self.models.iter().map(|m| m.name.clone()).collect()
    }

    /// Every platform suffix that has a verified runtime archive.
    ///
    /// Exists so the pin check can walk **what the catalogue actually holds** rather than a list
    /// written beside it: the first version of that test hardcoded six keys, and a seventh pin
    /// added the same day went unverified without anything failing.
    pub fn runtime_keys(&self) -> Vec<String> {
        self.runtimes.keys().cloned().collect()
    }

    /// The runtime archive for a platform suffix, e.g. `linux_x64` or `whisper_linux_x64`.
    ///
    /// `None` means this build has no verified archive for that platform — which the caller must
    /// report as a missing capability, never paper over by falling back to another platform's.
    pub fn runtime(&self, key: &str) -> Option<&Runtime> {
        self.runtimes.get(key)
    }

    /// The suffix naming the platform this binary is running on, or `None` where no runtime has
    /// been pinned for it yet. `#[cfg]`, not a runtime check, because it names the build target.
    pub fn platform_key() -> Option<&'static str> {
        #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
        {
            Some("linux_x64")
        }
        #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
        {
            Some("macos_arm64")
        }
        #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
        {
            Some("windows_x64")
        }
        // Android bundles its runtime in `jniLibs` and never fetches one; anything else is a
        // platform nobody has pinned an archive for. Both answer "not from here".
        #[cfg(not(any(
            all(target_os = "linux", target_arch = "x86_64"),
            all(target_os = "macos", target_arch = "aarch64"),
            all(target_os = "windows", target_arch = "x86_64")
        )))]
        {
            None
        }
    }

    /// The phone's whisper model name — a `[[models]]` entry to fetch on demand, or `None`.
    pub fn mobile_whisper(&self) -> Option<&str> {
        self.whisper_mobile.as_deref()
    }

    /// The desktop's speech-to-text weights — a `[[models]]` entry name, fetched on demand when
    /// audio transcription is switched on. Falls back to the phone's pick, then to `ggml-base.en`,
    /// so a catalogue that names neither still resolves to the entry both platforms have used.
    pub fn whisper_desktop(&self) -> Option<String> {
        let name = self.whisper_mobile.clone().unwrap_or_else(|| "ggml-base.en".to_string());
        self.model(&name).map(|_| name)
    }

    /// The context window for `name` — the model's own `ctx` override, else the manifest default.
    pub fn model_ctx(&self, name: &str) -> u32 {
        self.model(name).and_then(|m| m.ctx).unwrap_or(self.ctx)
    }

    /// The thread count for `name` — the model's own `threads` override, else the manifest default.
    pub fn model_threads(&self, name: &str) -> u32 {
        self.model(name).and_then(|m| m.threads).unwrap_or(self.threads)
    }

    /// How many layers to offload to a GPU (`-ngl`) under the current `gpu` policy. `"off"` ⇒ 0 (CPU
    /// only); `"auto"`/`"on"` ⇒ all (99) — safe to pass to a CPU-only build too (it has no GPU to
    /// offload to, so it silently runs on CPU), which is the "best GPU, else CPU" fallback in one flag.
    pub fn gpu_layers(&self) -> u32 {
        if self.gpu.eq_ignore_ascii_case("off") {
            0
        } else {
            99
        }
    }

    /// The phone's default model name — `default_mobile` if set, else the shared `default`.
    pub fn mobile_default(&self) -> &str {
        self.default_mobile.as_deref().unwrap_or(&self.default)
    }

    /// The phone's thread count — `threads_mobile` if set, else the shared `threads`.
    pub fn mobile_threads(&self) -> u32 {
        self.threads_mobile.unwrap_or(self.threads)
    }

    /// The gguf filename for `name`, or `None` if the catalogue has no such model.
    pub fn file(&self, name: &str) -> Option<&str> {
        self.model(name).map(|m| m.file.as_str())
    }

    /// The catalogued model named `name`, or `None`.
    pub fn model(&self, name: &str) -> Option<&Model> {
        self.models.iter().find(|m| m.name == name)
    }

    /// The Hugging Face download URL for `name`'s GGUF, or `None` if the model is unknown or has no
    /// `repo`. Uses the `resolve/main` path — the stable "download the actual file" URL (an LFS repo
    /// serves the real bytes here, not a pointer), which is what a plain HTTPS `GET` needs.
    pub fn download_url(&self, name: &str) -> Option<String> {
        let m = self.model(name)?;
        if m.repo.is_empty() {
            return None;
        }
        // The pinned commit when there is one — see `Model::revision`. `main` only for an entry that
        // pins no checksum either, where a moving target is the stated intent rather than an accident.
        let rev = m.revision.as_deref().unwrap_or("main");
        Some(format!("https://huggingface.co/{}/resolve/{rev}/{}", m.repo, m.file))
    }

    /// The projector's URL, from the **same pinned commit as the weights**.
    ///
    /// `None` for a text-only model, which is not an error: `/transcribe` reports that images
    /// cannot be read rather than handing a picture to a blind model, which is the one failure mode
    /// the vision ruling calls silent and damaging.
    pub fn mmproj_url(&self, name: &str) -> Option<String> {
        let m = self.model(name)?;
        let file = m.mmproj.as_deref()?;
        if m.repo.is_empty() {
            return None;
        }
        let rev = m.revision.as_deref().unwrap_or("main");
        Some(format!("https://huggingface.co/{}/resolve/{rev}/{file}", m.repo))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
        # a comment
        default = "lfm2.5-230m"
        port = 8081
        ctx = 2048
        threads = 4

        [[models]]
        name = "lfm2.5-230m"
        repo = "LiquidAI/LFM2.5-230M-GGUF"
        file = "LFM2.5-230M-Q4_K_M.gguf"
        note = "the default"

        [[models]]
        name = "lfm2.5-1.2b"
        repo = "LiquidAI/LFM2.5-1.2B-Instruct-GGUF"
        file = "LFM2.5-1.2B-Instruct-Q4_K_M.gguf"
    "#;

    #[test]
    fn parses_defaults_and_the_catalogue() {
        let m = Manifest::parse(SAMPLE);
        assert_eq!(m.default, "lfm2.5-230m");
        assert_eq!((m.port, m.ctx, m.threads), (8081, 2048, 4));
        assert_eq!(m.file("lfm2.5-230m"), Some("LFM2.5-230M-Q4_K_M.gguf"));
        assert_eq!(m.file("lfm2.5-1.2b"), Some("LFM2.5-1.2B-Instruct-Q4_K_M.gguf"));
        assert_eq!(m.file("nope"), None);
        // repo is captured even though it appears before `file` in the first block and after it
        // in the second — key order within a block must not matter.
        assert_eq!(m.model("lfm2.5-230m").unwrap().repo, "LiquidAI/LFM2.5-230M-GGUF");
        assert!(m.model("lfm2.5-230m").unwrap().sha256.is_none());
    }

    #[test]
    fn builds_the_hugging_face_download_url() {
        let m = Manifest::parse(SAMPLE);
        assert_eq!(
            m.download_url("lfm2.5-230m").as_deref(),
            Some("https://huggingface.co/LiquidAI/LFM2.5-230M-GGUF/resolve/main/LFM2.5-230M-Q4_K_M.gguf"),
        );
        assert_eq!(m.download_url("nope"), None);
    }

    /// A pinned checksum against a moving `main` is a permanent, unfixable mismatch the day the repo
    /// changes the file. The revision is what makes the URL name the same bytes forever.
    #[test]
    fn a_pinned_revision_is_what_the_url_resolves() {
        let m = Manifest::parse(
            "[[models]]\nname = \"x\"\nrepo = \"r/x\"\nfile = \"x.gguf\"\nrevision = \"deadbeef\"\n",
        );
        assert_eq!(
            m.download_url("x").as_deref(),
            Some("https://huggingface.co/r/x/resolve/deadbeef/x.gguf"),
        );
    }

    #[test]
    fn captures_an_optional_sha256() {
        let m = Manifest::parse(
            "[[models]]\nname = \"x\"\nrepo = \"r/x\"\nfile = \"x.gguf\"\nsha256 = \"abc123\"\n",
        );
        assert_eq!(m.model("x").unwrap().sha256.as_deref(), Some("abc123"));
    }

    #[test]
    fn per_device_defaults_fall_back_to_the_shared_ones() {
        let m = Manifest::parse(
            "default = \"big\"\ndefault_mobile = \"small\"\nthreads = 8\nthreads_mobile = 4\n",
        );
        assert_eq!(m.default, "big");
        assert_eq!(m.mobile_default(), "small");
        assert_eq!((m.threads, m.mobile_threads()), (8, 4));
        // Absent per-device fields fall back to the shared value.
        let m2 = Manifest::parse("default = \"only\"\nthreads = 6\n");
        assert_eq!(m2.mobile_default(), "only");
        assert_eq!(m2.mobile_threads(), 6);
    }

    #[test]
    fn gpu_policy_and_per_model_overrides() {
        let m = Manifest::parse(
            "default = \"a\"\nctx = 2048\nthreads = 8\ngpu = \"auto\"\n\
             [[models]]\nname = \"a\"\nfile = \"a.gguf\"\nrepo = \"r/a\"\nctx = 4096\n",
        );
        // gpu auto/on => offload all; off => none.
        assert_eq!(m.gpu_layers(), 99);
        assert_eq!(Manifest::parse("gpu = \"off\"").gpu_layers(), 0);
        assert_eq!(Manifest::parse("").gpu_layers(), 99); // default is GPU-when-available
                                                          // per-model ctx overrides the global; threads falls back to the global.
        assert_eq!(m.model_ctx("a"), 4096);
        assert_eq!(m.model_threads("a"), 8);
        assert_eq!(m.model_ctx("nope"), 2048); // unknown model → global default
    }

    #[test]
    fn a_malformed_number_keeps_the_builtin_default() {
        let m = Manifest::parse("ctx = not-a-number\nthreads = 8");
        assert_eq!(m.ctx, 2048);
        assert_eq!(m.threads, 8);
    }

    #[test]
    fn a_runtime_is_read_by_platform_and_carries_its_checksum() {
        let m = Manifest::parse(
            "runtime_url_linux_x64 = \"https://example.invalid/llama-linux.tar.gz\"\n\
             runtime_sha256_linux_x64 = \"abc123\"\n\
             runtime_url_whisper_linux_x64 = \"https://example.invalid/whisper.tar.gz\"\n\
             runtime_sha256_whisper_linux_x64 = \"def456\"\n",
        );
        let rt = m.runtime("linux_x64").expect("the linux runtime is catalogued");
        assert_eq!(rt.url, "https://example.invalid/llama-linux.tar.gz");
        assert_eq!(rt.sha256, "abc123");
        assert_eq!(m.runtime("whisper_linux_x64").unwrap().sha256, "def456");
        assert!(
            m.runtime("macos_arm64").is_none(),
            "a platform with no entry is absent, not empty"
        );
    }

    #[test]
    fn a_runtime_url_without_a_checksum_is_dropped_rather_than_fetched_unverified() {
        // The whole point of the pairing: a URL to an executable that we cannot verify must never
        // reach the fetch. Absent is a capability the caller reports; unverified is a binary run.
        let m = Manifest::parse(
            "runtime_url_linux_x64 = \"https://example.invalid/unverified.tar.gz\"\n\
             runtime_url_macos_arm64 = \"https://example.invalid/mac.tar.gz\"\n\
             runtime_sha256_macos_arm64 = \"\"\n",
        );
        assert!(m.runtime("linux_x64").is_none(), "no checksum, no entry");
        assert!(m.runtime("macos_arm64").is_none(), "an empty checksum is no checksum");
    }

    #[test]
    fn the_shipped_manifest_pins_a_verified_runtime_for_linux() {
        // Reads the real file, so a hand-edit that drops a checksum fails here rather than at a
        // user's first enable.
        let m = Manifest::parse(include_str!("../../../agents/models.toml"));
        let rt = m.runtime("linux_x64").expect("linux_x64 must stay pinned and verified");
        assert!(rt.url.starts_with("https://"), "{}", rt.url);
        assert_eq!(rt.sha256.len(), 64, "a sha256 is 64 hex characters: {}", rt.sha256);
        assert!(m.runtime("whisper_linux_x64").is_some(), "the audio runtime is pinned too");
    }
}
