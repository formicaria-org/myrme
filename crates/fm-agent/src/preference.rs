//! **A preference memory** — what this person keeps changing about the model's output, remembered
//! as a *description* and retrieved by context.
//!
//! ## Why this shape, and not a fine-tune
//!
//! Measured on the owner's own vault, the corpus grows at roughly **240 review events a year**,
//! split across four tools and three models. Against that, the destinations form a ladder: retrieval
//! works at **N ≈ 20**, curated SFT is a two-year project, and preference optimisation over a broad
//! distribution is out of reach for the foreseeable future. So this is the rung that is actually
//! reachable — and the one whose result the user can *read*, correct and delete, which a set of
//! weights is not.
//!
//! The design follows PRELUDE/CIPHER (arXiv:2404.15269), the published system closest to this app
//! and measured at **exactly T = 200 rounds** — this vault's annual scale. It reduced cumulative
//! edit cost by **31%** on summarisation and **73%** on email writing, with **no training at all**.
//!
//! ## The one detail that decides whether this works
//!
//! **Retrieve the inferred description, never the raw edits.** In the same paper, feeding back the
//! retrieved *examples* made email writing **worse than doing nothing** (32,405 vs 31,103 cumulative
//! edit cost). It is the short natural-language statement of what the person prefers that carries
//! the signal, not the diffs it was inferred from. That is why [`Preference`] stores a `description`
//! and why [`Memory::prompt_block`] never emits the underlying text.
//!
//! ## What lives here
//!
//! Pure: similarity, selection and prompt assembly, with no I/O and no model. Producing an embedding
//! is an [`Embed`] seam and inferring a description is an ordinary [`crate::LlmStep`] call, exactly
//! as transcription and web search are seams rather than dependencies. A year of preferences is
//! about **600 kB**, so the whole memory is small enough to hold in RAM and to read exhaustively.

use crate::AgentError;

/// Turn text into a vector, so two contexts can be compared without either being understood.
///
/// A seam, not a dependency: the orchestrator must stay testable with no model running, and the same
/// reasoning already applies to [`crate::LlmStep`], [`crate::WebSearch`] and
/// [`crate::transcribe::Transcribe`].
pub trait Embed {
    fn embed(&self, text: &str) -> Result<Vec<f32>, AgentError>;
}

/// One thing this person prefers, learned from one correction.
#[derive(Clone, Debug, PartialEq)]
pub struct Preference {
    /// The embedded context this was learned in — what the note and the request were about.
    pub embedding: Vec<f32>,
    /// **The learned artefact**: one short sentence, in words a person can read and edit.
    ///
    /// This is what gets retrieved. Storing it as language rather than as weights is the whole
    /// reason the result is inspectable — the user can see what the assistant thinks they want, and
    /// say otherwise.
    pub description: String,
    /// Which tool this was learned from. A preference about transcripts should not be handed to the
    /// research prompt: the corpus is per-tool, and so is what it teaches.
    pub tool: String,
    /// The proposal this came from, so a description can always be traced back to the correction
    /// that produced it — and dropped if that correction is later deprecated.
    pub source: String,
}

/// Cosine similarity, the standard nearest-context measure.
///
/// Returns `0.0` for a zero-length or mismatched vector rather than `NaN`: a degenerate embedding
/// must rank last, never poison a sort. (`f32::partial_cmp` on `NaN` returns `None`, which silently
/// makes a comparator inconsistent.)
pub fn cosine(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() {
        return 0.0;
    }
    let (mut dot, mut na, mut nb) = (0.0f32, 0.0f32, 0.0f32);
    for (x, y) in a.iter().zip(b) {
        dot += x * y;
        na += x * x;
        nb += y * y;
    }
    if na == 0.0 || nb == 0.0 {
        return 0.0;
    }
    dot / (na.sqrt() * nb.sqrt())
}

/// Everything learned so far, held in memory because a year of it is about 600 kB.
#[derive(Clone, Debug, Default)]
pub struct Memory {
    pub prefs: Vec<Preference>,
}

/// How many contexts to retrieve. **k = 5**, which CIPHER found generally better than k = 1: one
/// nearest neighbour is a single anecdote, and several agree on what is actually stable.
pub const RETRIEVE_K: usize = 5;

