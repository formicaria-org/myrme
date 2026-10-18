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

None yet. Planned: transcripts with known ingredients and steps, scored by what was kept and what
was invented. Phase B also plans the recipe answer as a JSON Schema, `{title, ingredients[],
steps[]}`. `tests/live_recipe.rs` is the existing manual run.
