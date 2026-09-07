//! The out-of-process study-assistant runner — **how a user invokes the agent.**
//!
//! It runs the deterministic pipeline against an already-running local model and produces a
//! **proposal**: a `proposal/<id>` branch plus a `proposes:` note, attributed to the model, which the
//! user reviews in the app's Collaboration view and merges. The agent edits **only** the one host note
//! and can never touch `main` — the same guardrailed write path a person uses.
//!
//! All vault I/O goes through a running `fm-serve` (the [`VaultAccess`] seam), never a second
//! `FileStore` or the git binary directly: fm-serve stays the single writer (no SQLite race), and the
//! path is identical to the resident agent's — the same code that will run in-process on Android.
//!
//! Typical use (formicaria running, after `pixi run fetch-model` and serving the model it prints):
//!
//! ```text
//! pixi run agent-propose --note <ULID> --ask "Summarize and tidy this"
//! pixi run agent-propose --note <ULID> --ask "Explain X" --input <ULID> --search "X basics"
//! ```

use clap::Parser;
use fm_agent::openai::OpenAiStep;
use fm_agent::search::SearxngSearch;
use fm_agent::{AgentError, InputDoc, ResearchRequest, SearchHit, StudyAssistant, WebSearch};
use fm_agent_run::fmserve::{FmServe, Origin, VaultAccess};

/// Ask the local study assistant to propose a change to one note.
#[derive(Parser)]
#[command(
    name = "fm-agent-run",
    about = "Propose a study-assistant edit to one note (never main)."
)]
struct Args {
    /// The running fm-serve to read/write through (start formicaria first).
    #[arg(long, default_value_t = 8765)]
    serve_port: u16,
    /// The note to improve. The proposal edits ONLY this note.
    #[arg(long)]
    note: String,
    /// What to ask the assistant to do, in your own words.
    #[arg(long)]
    ask: String,
    /// A note id to give it as reading — repeatable. The explicit input allow-list; it reads nothing else.
    #[arg(long = "input")]
    inputs: Vec<String>,
    /// Ask it to search the web (a seed query). Omit to skip the web entirely.
    #[arg(long)]
    search: Option<String>,
    /// The local model server's port and served name.
    #[arg(long, default_value_t = 8081)]
    model_port: u16,
    #[arg(long, default_value = "lfm2.5-230m")]
    model: String,
    /// The local SearXNG port (used only with --search).
    #[arg(long, default_value_t = 8888)]
    searxng_port: u16,
}

/// Used when no `--search` is given: the web is never touched, so this never runs.
struct NoSearch;
impl WebSearch for NoSearch {
    fn search(&self, _query: &str) -> Result<Vec<SearchHit>, AgentError> {
        Err(AgentError::new("no web search was requested"))
    }
}

fn main() {
    if let Err(e) = run(Args::parse()) {
        eprintln!("agent: {e}");
        std::process::exit(1);
    }
}

fn run(args: Args) -> Result<(), String> {
    let fm = FmServe::local(args.serve_port);
    if !fm.alive() {
        return Err(format!(
            "fm-serve is not answering on :{} — start formicaria first",
            args.serve_port
        ));
    }

    // Gather the explicitly-named inputs — the agent reads these and nothing else. Through the seam,
    // so fm-serve resolves each note (across vaults) and stays the single reader/writer.
    let mut inputs = Vec::new();
    for id in &args.inputs {
        let v = fm.get(id)?;
        let body = v["body"].as_str().unwrap_or_default();
        if body.is_empty() {
            return Err(format!("no such input note: {id}"));
        }
        inputs.push(InputDoc {
            label: v["title"].as_str().unwrap_or(id).to_string(),
            text: body.to_string(),
        });
    }

    let req = ResearchRequest {
        host_note: args.note.clone(),
        ask: args.ask.clone(),
        inputs,
        search: args.search.clone(),
    };

    // Run the deterministic pipeline against the running local model. Search is wired only when asked.
    let llm = OpenAiStep::local(args.model_port, &args.model);
    let draft = if args.search.is_some() {
        StudyAssistant::new(llm, SearxngSearch::local(args.searxng_port)).run(&req)
    } else {
        StudyAssistant::new(llm, NoSearch).run(&req)
    }
    .map_err(|e| e.to_string())?;

    // The proposal is created through fm-serve — the *same* guardrailed write path the resident agent
    // and a person use: the branch + `proposes:` note, attributed to the model, size-limited by the
    // target vault's own `vault.json` (refused, never truncated), committed by the single writer.
    let email = format!("{}@fm-agents.local", args.model);
    let origin =
        Origin { tool: "propose", query: Some(args.ask.clone()), sources: draft.sources.clone() };
    let prop =
        fm.create_proposal(&draft.host_note, &draft.new_body, &args.model, &email, &origin)?;
    let prop_id = prop["id"].as_str().unwrap_or("(unknown)");

    println!("Proposed change to note {}", draft.host_note);
    println!("  proposal note: {prop_id}");
    println!("  branch:        proposal/{prop_id}");
    println!("  by:            {} <{}>", args.model, email);
    if !draft.sources.is_empty() {
        println!("  sources:       {}", draft.sources.join(", "));
    }
    println!("\nReview and merge it in the app's Collaboration view.");
    Ok(())
}
