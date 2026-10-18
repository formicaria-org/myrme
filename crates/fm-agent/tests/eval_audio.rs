//! M4's eval — speech to text, against `methods/audio-text/eval/cases.json`: 30 Italian clips
//! (Multilingual LibriSpeech) and 30 English (LibriSpeech test-clean), scored by word error rate
//! ([`fm_agent::eval::wer`]) per language.
//!
//! **In the gate:** each case's reference scores 0 against itself, and every `recorded` transcript
//! is re-scored and printed. **By hand**, with the audio built (`pixi run eval-data -- speech`) and a
//! `whisper-server` running (`pixi run eval-speech -- <ggml model> <name>` does both):
//!
//! ```text
//! FM_WHISPER_PORT=8092 FM_WHISPER_MODEL=ggml-base [FM_EVAL_RECORD=1] \
//!   pixi run cargo test -p fm-agent --test eval_audio -- --ignored --nocapture
//! ```

use fm_agent::eval::wer;
use fm_agent::transcribe::{Transcribe, WhisperServer};
use serde_json::Value;
use std::collections::BTreeMap;

const CASES: &str =
    concat!(env!("CARGO_MANIFEST_DIR"), "/../../methods/audio-text/eval/cases.json");
const AUDIO: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../.eval-data/audio-text");

fn load() -> Value {
    serde_json::from_str(&std::fs::read_to_string(CASES).expect("the case file")).expect("JSON")
}

/// Mean WER per `(who, language)`.
fn table(rows: &[(String, String, f64)]) {
    let mut sums: BTreeMap<(String, String), (f64, u32)> = BTreeMap::new();
    for (who, lang, w) in rows {
        let e = sums.entry((who.clone(), lang.clone())).or_default();
        e.0 += w;
        e.1 += 1;
    }
    for ((who, lang), (sum, n)) in sums {
        println!("{who:28} {lang}  WER {:5.1}%  over {n} clips", 100.0 * sum / f64::from(n));
    }
}

#[test]
fn every_reference_scores_zero_and_recordings_are_scored() {
    let file = load();
    let mut rows = Vec::new();
    for case in file["cases"].as_array().expect("cases") {
        let reference = case["reference"].as_str().unwrap();
        assert!(!reference.is_empty(), "{} has no reference", case["id"]);
        assert_eq!(wer(reference, reference), 0.0);
        for (who, said) in case["recorded"].as_object().into_iter().flatten() {
            let lang = case["lang"].as_str().unwrap().to_string();
            rows.push((who.clone(), lang, wer(reference, said.as_str().unwrap_or_default())));
        }
    }
    table(&rows);
}

#[test]
#[ignore = "needs the audio (pixi run eval-data -- speech) and a whisper-server; set FM_WHISPER_PORT"]
fn a_live_reader_against_the_cases() {
    let Some(port) = std::env::var("FM_WHISPER_PORT").ok().and_then(|p| p.parse::<u16>().ok())
    else {
        eprintln!("skip: set FM_WHISPER_PORT");
        return;
    };
    let model = std::env::var("FM_WHISPER_MODEL").unwrap_or_else(|_| "ggml-base".into());
    let record = std::env::var("FM_EVAL_RECORD").is_ok();
    let reader = WhisperServer::local(port, &model);
    let mut file = load();
    let mut rows = Vec::new();
    let t0 = std::time::Instant::now();
    for case in file["cases"].as_array_mut().expect("cases") {
        let id = case["id"].as_str().unwrap().to_string();
        let audio = std::fs::read(format!("{AUDIO}/{id}.wav"))
            .unwrap_or_else(|_| panic!("{id}.wav missing — run `pixi run eval-data -- speech`"));
        let said = reader.transcribe(&audio, "audio/wav").unwrap_or_else(|e| format!("ERROR {e}"));
        let w = wer(case["reference"].as_str().unwrap(), &said);
        println!("{id:10} WER {:5.1}%  {}", 100.0 * w, said.chars().take(70).collect::<String>());
        rows.push((model.clone(), case["lang"].as_str().unwrap().to_string(), w));
        if record {
            case["recorded"][&model] = Value::String(said);
        }
    }
    table(&rows);
    println!("{:.1}s total", t0.elapsed().as_secs_f64());
    if record {
        std::fs::write(CASES, serde_json::to_string_pretty(&file).unwrap() + "\n").unwrap();
    }
}
