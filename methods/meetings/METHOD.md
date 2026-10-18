---
name: meetings
description: Read an email conversation and say which meetings are fixed or called off now, pointing at the sentence that says so. Used by Myrme's meeting pass on new mail; the date and time are read by Rust, never taken from the model.
---

# M1 — meetings from mail

**Trigger:** a schedule. The meeting pass runs when new mail arrives (formicaria's mail reader wakes
it).

**Capabilities:** `llm`.

## Pipeline

1. **The conversation note**, without Myrme's own box (`meetings::without_box`).
2. **One model call, held to `meetings::answer_schema()`**:
   - the instruction is `prompts::MEETINGS_INSTRUCTION`;
   - the conversation, cut from the front to fit the window, is `meetings::user_prompt`;
   - the answer is a list of `{status, quote, title, date, time, end, place}`.
3. **Rust checks and dates each item** (`meetings::resolve`):
   - the quote must be in the emails;
   - the date is read in that sentence, or earlier in the same email, counted from the day that
     email was sent;
   - the time is read only in that sentence;
   - a meeting that is already past is dropped.
4. **The plan** (`meetings::plan`) decides what to propose. The person accepts or rejects each
   proposal.

The prompt lives in `crates/fm-agent/src/prompts.rs`. This file describes the method; the Rust is
the authority on what runs.

## Eval

`eval/cases.json`: 17 invented conversations, in English and Italian. They cover:
- moves and call-offs, including a call-off followed by a new time;
- "that day";
- two meetings in one email;
- a link as the place;
- an end time;
- a meeting already past;
- a question accepted, a question declined, a question unanswered;
- three cases whose right answer is *nothing*.

Run them with `pixi run eval -- meetings <gguf> <name>`; `crates/fm-agent/tests/eval_meetings.rs`
explains the scoring.

| Model, 2026-10-18 (Q4_K_M or Q8_0, RTX 3050, 17 cases) | Free | Held to the schema |
|---|---|---|
| qwen3-4b-2507 | 10/17 | **17/17** |
| qwen3-vl-4b, the laptop default | 15/17 | **15/17**, about 4× faster |
| lfm2.5-vl-450m, the phone default | 6/17 | 5/17: it labels nearly every meeting "cancelled" |
| qwen3-4b-2507 on the CPU, stock grammar engine | | 17/17 in 115 s |
| qwen3-4b-2507 on the CPU, llguidance build | | 16/17 in 92 s |

## Decided on these numbers (`docs/context/decisions.md`, 2026-10-18)

- **Held to the schema.** Free answers left out the quote.
- **A question nobody has answered fixes nothing** (`Dropped::Unanswered`). Once someone else has
  replied, whether the reply accepted is the model's to read.
- **A quote of several sentences** is read in the one the model's own date or time words point at.
- **No llguidance:** about 20% faster on the CPU, no more accurate on this schema, and a custom
  llama.cpp build on every platform.
- **Not on the phone.** The 450M model cannot do this task. The pass is desktop-only today (mail is
  read on the computer), and stays so until a phone-sized model passes this eval.

## Open

- **Two meetings in one quote:** qwen3-vl-4b merged them into one item. Rust cannot recover a
  meeting the model left out.
- **A declined question:** some runs report the meeting as cancelled. The right reading is that
  nothing was ever fixed.
