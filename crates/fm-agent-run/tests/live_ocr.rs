//! **The OCR, end to end, on real pages** (Myrme light, 2026-10-20): fetch its files the way an
//! install does (`fetch::ensure_ocr`, the catalogue's pins), open it (`ocr::open`), read every page of
//! an image eval, and score it with the eval's own word error rate — the OCR's text where it is
//! confident, and how many pages that is. `#[ignore]`d: it downloads ~40 MB and needs the pages.
//!
//! ```text
//! FM_OCR_TOOLS=<a folder to fetch into> [FM_EVAL_CASES=<cases.json>] [FM_EVAL_IMAGES=<dir>] \
//!   pixi run cargo test -p fm-agent-run --features download --test live_ocr -- --ignored --nocapture
//! ```
//!
//! Measured in the spike (L1): typed pages 0.8% WER, every one confident; handwriting 61.3%, none.

#![cfg(feature = "download")]

use fm_agent_run::manifest::Manifest;
use std::path::PathBuf;

const CASES: &str =
    concat!(env!("CARGO_MANIFEST_DIR"), "/../../methods/image-text/eval/cases.json");
const IMAGES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../.eval-data/image-text");

#[test]
#[ignore = "downloads the OCR (~40 MB) and reads the eval's pages; set FM_OCR_TOOLS"]
fn the_ocr_reads_the_eval_pages_through_the_install_path() {
    let Ok(tools) = std::env::var("FM_OCR_TOOLS").map(PathBuf::from) else {
        eprintln!("skip: set FM_OCR_TOOLS");
        return;
    };
    let manifest = Manifest::parse(fm_agent_run::CATALOGUE);
    let fetched = fm_agent_run::fetch::ensure_ocr(
        &tools,
        &manifest,
        Manifest::platform_key(),
        &|_, _| {},
        &|| false,
    )
    .expect("the OCR's files fetch and verify");
    assert!(fetched, "the catalogue pins an OCR");
    assert!(fm_agent_run::fetch::ocr_staged(&tools, &manifest, Manifest::platform_key()));
    let ocr = fm_agent_run::ocr::open(&tools, &manifest, None).expect("the OCR opens");

    let cases_path = std::env::var("FM_EVAL_CASES").unwrap_or_else(|_| CASES.into());
    let images = std::env::var("FM_EVAL_IMAGES").unwrap_or_else(|_| IMAGES.into());
    let cases: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&cases_path).unwrap()).unwrap();
    let (mut sum, mut n, mut confident, mut secs) = (0.0, 0, 0, 0.0);
    for case in cases["cases"].as_array().unwrap() {
        let id = case["id"].as_str().unwrap();
        let Ok(bytes) = std::fs::read(format!("{images}/{id}.jpg")) else {
            eprintln!("{id}: no image");
            continue;
        };
        let t0 = std::time::Instant::now();
        let page = ocr.read_page(&bytes).expect("a page reads");
        secs += t0.elapsed().as_secs_f64();
        let wer = fm_agent::eval::wer(case["reference"].as_str().unwrap(), &page.text);
        println!(
            "{id:14} WER {:6.1}%  conf {:.2}  {}",
            wer * 100.0,
            page.confidence,
            if page.confident() { "ocr" } else { "→ vision" }
        );
        sum += wer;
        n += 1;
        confident += usize::from(page.confident());
    }
    assert!(n > 0, "no pages read");
    println!(
        "== {}: WER {:.1}% over {n} pages · {confident} confident · {:.2}s a page",
        ocr.model(),
        sum / n as f64 * 100.0,
        secs / n as f64
    );
}
