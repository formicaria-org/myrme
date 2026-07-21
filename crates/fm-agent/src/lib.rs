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

pub mod convo;
pub mod http;
pub mod launch;
pub mod openai;
pub mod preflight;
pub mod search;
pub mod watchdog;

/// The fixed **house-format instruction** given to the model as the system prompt for the writing
/// step. It formats within a closed set — Markdown plus the note vocabulary the renderers already
/// understand — and is **not** user-editable, the same literal-free discipline the renderers enforce.
pub const OUTPUT_FORMAT_INSTRUCTION: &str = "\
You write the body of a study note. Output ONLY the note body itself — do NOT repeat the task or \
these instructions, do NOT add a heading like '# Task', and do NOT wrap the whole answer in a code \
fence. Use GitHub-flavored Markdown, and ONLY: headings, paragraphs, bullet and numbered lists, \
tables, fenced code blocks (for code only), block quotes, callouts (`> [!note]` / `> [!tip]` / \
`> [!warning]`), Mermaid diagrams (```mermaid fenced), and KaTeX math ($…$ inline, $$…$$ block). Do \
not invent other syntax, do not add front-matter, and do not answer from memory: use only the \
provided notes and search results, and say plainly when they do not answer the question.";

/// The system prompt for the optional query-refinement step: rough request in, one clean search
/// query out. Bounded, single-shot, no tools.
pub const QUERY_REFINE_INSTRUCTION: &str = "\
Rewrite the user's request as a single, well-formed web-search query. Fix spelling and grammar and \
keep it short. Output only the query, nothing else.";

/// The system prompt for a **conversational reply** in a discussion (as opposed to writing a note
/// body). Concise, grounded in the provided context, never a wrapping fence.
pub const CHAT_INSTRUCTION: &str = "\
You are a study assistant talking in a note's discussion. Answer the user's question directly, \
conversationally, and concisely, using ONLY the conversation, notes, and search results provided — \
never from memory — and say plainly when they do not answer the question. Do NOT add a heading, do \
NOT repeat the question, and do NOT wrap the reply in a code fence — just the answer.";

