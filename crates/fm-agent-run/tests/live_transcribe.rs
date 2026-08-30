//! MANUAL live harness: drive the **real** audio→transcript flow through a running `fm-serve`
//! (the `FmServe` `VaultAccess`) and a running whisper.cpp `whisper-server` — the exact backend path
//! the UI's Transcribe action triggers. No LLM is needed (transcribe never calls the model), so this
//! exercises the pieces CI's fakes can't: `FmServe::blob_bytes` over the real `/api/blob` route, and
//! `transcribe_turn` threading a real blob → real whisper → a real `create_proposal`.
//!
//! Ignored by default. Env: `FM_SERVE_PORT`, `WHISPER_PORT`, `FM_NOTE` (host note id), `FM_ASSET`
//! (asset reference, e.g. `sha256:<hash>`). Orchestrated by the session's setup, not run in CI.

use fm_agent::convo::Intent;
use fm_agent_run::fmserve::FmServe;
use fm_agent_run::Agent;

#[test]
#[ignore = "needs a running fm-serve + whisper-server; set FM_SERVE_PORT, WHISPER_PORT, FM_NOTE, FM_ASSET"]
fn it_transcribes_through_the_real_fmserve_and_whisper() {
    let serve: u16 = std::env::var("FM_SERVE_PORT")
        .expect("FM_SERVE_PORT")
        .parse()
        .unwrap();
    let whisper: u16 = std::env::var("WHISPER_PORT")
        .expect("WHISPER_PORT")
        .parse()
        .unwrap();
    let note = std::env::var("FM_NOTE").expect("FM_NOTE");
    let asset = std::env::var("FM_ASSET").expect("FM_ASSET");

    let agent = Agent {
        fm: FmServe::local(serve),
        model_port: 0, // unused — transcribe never calls the LLM
        model: "whisper-test".into(),
        searxng_port: None,
        web_direct: false,
        whisper_port: Some(whisper),
        whisper_model: "ggml-base.en".into(),
        vision: false,
        max_reply_chars: 2000,
        retrieve: 3,
        history_budget: 4000,
    };
    let intent = Intent {
        ask: String::new(),
        search: false,
        propose: false,
        research: false,
        transcribe: Some(asset),
    };
    let (reply, _id) = agent
        .handle(&note, &intent, true, &|s| println!("stage: {s}"))
        .expect("the transcribe turn failed");
    println!("REPLY: {reply}");
    assert!(
        reply.contains("transcribed"),
        "expected a success reply, got: {reply}"
    );
}
