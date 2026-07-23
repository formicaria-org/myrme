//! A **manual, network-gated** end-to-end test of the grounded research profile against a *real* local
//! SearXNG and a *real* local model server — the "does it actually work, the way a user expects" check.
//!
//! It is `#[ignore]`d so `pixi run ci` (which forbids network and ships no model) never runs it, exactly
//! like the native-git differential harness. Run it deliberately on a host that has a SearXNG and a
//! `llama-server` up:
//!
//! ```text
//! FM_SEARXNG_PORT=8888 FM_MODEL_PORT=8081 FM_MODEL=qwen3-4b-2507 \
//!   pixi run cargo test -p fm-agent --test live_research -- --ignored --nocapture
//! ```
//!
//! What it checks on real data: the pipeline runs; the note is in the expected shape (claim bullets +
//! a `## Sources` section, or an honest "not supported" line); every source link is a well-formed
//! `http(s)` URL; and — the whole point of grounding — nothing the model could not back with a verbatim
//! quote from a real source reaches the note. Link *format* is checked here; link *resolution* would
//! need the (deferred) local reader proxy, since our own client is TLS-free/loopback-only.

use fm_agent::openai::OpenAiStep;
use fm_agent::search::{SearxngSearch, DEFAULT_ENGINES};
use fm_agent::{ResearchRequest, StudyAssistant};

fn env_port(key: &str) -> Option<u16> {
    std::env::var(key).ok().and_then(|v| v.parse().ok())
}

#[test]
#[ignore = "needs a live local SearXNG + model server; set FM_SEARXNG_PORT / FM_MODEL_PORT (/ FM_MODEL)"]
fn deep_research_over_a_real_searxng_and_model_yields_a_cited_note() {
    let (Some(searxng), Some(model_port)) = (env_port("FM_SEARXNG_PORT"), env_port("FM_MODEL_PORT")) else {
        eprintln!(
            "skip: set FM_SEARXNG_PORT and FM_MODEL_PORT (and optionally FM_MODEL) to run the live \
             deep-research test"
        );
        return;
    };
    let model = std::env::var("FM_MODEL").unwrap_or_else(|_| "qwen3-4b-2507".into());

    let agent = StudyAssistant::new(
        OpenAiStep::local(model_port, &model),
        SearxngSearch::local(searxng).with_engines(DEFAULT_ENGINES.iter().copied()),
    );

    // A real research question by default (what /research is for); override with FM_RESEARCH_Q so a
    // human can eyeball correctness on any topic.
    let ask = std::env::var("FM_RESEARCH_Q")
        .unwrap_or_else(|_| "What is retrieval-augmented generation (RAG) and why is it used with LLMs?".into());
    let req = ResearchRequest {
        host_note: "01LIVE_RESEARCH_HOST".into(),
        ask: ask.clone(),
        inputs: vec![],
        search: Some(ask),
    };

    let out = agent.research(&req).expect("the grounded research pipeline ran end to end");

    eprintln!("\n===== grounded note =====\n{}\n=========================", out.draft.new_body);
    eprintln!(
        "verified {} / dropped {} (quote-rate {:.2})",
        out.grounded.verified.len(),
        out.grounded.dropped.len(),
        out.grounded.verified_quote_rate(),
    );
    eprintln!("sources:\n{:#?}", out.draft.sources);

    assert!(!out.draft.new_body.trim().is_empty(), "the note must never be empty");

    if out.draft.new_body.contains("## Sources") {
        // A cited note: it must carry real links, every URL token must be well-formed, and every claim
        // line must carry a [n] citation (the format a user expects).
        assert!(out.draft.new_body.contains("http"), "a cited note must carry at least one real link");
        for tok in out.draft.new_body.split_whitespace().filter(|t| t.starts_with("http")) {
            assert!(
                tok.starts_with("http://") || tok.starts_with("https://"),
                "malformed source link: {tok}"
            );
        }
        for line in out.draft.new_body.lines().filter(|l| l.starts_with("- ") && !l.starts_with("- [")) {
            assert!(line.contains('[') && line.contains(']'), "a claim line must carry a [n] citation: {line}");
        }
    } else {
        // No verifiable support ⇒ the honest not-supported note, never a fabricated answer.
        assert!(
            out.draft.new_body.contains("did not support an answer"),
            "with no grounded claims the note must say so plainly, got: {}",
            out.draft.new_body
        );
    }

    // The grounding metric on real data: verified + dropped accounts for every claim the model made.
    assert_eq!(
        out.grounded.verified.len() + out.grounded.dropped.len(),
        out.grounded.total_claims(),
        "every claim is either verified or dropped — nothing is silently lost"
    );
}
