//! M3's eval — handwriting to text, against `methods/image-text/eval/cases.json`: 30 phone
//! photographs of handwritten pages (GNHK, CC BY 4.0), read with the house instruction
//! ([`fm_agent::prompts::IMAGE_INSTRUCTION`], or the model's own from the catalogue) and scored by
//! word error rate ([`fm_agent::eval::wer`]).
//!
//! **In the gate:** each reference scores 0 against itself; every `recorded` reading is re-scored.
//! **By hand:** `EVAL_MMPROJ=<projector> pixi run eval -- image <gguf> <name>`.

use fm_agent::eval::wer;
use fm_agent::imagetext::ReadImage;
use fm_agent::openai::OpenAiStep;
use serde_json::Value;
use std::collections::BTreeMap;

const CASES: &str =
    concat!(env!("CARGO_MANIFEST_DIR"), "/../../methods/image-text/eval/cases.json");
const IMAGES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../.eval-data/image-text");

fn load() -> Value {
    serde_json::from_str(&std::fs::read_to_string(CASES).expect("the case file")).expect("JSON")
}

#[test]
fn every_reference_scores_zero_and_recordings_are_scored() {
    let file = load();
    let mut sums: BTreeMap<String, (f64, u32)> = BTreeMap::new();
    for case in file["cases"].as_array().expect("cases") {
        let reference = case["reference"].as_str().unwrap();
        assert!(!reference.trim().is_empty(), "{} has no reference", case["id"]);
        assert_eq!(wer(reference, reference), 0.0);
        for (who, read) in case["recorded"].as_object().into_iter().flatten() {
            let e = sums.entry(who.clone()).or_default();
            e.0 += wer(reference, read.as_str().unwrap_or_default());
            e.1 += 1;
        }
    }
    for (who, (sum, n)) in sums {
        println!("{who:28} WER {:5.1}% over {n} pages", 100.0 * sum / f64::from(n));
    }
}

#[test]
#[ignore = "needs the images (pixi run eval-data -- handwriting) and a vision model server"]
fn a_live_reader_against_the_cases() {
    let Some(port) = std::env::var("FM_MODEL_PORT").ok().and_then(|p| p.parse::<u16>().ok()) else {
        eprintln!("skip: set FM_MODEL_PORT");
        return;
    };
    let model = std::env::var("FM_MODEL").unwrap_or_else(|_| "qwen3-vl-4b".into());
    let record = std::env::var("FM_EVAL_RECORD").is_ok();
    let instruction = std::env::var("FM_READ_PROMPT").ok();
    let reader = OpenAiStep::local(port, &model)
        .with_temperature(0.0)
        .with_max_tokens(1500)
        .with_image_instruction(instruction);
    let mut file = load();
    let (mut sum, mut n) = (0.0, 0u32);
    let t0 = std::time::Instant::now();
    // FM_EVAL_ONLY=<id,id>: a quick probe on a few pages (nothing is recorded for the others).
    let only: Option<Vec<String>> =
        std::env::var("FM_EVAL_ONLY").ok().map(|s| s.split(',').map(str::to_string).collect());
    for case in file["cases"].as_array_mut().expect("cases") {
        let id = case["id"].as_str().unwrap().to_string();
        if only.as_ref().is_some_and(|o| !o.contains(&id)) {
            continue;
        }
        // `FM_EVAL_IMAGES` reads the same pages from another folder, e.g. resized copies, to
        // measure what an image size costs in accuracy (2026-10-19, the speed study).
        let dir = std::env::var("FM_EVAL_IMAGES").unwrap_or_else(|_| IMAGES.to_string());
        let jpg = std::fs::read(format!("{dir}/{id}.jpg")).unwrap_or_else(|_| {
            panic!("{id}.jpg missing — run `pixi run eval-data -- handwriting`")
        });
        let read = reader.read_image(&jpg, "image/jpeg").unwrap_or_else(|e| format!("ERROR {e}"));
        let w = wer(case["reference"].as_str().unwrap(), &read);
        sum += w;
        n += 1;
        println!("{id:14} WER {:5.1}%", 100.0 * w);
        if record {
            case["recorded"][&model] = Value::String(read);
        }
    }
    println!(
        "== {model}: WER {:.1}% over {n} pages · {:.1}s total",
        100.0 * sum / f64::from(n),
        t0.elapsed().as_secs_f64()
    );
    if record {
        std::fs::write(CASES, serde_json::to_string_pretty(&file).unwrap() + "\n").unwrap();
    }
}
