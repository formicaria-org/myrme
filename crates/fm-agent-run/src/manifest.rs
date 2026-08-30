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
                    _ => {}
                },
            }
        }
        flush(&mut cur, &mut models); // the last block has no trailing [[models]] to flush it
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
        }
    }

    /// The phone's whisper model name — a `[[models]]` entry to fetch on demand, or `None`.
    pub fn mobile_whisper(&self) -> Option<&str> {
        self.whisper_mobile.as_deref()
    }

    /// The context window for `name` — the model's own `ctx` override, else the manifest default.
    pub fn model_ctx(&self, name: &str) -> u32 {
        self.model(name).and_then(|m| m.ctx).unwrap_or(self.ctx)
    }

    /// The thread count for `name` — the model's own `threads` override, else the manifest default.
    pub fn model_threads(&self, name: &str) -> u32 {
        self.model(name)
            .and_then(|m| m.threads)
            .unwrap_or(self.threads)
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
        Some(format!(
            "https://huggingface.co/{}/resolve/{rev}/{}",
            m.repo, m.file
        ))
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
        assert_eq!(
            m.file("lfm2.5-1.2b"),
            Some("LFM2.5-1.2B-Instruct-Q4_K_M.gguf")
        );
        assert_eq!(m.file("nope"), None);
        // repo is captured even though it appears before `file` in the first block and after it
        // in the second — key order within a block must not matter.
        assert_eq!(
            m.model("lfm2.5-230m").unwrap().repo,
            "LiquidAI/LFM2.5-230M-GGUF"
        );
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
}
