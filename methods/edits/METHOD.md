---
name: edits
description: Propose an edit to the note a discussion is about, as the person asked — fix, add, remove, restructure — keeping everything else as it was; the person reviews and merges the proposal.
---

# M5 — note edits

**Trigger:** the `/propose` command. **Capabilities:** `llm`.
**Wait:** class C, a job a person started — 10 s or less, or progress and a way to stop.

## Pipeline

1. The context holds the conversation, the note and its linked notes.
   - The note is given as `fm_agent::shown_note` gives it: its first 1200 characters, plus a line
     saying it goes on when it does.
2. **First, the kind of change, from the request alone** (`prompts::EDIT_KIND_INSTRUCTION`,
   `edits::kind_schema`): append, insert_after, replace, delete, or several.
3. **Then one call, held to `edits::answer_schema_for(kind)`**, under `prompts::EDITS_INSTRUCTION`. The answer
   is a list of edits: `replace` / `insert_after` / `delete` / `append`, each pointing at words
   copied from the note.
4. **Rust applies them to the whole note** (`edits::apply`), however much of it the model saw:
   - an edit whose words are not found exactly once is dropped and counted;
   - consecutive replaces with the same anchor are one replacement written in pieces;
   - an anchor with an escaped line break is found.
5. The runner proposes the result only if something applied. Otherwise it says it could not place
   the change.

## Eval

`eval/cases.json`: seven requests:
- fix a typo;
- add a list item;
- turn a paragraph into bullets;
- write a formula as LaTeX;
- remove a line;
- append to a long note (1,358 characters, longer than the 1,200 the model is shown);
- add a final line to a long note of near-identical sentences, the live failure.

Each is checked for what must stay, what must appear and what must go.

| Model, 2026-10-18 (RTX 3050) | Whole new body (before) | Anchored edits | + cut note marked, append and replace rules (7 cases) |
|---|---|---|---|
| qwen3-4b-2507 | 3/6 | 5/6 | **7/7** |
| qwen3-vl-4b, the laptop default | 3/6 | 5/6 | **7/7** |

With the whole-body format, **both models returned only the changed part**: the one fixed sentence,
or the one appended line. Accepting such a proposal would have deleted the rest of the note.

**At production settings** (the discussion history as the runner builds it, the runner's sampling),
the last column became 6/7 for both models. Deciding the kind first, and giving the history without
Myrme's own mention and commands (`convo::plain`), brought both to **7/7**.

The live end-to-end run (`agents/e2e-live.sh`) found the case the first six missed. With no sign
the note went on, a model "added a final line" by replacing the last sentence it could see. That
case is now `long-repetitive-final-line`, and the cut is marked.
