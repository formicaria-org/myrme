# formicaria model-benchmark — measured results

Every block below is produced by `agents/bench/run.py` driving a real `llama-server` through our own
case battery (`cases/*.json`, our real system prompts). **All numbers are measured on the named
device** — decode tok/s from llama.cpp's `timings.predicted_per_second`, RSS/VmHWM from `/proc`, VRAM
from `nvidia-smi`, MemAvailable from `/proc/meminfo`. Nothing here is estimated. Newest first; kept
for future reference (re-run to refresh, don't re-derive). See `README.md` for how to reproduce, and
`../../docs/context/device-resources.md` for the device specs these run on.

**How to read decode tok/s:** reading pace is ~5–10 tok/s, so anything above that streams smoothly.
These are *our-case* rates (real system prompt + real context), so they run a bit below the synthetic
`llama-bench` figures in `model-benchmarks-2026-07-22.md` — that gap is exactly why this harness
exists.

## Summary — computational cost on our real devices (our-case, measured 2026-07-24)

### Methodology (so every number below is reproducible and comparable)
- **Workload = our real case, not synthetic.** `run.py` drives a real `llama-server` over
  `POST /v1/chat/completions` (the path the app uses) with our **actual system prompts** and a fixed
  3-case battery: `chat` (discussion reply), `research` (grounded quote-first synthesis over a canned
  numbered source pack), `write` (house-format note). `temperature 0` (repeatable), **thinking off**
  (`enable_thinking:false` — a study assistant wants a direct answer), `cache_prompt:false` (cold
  eval, like a fresh turn), `n_predict` 200–256.
- **Every figure is measured on the named device:** decode tok/s = llama.cpp `timings.predicted_per_second`;
  peak RSS = `/proc/<pid>/status VmHWM`; VRAM = `nvidia-smi`; MemAvailable = `/proc/meminfo`. None estimated.
- **Phone** runs the standalone arm64 `llama-server` (llama.cpp b10081) on `/data/local/tmp` with the
  app's real settings **`-c 2048 -t 4 -ngl 0`**; **laptop** GPU rows use `-ngl 99 -t 8`, CPU rows `-ngl 0 -t 8`.
- **Grounding** = the research case's grounding signal. **Hardened 2026-07-24** from *counting*
  quote-shaped lines to **VERIFYING** them — each `> quote` is now checked as a verbatim substring of
  its cited source (`V/Q verbatim`, + MIS-CITED / HALLUCINATED-quote), with new `toolcall`
  (search-decision) and `abstain` (must-not-fabricate) cases. **⚠ The result rows below predate the
  hardening — their `N/M quotes` are *unverified counts*; re-run to get verified numbers.**
- **Safety:** one model at a time, hard-fit-checked; **the 8B was NOT run** — 4.7 GB Q4 exceeds the
  laptop's 4 GB VRAM and would OOM (a concurrent-run overcommit crashed the machine once on 2026-07-24;
  never again).

### 📱 Phone — Redmi (Dimensity 7300, 8 core, 7.4 GB RAM), CPU `-t 4`
| Model | size (Q4) | decode tok/s | peak RSS | grounding | verdict |
|---|---|---|---|---|---|
| **lfm2.5-1.2b** *(current default)* | 695 MB | **12.4** | 1503 MB | 3 claims / **0 quotes** | fast, but can't follow quote-first |
| qwen3-1.7b (think-off) | 1.1 GB | **8.9** | 2432 MB | 1 claim / **1 quote** | smarter grounding, ~28% slower — **open tradeoff** |

*Both load safely (MemAvailable held ≥2.5 GB). RAM is not the wall; phone CPU throughput is.*

### 💻 Laptop — i7-11800H, **RTX 3050 4 GB (Vulkan)**, 14.9 GB RAM
| Model | config | decode tok/s | RSS | VRAM | grounding | verdict |
|---|---|---|---|---|---|---|
| **qwen3-4b-2507** *(current default)* | **GPU** `-ngl 99` | **39.8** | 511 MB | 2769 MB | 2 / 2 | **the pick** — fast + well-grounded |
| qwen3-4b-2507 | CPU `-ngl 0` | 12.6 | 3029 MB | — | **3 / 3** | shows the **GPU = 3.2×** win |
| qwen3-1.7b (think-off) | CPU `-ngl 0` | 29.7 | 1681 MB | — | 1 / 1 | smaller reference |
| ~~qwen3-8b~~ | GPU | — | — | — | — | **not run** — 4.7 GB > 4 GB VRAM, would OOM |

*The 4 GB card caps the laptop near 4B; the 4B-on-GPU is fast **and** the best-grounded model measured.*

Synthetic `llama-bench` figures (higher, not our-case) are in
`../../docs/context/model-benchmarks-2026-07-22.md`; device specs in `device-resources.md`.

<!-- new results are appended below this line by: python3 agents/bench/run.py … >> results.md -->

### laptop · qwen3-4b-2507 · 2026-07-24
- model RSS 511 MB / VmHWM 2578 MB · VRAM 2769 MB · MemAvailable during run: 9757 MB (before 9930 MB)

