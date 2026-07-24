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

---

## 2026-07-24 — alternatives sweep: 3 laptop + 4 phone models vs the current defaults

Ran the hardened harness (verbatim-quote + tool-call + abstain checks) over the Part-4 shortlist of
`model-selection-research-2026-07-24-grounded.md`, one model at a time, memory-capped, arch-verified on
load. Weights on ext4 (NOT tmpfs — a tmpfs download ate RAM mid-sweep and was moved). Phone runs via the
staged arm64 `llama-server` b10081 on `/data/local/tmp/fmbench`, adb-forwarded, cleaned off after each.

### Verdict summary (verified quality, not counts)

**Laptop** (GPU `-ngl 99` unless noted; baseline Qwen3-4B-2507 = 39.8 t/s, 1/3 verbatim on its hardened CPU run):
| model | t/s | grounding V/Q | search call/decline | junk sources | fit |
|---|---|---|---|---|---|
| Nemotron-3-Nano-4B | 35.4 GPU | 0/2 | ✓/✓ | fabricates | GPU ✓ |
| **Qwen3-VL-4B** ⭐ | 40.1 GPU | **3/3** | ✗ missed / ✓ | **abstains** | GPU ✓ |
| Gemma-4-E4B | 10.9 CPU | 1/5 | ✓/✓ | abstains | CPU only (4.7 GB > 4 GB VRAM) |

**Phone** (CPU `-t 4 -ngl 0`; baseline LFM2.5-1.2B = 12.4 t/s, 0 verbatim quotes):
| model | size | t/s | grounding | search call/decline | junk sources |
|---|---|---|---|---|---|
| Qwen3.5-2B (thinking-off) | 1.2 GB | 7.5 | 0 quotes (7 claims) | ✓/✓ | abstains |
| MiniCPM5-1B | 0.66 GB | 13.5 | 0 claims | ✓/✗ over-searched | abstains |
| Qwen3-VL-2B | 1.1 GB | 7.8 | 0/3 (3 hallucinated) | ✓/✓ | abstains |
| LFM2.5-VL-1.6B | 0.7 GB | 11.8 | 0 quotes | ✓/✗ over-searched | **FABRICATES** |

**Findings:** (1) **Laptop — Qwen3-VL-4B is the upgrade candidate:** baseline speed, best-measured
grounding (3/3 verbatim), only fast model that abstains; the multimodal 2-for-1 preserves text as
predicted. Nemotron slower + weaker; Gemma-4 GPU-excluded (4.7 GB). (2) **Phone — no alternative beats
the incumbent:** NO ≤2B model produced a single verbatim quote (grounding is the sub-4B wall, exactly as
the research warned). Qwen3.5-2B has the cleanest behaviour (correct routing + abstains) but is ~40%
slower; MiniCPM5-1B is fastest but over-searches and skipped research; LFM2.5-VL-1.6B fabricates — the
bolt-on VL regression, measured. (3) All new arches loaded in b10076/b10081 (nemotron_h, qwen3vl,
gemma4, minicpm5, lfm2-vl). *(Grounding V/Q = verbatim/total quotes; caveat: absolute counts drift a
little with the 256-token budget — the verbatim RATIO is the signal.)*

### Raw harness blocks (newest first)

### laptop · nemotron3-nano-4b · 2026-07-24  (thinking OFF (direct-answer))
- model RSS 1773 MB / VmHWM 2933 MB · VRAM 2986 MB · MemAvailable during run: 5645 MB (before 6532 MB)

| case | prompt_n | gen_n | decode tok/s | prompt tok/s | wall s | quality |
|---|---|---|---|---|---|---|
| chat | 163 | 61 | **29.9** | 19.4 | 10.46 | ok |
| research | 385 | 256 | **35.4** | 51.1 | 15.01 | 3 claims / **0/2 verbatim** / 2 HALLUCINATED-quote |
| write | 333 | 115 | **35.1** | 357.8 | 4.5 | ok |
| toolcall-search | 372 | 36 | **35.8** | 57.5 | 7.67 | CALL ok |
| toolcall-nosearch | 390 | 9 | **40.1** | 361.6 | 1.35 | ok (correctly did not call) |
| abstain | 267 | 52 | **35.8** | 343.0 | 2.38 | FABRICATED (2 cites, 1 hallucinated-quotes on junk sources) |

_mean decode: **35.4 tok/s** over 6 cases._

### laptop · qwen3-vl-4b · 2026-07-24  (thinking OFF (direct-answer))
- model RSS 791 MB / VmHWM 2594 MB · VRAM 2782 MB · MemAvailable during run: 4505 MB (before 4770 MB)

| case | prompt_n | gen_n | decode tok/s | prompt tok/s | wall s | quality |
|---|---|---|---|---|---|---|
| chat | 160 | 99 | **40.5** | 514.8 | 2.77 | ok |
| research | 374 | 196 | **39.7** | 702.3 | 5.5 | 3 claims / **3/3 verbatim** |
| write | 324 | 99 | **39.7** | 641.9 | 3.05 | ok |
| toolcall-search | 250 | 142 | **39.7** | 684.8 | 3.99 | NO CALL (should have searched) |
| toolcall-nosearch | 266 | 25 | **41.0** | 646.6 | 1.03 | ok (correctly did not call) |
| abstain | 257 | 58 | **40.0** | 638.9 | 1.88 | ok (abstained — no fabricated citations) |

