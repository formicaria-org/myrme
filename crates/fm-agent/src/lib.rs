//! The study assistant, as a **deterministic orchestrator**.
//!
//! The agent *is* this class, not the model. It feeds well-defined inputs to a bounded LLM step and
//! turns the well-defined output into a proposal draft, in a fixed, auditable pipeline. **The model
//! never drives control flow, holds a tool, or loops** — every decision here is ours, so "never
//! berserk" is *structural*, not a hope: there is no autonomous loop to run away.
//!
//! The two model touch-points and the web sit behind narrow seams ([`LlmStep`], [`WebSearch`]), each
//! *text in → text out*, so the whole orchestrator is unit-tested with fakes and needs **no model and
//! no network**. Swapping Lucy 1.7B / LFM2.5 / a remote provider changes only which [`LlmStep`] is
//! plugged in; all resource-governance (preflight, cgroup, kill) wraps the orchestrator *process*,
//! never reaching in here.
//!
//! Its only output is a [`ProposalDraft`] — proposed text for the **one** host note the request
//! named. It creates and deletes nothing; the caller turns the draft into a guardrailed proposal via
//! `fm_app::commands::create_proposal`, so the agent inherits the same blast-radius bound a person has.
//!
//! The concrete seams live beside the orchestrator: [`openai`] (an [`LlmStep`] to a local model
//! server) and [`search`] (a text-only [`WebSearch`] to a local SearXNG), sharing minimal HTTP
//! plumbing in [`http`]. The orchestrator above never depends on them, so it stays testable with fakes.

pub mod adjunct;
pub mod convo;
pub mod grounding;
pub mod http;
pub mod imagetext;
pub mod launch;
/// Meetings found in an email exchange: the model quotes, Rust checks the quote and reads the date.
pub mod meetings;
pub mod openai;
pub mod preflight;
pub mod prompts;
pub mod search;
pub mod textmatch;
pub mod transcribe;
pub mod watchdog;

/// The fixed acknowledgement posted after a `/propose` turn. Deterministic on purpose: asking a tiny
/// model to "acknowledge in one sentence" is a meta-instruction it fails (it echoes the prompt), and
/// a proposal needs no model-written confirmation — so this is a constant, saving a call too.
pub const PROPOSAL_ACK: &str =
    "I've proposed an edit to this note — review and merge it in the Collaboration view.";

/// One conversational turn's output: a chat `reply` to post to the discussion, and — when `/propose`
/// was asked — a `proposal` body for the host note. The orchestrator owns the LLM calls that make it;
/// the caller (a store-aware runner) does the I/O (post the reply, create the proposal).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Turn {
    pub reply: Option<String>,
    pub proposal: Option<String>,
}

/// A well-defined error from a pipeline step. Deliberately opaque (a message): the orchestrator does
/// not branch on *why* a step failed, it stops.
#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct AgentError(String);

impl AgentError {
    pub fn new(msg: impl Into<String>) -> Self {
        Self(msg.into())
    }
}

/// Token accounting a model returns alongside its text — kept even when unused, because it is how a
/// caller sees the shape of what it paid for. Optional: a bare local server may omit it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Usage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
}

/// A model reply: the text, plus **why the model stopped** and its token usage. The finish reason is
/// the load-bearing addition (research 2026-07-21, mirroring smolagents/Goose): `"length"` means the
/// answer was **cut off at the token cap** — a truncation a study summary must never ship unnoticed —
/// versus `"stop"` for a complete answer. `None` when the server omits it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LlmResponse {
    pub content: String,
    pub finish_reason: Option<String>,
    pub usage: Option<Usage>,
}

impl LlmResponse {
    /// A complete (non-truncated) reply — the common shape a fake or a simple server returns.
    pub fn complete(content: impl Into<String>) -> Self {
        Self { content: content.into(), finish_reason: Some("stop".into()), usage: None }
    }

    /// Did the model stop because it hit the token cap? Then its text is truncated.
    pub fn truncated(&self) -> bool {
        self.finish_reason.as_deref() == Some("length")
    }
}

