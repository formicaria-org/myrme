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
use serde_json::json;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

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
    /// How often to check for new mentions. Kept snappy (1s) for fast pickup — a poll only fetches a
    /// discussion's thread when its message count changed, so an idle poll is one cheap `discussions`
    /// call, not a scan of every thread.
    #[arg(long, default_value_t = 1)]
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
    // Message ids already seen, so each is answered at most once. Seed it with every message that
    // already exists, so a fresh start (or a restart) answers only what arrives *while it watches* —
    // never the historical backlog of every discussion.
    let mut handled: HashSet<String> = HashSet::new();
    // Last message count we saw per discussion, from the discussions list. A poll skips any
    // discussion whose count is unchanged — no thread fetch, no work — so it can run often (snappy
    // pickup) while costing only one `discussions()` call plus a `thread()` for what actually moved.
    let mut last_count: HashMap<String, u64> = HashMap::new();
    if let Ok(discs) = agent.fm.discussions() {
        if let Some(arr) = discs.as_array() {
            for d in arr {
                let Some(id) = d["id"].as_str() else { continue };
                last_count.insert(id.to_string(), d["count"].as_u64().unwrap_or(0));
                let Ok(view) = agent.fm.thread(id) else { continue };
                let Some(msgs) = view["messages"].as_array() else { continue };
                for m in msgs {
                    if let Some(mid) = m["id"].as_str() {
                        handled.insert(mid.to_string());
                    }
                }
            }
        }
    }
    let mut last_full_scan = Instant::now();
    loop {
        std::thread::sleep(Duration::from_secs(a.poll_secs));
        if model.finished() {
            println!("model runtime ended — the model server exited (see the log above for why).");
            break;
        }
        if !agent.fm.alive() {
            println!("formicaria is gone — stopping the model.");
            stopper.stop();
            break;
        }

        // Heartbeat: tell fm-serve we're alive and what we're called, so the app can show the
        // assistant as online, offer it in the @-picker, and warn (instead of going silent) when a
        // mention arrives while it is off.
        agent.fm.present(&a.name);

        // Most polls use the cheap count-skip, but every ~15s do a full rescan regardless — so a
        // message can never be *permanently* missed if a count ever fails to reflect a new message.
        let force_scan = last_full_scan.elapsed() >= Duration::from_secs(15);
        if force_scan {
            last_full_scan = Instant::now();
        }

        let Ok(discs) = agent.fm.discussions() else { continue };
        let Some(arr) = discs.as_array() else { continue };
        for d in arr {
            let Some(id) = d["id"].as_str() else { continue };
            let count = d["count"].as_u64().unwrap_or(0);
            if !force_scan && last_count.get(id) == Some(&count) {
                continue; // nothing new here since we last looked — skip the thread fetch entirely
            }
            let Ok(view) = agent.fm.thread(id) else { continue };
            let Some(msgs) = view["messages"].as_array() else { continue };
            // Answer *every* pending mention in order, not just the last — a user who fires several
            // questions quickly (there is a poll delay) must get every one, never only the newest.
            // Identical resends within this batch (a double-tap on send) are coalesced to one answer.
            let mut asked_this_pass: HashSet<String> = HashSet::new();
            let mut posted = 0u64; // messages we add this pass, so we can advance last_count past them
            for m in msgs {
                let (Some(mid), Some(body)) = (m["id"].as_str(), m["body"].as_str()) else {
                    continue;
                };
                if !handled.insert(mid.to_string()) {
                    continue; // already seen (seeded backlog, our own replies, or an earlier pass)
                }
                let Some((_who, intent)) = convo::addressed(body, &[a.name.as_str()]) else {
                    continue;
                };
                if !asked_this_pass.insert(intent.ask.clone()) {
                    continue; // a duplicate of one we are already answering this pass — coalesce
                }

                // Show the user what the whole pipeline is doing, stage by stage, then clear it when
                // the reply lands or the turn fails — the wheel never spins forever.
                let question = intent.ask.clone();
                // Time every stage as we enter it (the on_stage events already mark the boundaries),
                // so the timing log can show where the turn actually spends its time.
                let marks = RefCell::new(Vec::<(String, Instant)>::new());
                let on_stage = |stage: &str| {
                    marks.borrow_mut().push((stage.to_string(), Instant::now()));
                    agent.fm.activity(id, stage, &question);
                };
                // Chat only in a discussion (nothing to propose an edit to there).
                let result = agent.handle(id, &intent, false, &on_stage);
                log_timing(&a.runtime, id, &question, &marks.borrow(), Instant::now(), result.is_ok());
                match result {
                    Ok((reply, reply_id)) => {
                        // Mark the agent's own reply seen so it never answers itself — the tiny
                        // model echoes the @name in its output, which would otherwise loop forever.
                        if let Some(rid) = reply_id {
                            handled.insert(rid);
                        }
                        posted += 1;
                        println!(
                            "@{} replied in {}: {}",
                            a.name,
                            &id[..8.min(id.len())],
                            reply.chars().take(60).collect::<String>()
                        );
                    }
                    Err(e) => {
                        // A timeout or failure must be *visible* (never a silent stall): tell the
                        // user and record the notice so it is not itself re-processed.
                        eprintln!("turn failed in {id}: {e}");
                        let notice = format!("⚠️ I couldn't finish that one — {e}. Try again, or simplify it.");
                        if let Ok(meta) = agent.fm.reply(id, &notice) {
                            posted += 1;
                            if let Some(rid) = meta["id"].as_str() {
                                handled.insert(rid.to_string());
                            }
                        }
                    }
                }
                agent.fm.activity_done(id);
            }
            // Advance our watermark past everything now in this discussion (the messages we just saw
            // plus the replies we posted), so we skip it next poll until something new actually lands.
            last_count.insert(id.to_string(), count + posted);
        }
    }

    let outcome = model.wait();
    println!("model stopped: {outcome:?}");
    Ok(())
}

/// Append one turn's per-stage timing to `<runtime>/timing.jsonl` and echo a one-line summary, so it
/// is clear where the orchestration spends its time (the LLM is often *not* the slow part — a web
/// search or retrieval can dominate). Best-effort: never breaks or slows a turn.
fn log_timing(runtime: &Path, disc: &str, question: &str, marks: &[(String, Instant)], ended: Instant, ok: bool) {
    if marks.is_empty() {
        return;
    }
    // Duration of each stage = gap to the next stage's start (the last runs until the turn ended).
    let dur = |i: usize, t: &Instant| {
        let next = marks.get(i + 1).map(|(_, t2)| *t2).unwrap_or(ended);
        next.saturating_duration_since(*t).as_millis() as u64
    };
    let mut stages = serde_json::Map::new();
    for (i, (name, t)) in marks.iter().enumerate() {
        stages.insert(name.clone(), json!(dur(i, t)));
    }
    let total_ms = ended.saturating_duration_since(marks[0].1).as_millis() as u64;
    let slowest = marks
        .iter()
        .enumerate()
        .map(|(i, (name, t))| (name.as_str(), dur(i, t)))
        .max_by_key(|(_, ms)| *ms)
        .map(|(name, ms)| format!("{name} {ms}ms"))
        .unwrap_or_default();
    println!("  ⏱ turn {total_ms}ms (slowest: {slowest})");

    let ts = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let line = json!({
        "ts": ts,
        "disc": disc,
        "q": question.chars().take(80).collect::<String>(),
        "total_ms": total_ms,
        "ok": ok,
        "stages": stages,
    });
    let path = runtime.join("timing.jsonl");
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
        let _ = writeln!(f, "{line}");
    }
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
