//! The **one** reader of `agents/models.toml` — the model catalogue + runtime defaults. It replaces
//! the three divergent `awk` copies (`fetch.sh`, `serve.sh`, `agent-serve.sh`) and the hardcoded clap
//! defaults, so "the single place that says which model to run" is actually single.
//!
//! A tiny hand parser for this fixed, simple shape (top-level `key = value` + `[[models]]` blocks with
//! `name`/`file`) — no `toml` dep, keeping the runner dep-free for Android, in the same std-only,
//! minimal-deps stance as the hand-rolled HTTP client. Unknown keys and comments are ignored.

use std::path::Path;

/// The runtime defaults and the model list from `models.toml`.
pub struct Manifest {
    /// The model run when none is named.
    pub default: String,
    /// The model server port.
    pub port: u16,
    /// Context window (bounds the KV cache).
    pub ctx: u32,
    /// Inference threads.
    pub threads: u32,
    /// `(name, gguf-filename)` for each catalogued model.
    models: Vec<(String, String)>,
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
        let mut models: Vec<(String, String)> = Vec::new();
        let mut in_models = false;
        let mut cur_name: Option<String> = None;

        for raw in text.lines() {
            let line = raw.split('#').next().unwrap_or("").trim();
            if line.is_empty() {
                continue;
            }
            if line == "[[models]]" {
                in_models = true;
                cur_name = None;
                continue;
            }
            let Some((key, val)) = line.split_once('=') else { continue };
            let key = key.trim();
            let val = val.trim().trim_matches('"').trim();
            if in_models {
                match key {
                    "name" => cur_name = Some(val.to_string()),
                    "file" => {
                        if let Some(n) = cur_name.take() {
                            models.push((n, val.to_string()));
                        }
                    }
                    _ => {}
                }
            } else {
                match key {
                    "default" => default = val.to_string(),
                    "port" => port = val.parse().unwrap_or(port),
                    "ctx" => ctx = val.parse().unwrap_or(ctx),
                    "threads" => threads = val.parse().unwrap_or(threads),
                    _ => {}
                }
            }
        }
        Manifest { default, port, ctx, threads, models }
    }

    /// The gguf filename for `name`, or `None` if the catalogue has no such model.
    pub fn file(&self, name: &str) -> Option<&str> {
        self.models.iter().find(|(n, _)| n == name).map(|(_, f)| f.as_str())
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
    }

    #[test]
    fn a_malformed_number_keeps_the_builtin_default() {
        let m = Manifest::parse("ctx = not-a-number\nthreads = 8");
        assert_eq!(m.ctx, 2048);
        assert_eq!(m.threads, 8);
    }
}