_mean decode: **40.1 tok/s** over 6 cases._

### laptop · gemma-4-e4b · 2026-07-24  (thinking OFF (direct-answer))
- model RSS 5363 MB / VmHWM 5363 MB · MemAvailable during run: 9953 MB (before 10110 MB)

| case | prompt_n | gen_n | decode tok/s | prompt tok/s | wall s | quality |
|---|---|---|---|---|---|---|
| chat | 159 | 75 | **10.8** | 42.0 | 10.75 | ok |
| research | 373 | 256 | **9.8** | 47.7 | 33.86 | 5 claims / **1/5 verbatim** / 4 HALLUCINATED-quote |
| write | 328 | 256 | **10.1** | 98.5 | 28.64 | ok |
| toolcall-search | 168 | 27 | **11.3** | 21.2 | 10.33 | CALL ok |
| toolcall-nosearch | 183 | 8 | **12.5** | 26.3 | 7.61 | ok (correctly did not call) |
| abstain | 254 | 26 | **10.9** | 53.5 | 7.14 | ok (abstained — no fabricated citations) |

_mean decode: **10.9 tok/s** over 6 cases._

### phone · qwen3.5-2b · 2026-07-24  (thinking OFF (direct-answer))
- model RSS 2861 MB / VmHWM 2861 MB · MemAvailable during run: 2367 MB (before 2475 MB)

| case | prompt_n | gen_n | decode tok/s | prompt tok/s | wall s | quality |
|---|---|---|---|---|---|---|
| chat | 167 | 68 | **9.1** | 62.6 | 10.19 | ok |
| research | 381 | 204 | **7.8** | 50.9 | 33.63 | 7 claims / **0/0 verbatim** |
| write | 332 | 256 | **8.6** | 50.8 | 36.3 | ok |
| toolcall-search | 377 | 42 | **8.6** | 48.7 | 12.79 | CALL ok |
| toolcall-nosearch | 393 | 49 | **4.4** | 35.5 | 22.31 | ok (correctly did not call) |
| abstain | 262 | 92 | **6.3** | 30.1 | 23.35 | ok (abstained — no fabricated citations) |

_mean decode: **7.5 tok/s** over 6 cases._

### phone · minicpm5-1b · 2026-07-24  (thinking OFF (direct-answer))
- model RSS 1213 MB / VmHWM 1266 MB · MemAvailable during run: 3242 MB (before 3475 MB)

| case | prompt_n | gen_n | decode tok/s | prompt tok/s | wall s | quality |
|---|---|---|---|---|---|---|
| chat | 164 | 63 | **13.3** | 97.4 | 6.47 | ok |
| research | 373 | 102 | **13.7** | 98.3 | 11.26 | 0 claims / **0/0 verbatim** |
| write | 331 | 256 | **12.7** | 94.3 | 23.66 | ok |
| toolcall-search | 316 | 52 | **14.3** | 99.2 | 6.88 | CALL ok |
| toolcall-nosearch | 329 | 66 | **13.6** | 96.9 | 8.27 | WRONG: called ['web_search'] |
| abstain | 262 | 108 | **13.5** | 96.5 | 10.74 | ok (abstained — no fabricated citations) |

_mean decode: **13.5 tok/s** over 6 cases._

### phone · qwen3-vl-2b · 2026-07-24  (thinking OFF (direct-answer))
- model RSS 2611 MB / VmHWM 2611 MB · MemAvailable during run: 2474 MB (before 2786 MB)

| case | prompt_n | gen_n | decode tok/s | prompt tok/s | wall s | quality |
|---|---|---|---|---|---|---|
| chat | 160 | 133 | **8.0** | 48.9 | 20.01 | ok |
| research | 374 | 256 | **7.4** | 49.4 | 42.33 | 3 claims / **0/3 verbatim** / 3 HALLUCINATED-quote |
| write | 324 | 234 | **7.4** | 47.6 | 38.47 | ok |
| toolcall-search | 250 | 33 | **8.2** | 48.5 | 9.31 | CALL ok |
| toolcall-nosearch | 266 | 13 | **7.8** | 49.2 | 7.12 | ok (correctly did not call) |
| abstain | 257 | 69 | **7.7** | 48.1 | 14.38 | ok (abstained — no fabricated citations) |

_mean decode: **7.8 tok/s** over 6 cases._

### phone · lfm2.5-vl-1.6b · 2026-07-24  (thinking OFF (direct-answer))
- model RSS 1518 MB / VmHWM 1520 MB · MemAvailable during run: 3188 MB (before 3274 MB)

| case | prompt_n | gen_n | decode tok/s | prompt tok/s | wall s | quality |
|---|---|---|---|---|---|---|
| chat | 168 | 102 | **12.0** | 70.7 | 10.91 | ok |
| research | 391 | 148 | **11.7** | 72.1 | 18.16 | 3 claims / **0/0 verbatim** |
| write | 348 | 256 | **11.9** | 72.0 | 26.35 | ok |
| toolcall-search | 187 | 24 | **11.9** | 70.3 | 4.72 | CALL ok |
| toolcall-nosearch | 199 | 20 | **11.3** | 70.7 | 4.62 | WRONG: called ['web_search'] |
| abstain | 271 | 104 | **11.8** | 70.9 | 12.64 | FABRICATED (2 cites, 0 hallucinated-quotes on junk sources) |

_mean decode: **11.8 tok/s** over 6 cases._
