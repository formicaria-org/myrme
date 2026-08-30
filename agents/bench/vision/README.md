# Vision fixtures — typeset ground truth for image→text

Five images with the transcript a correct reading should produce (`ground-truth.json`), so the
same set can **score a model** and not only exercise the plumbing. Regenerate with
`python3 make-fixtures.py` (Pillow + DejaVu); they are deterministic.

Deliberately **typeset, not handwritten**. A fixture whose ground truth is itself ambiguous tests
the reader rather than the pipeline. Handwriting is the harder, real case and wants its own set,
where disagreement with the truth is the finding instead of the noise.

## Measured on `qwen3-vl-4b` + `mmproj-…-F16`, 2026-08-30 (RTX 3050, 3562/4096 MiB)

| fixture | result |
|---|---|
| `01-notes.png` | exact, 39 tokens |
| `02-math.png` | text exact; **returned plain, not LaTeX** |
| `03-pseudocode.png` | exact, correctly fenced, indentation kept |
| `04-table.png` | correct Markdown table, but read **`LiFeP04`** — a zero for the letter O |
| `05-plot.png` | title, axes, ticks and legend transcribed; **invented data-point values** |

Two of those are *errors a person would fix*, which is what makes this set useful: the corrections
are the supervision signal, and the last two rows are the ones worth collecting.

## The prompt lesson, which cost an afternoon to find

The first instruction was a tidy **bulleted list** of formatting rules. On the plot — the hardest
input — the model reproduced *the list itself* as if it were the image's content and looped until it
hit the context window: 1414 tokens, `truncated = 1`, and the turn correctly failed on the
truncation guard. Rewritten as prose it settles at 36–184 tokens with `finish_reason = stop`.

This repo already knew the shape: the chat path uses no heading scaffolding because a small model
parroted that back too. **A list in the prompt is a list the model can mistake for the answer.**