/// The **one** bounded model touch-point: text in → text out (plus why it stopped), no tools, no
/// loop, no state. Real implementations wrap a local llama.cpp/LFM step or a remote provider; tests
/// use a fake.
pub trait LlmStep {
    fn complete(&self, system: &str, user: &str) -> Result<LlmResponse, AgentError>;
}

/// The web seam: a **text-only** search+fetch the *orchestrator* runs — the model never calls it.
/// Real implementations point at a private SearXNG / a hosted API; tests use a fake.
pub trait WebSearch {
    fn search(&self, query: &str) -> Result<Vec<SearchHit>, AgentError>;
}

/// So a caller can pick the backend at runtime (the desktop's SearXNG proxy vs. the phone's
/// in-process HTTPS search) and hand `StudyAssistant::new` a `Box<dyn WebSearch>` without the whole
/// runner going generic over the choice.
impl WebSearch for Box<dyn WebSearch> {
    fn search(&self, query: &str) -> Result<Vec<SearchHit>, AgentError> {
        (**self).search(query)
    }
}

/// One text-only web result. No binaries ever enter the pipeline (see the plan's text-only rule).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchHit {
    pub title: String,
    pub url: String,
    pub text: String,
}

/// A user-provided input the agent may read — a note body or an asset's text, **already resolved to
/// text by the caller**. The agent never roams the vault: it sees exactly these and nothing else.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InputDoc {
    pub label: String,
    pub text: String,
}

/// What the user hands the agent, from a host note's discussion. The orchestrator — not the model —
/// decides what may be read: only what is named here is ever seen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResearchRequest {
    /// The note whose discussion invoked the agent — the **only** file a resulting proposal may
    /// change.
    pub host_note: String,
    /// The user's request, in their own words.
    pub ask: String,
    /// The explicit, user-supplied input documents the agent may read.
    pub inputs: Vec<InputDoc>,
    /// A web-search request, or `None` to skip the web entirely. The seed is refined before use.
    pub search: Option<String>,
}

/// The agent's **well-defined output**: proposed new text for the host note, plus the sources it drew
/// on (for the proposal's discussion). Deterministic given the seam outputs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProposalDraft {
    pub host_note: String,
    pub new_body: String,
    pub sources: Vec<String>,
}

/// The output of the **grounded research** pipeline: the [`ProposalDraft`] to turn into a proposal,
/// plus the [`GroundedNote`](crate::grounding::GroundedNote) it was built from — so the caller can
/// surface how much was verified vs. dropped ("2 unsupported claims were dropped") and log the
/// deterministic grounding metric.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Research {
    pub draft: ProposalDraft,
    pub grounded: crate::grounding::GroundedNote,
}

/// The deterministic study-assistant agent. Generic over its two seams so the whole pipeline is
/// exercised with fakes in tests and with real (model, network) implementations in production.
pub struct StudyAssistant<L: LlmStep, S: WebSearch> {
    llm: L,
    web: S,
}

impl<L: LlmStep, S: WebSearch> StudyAssistant<L, S> {
    pub fn new(llm: L, web: S) -> Self {
        Self { llm, web }
    }

