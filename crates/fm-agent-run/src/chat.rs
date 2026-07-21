//! A **warm-session chat REPL** against a note's discussion — talking to a **running `fm-serve`** over
//! its HTTP API, so fm-serve stays the single vault writer (no second `FileStore`, safe alongside the
//! live app; your replies appear in the app instantly). The model stays warm for the whole session —
//! the fastest strategy on a weak device (cold-load once, each turn a fast call).
//!
//!   pixi run serve-model          # warm model (one terminal)
//!   pixi run serve                # formicaria / fm-serve (its own terminal)
//!   pixi run agent-chat -- --note <ULID>   # chat (uses fm-serve, no double storage)
//!
//! Type a message; `/search` and `/propose` are commands; empty line or Ctrl-D quits.

mod fmserve;

use clap::Parser;
use fm_agent::openai::OpenAiStep;
use fm_agent::{convo, AgentError, InputDoc, SearchHit, StudyAssistant, WebSearch};
use fmserve::FmServe;
use std::io::{BufRead, Write};

#[derive(Parser)]
#[command(name = "agent-chat", about = "Chat with the study assistant in a note's discussion (via fm-serve).")]
struct Args {
    /// The note whose discussion to chat in. A /propose edits only this note.
    #[arg(long)]
    note: String,
    /// The running fm-serve's port (the single vault store).
    #[arg(long, default_value_t = 8080)]
    serve_port: u16,
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
    let fm = FmServe::local(args.serve_port);
    let email = format!("{}@fm-agents.local", args.model);
    let max = Some(args.max_reply_chars);

    println!(
        "Study assistant ready (model {}, warm; store via fm-serve on :{}).\n\
         Commands: /search (web — not wired yet), /propose (propose an edit to this note).\n\
         Empty line or Ctrl-D to quit.\n",
        args.model, args.serve_port
    );

    let stdin = std::io::stdin();
    loop {
        print!("you> ");
        std::io::stdout().flush().ok();
        let mut line = String::new();
        if stdin.lock().read_line(&mut line).map_err(|e| e.to_string())? == 0 {
            break;
        }
        let msg = line.trim();
        if msg.is_empty() {
            break;
        }

        // Record the user's message in the discussion (fm-serve is the writer).
        fm.reply(&args.note, msg)?;

        let intent = convo::parse(msg);
        let history = thread_text(&fm, &args.note)?;
        // Context = the host note itself (what the discussion is about) + related notes (RAG/FTS).
        let mut context = host_note(&fm, &args.note)?;
        context.extend(retrieve(&fm, &intent.ask, &args.note, args.retrieve)?);

        let agent = StudyAssistant::new(OpenAiStep::local(args.model_port, &args.model), NoWeb);
        let turn = agent.turn(&history, &intent, &context, max).map_err(|e| e.to_string())?;

        if let Some(reply) = &turn.reply {
            fm.reply(&args.note, reply)?;
            println!("\n{}> {reply}\n", args.model);
        }
        if let Some(body) = &turn.proposal {
            let made = fm.create_proposal(&args.note, body, &args.model, &email)?;
            let pid = made["id"].as_str().unwrap_or("?");
            println!("(proposed an edit → proposal {pid} on branch proposal/{pid})\n");
        }
    }
    println!("bye.");
    Ok(())
}

/// The discussion so far as plain text, oldest first — the conversation history for the model.
fn thread_text(fm: &FmServe, note: &str) -> Result<String, String> {
    let view = fm.thread(note)?;
    let mut s = String::new();
    if let Some(msgs) = view["messages"].as_array() {
        for m in msgs {
            if let Some(body) = m["body"].as_str() {
                s.push_str("- ");
                s.push_str(body.trim());
                s.push('\n');
            }
        }
    }
    Ok(s)
}

/// The host note itself — it is what the discussion is about, so always included.
fn host_note(fm: &FmServe, note: &str) -> Result<Vec<InputDoc>, String> {
    let v = fm.get(note)?;
    let body = v["body"].as_str().unwrap_or_default();
    if body.is_empty() {
        return Ok(Vec::new());
    }
    let title = v["title"].as_str().unwrap_or("this note");
    Ok(vec![InputDoc { label: format!("{title} (this note)"), text: body.chars().take(1200).collect() }])
}

/// RAG: the most relevant notes to the ask (FTS/BM25 over the vault), trimmed. Excludes the host note.
fn retrieve(fm: &FmServe, ask: &str, host: &str, k: usize) -> Result<Vec<InputDoc>, String> {
    if ask.trim().is_empty() {
        return Ok(Vec::new());
    }
    let hits = fm.search(ask)?;
    let mut docs = Vec::new();
    let Some(arr) = hits.as_array() else { return Ok(docs) };
    for h in arr.iter().filter(|h| h["id"].as_str() != Some(host)).take(k) {
        let Some(id) = h["id"].as_str() else { continue };
        if let Ok(note) = fm.get(id) {
            let body = note["body"].as_str().unwrap_or_default();
            let title = note["title"].as_str().unwrap_or(id);
            docs.push(InputDoc { label: title.to_string(), text: body.chars().take(600).collect() });
        }
    }
    Ok(docs)
}
