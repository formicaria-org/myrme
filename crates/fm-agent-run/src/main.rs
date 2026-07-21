//! The out-of-process study-assistant runner — **how a user invokes the agent.**
//!
//! It reads the vault, runs the deterministic pipeline against an already-running local model, and
//! produces a **proposal**: a `proposal/<id>` branch plus a `proposes:` note, attributed to the
//! model, which the user reviews in the app's Collaboration view and merges. The agent edits **only**
//! the one host note and can never touch `main` — the same guardrailed write path a person uses.
//!
//! Typical use (after `pixi run fetch-model` and serving the model it prints):
//!
//! ```text
//! pixi run agent-propose --vault . --note <ULID> --ask "Summarize and tidy this"
//! pixi run agent-propose --vault . --note <ULID> --ask "Explain X" --input <ULID> --search "X basics"
//! ```

use clap::Parser;
use fm_agent::openai::OpenAiStep;
use fm_agent::search::SearxngSearch;
use fm_agent::{AgentError, InputDoc, ResearchRequest, SearchHit, StudyAssistant, WebSearch};
use fm_core::{FileStore, Store};
use std::path::PathBuf;

/// Ask the local study assistant to propose a change to one note.
#[derive(Parser)]
#[command(name = "fm-agent-run", about = "Propose a study-assistant edit to one note (never main).")]
struct Args {
    /// The vault: a git repo of notes.
    #[arg(long)]
    vault: PathBuf,
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
    let mut store = FileStore::open(&args.vault).map_err(|e| e.to_string())?;

    // Gather the explicitly-named inputs — the agent reads these and nothing else.
    let mut inputs = Vec::new();
    for id in &args.inputs {
        let oid = id.parse().map_err(|_| format!("not a note id: {id}"))?;
        let obj = store
            .get(oid)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| format!("no such input note: {id}"))?;
        inputs.push(InputDoc {
            label: obj.title.clone().unwrap_or_else(|| id.clone()),
            text: obj.body.clone(),
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

    // Turn the draft into a guardrailed proposal, attributed to the model. The vault's own
    // `vault.json` sets the size limits; a breach is refused, never truncated.
    let limits = fm_core::descriptor::Descriptor::read(&args.vault)
        .map_err(|e| e.to_string())?
        .proposal_limits;
    let email = format!("{}@fm-agents.local", args.model);
    let prop = fm_app::commands::create_proposal(
        &mut store,
        &args.vault,
        &draft.host_note,
        &draft.new_body,
        &limits,
        Some((args.model.as_str(), email.as_str())),
    )
    .map_err(|e| e.to_string())?;

    // Commit the proposal note so it lists in Collaboration after a restart (the branch is already
    // its own commit). Best-effort, mirroring the app's own path.
    if fm_core::git::available() {
        let _ = fm_core::git::commit_all(&args.vault, "backup: proposal", &store.written());
    }

    println!("Proposed change to note {}", draft.host_note);
    println!("  proposal note: {}", prop.id);
    println!("  branch:        proposal/{}", prop.id);
    println!("  by:            {} <{}>", args.model, email);
    if !draft.sources.is_empty() {
        println!("  sources:       {}", draft.sources.join(", "));
    }
    println!("\nReview and merge it in the app's Collaboration view.");
    Ok(())
}