impl Memory {
    /// The `k` preferences learned in the most similar contexts, best first.
    ///
    /// Filtered by `tool` when one is given: what someone wants from a transcript and what they want
    /// from a research note are different preferences, and mixing them is how a memory starts
    /// contradicting itself.
    pub fn nearest(&self, query: &[f32], tool: Option<&str>, k: usize) -> Vec<&Preference> {
        let mut scored: Vec<(f32, &Preference)> = self
            .prefs
            .iter()
            .filter(|p| tool.is_none_or(|t| p.tool == t))
            .map(|p| (cosine(query, &p.embedding), p))
            .collect();
        // Descending by score. `total_cmp` rather than `partial_cmp().unwrap()`: a NaN would panic
        // the sort, and this input comes from a model.
        scored.sort_by(|a, b| b.0.total_cmp(&a.0));
        scored.into_iter().take(k).map(|(_, p)| p).collect()
    }

    /// The block to prepend to a prompt: the retrieved *descriptions*, deduplicated, and nothing
    /// else.
    ///
    /// **Never the examples.** Feeding back retrieved edits measured *worse than no learning at all*
    /// (arXiv:2404.15269); the inferred description is what carries the signal. Returns `None` when
    /// there is nothing to say, so a caller adds no empty scaffolding to the prompt — a small model
    /// will happily parrot an empty heading back at the user.
    pub fn prompt_block(&self, query: &[f32], tool: Option<&str>, k: usize) -> Option<String> {
        let mut seen: Vec<&str> = Vec::new();
        for p in self.nearest(query, tool, k) {
            let d = p.description.trim();
            if !d.is_empty() && !seen.iter().any(|s| s.eq_ignore_ascii_case(d)) {
                seen.push(d);
            }
        }
        if seen.is_empty() {
            return None;
        }
        let mut out = String::from("What this person tends to prefer, from past corrections:\n");
        for d in seen {
            out.push_str("- ");
            out.push_str(d);
            out.push('\n');
        }
        Some(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pref(desc: &str, tool: &str, emb: &[f32]) -> Preference {
        Preference {
            embedding: emb.to_vec(),
            description: desc.into(),
            tool: tool.into(),
            source: "01ABC".into(),
        }
    }

    #[test]
    fn similarity_is_bounded_and_degenerate_input_ranks_last_rather_than_poisoning_the_sort() {
        assert!((cosine(&[1.0, 0.0], &[1.0, 0.0]) - 1.0).abs() < 1e-6);
        assert!(cosine(&[1.0, 0.0], &[0.0, 1.0]).abs() < 1e-6);
        // A zero vector and a mismatched length are 0.0, never NaN — a NaN comparator is
        // inconsistent and `sort_by` may panic on it.
        assert_eq!(cosine(&[0.0, 0.0], &[1.0, 1.0]), 0.0);
        assert_eq!(cosine(&[1.0], &[1.0, 1.0]), 0.0);
    }

    #[test]
    fn the_nearest_contexts_come_back_first_and_only_for_the_asking_tool() {
        let m = Memory {
            prefs: vec![
                pref("keep transcripts verbatim", "transcribe", &[1.0, 0.0]),
                pref("cite every claim", "research", &[0.0, 1.0]),
                pref("never invent a source", "research", &[0.1, 0.9]),
            ],
        };
        let near = m.nearest(&[0.0, 1.0], Some("research"), 5);
        assert_eq!(near.len(), 2, "a transcript preference is not a research preference");
        assert_eq!(near[0].description, "cite every claim", "closest first");
        // Unfiltered, the transcript preference is reachable again.
        assert_eq!(m.nearest(&[1.0, 0.0], None, 5).len(), 3);
    }

    #[test]
    fn the_prompt_block_carries_descriptions_only_and_never_the_examples() {
        let m = Memory {
            prefs: vec![
                pref("cite every claim", "research", &[0.0, 1.0]),
                pref("Cite Every Claim", "research", &[0.0, 0.9]), // same thing, said twice
            ],
        };
        let block = m.prompt_block(&[0.0, 1.0], Some("research"), RETRIEVE_K).unwrap();
        assert!(block.contains("- cite every claim"));
        // Deduplicated: a preference repeated across five contexts is one preference, and repeating
        // it five times in a prompt is how a small model starts obeying it to the exclusion of the
        // actual request.
        assert_eq!(block.matches("- ").count(), 1, "got:\n{block}");
        // Retrieving the raw edits measured WORSE than no learning at all; only the inferred
        // description is ever emitted.
        assert!(!block.contains("01ABC"));
    }

    #[test]
    fn an_empty_memory_adds_no_scaffolding_to_the_prompt() {
        // Not `Some("")`: a heading with nothing under it is something a small model will happily
        // parrot back at the user.
        assert_eq!(Memory::default().prompt_block(&[1.0, 0.0], None, RETRIEVE_K), None);
        let m = Memory { prefs: vec![pref("   ", "research", &[1.0, 0.0])] };
        assert_eq!(m.prompt_block(&[1.0, 0.0], None, RETRIEVE_K), None);
    }
}
