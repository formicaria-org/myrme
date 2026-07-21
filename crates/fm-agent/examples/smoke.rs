//! A manual orchestration smoke test against a **live** local model server (not run in CI).
//!
//! Start a server first, e.g. llama.cpp's `llama-server -m <model.gguf> --host 127.0.0.1 --port 8081`,
//! then: `cargo run -p fm-agent --example smoke`. It runs the deterministic pipeline end to end and
//! prints the `ProposalDraft` the agent would hand to `create_proposal`.

use fm_agent::openai::OpenAiStep;
use fm_agent::{AgentError, InputDoc, ResearchRequest, SearchHit, StudyAssistant, WebSearch};

/// No web this run — the request sets `search: None`, so this is never called. Keeps the example
/// dependency-free while still exercising the real write step.
struct NoSearch;
impl WebSearch for NoSearch {
    fn search(&self, _query: &str) -> Result<Vec<SearchHit>, AgentError> {
        Err(AgentError::new("search not configured for this smoke test"))
    }
}

fn main() {
    let agent = StudyAssistant::new(OpenAiStep::local(8081, "lfm2.5-230m"), NoSearch);
    let req = ResearchRequest {
        host_note: "01DEMOHOSTNOTE".into(),
        ask: "Summarize the note in two short Markdown bullet points. Use only the note.".into(),
        inputs: vec![InputDoc {
            label: "mRNA note".into(),
            text: "mRNA vaccines deliver instructions for cells to make a harmless spike protein, \
                   which trains the immune system. They do not alter DNA."
                .into(),
        }],
        search: None,
    };

    match agent.run(&req) {
        Ok(draft) => {
            println!("host_note: {}", draft.host_note);
            println!("sources:   {:?}", draft.sources);
            println!("--- proposed new_body ---\n{}", draft.new_body);
        }
        Err(e) => eprintln!("agent error: {e}"),
    }
}