/// The system prompt for compressing older conversation so it fits a tiny model's context window.
pub const SUMMARY_INSTRUCTION: &str = "\
Summarize the following conversation compactly, keeping the facts, decisions, and open questions a \
reader would need to continue it. Plain prose, a few sentences at most. Output only the summary.";

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

    /// Run the fixed pipeline: (1) optionally refine the search query, (2) search text-only if asked,
    /// (3) assemble a deterministic prompt from the fixed instruction + the named inputs + the hits,
    /// (4) one bounded generation call, (5) return the draft. **No autonomous loop** — the model is
    /// called at most twice, and never chooses what happens next.
    pub fn run(&self, req: &ResearchRequest) -> Result<ProposalDraft, AgentError> {
        if req.host_note.trim().is_empty() {
            return Err(AgentError::new("a request must name its host note"));
        }

        let mut sources = Vec::new();

        // (1)–(2) Web, only if the user asked for it. The orchestrator runs the search; the model
        // does not "call a tool".
        let hits = match req.search.as_deref().map(str::trim) {
            Some(seed) if !seed.is_empty() => {
                let query = self.refine_query(seed)?;
                let hits = self.web.search(&query)?;
                sources.extend(hits.iter().map(|h| format!("{} — {}", h.title, h.url)));
                hits
            }
            _ => Vec::new(),
        };
        sources.extend(req.inputs.iter().map(|d| d.label.clone()));

        // (3)–(4) Deterministic prompt, one bounded writing call under the fixed house format.
        let user = assemble_prompt(req, &hits);
        let resp = self.llm.complete(OUTPUT_FORMAT_INSTRUCTION, &user)?;
        // A note cut off at the token cap must never be proposed silently — refuse, don't ship half.
        if resp.truncated() {
            return Err(AgentError::new(
                "the model's answer was cut off at the token limit — raise max_tokens and re-run",
            ));
        }

        // (5) Well-defined output for the one host note. Strip a wrapping code fence — small models
        // often wrap the whole answer in ```markdown despite being told not to.
        Ok(ProposalDraft {
            host_note: req.host_note.clone(),
            new_body: strip_wrapping_fence(&resp.content),
            sources,
        })
    }

    /// Compress a block of prior conversation into a short summary that fits a tiny model's context —
    /// **summarize-before-overflow**. One bounded call; on any failure the caller keeps the recent
    /// turns without the summary rather than losing the conversation.
    pub fn summarize(&self, text: &str) -> Result<String, AgentError> {
        let resp = self.llm.complete(SUMMARY_INSTRUCTION, text)?;
        Ok(strip_wrapping_fence(&resp.content))
    }

    /// One bounded LLM call that turns a rough request into a search query, falling back to the seed
    /// if the model returns nothing usable — the pipeline never stalls on an empty refinement. A
    /// truncated query is harmless (it is still a query), so it is not refused here.
    fn refine_query(&self, seed: &str) -> Result<String, AgentError> {
        let refined = self.llm.complete(QUERY_REFINE_INSTRUCTION, seed)?;
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
            let user = format!("{ctx}\n\n# Task\n{}", intent.ask.trim());
            let resp = self.llm.complete(OUTPUT_FORMAT_INSTRUCTION, &user)?;
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
        let reply = if proposal.is_some() {
            PROPOSAL_ACK.to_string()
        } else {
            let user = format!("{ctx}\n\n# Question\n{}", intent.ask.trim());
            let resp = self.llm.complete(CHAT_INSTRUCTION, &user)?;
            crate::convo::cap_reply(&strip_wrapping_fence(&resp.content), max_reply_chars)
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

/// Assemble a conversational turn's context: the prior conversation, then the retrieved notes and
/// search results — clearly delimited so the exact text is trivial to assert.
fn assemble_turn_context(history: &str, context: &[InputDoc]) -> String {
    let mut p = String::new();
    if !history.trim().is_empty() {
        p.push_str("# Conversation so far\n");
        p.push_str(history.trim());
        p.push('\n');
    }
    if !context.is_empty() {
        p.push_str("\n# Notes and search results\n");
        for d in context {
            p.push_str(&format!("## {}\n{}\n", d.label.trim(), d.text.trim()));
        }
    }
    p
}

/// Assemble the writing step's user prompt from the request and the search hits, in a fixed,
/// clearly-delimited structure. Free function (no `self`) so the exact text is trivial to assert.
fn assemble_prompt(req: &ResearchRequest, hits: &[SearchHit]) -> String {
    let mut p = String::new();
    p.push_str("# Task\n");
    p.push_str(req.ask.trim());
    p.push('\n');

    if !req.inputs.is_empty() {
        p.push_str("\n# Provided notes\n");
        for d in &req.inputs {
            p.push_str(&format!("## {}\n{}\n", d.label.trim(), d.text.trim()));
        }
    }

    if !hits.is_empty() {
        p.push_str("\n# Search results\n");
        for h in hits {
            p.push_str(&format!("## {} ({})\n{}\n", h.title.trim(), h.url.trim(), h.text.trim()));
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
            Ok(LlmResponse { content: "half a not".into(), finish_reason: Some("length".into()), usage: None })
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

    #[test]
    fn with_search_it_refines_then_searches_then_writes_once() {
        let llm = FakeLlm::new(&["mrna vaccine mechanism", "THE WRITTEN NOTE"]);
        let web = FakeWeb {
            hits: vec![SearchHit { title: "CDC".into(), url: "https://cdc.gov".into(), text: "facts".into() }],
            seen: RefCell::new(Vec::new()),
        };
        let agent = StudyAssistant::new(llm, web);

        let draft = agent.run(&req(Some("whats mrna vacine"))).unwrap();

        assert_eq!(draft.host_note, "01HOST");
        assert_eq!(draft.new_body, "THE WRITTEN NOTE");
        // Sources = the hit, then the named input.
        assert_eq!(draft.sources, vec!["CDC — https://cdc.gov".to_string(), "my notes".to_string()]);

        // The web was searched with the *refined* query, not the raw one.
        assert_eq!(agent.web.seen.borrow().as_slice(), &["mrna vaccine mechanism".to_string()]);
        // Exactly two model calls: refine, then write.
        let seen = agent.llm.seen.borrow();
        assert_eq!(seen.len(), 2);
        assert_eq!(seen[0].0, QUERY_REFINE_INSTRUCTION);
        assert_eq!(seen[1].0, OUTPUT_FORMAT_INSTRUCTION);
        // The writing prompt carried the ask, the named input, and the search text — nothing else.
        let write_prompt = &seen[1].1;
        assert!(write_prompt.contains("explain mRNA vaccines"));
        assert!(write_prompt.contains("prior context"));
        assert!(write_prompt.contains("facts"));
    }

    #[test]
    fn without_search_the_web_is_never_touched_and_the_model_is_called_once() {
        let llm = FakeLlm::new(&["NOTE FROM INPUTS ONLY"]);
        let web = FakeWeb { hits: vec![], seen: RefCell::new(Vec::new()) };
        let agent = StudyAssistant::new(llm, web);

        let draft = agent.run(&req(None)).unwrap();

        assert_eq!(draft.new_body, "NOTE FROM INPUTS ONLY");
        assert_eq!(draft.sources, vec!["my notes".to_string()]);
        assert!(agent.web.seen.borrow().is_empty(), "web must not be touched when no search asked");
        assert_eq!(agent.llm.seen.borrow().len(), 1, "only the writing call");
    }

    #[test]
    fn an_empty_refinement_falls_back_to_the_seed_query() {
        let llm = FakeLlm::new(&["   ", "written"]);
        let web = FakeWeb { hits: vec![], seen: RefCell::new(Vec::new()) };
        let agent = StudyAssistant::new(llm, web);

        agent.run(&req(Some("seed query"))).unwrap();
        assert_eq!(agent.web.seen.borrow().as_slice(), &["seed query".to_string()]);
    }

    #[test]
    fn a_wrapping_code_fence_is_stripped_but_inner_blocks_survive() {
        let web = FakeWeb { hits: vec![], seen: RefCell::new(Vec::new()) };
        let agent = StudyAssistant::new(FakeLlm::new(&["```markdown\n# Title\n\n- a\n- b\n```"]), web);
        let draft = agent.run(&req(None)).unwrap();
        assert_eq!(draft.new_body, "# Title\n\n- a\n- b");
        // A body with a genuine inner code block, not a wrapping fence, is left intact.
        assert_eq!(super::strip_wrapping_fence("see this:\n```rust\nfn x(){}\n```"), "see this:\n```rust\nfn x(){}\n```");
    }

    #[test]
    fn a_chat_turn_replies_and_does_not_propose() {
        use crate::convo::Intent;
        let web = FakeWeb { hits: vec![], seen: RefCell::new(Vec::new()) };
        let agent = StudyAssistant::new(FakeLlm::new(&["Here is the answer."]), web);
        let intent = Intent { ask: "what does the note say?".into(), search: false, propose: false };
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
        let intent = Intent { ask: "tidy this".into(), search: false, propose: true };
        let turn = agent.turn("", &intent, &[], Some(200)).unwrap();
        assert_eq!(turn.proposal.as_deref(), Some("# Clean note\n\n- point"));
        assert_eq!(turn.reply.as_deref(), Some(super::PROPOSAL_ACK));
        assert_eq!(agent.llm.seen.borrow().len(), 1, "one call to write; the ack is deterministic");
    }

    #[test]
    fn a_turn_reply_is_capped_at_the_max_length() {
        use crate::convo::Intent;
        let web = FakeWeb { hits: vec![], seen: RefCell::new(Vec::new()) };
        let agent = StudyAssistant::new(FakeLlm::new(&["this reply is quite a bit too long for the cap"]), web);
        let intent = Intent { ask: "hi".into(), search: false, propose: false };
        let turn = agent.turn("", &intent, &[], Some(12)).unwrap();
        assert!(turn.reply.unwrap().chars().count() <= 12);
    }

    #[test]
    fn a_truncated_write_is_refused_not_proposed_half_finished() {
        let web = FakeWeb { hits: vec![], seen: RefCell::new(Vec::new()) };
        let agent = StudyAssistant::new(TruncatingLlm, web);
        let err = agent.run(&req(None)).unwrap_err();
        assert!(format!("{err}").contains("cut off"), "a truncated note must be refused: {err}");
    }

    #[test]
    fn a_request_without_a_host_note_is_refused_before_any_call() {
        let llm = FakeLlm::new(&[]);
        let web = FakeWeb { hits: vec![], seen: RefCell::new(Vec::new()) };
        let agent = StudyAssistant::new(llm, web);

        let mut r = req(Some("x"));
        r.host_note = "  ".into();
        assert!(agent.run(&r).is_err());
        assert!(agent.llm.seen.borrow().is_empty(), "must not call the model on a bad request");
        assert!(agent.web.seen.borrow().is_empty());
    }
}
