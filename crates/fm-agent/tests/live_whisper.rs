//! MANUAL live harness: exercise the real [`WhisperServer`] client against a running whisper.cpp
//! `whisper-server`. Ignored by default (needs the staged runtime + weights + a wav), like the
//! model-leaf harnesses the plan calls for — hermetic CI proves the wire shape against a fake; this
//! proves it against the real binary. Run it with:
//!
//! ```text
//! LD_LIBRARY_PATH=agents/runtime agents/runtime/whisper-server \
//!   -m agents/models/ggml-base.en.bin --host 127.0.0.1 --port 8091 &
//! WHISPER_PORT=8091 WHISPER_WAV=agents/models/jfk.wav \
//!   pixi run -e default cargo test -p fm-agent --test live_whisper -- --ignored --nocapture
//! ```

use fm_agent::transcribe::{Transcribe, WhisperServer};

#[test]
#[ignore = "needs a running whisper-server; set WHISPER_PORT and WHISPER_WAV"]
fn it_transcribes_a_real_clip_through_the_shipped_client() {
    let port: u16 = std::env::var("WHISPER_PORT")
        .expect("set WHISPER_PORT to the running whisper-server port")
        .parse()
        .expect("WHISPER_PORT must be a u16");
    let wav = std::env::var("WHISPER_WAV").expect("set WHISPER_WAV to a wav file path");
    let bytes = std::fs::read(&wav).unwrap_or_else(|e| panic!("read {wav}: {e}"));

    let text = WhisperServer::local(port, "ggml-base.en")
        .transcribe(&bytes, "audio/wav")
        .expect("real transcription failed");

    println!("--- transcript ---\n{text}\n------------------");
    assert!(!text.trim().is_empty(), "the real server returned an empty transcript");
}
