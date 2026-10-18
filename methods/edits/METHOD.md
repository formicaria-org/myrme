---
name: edits
description: Propose an edit to the note a discussion is about, as the person asked — fix, add, remove, restructure — keeping everything else as it was; the person reviews and merges the proposal.
---

# M5 — note edits

**Trigger:** the `/propose` command. **Capabilities:** `llm`, plus `web` with `/search`.

## Pipeline

1. The context holds the conversation, the note and its linked notes. The runner passes the
   first 1200 characters of each.
2. **One call, held to `edits::answer_schema()`**, under `prompts::EDITS_INSTRUCTION`. The answer
   is a list of edits: `replace` / `insert_after` / `delete` / `append`, each pointing at words
   copied from the note.
3. **Rust applies them to the whole note** (`edits::apply`), however much of it the model saw:
   - an edit whose words are not found exactly once is dropped and counted;
   - consecutive replaces with the same anchor are one replacement written in pieces;
   - an anchor with an escaped line break is found.
4. The runner proposes the result only if something applied. Otherwise it says it could not place
   the change.

## Eval

`eval/cases.json`: six requests:
- fix a typo;
- add a list item;
- turn a paragraph into bullets;
- write a formula as LaTeX;
- remove a line;
- append to a long note (1,358 characters, longer than the 1,200 the model is shown).

Each is checked for what must stay, what must appear and what must go.

| Model, 2026-10-18 (RTX 3050) | Whole new body (before) | Anchored edits |
|---|---|---|
| qwen3-4b-2507 | 3/6 | **5/6** |
| qwen3-vl-4b, the laptop default | 3/6 | **5/6** |

With the whole-body format, **both models returned only the changed part**: the one fixed sentence,
or the one appended line. Accepting such a proposal would have deleted the rest of the note.

**Open:** `latex`. Both models replace the sentence with the formula and lose the words "Kinetic
energy". It is a judgment the ask leaves open, and is kept strict in the case.
