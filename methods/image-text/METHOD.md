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

None yet. Phase C plans a 50-image set from the owner's own notes, since no public handwriting
benchmark exists for small vision models (`docs/research/2026-10-18-dedicated-methods-per-task.md`).
`agents/bench/vision/` holds the earlier typed-note fixtures and their results.

## Planned (Phase C)

- Route by content. Printed text goes to PP-OCRv5 first, escalating to the vision model only when
  confidence is low; math crops go to a formula model.
- This adds ONNX Runtime, which widens what ships and needs its own `decisions.md` entry.
