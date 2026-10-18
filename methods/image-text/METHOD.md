---
name: image-text
description: Turn an image in a note — handwriting, a board, a photographed page — into a digital transcript, with math as LaTeX and code as fenced blocks, inserted beside the image as a proposal.
---

# M3 — image to text

**Trigger:** a file's type. `/transcribe`, or `/transcribe <asset>`, reads every image in the note
that has not been read yet. **Capabilities:** `vision`, meaning a dedicated reader if one runs, else
the main model if it can see, else nobody (`Agent::image_reader`, formicaria `decisions.md`
2026-10-10).

## Pipeline

1. The image bytes go to the reader with `prompts::IMAGE_INSTRUCTION`, or the model's own
   `read_prompt` from `agents/models.toml`.
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
| lfm2.5-vl-450m (the phone), house instruction | 82.7% | 8/30 | 95.6% | ≈5 s |
| lfm2.5-vl-450m, its catalogue instruction | 96.4% | 9/30 | 155.5% (9 pages loop) | ≈3 s |

**Decided on these numbers:**
- **The laptop reader is capped at 1024 image tokens** (`image_max_tokens`, a catalogue key that
  `Manifest::add_projector` passes as `--image-max-tokens`). Uncapped, a full-size photograph is
  more than the projector on the CPU can encode in time.
- **The phone reader cannot read handwriting.** It describes pages, or loops. This is the
  dedicated-reader work in Phase C (PaddleOCR-VL or a printed-text OCR first,
  `docs/research/2026-10-18-dedicated-methods-per-task.md`), not something to prompt away.

`agents/bench/vision/` holds the earlier typed-note fixtures and their results.

## Planned (Phase C)

- Route by content. Printed text goes to PP-OCRv5 first, escalating to the vision model only when
  confidence is low; math crops go to a formula model.
- This adds ONNX Runtime, which widens what ships and needs its own `decisions.md` entry.
