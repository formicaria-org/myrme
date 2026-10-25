# K2-Horizon-7B on the laptop CPU, reasoning off and low (measurements, 2026-10-25)

> **Status: evidence, not a ruling. Nothing adopted; qwen3-vl-4b stays the laptop model.** With
> reasoning off, K2-Horizon-7B scores **70/80** exact on the five text evals against qwen3-vl-4b's
> **76/80**, writes at about **half the speed** (7.6 against 13.8 tok/s) and peaks at **~10 GB**
> resident against ~5.5 GB. At `reasoning_effort: low` it gives **no answer at all** on this runtime
> (§4). Its Q4_0 was not run (§5).

## 0. Why it was tried

K2-Horizon-7B (Institute of Foundation Models, September 2026, Apache-2.0, dense 7B) scores 21 on the
Artificial Analysis Intelligence Index v4.3.2, above Gemma 4 26B-A4B (17) and Qwen3.6 35B-A3B (18), and
its Q4_K_M (5.59 GB) fits the laptop. But that 21 is "the reasoning version", and the model card says
every reported result uses `reasoning_effort: high` with at least 32k output tokens. No score with
reasoning off or low is published anywhere, so the owner asked for one measured here, at the settings
Myrme would run it with.

## 1. How it was run

| | |
|---|---|
| Runtime | llama.cpp 0.7.0-dev (build 11731), CPU: `-dev none -t 8 -ngl 0`, ctx 8192, as the 2026-10-19 shortlist screen. The app's runtime (build 10076) cannot load K2: its architecture needs build 11454 or later |
| Code | Myrme **v0.11.0** (`5481399`), in a clean clone with its own pixi env and target, so the release made that morning was not disturbed |
| Evals | `pixi run eval` with `EVAL_NO_RECORD=1`: meetings, titles, edits, research, recipe; meetings and recipe `FM_EVAL_MODE=constrained`. Layout as production picks it: qwen3-vl-4b has `shared_mail_prompt` (layout `b`); K2 has no catalogue entry, so `old` |
| K2 | `IFM/K2-Horizon-7B-GGUF` Q4_K_M, revision `7a75dbd`, SHA-256 `eb89c15a…2e4e` checked. Off: `--chat-template-kwargs {"enable_thinking":false}`; low: `{"reasoning_effort":"low"}` |
| qwen3-vl-4b | the catalogue's `Qwen3VL-4B-Instruct-Q4_K_M.gguf`, `{"enable_thinking":false}`, text only (no projector) |
| Conditions | on AC; one model server at a time, and none other ran during any eval (the run checked every 2 s); CPU 85–96 °C after the evals, the same for both models |

Measured per eval: the eval's own scores; from the server log, prompt and generated tokens, read and
write tok/s, time to first token; the server's peak resident memory and the lowest `MemAvailable`,
sampled each second. The logs and files lived outside the repo and were deleted when the study ended.

## 2. Accuracy

| eval | **K2-Horizon-7B, off** | **qwen3-vl-4b** |
|---|---|---|
| meetings (constrained) /23 | 21 (well-formed 23) | **22** (well-formed 23) |
| titles /12 | 10 | **12** |
| edits /7 | **7** | 6 |
| research /8 (exact · grounded) | 8 · **8** | 8 · 7 |
| recipe /30 (exact · found /257 · invented) | 24 · 254 · 5 | **28 · 256 · 1** |
| **exact, all five** | **70/80** | **76/80** |

- The sets have grown since `2026-10-20-small-model-benchmark-summary.md` (meetings has 23 threads,
  not 17): do not compare these rows with that document's.
- One run per model. Titles and edits run at production's 0.2, so they are the noisiest rows; a
  one-case difference there is not a finding. Recipe's 24 against 28, with 5 invented ingredients
  against 1, is the clearest gap.

## 3. Speed and memory

| eval | K2: first token · write · per request | qwen3-vl-4b: first token · write · per request |
|---|---|---|
| meetings | 1.8 s · 7.7 tok/s · 9.1 s | 1.3 s · 13.7 tok/s · 5.8 s |
| titles | 1.8 s · 7.9 · 4.5 s | 2.4 s · 13.3 · 4.2 s |
| edits | 1.7 s · 7.7 · 4.9 s | 0.9 s · 14.7 · 3.1 s |
| research | 1.6 s · 7.6 · 11.2 s | 0.9 s · 13.8 · 7.9 s |
| recipe | 3.5 s · 7.2 · 38.0 s | 2.1 s · 13.4 · 22.5 s |
| **all five, wall** | **1592 s** | **991 s** |

- **Reading:** K2 61–64 tok/s, qwen3-vl-4b 100–110 tok/s. Titles is the one row where K2 reaches its
  first token sooner: layout `b` sends qwen3-vl-4b about twice the prompt (245 tokens against 116).
- **Memory:** K2's server peaked at 9.9–10.1 GB in every eval, against qwen3-vl-4b's 5.5–5.7 GB (7.9 GB
  in recipe). The lowest `MemAvailable` was ~5.0 GB for both, in recipe. Why K2 holds ~4 GB more than
  its 5.6 GB file at ctx 8192 was not looked into.

## 4. Low effort gives no answer on this runtime

K2's chat template (read from the GGUF) opens the thinking block by effort: `<ifm|think>` for high,
`<ifm|think_fast>` for medium, `<ifm|think_faster>` for low. With `enable_thinking: false` it writes an
empty `<ifm|think>\n</ifm|think>` itself, which is why the off runs answered directly (three hand checks:
answers in 1–3 s, no reasoning).

At low, the 7B opened `<ifm|think_faster>` and closed with `</ifm|think>`. llama.cpp never saw the block
close, so the whole reply, answer included, landed in `reasoning_content` and `content` came back
**empty in all three hand checks** (18–64 tokens, 3–9 s). The low evals were therefore not run: they
would have scored empty answers. The answers themselves were right, so this is a tag mismatch, not the
model failing; a runtime or template that accepts the mismatched close tag might change it. Not tried.

## 5. Not measured

- **Q4_0.** `bartowski/K2-Horizon-7B-GGUF` Q4_0 (5.43 GB, revision `0367e19`, SHA-256
  `01db6be9…6fa6`) was queued, but the download ran at ~1 MiB/s and was stopped at 754 of 5,178 MiB
  (and deleted with the other files). It is barely smaller than the Q4_K_M, so it could not
  close a 2× speed gap on file size alone.
- **The newer weights.** IFM replaced the 7B's main weights with an "anti doom loop" checkpoint on
  2026-10-01; its own GGUF dates from 2026-09-22, so these numbers are the earlier weights. bartowski's
  build (2026-10-07) is most likely the newer one.
- **The GPU.** No Vulkan build at 11454 or later is on the laptop, and 5.6 GB does not fit the laptop's
  4 GB GPU whole, so only a partial offload would be possible.
- **Reasoning at high,** the setting the published scores use: minutes per answer at 7–8 tok/s.
