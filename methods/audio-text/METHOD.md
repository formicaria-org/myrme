---
name: audio-text
description: Turn a recording in a note into a transcript in any language, inserted beside it as a proposal; and dictation, writing what is said at the caret.
---

# M4 — speech to text, and dictation

**Trigger:** a file's type (`/transcribe` on audio), and the dictation control in the editor.
**Capabilities:** `speech` (whisper.cpp).
**Wait:** class B, at the point of action — 2 s a recording or less, live for dictation.

## Pipeline

- The audio goes to whisper with `-l auto` (`ggml-base` by default; *better listening* uses
  `ggml-small`).
- The transcript is inserted after the recording (`adjunct`). It is insertion-only, and a clip
  already read is skipped.
- Dictation writes the words at the caret. A recording is never lost.

## Eval

`eval/cases.json`: 60 clips of read speech, 30 Italian (Multilingual LibriSpeech) and 30 English
(LibriSpeech test-clean), spread over each test file, CC BY 4.0 (see `NOTICE.md`).
`pixi run eval-data -- speech` fetches the pinned files and checks every clip. Scored by word error
rate (`fm_agent::eval::wer`) per language. Run it with
`pixi run eval-speech -- <weights> <name>`, which uses `-l auto` as formicaria does.

| Reader, 2026-10-18 (CPU, 60 clips) | English WER | Italian WER | Time |
|---|---|---|---|
| whisper `ggml-base` (the default) | 6.3% | 36.1% | 127 s |
| whisper `ggml-small` (*better listening*) | **4.1%** | **23.1%** | 363 s |

*Better listening* removes about a third of the errors in Italian, at about three times the time.
For someone who speaks Italian it is the one to suggest. Whether formicaria suggests it by language
is the owner's call. These are clean
audiobook readings. A voice memo in a kitchen will do worse, and the owner's own recordings are
the realistic set still missing.

## Planned (Phase C)

- Voice-activity detection (Silero) in front of whisper.
- Moonshine for live English dictation.
