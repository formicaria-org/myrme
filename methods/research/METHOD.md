---
name: research
description: Answer a question from numbered sources as a list of claims, each citing its source and quoting it word for word; Rust drops every claim whose quote is not really in its source, and the note is proposed with its Sources list.
---

# M2 — grounded research

**Trigger:** the `/research` command. **Capabilities:** `llm` and `web`. The web runs only because
the person asked for it.

## Pipeline (`StudyAssistant::research`)

1. Refine the request into one search query (`prompts::QUERY_REFINE_INSTRUCTION`).
2. Search, then number the sources. Inputs the person named come after the web hits.
3. One write, **held to `grounding::answer_schema()`** under `prompts::GROUNDED_JSON_INSTRUCTION`:
   - first `answerable` (do the sources state what was asked?), then the claims, each with its
     source number and a quote copied word for word;
   - `grounding::claims_from_json` turns them into the list `verify` reads;
   - if the note runs too long, it retries once with fewer, shorter sources.
4. **Rust verifies** (`grounding::verify`): a claim whose quote is not in its cited source is
   dropped. The Sources section is assembled by Rust from the verified set.

## Eval

`eval/cases.json`: 8 cases about invented things, so nothing can be answered from memory:
- one source, two sources;
- one relevant source next to one that is not;
- Italian;
- numbers and units;
- a partial answer;
- two cases where the sources do not answer, one unrelated and one close to the topic.

Each case is scored on two counts:
- **exact:** the expected facts appear inside verified quotes, from relevant sources only; or
  nothing at all where the sources do not answer;
- **grounded:** no claim was dropped.

The cases run the write step and the verification. The web search is not part of the eval.

| Model, 2026-10-18 (RTX 3050) | Before the parser fix | After it | Held to the schema |
|---|---|---|---|
| qwen3-4b-2507 | 5/8 exact · 5/8 grounded | 7/8 · 7/8 | **8/8 · 7/8** |
| qwen3-vl-4b, the laptop default | 5/8 · 5/8 | 7/8 · 7/8 | **8/8 · 7/8** |

**What the first run found was the parser, not the models.** Correct answers were dropped
because:
- the citation sat on the quote line, or was missing;
- the citation was glued to the end of the quote;
- the quote ended in a full stop where the source goes on.

`grounding::verify` now handles all three:
- it reads a citation found on the quote line;
- it credits a claim with no citation to the **single** source holding its quote;
- it ignores closing punctuation.

The guarantee is unchanged: a claim ships only with a quote found word for word in its source.

`abstain-near-topic` (a cited "the sources do not say") was fixed by the schema: it asks whether
there is an answer before any claim.

## Planned (Phase C)

local-deep-researcher's fixed loop (query, search, extract, rank, write, check), with main-content
extraction and a small embedder for ranking.
