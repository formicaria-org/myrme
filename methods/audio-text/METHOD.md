---
name: audio-text
description: Turn a recording in a note into a transcript in any language, inserted beside it as a proposal; and dictation, writing what is said at the caret.
---

# M4 — speech to text, and dictation

**Trigger:** a file's type (`/transcribe` on audio), and the dictation control in the editor.
**Capabilities:** `speech` (whisper.cpp).

## Pipeline

- The audio goes to whisper with `-l auto` (`ggml-base` by default; *better listening* uses
  `ggml-small`).
- The transcript is inserted after the recording (`adjunct`). It is insertion-only, and a clip
  already read is skipped.
- Dictation writes the words at the caret. A recording is never lost.

## Eval

None yet. Planned: a small multilingual clip set (English, Italian and the owner's other languages),
scored by word error rate. `tests/live_whisper.rs` is the existing manual run.

## Planned (Phase C)

- Voice-activity detection (Silero) in front of whisper.
- Moonshine for live English dictation.
