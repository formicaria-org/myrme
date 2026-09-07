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
#[command(
    name = "agent-serve",
    about = "Serve the model + answer @name mentions; tied to formicaria's life."
)]
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
    /// Search the web **in-process over HTTPS** instead of through the local proxy.
    ///
    /// What the phone has always done, and what a desktop without `python3` needs — the proxy is a
    /// Python script, so a released build that spawns no shell has no way to run it. Ignored when
    /// `--searxng-port` is given: an explicit proxy wins, because someone who started one meant it.
    /// Costs the DuckDuckGo engine, which needs HTML scraping the in-process client does not do;
    /// wikipedia, GitHub and arXiv remain.
    #[arg(long, default_value_t = false)]
    web_direct: bool,
    /// The local `whisper.cpp` server port (enables **/transcribe** audio→transcript). Omit to run
    /// without audio transcription. When set, a `whisper-server` is launched under the same watchdog
    /// as the model, from `<agents_dir>/runtime/whisper-server` + `<agents_dir>/models/<whisper-model>.bin`.
    #[arg(long)]
    whisper_port: Option<u16>,
    /// The whisper weights label — the file `models/<whisper-model>.bin` and the transcript provenance.
    #[arg(long, default_value = "ggml-base.en")]
    whisper_model: String,
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
    let file = manifest.file(&model_name).ok_or_else(|| {
        format!("model '{model_name}' is not in {}/models.toml", a.agents_dir.display())
    })?;
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
    // The multimodal projector, when this model has one and it has actually been fetched. Present ⇒
    // the same server also answers vision turns (`/describe`); absent ⇒ it serves text-only, exactly
    // as it always has, and the agent says so rather than asking a blind model to read a picture.
    let mmproj = manifest
        .model(&model_name)
        .and_then(|m| m.mmproj.clone())
        .map(|f| a.agents_dir.join("models").join(f))
        .filter(|p| p.exists());

    let bin = runtime.join("llama-server");
    let mut cmd = Command::new(&bin);
    cmd.env("LD_LIBRARY_PATH", &runtime).arg("-m").arg(&model_gguf).args([
        "--host",
        "127.0.0.1",
        "--port",
        &model_port.to_string(),
        "-c",
        &ctx.to_string(),
        "-t",
        &threads.to_string(),
        "-ngl",
        &ngl.to_string(),
        "--no-warmup",
    ]);
    if let Some(proj) = &mmproj {
        cmd.arg("--mmproj").arg(proj);
    }
    let model_bytes = std::fs::metadata(&model_gguf).map(|m| m.len()).unwrap_or(500_000_000);
    // Resident, not one-shot: no wall-clock cliff (a per-turn cap is already on the model call), so it
    // stays warm for the whole session instead of being SIGKILLed after 5 minutes.
    let model = SupervisedModel::launch(
        cmd,
        SystemMonitor,
        &Need::new(model_bytes, a.headroom),
        Limits::resident(),
    )?;
    fm_agent_run::watch::wait_ready(model_port);

    // Optional audio→transcript runtime: a second supervised process (`whisper-server`), launched only
    // when `--whisper-port` is given, killed on exit like the model. Laptop v1; the phone audio path is
    // a later spike. If its binary/weights aren't staged, launch fails loudly rather than degrading.
    let whisper = if let Some(wport) = a.whisper_port {
        let wbin = runtime.join("whisper-server");
        let wmodel = a.agents_dir.join("models").join(format!("{}.bin", a.whisper_model));
        if !wbin.exists() || !wmodel.exists() {
            return Err(format!(
                "whisper is enabled (--whisper-port {wport}) but its runtime or weights are missing: \
                 expected {} and {}. Run `pixi run fetch-whisper` first.",
                wbin.display(),
                wmodel.display()
            ));
        }
        let mut wcmd = Command::new(&wbin);
        wcmd.env("LD_LIBRARY_PATH", &runtime).args([
            "--host",
            "127.0.0.1",
            "--port",
            &wport.to_string(),
            "-m",
            wmodel.to_str().unwrap_or_default(),
            "-t",
            &threads.to_string(),
        ]);
        let wbytes = std::fs::metadata(&wmodel).map(|m| m.len()).unwrap_or(200_000_000);
        let w = SupervisedModel::launch(
            wcmd,
            SystemMonitor,
            &Need::new(wbytes, a.headroom),
            Limits::resident(),
        )?;
        // Wait for it to accept connections (it loads the model, then listens). Best-effort: the first
        // /inference blocks until ready anyway.
        for _ in 0..40 {
            if std::net::TcpStream::connect(("127.0.0.1", wport)).is_ok() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(500));
        }
        Some(w)
    } else {
        None
    };

    let agent = Agent {
        fm: FmServe::local(a.serve_port),
        model_port,
        model: name.clone(),
        searxng_port: a.searxng_port,
        web_direct: a.web_direct,
        whisper_port: a.whisper_port,
        // Vision is a property of the *weights loaded*, not a second server — so it is simply
        // whether the projector made it onto the command line above.
        vision: mmproj.is_some(),
        whisper_model: a.whisper_model.clone(),
        max_reply_chars: a.max_reply_chars.unwrap_or(manifest.max_reply_chars),
        retrieve: a.retrieve,
        history_budget: a.history_budget,
    };
    println!(
        "agent '@{}' listening — model warm on :{}, watching fm-serve :{}{}{}. Mention @{} in a discussion.",
        name,
        model_port,
        a.serve_port,
        if a.searxng_port.is_some() || a.web_direct { ", web on" } else { "" },
        if a.whisper_port.is_some() { ", audio on" } else { "" },
        name,
    );

    // The reusable watch loop — the same one the in-process mobile app runs, just with FmServe here.
    let stopper = model.stopper();
    fm_agent_run::watch::serve_loop(
        &agent,
        &name,
        &runtime,
        a.poll_secs,
        &|| model.finished(),
        &|| stopper.stop(),
    );

    // Take the whisper runtime down with the agent — no orphaned second process holding its model.
    if let Some(w) = whisper {
        w.stopper().stop();
        let _ = w.wait();
    }

    let outcome = model.wait();
    println!("model stopped: {outcome:?}");
    Ok(())
}
