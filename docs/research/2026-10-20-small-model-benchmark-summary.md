# Small-model benchmarks for the phone and Myrme light — what was run, how, and the results (2026-10-20)

> **Status: evidence, not a ruling.** This collects every measurement behind the choice of a phone
> model and of Myrme light's tools, made on 2026-10-19/20. Every number below was checked against the
> run's own log, or against the answers recorded in `methods/*/eval/cases.json`. A row with no
> run says "not run". The detail and the reasoning are in:
> - `2026-10-19-speed.md` (phone timings, the 450M, Qwen3.5-0.8B's photo cap);
> - `2026-10-19-myrme-light.md` (the dedicated tools);
> - `2026-10-19-scouting-phone-models.md` §4 (the leaderboard shortlist and the owner's real mail).

## 1. How each batch was run

| Batch | Runtime | Hardware | Settings | Log (outside the repo) |
|---|---|---|---|---|
| **A. Shortlist screen** | llama.cpp **b11731** (release build) | laptop CPU (i7-11800H, `-dev none -t 8`) | thinking off: `enable_thinking: false`; K2 `reasoning_effort: low`; LFM2.5-2.6B none (it has no switch). Meetings constrained, layout `old`; titles layout `old` | `~/bench-scratch/shortlist/` |
| **B. MiniCPM-V vs Qwen3.5-0.8B** | b11731 | laptop CPU, 8 threads, on battery | thinking off; meetings and recipe in both modes (free, constrained) | `~/bench-scratch/minicpm/evals/` |
| **C. Real mail** | b11731 | laptop CPU, run alone | meetings constrained; Qwen3-VL-4B in its production layout `b`, the others `old`; each model run twice, same scores | `~/bench-scratch/realmail/` (private) |
| **D. Phone** | b10081 (Myrme's pinned Android build) | Dimensity 7300, CPU | see `2026-10-19-speed.md` | speed doc |
| **E. Light tools** | ONNX Runtime 1.23 (OCR), 1.20 (GLiNER, e5) via `ort` | laptop CPU, on battery; OCR 1 page at a time, GLiNER and e5 4 threads | as in the light doc | `~/bench-scratch/light-*.log` |
| **F. Recorded answers** | the run that recorded them | mostly laptop GPU (Vulkan), b10076 | replayed by the evals' gate tests | `methods/*/eval/cases.json` |

**Common to every eval:**
- **Temperatures:** meetings, research and recipe run at 0. Titles and edits use production's client
  setting, **0.2**, and run once, so edits (7 cases) are the noisiest score here.
- **Quantization:** Q4_K_M, except **Qwen3.5-0.8B and LFM2.5-2.6B at Q4_0**, read from each GGUF's
  `general.file_type`. Qwen3.5-0.8B's Q4_0 is the file the catalogue ships.
- **The sets:**
  - meetings: 17 invented threads;
  - titles: 12 invented threads;
  - edits: 7 invented notes;
  - research: 8 questions with invented sources;
  - recipe: 30 Wikibooks recipes turned into speech;
  - images: 30 handwritten pages (GNHK) and 30 typed pages (Wikibooks text rendered as photos);
  - **real mail: the owner's 5 conversations**, private, never committed: 4 accepted meetings and
    1 with no meeting.

## 2. Text methods, invented sets

Exact matches, then the total time for the set on the laptop CPU (batches A, B). Rows marked ‡ are
recorded answers (batch F), on another runtime and device: compare them with care.

| Model | GGUF | Meetings /17 | Titles /12 | Edits /7 | Research /8 (exact · grounded) | Recipe /30 (exact · found /257 · invented) |
|---|---|---|---|---|---|---|
| Qwen3-VL-4B ‡ (laptop model) | 2.50 GB | 15–16 (b11731 CPU: 16) | 12 (`decisions.md`) | 7; 6 in 3 other runs | 7 · 7 | **28 · 256** (constrained and free) |
| K2-Horizon-3.7B | 3.16 GB | 12 · 84 s | 11 · 30 s | 5 · 183 s | not run | not run |
| Qwen3.5-2B | 1.40 GB | 11 · 54 s | 12 · 20 s | 3 · 27 s | not run | not run |
| MiniCPM5-2B | 1.56 GB | 7 · 10 s | 12 · 16 s | 4 · 25 s | not run | not run |
| MiniCPM5-1B | 0.69 GB | 10 · 50 s (16/17 well-formed) | 6 · 8 s | 3 · 64 s | not run | not run |
| MiniCPM-V 4.7 1B | 0.67 + 1.11 GB | 7 · 47 s (free 7) | not run | 4 · 56 s | 7 · 7 · 21 s | free 21 · 227 · 1 (158 s); constrained 15 · 249 · 2 |
| **Qwen3.5-0.8B** | 0.51 + 0.21 GB | **14** · 19 s (free 13) | not run | 1 · 7 s | 5 · **8** · 15 s | free 20 · 248 · 1 (189 s); constrained 17 · 225 · 1 |
| LFM2.5-VL-450M (phone default) | 0.23 GB (Q4_K_M) | 5 · 15 s | 9 ‡ | 0 ‡ | 6 · 4 ‡ (Q4_K_M) | 9 · 89 · 16 ‡ (Q4_K_M) |
| LFM2.5-2.6B | 1.59 GB | out: always reasons; one question ran 17 s and was cut off at 300 tokens with no answer | — | — | — | — |

The 450M's ‡ figures are from the speed study's laptop-GPU runs (`2026-10-19-improving-the-fast-model.md`
§1). They are not phone runs.

## 3. Meetings on the owner's real mail (batch C)

Five conversations, 3–7 emails each, 1.5–4.5k characters.

| Model | Exact /5, day rule (0.6.0) | **Exact /5, time rule** | Well-formed | 5 threads |
|---|---|---|---|---|
| Qwen3-VL-4B | 4 | **4** | 5 | 84 s |
| K2-Horizon-3.7B | 2 | **4** | 5 | 98 s |
| **Qwen3.5-0.8B** | 3 | 3 | 5 | **15 s** |
| Qwen3.5-2B | 1 | 2 | 5 | 69 s |
| MiniCPM-V 4.7 1B | 1 | 1 | 5 | 32 s |
| MiniCPM5-1B | 1 | 1 | 4 | 49 s |
| MiniCPM5-2B | 1 | 1 | 5 | 21 s |
| LFM2.5-VL-450M | 0 | 0 | 4 | 14 s |
| LFM2.5-2.6B | 0 | not rerun | 0 | 322 s |

**The time rule** is the fix committed with this document. A meeting that is not a call-off is past
if it began before the newest email was sent; a call-off keeps the day rule.
- It moves only K2-Horizon and Qwen3.5-2B, the two models that also reported the meeting held
  earlier on the last email's day.
- No answer recorded in `cases.json` changes score under it.

**The caveats:**
- **Five cases cannot rank close models.** They show what breaks on real mail.
- **The set favours Qwen3-VL-4B.** The four meetings are the ones it found and the owner accepted.

## 4. Time per call: one long real thread (~1,500 tokens), laptop CPU, alone

Request: the hand-check instructions with the longest real thread as the message, not the eval's
exact prompt. The reading/writing split is representative; the seconds are approximate.

| Model | Reading | Writing | Total |
|---|---|---|---|
| Qwen3.5-0.8B | 2.6 s (591 tok/s) | 61 tok, 1.2 s (51 tok/s) | **3.8 s** |
| Qwen3.5-2B | 4.9 s (317 tok/s) | 116 tok, 4.8 s (24 tok/s) | 9.7 s |
| K2-Horizon-3.7B | 12.5 s (112 tok/s) | 88 tok, 7.8 s (11 tok/s) | 20.3 s |
| Qwen3-VL-4B | 12.9 s (117 tok/s) | 175 tok, 14.8 s (12 tok/s) | 27.7 s |

- **Reading speed follows parameter count.**
- **Writing speed follows file size**: each new token reads the whole model from memory.
- **Larger models also wrote more**: longer quotes and extra entries.
- K2 produced no reasoning text at `low`.

## 5. Images (word error rate over 30 pages; lower is better)

| Reader | Typed | Handwriting | Time | How |
|---|---|---|---|---|
| **PP-OCRv6 small** (OCR, ~30 MB) | **0.8%** | 61.3% | 0.6–1.0 s a page | batch E |
| PP-OCRv6 medium | 0.8% | 54.4% | 3.0 s (handwriting) – 5.7 s (typed) a page | batch E |
| Qwen3-VL-4B ‡ | not run | **29.0%** | — | recorded answers |
| Qwen3.5-0.8B, image tokens capped at 1024 | 2.3% | 43.4% | 30 typed pages 179 s, 30 handwritten 253 s | laptop GPU, b10076 |
| the same, on the phone (10 handwritten pages) | — | 44.8% | median 28 s a page; 2 of 10 looped | b10081, ctx 4096 |
| Qwen3.5-0.8B, uncapped | 7.0% | 49.3% | does not fit the phone's window | speed doc |
| MiniCPM-V 4.7 1B | not run | 45.4% | 16.1 s a page (mean), 733 prompt tokens | b11731, laptop CPU |
| LFM2.5-VL-450M | 49.0% | 96.3% | — | speed doc (laptop GPU / phone) |

Reading aids that did not help:
- A vocabulary correction of the OCR's handwriting: 61.3% → 55.3–59.4%.
- Word Beam Search on the same crops: 64.1% → 58.4%.
- A vocabulary correction of MiniCPM-V's transcript: 45.4% → 45.9%.

**The routing measured:** typed pages read at an OCR confidence of 0.99–1.00, handwriting at
0.63–0.95. Pages at 0.97 or above take the OCR's text.

## 6. Dedicated tools for Myrme light (batch E)

| Task | Tool | Result | Time |
|---|---|---|---|
| Titles | GLiNER multi v2.1 fp32; labels `person`, `job title`, `organization`; threshold 0.3 | **11/12** | 0.04 s a conversation |
| Titles | the same without `person`, threshold 0.5 | 10/12 | 0.04 s |
| Titles | GLiNER int8 export | 0/12 | 0.02 s |
| Recipes | GLiNER `ingredient`, the densest sentence, threshold 0.3 | 13/30 · 206/257 · 5 invented | 0.8 s a recipe |
| Recipes | every span, threshold 0.3 | 2/30 · 231/257 · 81 invented | 0.8 s |
| Research | multilingual-e5-small, extractive; abstains below 0.85 | 4/8 | 0.04 s a question |
| Meetings | GLiNER probe | finds days, times and places; not whether a meeting moved or was cancelled | — |

## 7. On the phone (batch D, from `2026-10-19-speed.md`), seconds per request

| Model | Meetings | Research | Edits | Recipe |
|---|---|---|---|---|
| Qwen3.5-0.8B Q4_0 | 3.7 | 6.9 | 2.8 | 30.9 |
| LFM2.5-VL-450M | 4.3 | 6.0 | 0.9 | 14.0 |

## 8. Not run

- **Four of the leaderboard's top ten** (`2026-10-19-scouting-phone-models.md` §3) were never
  tested. All have Q4_K_M files under Apache-2.0 or MIT:

  | Model | Q4_K_M file |
  |---|---|
  | G9v3-3B | 1.90 GB |
  | Granite 4.2 3B | 2.24 GB |
  | Nanbeige4.1-3B | 2.44 GB |
  | Phi-4 Mini | 2.49 GB |

- **For the tested shortlist:**
  - research and recipe were not run for MiniCPM5-1B and 2B, Qwen3.5-2B or K2-Horizon;
  - handwriting was not run for Qwen3.5-2B (it has a vision projector);
  - titles were not run for Qwen3.5-0.8B or MiniCPM-V on b11731.
- **Nothing from the shortlist, the OCR or GLiNER has run on the phone.**

## 9. What the evidence supports

**Phone and Myrme light:**
- Qwen3.5-0.8B for the language tasks.
- The OCR for typed pages (it routes the rest to the vision model).
- GLiNER for titles.
- Rust for dates and checks.

No tested model under 2 GB beats Qwen3.5-0.8B, and none of any size matches it per second.

**The phone's default:** LFM2.5-VL-450M is faster only. It scores 0/5 on real mail, 96% WER on
handwriting and 49% on typed pages.

**Laptop:** Qwen3-VL-4B stays. K2-Horizon ties it on real mail with the time rule and is close on
edits, but it reads no images.

**Weak everywhere at phone size:**
- **Handwriting:** 43–45%, against 29% at 4B.
- **Edits:** 0–3/7.
