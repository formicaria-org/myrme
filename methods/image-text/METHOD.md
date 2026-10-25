---
name: image-text
description: Turn an image in a note — handwriting, a board, a photographed page — into a digital transcript, with math as LaTeX and code as fenced blocks, inserted beside the image as a proposal.
---

# M3 — image to text

**Trigger:** a file's type. `/transcribe`, or `/transcribe <asset>`, reads every image in the note
that has not been read yet. **Capabilities:** `vision`, meaning a dedicated reader if one runs, else
the main model if it can see, else nobody (`Agent::image_reader`, formicaria `decisions.md`
2026-10-10).
**Wait:** class B, at the point of action — 2 s a page or less; a page the vision model reads takes longer, so it shows its progress.

## Pipeline

0. **The OCR reads first** (since Myrme 0.7.0: PP-OCRv6 small on ONNX Runtime, `fm-agent-run`'s
   `ocr.rs`, on Linux, macOS and the phone; not on Windows yet). Its text stands when its confidence
   is at least `ocr::CONFIDENT` (0.97), or when there is no vision model; the note then names the OCR.
   Otherwise step 1 reads the page. The OCR gives plain lines, so a printed table or code block comes
   back as lines of text.
1. The image bytes go to the vision reader with `prompts::IMAGE_INSTRUCTION`, or the model's own
   `read_prompt` from the catalogue (`crates/fm-agent-run/models.toml`).
2. The transcript is inserted after the image as a provenance-marked block (`adjunct`). The block is
   insertion-only and idempotent per (image, reader, model).

## Eval

`eval/cases.json`: 30 phone photographs of handwritten pages from GNHK's test split (CC BY 4.0, see
`NOTICE.md`), scored by word error rate against GNHK's own word annotation. Run it with
`EVAL_MMPROJ=<projector> pixi run eval -- image <gguf> <name>`.

| Reader, 2026-10-18 (RTX 3050) | Median WER | Pages under 50% | Mean | Time |
|---|---|---|---|---|
| qwen3-vl-4b, projector on the CPU, uncapped | — | 0/30 | — | every page over the 120 s limit |
| **qwen3-vl-4b, `image_max_tokens = 1024`** (the catalogue now) | **17.9%** | **24/30** | 29.0% | ≈35 s a page |
| lfm2.5-vl-450m (the phone until 2026-10-20), house instruction | 82.7% | 8/30 | 95.6% | ≈5 s |
| lfm2.5-vl-450m, its catalogue instruction | 96.4% | 9/30 | 155.5% (9 pages loop) | ≈3 s |

**Decided on these numbers:**
- **The laptop reader is capped at 1024 image tokens** (`image_max_tokens`, a catalogue key that
  `Manifest::add_projector` passes as `--image-max-tokens`). Uncapped, a full-size photograph is
  more than the projector on the CPU can encode in time.
- **The phone reader cannot read handwriting.** It describes pages, or loops. This is the
  dedicated-reader work in Phase C (PaddleOCR-VL or a printed-text OCR first,
  `docs/research/2026-10-18-dedicated-methods-per-task.md`), not something to prompt away.

**Since then (2026-10-20):**
- **The OCR, through the install path** (`crates/fm-agent-run/tests/live_ocr.rs`, laptop CPU):
  30 typed pages at 0.8% word error, all 30 confident, ≈1 s a page; these 30 handwritten pages at
  61.3%, none confident, so every handwritten page goes to the vision model.
- **The phone reads with Qwen3.5-0.8B**, image tokens capped at 1024: about 43% word error on these
  handwritten pages and 2.3% on typed ones; on the phone, a median 28 s a page
  (`docs/research/2026-10-20-small-model-benchmark-summary.md` §5).

`agents/bench/vision/` holds the earlier typed-note fixtures and their results.

## Planned (Phase C)

- Printed text by the OCR first is done (Myrme 0.7.0, `docs/context/decisions.md` 2026-10-20, which
  also added ONNX Runtime). Still planned: math crops to a formula model, and a better reader for
  handwriting on the phone.
