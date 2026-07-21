//! A **warm-session chat REPL** against a note's discussion — the fastest invocation on a
//! resource-poor device: the model server stays warm for the whole session, so each turn is one fast
//! call (cold-load once, not per message). You talk to the study assistant in the terminal; it reads
//! the note's discussion as context, retrieves relevant notes from your vault (RAG over FTS), replies
//! in the discussion, and — on `/propose` — creates a guardrailed proposal edit to the note.
//!
//! Serve a model first (see `agents/README.md`), then:
//!   pixi run agent-chat -- --vault . --note <ULID>
//! Type a message; `/search` and `/propose` are commands; an empty line or Ctrl-D quits.

use clap::Parser;
use fm_agent::openai::OpenAiStep;
use fm_agent::{convo, AgentError, InputDoc, SearchHit, StudyAssistant, WebSearch};
use fm_core::{FileStore, Store};
use std::io::{BufRead, Write};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "agent-chat", about = "Chat with the study assistant in a note's discussion.")]
struct Args {
    /// The vault: a git repo of notes.
    #[arg(long)]
    vault: PathBuf,
    /// The note whose discussion to chat in. The agent's proposals edit only this note.
    #[arg(long)]
    note: String,
    /// The warm model server's port and served name.
    #[arg(long, default_value_t = 8081)]
    model_port: u16,
    #[arg(long, default_value = "lfm2.5-230m")]
    model: String,
    /// Hard cap on the agent's reply length (characters).
    #[arg(long, default_value_t = 600)]
    max_reply_chars: usize,
    /// How many relevant notes to retrieve as context per turn (RAG over FTS).
    #[arg(long, default_value_t = 3)]
    retrieve: usize,
}

/// turn() never calls this (it takes pre-assembled context); present only to satisfy the type.
struct NoWeb;
impl WebSearch for NoWeb {
    fn search(&self, _query: &str) -> Result<Vec<SearchHit>, AgentError> {
        Err(AgentError::new("web search is not wired into the chat REPL yet"))
    }
}

fn main() {
    if let Err(e) = run() {
        eprintln!("agent-chat: {e}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let args = Args::parse();
    let mut store = FileStore::open(&args.vault).map_err(|e| e.to_string())?;
    let limits = fm_core::descriptor::Descriptor::read(&args.vault)
        .map_err(|e| e.to_string())?
        .proposal_limits;
    let email = format!("{}@fm-agents.local", args.model);
    let max = Some(args.max_reply_chars);

    println!(
        "Study assistant ready (model {}, warm). Type a message.\n\
         Commands: /search (web — not wired yet), /propose (propose an edit to this note).\n\
         Empty line or Ctrl-D to quit.\n",
        args.model
    );

    let stdin = std::io::stdin();
    loop {
        print!("you> ");
        std::io::stdout().flush().ok();
        let mut line = String::new();
        if stdin.lock().read_line(&mut line).map_err(|e| e.to_string())? == 0 {
            break; // Ctrl-D
        }
        let msg = line.trim();
        if msg.is_empty() {
            break;
        }

        // Record the user's message in the discussion.
        fm_app::commands::reply(&mut store, &args.note, msg).map_err(|e| e.to_string())?;

        let intent = convo::parse(msg);
        let history = thread_as_text(&store, &args.note)?;
        // Context = the host note itself (what the discussion is about) + related notes (RAG/FTS).
        let mut context = host_note(&store, &args.note)?;
        context.extend(retrieve(&store, &intent.ask, &args.note, args.retrieve)?);

        let agent = StudyAssistant::new(OpenAiStep::local(args.model_port, &args.model), NoWeb);
        let turn = agent.turn(&history, &intent, &context, max).map_err(|e| e.to_string())?;

        if let Some(reply) = &turn.reply {
            fm_app::commands::reply(&mut store, &args.note, reply).map_err(|e| e.to_string())?;
            println!("\n{}> {reply}\n", args.model);
        }
        if let Some(body) = &turn.proposal {
            let prop = fm_app::commands::create_proposal(
                &mut store,
                &args.vault,
                &args.note,
                body,
                &limits,
                Some((args.model.as_str(), email.as_str())),
            )
            .map_err(|e| e.to_string())?;
            println!("(proposed an edit → proposal {} on branch proposal/{})\n", prop.id, prop.id);
        }

        if fm_core::git::available() {
            let _ = fm_core::git::commit_all(&args.vault, "backup: chat", &store.written());
        }
    }
    println!("bye.");
    Ok(())
}

/// The host note itself, as context — it is what the discussion is about, so it is always included.
fn host_note(store: &FileStore, note: &str) -> Result<Vec<InputDoc>, String> {
    let Ok(id) = note.parse() else { return Ok(Vec::new()) };
    match store.get(id).map_err(|e| e.to_string())? {
        Some(obj) => Ok(vec![InputDoc {
            label: format!("{} (this note)", obj.title.clone().unwrap_or_default()),
            text: obj.body.chars().take(1200).collect(),
        }]),
        None => Ok(Vec::new()),
    }
}

/// The discussion so far as plain text, oldest first — the conversation history for the model.
fn thread_as_text(store: &FileStore, note: &str) -> Result<String, String> {
    let view = fm_app::commands::thread(store, note).map_err(|e| e.to_string())?;
    let mut s = String::new();
    for m in &view.messages {
        s.push_str("- ");
        s.push_str(m.body.trim());
        s.push('\n');
    }
    Ok(s)
}

/// RAG: the most relevant notes to the ask (FTS/BM25 over the vault), as trimmed context. Excludes
/// the host note itself and caps each note's text so a tiny model's context stays tight.
fn retrieve(store: &FileStore, ask: &str, host: &str, k: usize) -> Result<Vec<InputDoc>, String> {
    if ask.trim().is_empty() {
        return Ok(Vec::new());
    }
    let hits = fm_app::commands::search(store, ask).map_err(|e| e.to_string())?;
    let mut docs = Vec::new();
    for h in hits.iter().filter(|h| h.id != host).take(k) {
        let Ok(id) = h.id.parse() else { continue };
        if let Ok(Some(obj)) = store.get(id) {
            let text: String = obj.body.chars().take(600).collect();
            docs.push(InputDoc { label: obj.title.unwrap_or_else(|| h.id.clone()), text });
        }
    }
    Ok(docs)
}
