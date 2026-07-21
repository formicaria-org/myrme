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

/// The fixed **house-format instruction** given to the model as the system prompt for the writing
/// step. It formats within a closed set — Markdown plus the note vocabulary the renderers already
/// understand — and is **not** user-editable, the same literal-free discipline the renderers enforce.
pub const OUTPUT_FORMAT_INSTRUCTION: &str = "\
You write the body of a study note. Output GitHub-flavored Markdown only, using ONLY: headings, \
paragraphs, bullet and numbered lists, tables, fenced code blocks, block quotes, callouts \
(`> [!note]` / `> [!tip]` / `> [!warning]`), Mermaid diagrams (```mermaid fenced), and KaTeX math \
($…$ inline, $$…$$ block). Do not invent other syntax, do not add front-matter, and do not answer \
from memory: use only the provided inputs and search results, and state plainly when they do not \
answer the question.";

/// The system prompt for the optional query-refinement step: rough request in, one clean search
/// query out. Bounded, single-shot, no tools.
pub const QUERY_REFINE_INSTRUCTION: &str = "\
Rewrite the user's request as a single, well-formed web-search query. Fix spelling and grammar and \
keep it short. Output only the query, nothing else.";

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

/// The **one** bounded model touch-point: text in → text out, no tools, no loop, no state. Real
/// implementations wrap a local llama.cpp/LFM step or a remote provider; tests use a fake.
pub trait LlmStep {
    fn complete(&self, system: &str, user: &str) -> Result<String, AgentError>;
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
        let new_body = self.llm.complete(OUTPUT_FORMAT_INSTRUCTION, &user)?;

        // (5) Well-defined output for the one host note.
        Ok(ProposalDraft { host_note: req.host_note.clone(), new_body, sources })
    }

    /// One bounded LLM call that turns a rough request into a search query, falling back to the seed
    /// if the model returns nothing usable — the pipeline never stalls on an empty refinement.
    fn refine_query(&self, seed: &str) -> Result<String, AgentError> {
        let refined = self.llm.complete(QUERY_REFINE_INSTRUCTION, seed)?;
        let refined = refined.trim();
        Ok(if refined.is_empty() { seed.to_string() } else { refined.to_string() })
    }
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
        fn complete(&self, system: &str, user: &str) -> Result<String, AgentError> {
            self.seen.borrow_mut().push((system.to_string(), user.to_string()));
            self.replies.borrow_mut().pop().ok_or_else(|| AgentError::new("no canned reply left"))
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