    /// Run the **grounded research** pipeline: refine → search → number the sources → one bounded
    /// *quote-first* write → **deterministic substring-verify** ([`grounding::verify`]) that drops any
    /// claim whose verbatim quote is not found in its cited source → a note whose `## Sources` list is
    /// assembled by us from the *verified* set, so a source number or URL can never be hallucinated.
    /// Every claim in the returned body is backed by a quote actually present in a real source; the
    /// model never ships an unfounded claim. It runs on the [`LlmStep`]/[`WebSearch`] seams, so it is
    /// exercised entirely with fakes.
    pub fn research(&self, req: &ResearchRequest) -> Result<Research, AgentError> {
        if req.host_note.trim().is_empty() {
            return Err(AgentError::new("a request must name its host note"));
        }
        // Grounded research is search-first — with no seed there is nothing external to ground on.
        let hits = match req.search.as_deref().map(str::trim) {
            Some(seed) if !seed.is_empty() => {
                let query = self.refine_query(seed)?;
                self.web.search(&query)?
            }
            _ => Vec::new(),
        };
        // Number every source uniformly: web/wikipedia/github/arxiv hits first, then the user's named
        // inputs. The `id` is the `[n]` the model cites and the verifier checks — nothing else marks
        // where a source came from.
        let mut sources: Vec<crate::grounding::Source> = Vec::new();
        for h in &hits {
            sources.push(crate::grounding::Source {
                id: sources.len() + 1,
                title: h.title.clone(),
                url: h.url.clone(),
                text: h.text.clone(),
            });
        }
        for d in &req.inputs {
            sources.push(crate::grounding::Source {
                id: sources.len() + 1,
                title: d.label.clone(),
                url: String::new(),
                text: d.text.clone(),
            });
        }

        // One grounded write under the quote-first contract, over a numbered pack that MUST fit the
        // model's (small, local) context with room to finish the note. A tiny model favors a tight
        // top-K anyway (research 2026-07-22), so cap the sources and truncate each; if a source-heavy
        // query still overflows, retry once tighter rather than shipping — or refusing — a cut-off note.
        let write = |max_src: usize,
                     chars: usize|
         -> Result<(LlmResponse, Vec<crate::grounding::Source>), AgentError> {
            let used: Vec<_> = sources.iter().take(max_src).cloned().collect();
            let pack = crate::grounding::pack_sources(&used, chars);
            let user =
                format!("{}\n{}\n\n# Numbered sources\n{}", heading::TASK, req.ask.trim(), pack);
            Ok((self.llm.complete(prompts::GROUNDED_WRITE_INSTRUCTION, &user)?, used))
        };
        let (resp, used) = {
            let (r, u) = write(5, 700)?;
            if r.truncated() {
                write(3, 450)? // tighter second try: fewer, shorter sources leave more room to finish
            } else {
                (r, u)
            }
        };
        if resp.truncated() {
            return Err(AgentError::new(
                "the note was still too long after trimming sources — try a narrower question",
            ));
        }

        // Deterministic grounding: verify quotes, drop the unsupported, assemble the note + Sources.
        let grounded = crate::grounding::verify(&resp.content, &used);
        let draft = ProposalDraft {
            host_note: req.host_note.clone(),
            new_body: grounded.body.clone(),
            sources: used
                .iter()
                .map(|s| {
                    if s.url.is_empty() {
                        s.title.clone()
                    } else {
                        format!("{} — {}", s.title, s.url)
                    }
                })
                .collect(),
        };
        Ok(Research { draft, grounded })
    }

    /// One bounded LLM call that turns a rough request into a search query, falling back to the seed
    /// if the model returns nothing usable — the pipeline never stalls on an empty refinement. A
    /// truncated query is harmless (it is still a query), so it is not refused here.
    fn refine_query(&self, seed: &str) -> Result<String, AgentError> {
        let refined = self.llm.complete(prompts::QUERY_REFINE_INSTRUCTION, seed)?;
        let refined = refined.content.trim();
        Ok(if refined.is_empty() { seed.to_string() } else { refined.to_string() })
    }

