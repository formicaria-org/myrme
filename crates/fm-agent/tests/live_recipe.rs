//! MANUAL live harness for `/recipe` (`decisions.md` 2026-10-18, *a recipe from a recording*): a
//! real transcript through a real model, then the check that keeps only what was said. Ignored by
//! default, like `live_whisper.rs`. Run it with a model server up and a transcript in a file:
//!
//! ```text
//! LD_LIBRARY_PATH=agents/runtime agents/runtime/llama-server \
//!   -m agents/models/Qwen3VL-4B-Instruct-Q4_K_M.gguf --host 127.0.0.1 --port 8095 -c 4096 &
//! LLM_PORT=8095 RECIPE_SAID=/path/to/transcript.txt \
//!   pixi run cargo test -p fm-agent --test live_recipe -- --ignored --nocapture
//! ```

use fm_agent::openai::OpenAiStep;
use fm_agent::recipe::{body, keep_what_was_said, read, INSTRUCTION};
use fm_agent::LlmStep;

#[test]
#[ignore = "needs a running model server; set LLM_PORT and RECIPE_SAID"]
fn a_real_recording_becomes_a_recipe_of_only_what_was_said() {
    let port: u16 = std::env::var("LLM_PORT").expect("set LLM_PORT").parse().expect("a u16");
    let path = std::env::var("RECIPE_SAID").expect("set RECIPE_SAID to a transcript file");
    let said = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {path}: {e}"));

    let answer = OpenAiStep::local(port, "local")
        .with_timeout(std::time::Duration::from_secs(600))
        .complete(INSTRUCTION, &said)
        .expect("the model answered")
        .content;
    println!("--- answer ---\n{answer}\n--------------");
    let parts = read(&answer).expect("the answer holds a recipe");
    let (kept, dropped) = keep_what_was_said(parts, &said);
    println!("--- note ({dropped} dropped) ---\n{}", body(&kept, "Ricetta", "ID", &said));
    assert!(!kept.ingredients.is_empty(), "no ingredient survived the check");
}
