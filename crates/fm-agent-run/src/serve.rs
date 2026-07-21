//! The **resident agent**: launches the model under the watchdog, then watches formicaria's
//! discussions and **responds when a user `@name`-mentions it** — so you talk to the assistant *in
//! the app's discussion box*, no CLI needed. It is tied to formicaria's life: when fm-serve stops
//! answering (app closed), it stops the model and exits — model on with the app, off with it, no
//! orphan. `/search` (with `--searxng-port`) searches the web; a plain discussion is chat-only.
//!
//!   pixi run agent-serve -- --searxng-port 8888

use clap::Parser;
use fm_agent::convo;
use fm_agent::launch::SupervisedModel;
use fm_agent::preflight::Need;
use fm_agent::watchdog::{Limits, SystemMonitor};
use fm_agent_run::fmserve::FmServe;
use fm_agent_run::Agent;
use std::collections::HashSet;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::Duration;

#[derive(Parser)]
#[command(name = "agent-serve", about = "Serve the model + answer @name mentions; tied to formicaria's life.")]
struct Args {
    #[arg(long)]
    model_gguf: PathBuf,
    #[arg(long, default_value = "agents/runtime")]
    runtime: PathBuf,
    #[arg(long, default_value_t = 8081)]
    model_port: u16,
    /// The fm-serve port to serve/watch.
    #[arg(long, default_value_t = 8765)]
    serve_port: u16,
    /// The local web-search proxy port (enables /search); omit to run without the web.
    #[arg(long)]
    searxng_port: Option<u16>,
    /// The name users address it by (`@name`) and the git author of its proposals.
    #[arg(long, default_value = "lfm2.5-230m")]
    name: String,
    #[arg(long, default_value_t = 2048)]
    ctx: u32,
    #[arg(long, default_value_t = 4)]
    threads: u32,
    #[arg(long, default_value_t = 600)]
    max_reply_chars: usize,
    #[arg(long, default_value_t = 3)]
    retrieve: usize,
    #[arg(long, default_value_t = 4000)]
    history_budget: usize,
    #[arg(long, default_value_t = 1_000_000_000)]
    headroom: u64,
    #[arg(long, default_value_t = 3)]
    poll_secs: u64,
}

fn main() {
    if let Err(e) = run() {
        eprintln!("agent-serve: {e}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let a = Args::parse();

    // Launch the model under the watchdog (preflight → resource caps → guaranteed kill).
    let bin = a.runtime.join("llama-server");
    let mut cmd = Command::new(&bin);
    cmd.env("LD_LIBRARY_PATH", &a.runtime)
        .arg("-m")
        .arg(&a.model_gguf)
        .args([
            "--host", "127.0.0.1",
            "--port", &a.model_port.to_string(),
            "-c", &a.ctx.to_string(),
            "-t", &a.threads.to_string(),
            "--no-warmup",
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let model_bytes = std::fs::metadata(&a.model_gguf).map(|m| m.len()).unwrap_or(500_000_000);
    let model = SupervisedModel::launch(cmd, SystemMonitor, &Need::new(model_bytes, a.headroom), Limits::conservative())?;
    wait_ready(a.model_port);

    let agent = Agent {
        fm: FmServe::local(a.serve_port),
        model_port: a.model_port,
        model: a.name.clone(),
        searxng_port: a.searxng_port,
        max_reply_chars: a.max_reply_chars,
        retrieve: a.retrieve,
        history_budget: a.history_budget,
    };
    println!(
        "agent '@{}' listening — model warm on :{}, watching fm-serve :{}{}. Mention @{} in a discussion.",
        a.name,
        a.model_port,
        a.serve_port,
        if a.searxng_port.is_some() { ", web on" } else { "" },
        a.name,
    );

    let stopper = model.stopper();
    // Message ids already seen, so each is answered at most once.
    let mut handled: HashSet<String> = HashSet::new();
    loop {
        std::thread::sleep(Duration::from_secs(a.poll_secs));
        if model.finished() {
            println!("model runtime ended.");
            break;
        }
        if !agent.fm.alive() {
            println!("formicaria is gone — stopping the model.");
            stopper.stop();
            break;
        }

        let Ok(discs) = agent.fm.discussions() else { continue };
        let Some(arr) = discs.as_array() else { continue };
        for d in arr {
            let Some(id) = d["id"].as_str() else { continue };
            let Ok(view) = agent.fm.thread(id) else { continue };
            let Some(msgs) = view["messages"].as_array() else { continue };
            let Some(last) = msgs.last() else { continue };
            let (Some(mid), Some(body)) = (last["id"].as_str(), last["body"].as_str()) else {
                continue;
            };
            if !handled.insert(mid.to_string()) {
                continue; // already processed this message
            }
            if let Some((_who, intent)) = convo::addressed(body, &[a.name.as_str()]) {
                // Chat only in a discussion (nothing to propose an edit to there).
                match agent.handle(id, &intent, false) {
                    Ok(reply) => println!(
                        "@{} replied in {}: {}",
                        a.name,
                        &id[..8.min(id.len())],
                        reply.chars().take(60).collect::<String>()
                    ),
                    Err(e) => eprintln!("turn failed in {id}: {e}"),
                }
            }
        }
    }

    let outcome = model.wait();
    println!("model stopped: {outcome:?}");
    Ok(())
}

/// Wait for the model server to answer /health (up to ~40 s).
fn wait_ready(port: u16) {
    let req = format!("GET /health HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n");
    for _ in 0..40 {
        if let Ok(raw) = fm_agent::http::send("127.0.0.1", port, req.as_bytes(), Duration::from_secs(2)) {
            if String::from_utf8_lossy(&raw).contains("\"status\":\"ok\"") {
                return;
            }
        }
        std::thread::sleep(Duration::from_secs(1));
    }
}