    /// Handle one **conversational turn** in a discussion — the heart of the warm session loop.
    /// `history` is the prior conversation (oldest→newest, already summarized to fit by the caller);
    /// `intent` is the parsed latest message; `context` is the retrieved notes + web results the
    /// caller assembled (RAG + search). It always produces a chat `reply` (so the discussion sees the
    /// agent respond), capped at `max_reply_chars`, and a `proposal` body when `/propose` was asked.
    /// The orchestrator owns every LLM call; the caller does the I/O (post the reply, create the
    /// proposal). The user does not care how the reply arrives — this is the layer that makes it smooth.
    pub fn turn(
        &self,
        history: &str,
        intent: &crate::convo::Intent,
        context: &[InputDoc],
        max_reply_chars: Option<usize>,
    ) -> Result<Turn, AgentError> {
        let ctx = assemble_turn_context(history, context);

        // A proposal edit to the host note, when asked — reuses the house-format write step.
        let proposal = if intent.propose {
            let user = format!("{ctx}\n\n{}\n{}", heading::TASK, intent.ask.trim());
            let resp = self.llm.complete(prompts::OUTPUT_FORMAT_INSTRUCTION, &user)?;
            if resp.truncated() {
                return Err(AgentError::new(
                    "the proposed note was cut off at the token limit — raise max_tokens",
                ));
            }
            Some(strip_wrapping_fence(&resp.content))
        } else {
            None
        };

        // A conversational reply — always, so the discussion sees the agent respond. A proposal turn
        // uses a fixed acknowledgement (no second model call — a tiny model fails that meta-task); a
        // chat turn gets a real model-written answer.
        //
        // **Minimal pre/post (owner, 2026-07-22).** The chat prompt is just the provided context, then
        // the conversation, then the question — no `# Question`/`# Notes` scaffolding a tiny model
        // parrots back. The only post-processing is the length cap (the "never berserk" safety bound);
        // the reply is otherwise the model's own words. Earlier heavy orchestration (cross-note RAG +
        // echo-stripping) made a 230M model reply with leaked context, so it was removed.
        let reply = if proposal.is_some() {
            PROPOSAL_ACK.to_string()
        } else {
            let mut user = String::new();
            for doc in context {
                let t = doc.text.trim();
                if !t.is_empty() {
                    user.push_str(t);
                    user.push_str("\n\n");
                }
            }
            let h = history.trim();
            if !h.is_empty() {
                user.push_str(h);
                user.push('\n');
            }
            user.push_str(intent.ask.trim());
            let resp = self.llm.complete(prompts::CHAT_INSTRUCTION, &user)?;
            normalize_answer(&resp.content, max_reply_chars)
        };

        Ok(Turn { reply: Some(reply), proposal })
    }
}

/// Strip a single fence that wraps the *entire* answer (```` ```markdown … ``` ````) — a common
/// small-model tic. Leaves genuine inner code blocks alone; only unwraps when the whole thing is one
/// fence. Free function so it is trivially tested.
fn strip_wrapping_fence(s: &str) -> String {
    let t = s.trim();
    if let Some(rest) = t.strip_prefix("```") {
        if let Some((_lang, body)) = rest.split_once('\n') {
            if let Some(inner) = body.trim_end().strip_suffix("```") {
                return inner.trim().to_string();
            }
        }
    }
    t.to_string()
}

/// The section headings the proposal/context assemblers inject into a prompt. Defined **once** so a
/// second hand-typed copy cannot silently drift from the assembler — the trap this repo has been bitten
/// by. (The chat path no longer scaffolds with headings; see `turn`'s minimal prompt.)
mod heading {
    pub const CONVERSATION: &str = "# Conversation so far";
    pub const NOTES: &str = "# Notes and search results";
    pub const TASK: &str = "# Task";
}

/// Post-process a chat answer: normalise math delimiters to the note vocabulary, then cap the length
/// (the "never berserk" bound). Both are *format-correctness* steps, not the cosmetic echo-stripping we
/// removed — the reply is otherwise the model's own text. Models emit standard LaTeX (`\(..\)`, `\[..\]`)
/// but formicaria renders KaTeX `$..$`/`$$..$$`, so unconverted math shows as literal source in the note.
fn normalize_answer(content: &str, max_chars: Option<usize>) -> String {
    crate::convo::cap_reply(&normalize_math(content.trim()), max_chars)
}

/// Convert the standard-LaTeX math delimiters models emit into the KaTeX delimiters the renderer reads:
/// `\[ … \]` → `$$ … $$` (display) and `\( … \)` → `$ … $` (inline). Deterministic; leaves prose and
/// already-`$` math untouched.
fn normalize_math(s: &str) -> String {
    s.replace("\\[", "$$").replace("\\]", "$$").replace("\\(", "$").replace("\\)", "$")
}

