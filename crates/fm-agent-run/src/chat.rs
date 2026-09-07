//! A **warm-session chat REPL** against a note's discussion, via a running fm-serve (single store, no
//! double storage). The model stays warm for the whole session — fastest on a weak device. `/search`
//! searches the web (through the local proxy, if `--searxng-port` is given); `/propose` proposes an
//! edit to the note. Your replies appear in the formicaria app instantly.
//!
//!   pixi run agent-serve -- --searxng-port 8888   # model (warm) + web + @name watcher
//!   pixi run agent-chat -- --note <ULID> --searxng-port 8888

use clap::Parser;
use fm_agent::convo;
use fm_agent_run::fmserve::{FmServe, VaultAccess};
use fm_agent_run::Agent;
use std::io::{BufRead, Write};

#[derive(Parser)]
#[command(
    name = "agent-chat",
    about = "Chat with the study assistant in a note's discussion (via fm-serve)."
)]
struct Args {
    #[arg(long)]
    note: String,
    #[arg(long, default_value_t = 8765)]
    serve_port: u16,
    #[arg(long, default_value_t = 8081)]
    model_port: u16,
    #[arg(long, default_value = "lfm2.5-230m")]
    model: String,
    /// The local web-search proxy port (enables /search); omit to run without the web.
    #[arg(long)]
    searxng_port: Option<u16>,
    #[arg(long, default_value_t = 600)]
    max_reply_chars: usize,
    #[arg(long, default_value_t = 3)]
    retrieve: usize,
    #[arg(long, default_value_t = 4000)]
    history_budget: usize,
}

fn main() {
    if let Err(e) = run() {
        eprintln!("agent-chat: {e}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let a = Args::parse();
    let agent = Agent {
        fm: FmServe::local(a.serve_port),
        model_port: a.model_port,
        model: a.model.clone(),
        searxng_port: a.searxng_port,
        web_direct: false, // CLI/desktop uses the local proxy (--searxng-port), never in-process HTTPS
        // The chat REPL doesn't transcribe (no audio-artifact selection); leave the runtime off.
        whisper_port: None,
        whisper_model: "ggml-base.en".into(),
        vision: false,
        max_reply_chars: a.max_reply_chars,
        retrieve: a.retrieve,
        history_budget: a.history_budget,
    };

    println!(
        "Study assistant ready (model {}, warm; store via fm-serve :{}{}).\n\
         /search searches the web, /propose proposes an edit to this note. Empty line / Ctrl-D quits.\n",
        a.model,
        a.serve_port,
        if a.searxng_port.is_some() { ", web on" } else { ", web off" }
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
        // Record the user's message, then answer (propose allowed — this is a note's discussion).
        agent.fm.reply(&a.note, msg)?;
        let intent = convo::parse(msg);
        let (reply, _) = agent.handle(&a.note, &intent, true, &|stage| println!("  … {stage}"))?;
        println!("\n{}> {reply}\n", a.model);
    }
    println!("bye.");
    Ok(())
}
