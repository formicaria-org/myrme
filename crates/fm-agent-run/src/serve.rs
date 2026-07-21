//! The agent's **model runtime, tied to formicaria's life.** It launches the local model under the
//! watchdog (preflight + resource caps + guaranteed kill) and **stops it the moment fm-serve stops
//! answering** — so the model is on with formicaria and off with it, never left orphaned burning
//! resources. Start it alongside formicaria when the agent is enabled; when the app closes, this
//! exits and the model with it.

use clap::Parser;
use fm_agent::launch::SupervisedModel;
use fm_agent::preflight::Need;
use fm_agent::watchdog::{Limits, SystemMonitor};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::Duration;

#[derive(Parser)]
#[command(name = "agent-serve", about = "Serve the model, tied to formicaria's life (off when fm-serve is gone).")]
struct Args {
    /// The GGUF model file to serve.
    #[arg(long)]
    model_gguf: PathBuf,
    /// The llama.cpp runtime dir (holds llama-server + its shared libs).
    #[arg(long, default_value = "agents/runtime")]
    runtime: PathBuf,
    /// Port to serve the model on.
    #[arg(long, default_value_t = 8081)]
    model_port: u16,
    /// The fm-serve port to watch — when it stops answering, stop the model.
    #[arg(long, default_value_t = 8765)]
    serve_port: u16,
    #[arg(long, default_value_t = 2048)]
    ctx: u32,
    #[arg(long, default_value_t = 4)]
    threads: u32,
    /// How much headroom (bytes) to require above the model file, for preflight.
    #[arg(long, default_value_t = 1_000_000_000)]
    headroom: u64,
}

fn main() {
    if let Err(e) = run() {
        eprintln!("agent-serve: {e}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let args = Args::parse();
    let bin = args.runtime.join("llama-server");

    let mut cmd = Command::new(&bin);
    cmd.env("LD_LIBRARY_PATH", &args.runtime)
        .arg("-m")
        .arg(&args.model_gguf)
        .args([
            "--host", "127.0.0.1",
            "--port", &args.model_port.to_string(),
            "-c", &args.ctx.to_string(),
            "-t", &args.threads.to_string(),
            "--no-warmup",
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    // Preflight needs the model's footprint; the file size is a good floor.
    let model_bytes = std::fs::metadata(&args.model_gguf).map(|m| m.len()).unwrap_or(500_000_000);
    let need = Need::new(model_bytes, args.headroom);

    let model = SupervisedModel::launch(cmd, SystemMonitor, &need, Limits::conservative())?;
    println!(
        "model serving on 127.0.0.1:{} — tied to fm-serve on :{} (stops when the app closes).",
        args.model_port, args.serve_port
    );

    // Watch fm-serve. When it stops answering (app closed), or the watchdog already stopped the
    // model, end. The watchdog runs on its own thread; this loop only decides when to trip its stop.
    let stopper = model.stopper();
    loop {
        std::thread::sleep(Duration::from_secs(3));
        if model.finished() {
            println!("the model runtime ended (watchdog or exit).");
            break;
        }
        if !fmserve_alive(args.serve_port) {
            println!("formicaria is gone — stopping the model.");
            stopper.stop();
            break;
        }
    }

    let outcome = model.wait();
    println!("model stopped: {outcome:?}");
    Ok(())
}

/// Is fm-serve answering on `port`? A `GET /api/alive` → 200. No Origin header, so it passes the
/// CSRF guard; unreachable/timeout ⇒ not alive (⇒ stop the model).
fn fmserve_alive(port: u16) -> bool {
    let request = format!(
        "GET /api/alive HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n"
    );
    match fm_agent::http::send("127.0.0.1", port, request.as_bytes(), Duration::from_secs(2)) {
        Ok(raw) => String::from_utf8_lossy(&raw).starts_with("HTTP/1.1 200"),
        Err(_) => false,
    }
}