/// Assemble a conversational turn's context: the prior conversation, then the retrieved notes and
/// search results — clearly delimited so the exact text is trivial to assert.
fn assemble_turn_context(history: &str, context: &[InputDoc]) -> String {
    let mut p = String::new();
    if !history.trim().is_empty() {
        p.push_str(heading::CONVERSATION);
        p.push('\n');
        p.push_str(history.trim());
        p.push('\n');
    }
    if !context.is_empty() {
        p.push('\n');
        p.push_str(heading::NOTES);
        p.push('\n');
        for d in context {
            p.push_str(&format!("## {}\n{}\n", d.label.trim(), d.text.trim()));
        }
    }
    p
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    /// A fake model that records every (system, user) call and returns canned outputs in order.
    struct FakeLlm {
        replies: RefCell<Vec<String>>,
        seen: RefCell<Vec<(String, String)>>,
    }
    impl FakeLlm {
        fn new(replies: &[&str]) -> Self {
            Self {
                replies: RefCell::new(replies.iter().rev().map(|s| s.to_string()).collect()),
                seen: RefCell::new(Vec::new()),
            }
        }
    }
    impl LlmStep for FakeLlm {
        fn complete(&self, system: &str, user: &str) -> Result<LlmResponse, AgentError> {
            self.seen.borrow_mut().push((system.to_string(), user.to_string()));
            self.replies
                .borrow_mut()
                .pop()
                .map(LlmResponse::complete)
                .ok_or_else(|| AgentError::new("no canned reply left"))
        }
    }

    /// A model that always stops at the token cap — its text is truncated.
    struct TruncatingLlm;
    impl LlmStep for TruncatingLlm {
        fn complete(&self, _: &str, _: &str) -> Result<LlmResponse, AgentError> {
            Ok(LlmResponse {
                content: "half a not".into(),
                finish_reason: Some("length".into()),
                usage: None,
            })
        }
    }

    /// A fake web that records its query and returns canned hits.
    struct FakeWeb {
        hits: Vec<SearchHit>,
        seen: RefCell<Vec<String>>,
    }
    impl WebSearch for FakeWeb {
        fn search(&self, query: &str) -> Result<Vec<SearchHit>, AgentError> {
            self.seen.borrow_mut().push(query.to_string());
            Ok(self.hits.clone())
        }
    }

    fn req(search: Option<&str>) -> ResearchRequest {
        ResearchRequest {
            host_note: "01HOST".into(),
            ask: "explain mRNA vaccines".into(),
            inputs: vec![InputDoc { label: "my notes".into(), text: "prior context".into() }],
            search: search.map(String::from),
        }
    }

    fn propose(ask: &str) -> crate::convo::Intent {
        crate::convo::Intent {
            ask: ask.into(),
            search: false,
            propose: true,
            research: false,
            transcribe: None,
        }
    }

    #[test]
    fn an_empty_refinement_falls_back_to_the_seed_query() {
        let llm = FakeLlm::new(&["   ", "written"]);
        let web = FakeWeb { hits: vec![], seen: RefCell::new(Vec::new()) };
        let agent = StudyAssistant::new(llm, web);

        agent.research(&req(Some("seed query"))).unwrap();
        assert_eq!(agent.web.seen.borrow().as_slice(), &["seed query".to_string()]);
    }

    #[test]
    fn a_wrapping_code_fence_is_stripped_but_inner_blocks_survive() {
        let web = FakeWeb { hits: vec![], seen: RefCell::new(Vec::new()) };
        let agent =
            StudyAssistant::new(FakeLlm::new(&["```markdown\n# Title\n\n- a\n- b\n```"]), web);
        let turn = agent.turn("", &propose("tidy this"), &[], None).unwrap();
        assert_eq!(turn.proposal.as_deref(), Some("# Title\n\n- a\n- b"));
        // A body with a genuine inner code block, not a wrapping fence, is left intact.
        assert_eq!(
            super::strip_wrapping_fence("see this:\n```rust\nfn x(){}\n```"),
            "see this:\n```rust\nfn x(){}\n```"
        );
    }

    #[test]
    fn normalize_answer_fixes_math_and_caps_only() {
        // No echo-stripping — a reply that mentions the question, or has headings, is shown as-is.
        assert_eq!(super::normalize_answer("2 + 2 = 4.", None), "2 + 2 = 4.");
        assert_eq!(super::normalize_answer("# Answer\nParis.", None), "# Answer\nParis.");
        // Math delimiters are converted to the note's KaTeX vocabulary so they actually render.
        assert_eq!(
            super::normalize_math("display \\[ a=b \\] and inline \\( x \\)"),
            "display $$ a=b $$ and inline $ x $"
        );
        assert_eq!(super::normalize_answer("Bayes: \\[ P(H|E) \\]", None), "Bayes: $$ P(H|E) $$");
        // Already-$ math is untouched.
        assert_eq!(super::normalize_math("$E=mc^2$ and $$x$$"), "$E=mc^2$ and $$x$$");
        // The safety bound: length. cap_reply trims to the cap at a word boundary.
        let long = "word ".repeat(50);
        assert!(super::normalize_answer(&long, Some(20)).chars().count() <= 21);
    }

    #[test]
    fn a_chat_turn_replies_and_does_not_propose() {
        use crate::convo::Intent;
        let web = FakeWeb { hits: vec![], seen: RefCell::new(Vec::new()) };
        let agent = StudyAssistant::new(FakeLlm::new(&["Here is the answer."]), web);
        let intent = Intent {
            ask: "what does the note say?".into(),
            search: false,
            propose: false,
            research: false,
            transcribe: None,
        };
        let ctx = [InputDoc { label: "note".into(), text: "the note body".into() }];
        let turn = agent.turn("earlier: hi", &intent, &ctx, Some(200)).unwrap();
        assert_eq!(turn.reply.as_deref(), Some("Here is the answer."));
        assert!(turn.proposal.is_none());
        // Exactly one model call (the reply); the writing prompt carried the history + the note.
        let seen = agent.llm.seen.borrow();
        assert_eq!(seen.len(), 1);
        assert!(seen[0].1.contains("earlier: hi") && seen[0].1.contains("the note body"));
    }

    #[test]
    fn a_propose_turn_produces_a_proposal_and_a_short_reply() {
        use crate::convo::Intent;
        let web = FakeWeb { hits: vec![], seen: RefCell::new(Vec::new()) };
        // Only one canned reply is needed — the proposal body; the acknowledgement is deterministic.
        let agent = StudyAssistant::new(FakeLlm::new(&["# Clean note\n\n- point"]), web);
        let intent = Intent {
            ask: "tidy this".into(),
            search: false,
            propose: true,
            research: false,
            transcribe: None,
        };
        let turn = agent.turn("", &intent, &[], Some(200)).unwrap();
        assert_eq!(turn.proposal.as_deref(), Some("# Clean note\n\n- point"));
        assert_eq!(turn.reply.as_deref(), Some(super::PROPOSAL_ACK));
        assert_eq!(agent.llm.seen.borrow().len(), 1, "one call to write; the ack is deterministic");
    }

    #[test]
    fn a_turn_reply_is_capped_at_the_max_length() {
        use crate::convo::Intent;
        let web = FakeWeb { hits: vec![], seen: RefCell::new(Vec::new()) };
        let agent = StudyAssistant::new(
            FakeLlm::new(&["this reply is quite a bit too long for the cap"]),
            web,
        );
        let intent = Intent {
            ask: "hi".into(),
            search: false,
            propose: false,
            research: false,
            transcribe: None,
        };
        let turn = agent.turn("", &intent, &[], Some(12)).unwrap();
        assert!(turn.reply.unwrap().chars().count() <= 12);
    }

    #[test]
    fn a_truncated_write_is_refused_not_proposed_half_finished() {
        let web = FakeWeb { hits: vec![], seen: RefCell::new(Vec::new()) };
        let agent = StudyAssistant::new(TruncatingLlm, web);
        let err = agent.turn("", &propose("tidy this"), &[], None).unwrap_err();
        assert!(format!("{err}").contains("cut off"), "a truncated note must be refused: {err}");
    }

    #[test]
    fn a_request_without_a_host_note_is_refused_before_any_call() {
        let llm = FakeLlm::new(&[]);
        let web = FakeWeb { hits: vec![], seen: RefCell::new(Vec::new()) };
        let agent = StudyAssistant::new(llm, web);

        let mut r = req(Some("x"));
        r.host_note = "  ".into();
        assert!(agent.research(&r).is_err());
        assert!(agent.llm.seen.borrow().is_empty(), "must not call the model on a bad request");
        assert!(agent.web.seen.borrow().is_empty());
    }

    #[test]
    fn research_grounds_the_note_verifies_quotes_and_drops_the_unsupported() {
        // Two real-ish web sources; the model refines the query, then writes a quote-first draft with
        // two supported claims and one fabricated. research() must keep the two, drop the fabrication,
        // and build a Sources list of the real links from the verified set.
        let web = FakeWeb {
            hits: vec![
                SearchHit {
                    title: "Paris".into(),
                    url: "https://en.wikipedia.org/wiki/Paris".into(),
                    text: "Paris is the capital and most populous city of France.".into(),
                },
                SearchHit {
                    title: "Eiffel Tower".into(),
                    url: "https://en.wikipedia.org/wiki/Eiffel_Tower".into(),
                    text: "The Eiffel Tower is a wrought-iron lattice tower on the Champ de Mars in Paris, France.".into(),
                },
            ],
            seen: RefCell::new(Vec::new()),
        };
        let draft = "\
- Paris is the capital of France [1]\n\
> \"Paris is the capital and most populous city of France\"\n\
- The Eiffel Tower stands in Paris [2]\n\
> \"The Eiffel Tower is a wrought-iron lattice tower on the Champ de Mars in Paris\"\n\
- The Eiffel Tower is made of solid gold [2]\n\
> \"the Eiffel Tower is made of solid gold\"";
        let agent =
            StudyAssistant::new(FakeLlm::new(&["capital of France Eiffel Tower", draft]), web);

        let mut r = req(Some("whats the capitol of france and the famus tower"));
        r.inputs = vec![]; // pure web grounding
        let out = agent.research(&r).unwrap();

        assert_eq!(out.grounded.verified.len(), 2, "the two supported facts survive");
        assert_eq!(out.grounded.dropped.len(), 1, "the 'solid gold' fabrication is dropped");
        assert!(out.draft.new_body.contains("Paris is the capital of France"));
        assert!(out.draft.new_body.contains("The Eiffel Tower stands in Paris"));
        // Every statement carries the inline link it came from (web-search mode).
        assert!(out.draft.new_body.contains("[\\[1\\]](https://en.wikipedia.org/wiki/Paris)"));
        assert!(out
            .draft
            .new_body
            .contains("[\\[2\\]](https://en.wikipedia.org/wiki/Eiffel_Tower)"));
        assert!(
            !out.draft.new_body.contains("solid gold"),
            "a fabricated claim must never reach the note"
        );
        // We assemble the Sources section from the verified set — real, un-hallucinable links.
        assert!(out.draft.new_body.contains("## Sources"));
        assert!(out.draft.new_body.contains("https://en.wikipedia.org/wiki/Paris"));
        assert!(out
            .draft
            .sources
            .iter()
            .any(|s| s.contains("https://en.wikipedia.org/wiki/Eiffel_Tower")));
        // The web was searched with the refined query; exactly two model calls: refine, then grounded write.
        assert_eq!(
            agent.web.seen.borrow().as_slice(),
            &["capital of France Eiffel Tower".to_string()]
        );
        let seen = agent.llm.seen.borrow();
        assert_eq!(seen.len(), 2);
        assert_eq!(seen[0].0, prompts::QUERY_REFINE_INSTRUCTION);
        assert_eq!(seen[1].0, prompts::GROUNDED_WRITE_INSTRUCTION);
    }

    #[test]
    fn research_retries_tighter_then_refuses_a_note_that_still_will_not_fit() {
        // A source-heavy query whose note keeps overflowing the small context: research retries with
        // fewer/shorter sources, and only if it STILL won't fit does it refuse — with a clear, actionable
        // message — rather than shipping a truncated note.
        let web = FakeWeb {
            hits: vec![SearchHit {
                title: "T".into(),
                url: "https://t".into(),
                text: "long source text".into(),
            }],
            seen: RefCell::new(Vec::new()),
        };
        let agent = StudyAssistant::new(TruncatingLlm, web);
        let err = agent.research(&req(Some("some very broad topic"))).unwrap_err();
        assert!(format!("{err}").contains("narrower question"), "got: {err}");
    }
}
