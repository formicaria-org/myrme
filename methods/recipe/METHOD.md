---
name: recipe
description: Make a recipe of a note — its recording transcribed, or its text as written — shaped into ingredients, method and times, keeping only what was said, proposed as a new recipe note.
---

# M7 — recipe from a recording

**Trigger:** the `/recipe` command. **Capabilities:** `llm`, plus `speech` when the note holds a
recording.

## Pipeline (`recipe.rs`)

1. Transcribe the recording, or take the note's text.
2. One model call shapes the text into parts.
3. **Rust keeps only what was said:**
   - an ingredient none of whose words was said is dropped;
   - so is a step most of whose words were not;
   - the reply says how many were dropped.
4. The whole transcript goes into the note under *What was said*.

## Eval

`eval/cases.json`: 30 Wikibooks Cookbook recipes (CC BY-SA 4.0, see `eval/LICENSE.md`). Each one's
ingredients and procedure is turned into one run of speech by `agents/eval-data.py recipes`; that is
the input this method gets after speech to text, which is M4's job. Each case is scored by
`fm_agent::eval::recipe_outcome` on three counts:
- every listed ingredient found;
- none invented;
- at least half the steps.

| Model, 2026-10-18 (RTX 3050, 30 recipes) | Free | Held to `recipe::answer_schema()` |
|---|---|---|
| qwen3-vl-4b, the laptop default | **28/30** · ingredients 256/257 · 1 invented · 260 s | 28/30 · same · 276 s |
| qwen3-4b-2507 | 26/30 · 254/257 · 5 invented · 324 s | 26/30 · same · 370 s |

**Decided on these numbers:** the recipe method stays free. A schema here bought nothing and cost
time, because the free answers were already well-formed JSON. `recipe::answer_schema()` is kept for
the next comparison. `tests/live_recipe.rs` remains the manual run on a real recording.