| case | prompt_n | gen_n | decode tok/s | prompt tok/s | wall s | quality |
|---|---|---|---|---|---|---|
| chat | 160 | 95 | **40.4** | 263.1 | 3.1 | ok |
| research | 374 | 200 | **39.7** | 675.1 | 5.73 | 2 claims / 2 quotes / 3 cites |
| write | 324 | 200 | **39.3** | 617.0 | 5.73 | ok |

_mean decode: **39.8 tok/s** over 3 cases (baseline — the current laptop default, on GPU)._

### phone · lfm2.5-1.2b · 2026-07-24
- model RSS 1503 MB / VmHWM 1503 MB · MemAvailable during run: 2950 MB (before 3018 MB) · standalone llama-server on /data/local/tmp, `-c 2048 -t 4 -ngl 0` (app settings)

| case | prompt_n | gen_n | decode tok/s | prompt tok/s | wall s | quality |
|---|---|---|---|---|---|---|
| chat | 168 | 44 | **12.7** | 100.6 | 5.17 | ok |
| research | 391 | 51 | **12.2** | 105.0 | 7.95 | 3 claims / 0 quotes / 1 cites |
| write | 348 | 189 | **12.2** | 105.6 | 18.79 | ok |

_mean decode: **12.4 tok/s** (phone baseline — current default). Note: 0 verbatim quotes — the 1.2B can't follow the quote-first grounding contract._

### phone · qwen3-1.7b · 2026-07-24  (candidate — a real speed↔quality tradeoff)
- **phone decode: 8.9 tok/s** (measured on-device, `-c 2048 -t 4 -ngl 0`; vs the 1.2B's 12.4 — ~28% slower, still above reading pace) · model RSS 2380 MB / VmHWM 2432 MB · MemAvailable held 2.5 GB (loaded safely).
- **Quality (thinking OFF, device-independent, re-measured with the fixed harness):** chat ok · write ok · research **1 claim / 1 quote / 2 cites** — it *followed the quote-first grounding contract*, where the 1.2B managed **0 quotes**. Genuinely smarter on our hardest instruction.

_**Correction — the first run's "EMPTY" was a harness bug, not the model.** Qwen3 is a thinking model:
with a 200-token budget it spent everything inside `<think>`, so `content` came back empty (the tokens
were in `reasoning_content`, which the harness didn't read). Fixed in `run.py`: send
`enable_thinking:false` (a study assistant wants a direct answer) and read `reasoning_content` /
`finish_reason`. **Every model has its own special tokens + chat template** — `llama-server` applies the
GGUF's own template (so BOS/EOS/`<|im_start|>` etc. are correct), but knobs like `enable_thinking` are
**model-specific** (harmlessly ignored by templates that don't use them); do not assume one config
generalizes. **App prerequisite to actually ship a thinking model:** the app's OpenAiStep needs the same
`enable_thinking:false`, or it hits the identical empty. Verdict: 1.7B is smarter-but-slower, not broken
— the pick is the owner's speed-vs-quality call._

### laptop · qwen3-4b-2507 · 2026-07-24  (CPU, `-ngl 0 -t 8`, thinking off)
- model RSS 3029 MB / VmHWM 3029 MB · MemAvailable during run: 5766 MB

| case | prompt_n | gen_n | decode tok/s | prompt tok/s | wall s | quality |
|---|---|---|---|---|---|---|
| chat | 160 | 104 | **13.0** | 69.9 | 10.31 | ok |
| research | 374 | 256 | **12.8** | 222.3 | 21.68 | 3 claims / 3 quotes / 5 cites |
| write | 324 | 256 | **12.0** | 182.5 | 23.1 | ok |

_mean decode: **12.6 tok/s** — the same qwen3-4b as the GPU row but CPU-only: the **GPU gives 3.2× (39.8 vs 12.6)**. Best grounding measured (3/3) — the 4B follows the quote-first contract cleanly._

### laptop · qwen3-4b-2507 · 2026-07-24  (FIRST HARDENED RUN — CPU, thinking off)
- The hardened harness's first run, and it immediately overturned a pre-hardening result:

| case | decode tok/s | quality (VERIFIED) |
|---|---|---|
| research | 12.1 | **1/3 verbatim** / 2 HALLUCINATED-quote |
| toolcall-search | 13.3 | **CALL ok** (emitted a well-formed web_search) |
| toolcall-nosearch | 14.5 | **ok** (correctly did NOT search — note answered it) |
| abstain | 13.1 | **FABRICATED** (invented a cited claim on junk sources) |

_**The finding that justifies the hardening:** the *counting* harness scored this same model's research
case "3 claims / 3 quotes / 5 cites" — looked perfect. Verified, only **1 of 3 quotes is actually a
verbatim copy** of its source; the other two are hallucinated quote-shaped lines. And on novel junk
sources the model **fabricated a cited claim instead of abstaining.** So every pre-hardening "quotes"
number in this file OVERSTATED grounding. Tool-call routing, by contrast, works well (calls when needed,
declines when not). Re-run the shortlist under the hardened harness for the real grounding picture._
