//! Read page images with the OCR and print, per page, its time, mean confidence and whether the
//! assistant would keep its text — the phone's timing run for Myrme light's OCR (2026-10-20), and a
//! quick check anywhere else.
//!
//! ```text
//! ocr_page <dir with the OCR's files and libonnxruntime> <image>...
//! ```

#[cfg(feature = "ocr")]
fn main() -> Result<(), String> {
    use fm_agent_run::ocr::{PpOcr, ReadPage};
    use std::path::Path;
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (dir, pages) = args.split_first().ok_or("usage: ocr_page <dir> <image>...")?;
    let dir = Path::new(dir);
    let lib = ["libonnxruntime.so", "libonnxruntime.so.1.23.2", "libonnxruntime.1.23.2.dylib"]
        .iter()
        .map(|n| dir.join(n))
        .find(|p| p.exists())
        .ok_or("no ONNX Runtime library in the dir")?;
    let t0 = std::time::Instant::now();
    let ocr = PpOcr::open(
        &lib,
        &dir.join("pp-ocrv6_small_det.onnx"),
        &dir.join("pp-ocrv6_small_rec.onnx"),
        &dir.join("ppocrv6_dict.txt"),
    )?;
    println!("opened in {:.2}s", t0.elapsed().as_secs_f64());
    let mut total = 0.0;
    for p in pages {
        let bytes = std::fs::read(p).map_err(|e| format!("{p}: {e}"))?;
        let t = std::time::Instant::now();
        let page = ocr.read_page(&bytes)?;
        let secs = t.elapsed().as_secs_f64();
        total += secs;
        let name = Path::new(p).file_name().and_then(|n| n.to_str()).unwrap_or(p);
        println!(
            "{name:16} {secs:6.2}s  conf {:.2}  {}  {} chars",
            page.confidence,
            if page.confident() { "ocr" } else { "-> vision" },
            page.text.chars().count()
        );
    }
    println!("== {} pages, {:.2}s a page", pages.len(), total / pages.len().max(1) as f64);
    Ok(())
}

#[cfg(not(feature = "ocr"))]
fn main() {
    eprintln!("built without the `ocr` feature");
}
