//! The **resident agent**: launches the model under the watchdog, then watches formicaria's
//! discussions and **responds when a user `@name`-mentions it** — so you talk to the assistant *in
//! the app's discussion box*, no CLI needed. It is tied to formicaria's life: when fm-serve stops
//! answering (app closed), it stops the model and exits — model on with the app, off with it, no
//! orphan. `/search` (with `--searxng-port`) searches the web; a plain discussion is chat-only.
//!
//!   pixi run agent-serve -- --searxng-port 8888

use clap::Parser;
use fm_agent::launch::SupervisedModel;
use fm_agent::preflight::Need;
use fm_agent::watchdog::{Limits, SystemMonitor};
use fm_agent_run::fmserve::FmServe;
use fm_agent_run::manifest::Manifest;
use fm_agent_run::Agent;
use std::path::PathBuf;
use std::process::Command;

#[derive(Parser)]
#[command(name = "agent-serve", about = "Serve the model + answer @name mentions; tied to formicaria's life.")]
struct Args {
    /// Where `models.toml`, `models/`, and `runtime/` live — the single manifest resolves the rest.
    #[arg(long, default_value = "agents")]
    agents_dir: PathBuf,
    /// Which catalogued model to run; defaults to the manifest's `default`.
    #[arg(long)]
    model: Option<String>,
    /// The model server port; defaults to the manifest's `port`.
    #[arg(long)]
    model_port: Option<u16>,
    /// The fm-serve port to serve/watch.
    #[arg(long, default_value_t = 8765)]
    serve_port: u16,
    /// The local web-search proxy port (enables /search); omit to run without the web.
    #[arg(long)]
    searxng_port: Option<u16>,
    /// The name users address it by (`@name`) and the git author of its proposals; defaults to the model.
    #[arg(long)]
    name: Option<String>,
    /// Context window; defaults to the manifest's `ctx`.
    #[arg(long)]
    ctx: Option<u32>,
    /// Inference threads; defaults to the manifest's `threads`.
    #[arg(long)]
    threads: Option<u32>,
    /// GPU layers to offload (`-ngl`); defaults from the manifest's `gpu` policy (99 unless `off`).
    #[arg(long)]
    ngl: Option<u32>,
    /// Longest reply in characters; defaults from the manifest's `max_reply_chars`.
    #[arg(long)]
    max_reply_chars: Option<usize>,
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

    // The manifest is the single source for which model, and the runtime defaults; flags override.
    let manifest = Manifest::read(&a.agents_dir.join("models.toml"))?;
    let model_name = a.model.clone().unwrap_or_else(|| manifest.default.clone());
    let file = manifest
        .file(&model_name)
        .ok_or_else(|| format!("model '{model_name}' is not in {}/models.toml", a.agents_dir.display()))?;
    let model_gguf = a.agents_dir.join("models").join(file);
    let runtime = a.agents_dir.join("runtime");
    let name = a.name.clone().unwrap_or_else(|| model_name.clone());
    let model_port = a.model_port.unwrap_or(manifest.port);
    // Per-model settings from the manifest (each falls back to the global default), CLI flags override.
    let ctx = a.ctx.unwrap_or_else(|| manifest.model_ctx(&model_name));
    let threads = a.threads.unwrap_or_else(|| manifest.model_threads(&model_name));
    // GPU by default, CPU fallback: `-ngl 99` under the manifest's `gpu` policy. On a machine with a
    // GPU (and a GPU-capable runtime), the model offloads; with no GPU it silently runs on CPU. `--ngl`
    // overrides.
    let ngl = a.ngl.unwrap_or_else(|| manifest.gpu_layers());

    // Launch the model under the watchdog (preflight → resource caps → guaranteed kill). Its
    // stdout/stderr are inherited (not nulled) so a crash is visible in the agent log.
    let bin = runtime.join("llama-server");
    let mut cmd = Command::new(&bin);
    cmd.env("LD_LIBRARY_PATH", &runtime)
        .arg("-m")
        .arg(&model_gguf)
        .args([
            "--host", "127.0.0.1",
            "--port", &model_port.to_string(),
            "-c", &ctx.to_string(),
            "-t", &threads.to_string(),
            "-ngl", &ngl.to_string(),
            "--no-warmup",
        ]);
    let model_bytes = std::fs::metadata(&model_gguf).map(|m| m.len()).unwrap_or(500_000_000);
    // Resident, not one-shot: no wall-clock cliff (a per-turn cap is already on the model call), so it
    // stays warm for the whole session instead of being SIGKILLed after 5 minutes.
    let model = SupervisedModel::launch(cmd, SystemMonitor, &Need::new(model_bytes, a.headroom), Limits::resident())?;
    fm_agent_run::watch::wait_ready(model_port);

    let agent = Agent {
        fm: FmServe::local(a.serve_port),
        model_port,
        model: name.clone(),
        searxng_port: a.searxng_port,
        max_reply_chars: a.max_reply_chars.unwrap_or(manifest.max_reply_chars),
        retrieve: a.retrieve,
        history_budget: a.history_budget,
    };
    println!(
        "agent '@{}' listening — model warm on :{}, watching fm-serve :{}{}. Mention @{} in a discussion.",
        name,
        model_port,
        a.serve_port,
        if a.searxng_port.is_some() { ", web on" } else { "" },
        name,
    );

    // The reusable watch loop — the same one the in-process mobile app runs, just with FmServe here.
    let stopper = model.stopper();
    fm_agent_run::watch::serve_loop(&agent, &name, &runtime, a.poll_secs, &|| model.finished(), &|| stopper.stop());

    let outcome = model.wait();
    println!("model stopped: {outcome:?}");
    Ok(())
}
