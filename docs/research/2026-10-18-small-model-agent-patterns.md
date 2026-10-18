# Agent architecture patterns for small (0.3B–8B) on-device models (research, 2026-10-18)

> **Research, not a ruling.** About 20 searches and fetches by a research agent on 2026-10-18,
> condensed here. Items marked **[unverified]** were not confirmed against a primary source. The
> decisions this informs are in `../context/plan.md` and `../context/decisions.md`.

## TL;DR
1. **Fixed workflows hold up at small scale; autonomous loops do not.** Official BFCL: small
   models get ~80–88% of *single* calls right, but multi-turn collapses below ~3B
   (Qwen3-1.7B 11%, Qwen3-0.6B 3.6%, Llama-3.2-3B 4%, Gemma-3-4B 0.4%). Myrme's existing design
   (code picks the tool, the model fills one slot, a human accepts) is on the reliable side.
2. **Fewer tools and shorter prompts are the most reliable gain** (TinyAgent, "Less is More": ~4
   tools shown beats 16+).
3. **Constrained decoding is nearly free and slightly helps**; llama.cpp's native GBNF has the most
   schema-compile gaps, **llguidance** (a llama.cpp build option, written in Rust) covers far more.
4. **Specialist small models beat general ones on narrow jobs** (xLAM-2, TinyAgent 1.1B vs
   GPT-4-Turbo, FunctionGemma 58→85% after fine-tune, LFM2-350M-Extract vs Gemma 3 4B).

## 1. Control patterns at small scale
Anthropic's *Building effective agents* (Dec 2024,
https://www.anthropic.com/engineering/building-effective-agents) separates **workflows** (code
paths you define: chaining, routing, parallelisation, orchestrator-workers, evaluator-optimizer)
from **agents** (the model directs itself), and advises the simplest pattern that works.

| Pattern | Evidence at small scale |
|---|---|
| **Single-shot function calling** | BFCL v4 Non-Live AST: Qwen3-4B-2507 87.9, Qwen3-8B 87.6, Qwen3-1.7B 82.9, Llama-3.2-3B 82.7, Qwen3-0.6B 71.8 (https://gorilla.cs.berkeley.edu/data_overall.csv, board dated 2025-12-16) |
| **ReAct / multi-turn loops** | BFCL multi-turn: Qwen3-8B 41.8, Qwen3-4B 22.1, Qwen3-1.7B 11.0, Qwen3-0.6B 3.6, Llama-3.2-3B 4.0, Gemma-3-4B 0.4, 1B models 0. TinyLLM (arXiv 2511.22138) attributes it to lost context across turns |
| **Plan-then-execute / compiled plans** | TinyAgent (BAIR, May 2024, https://bair.berkeley.edu/blog/2024/05/29/tiny-agent/): LLMCompiler-style plan once, code executes; TinyLlama-1.1B 12.7%→80.1% after LoRA on ~80K GPT-4 plans, GPT-4-Turbo 79.1%. Profile-Then-Reason (arXiv 2604.04131): beat ReAct in 16/24 configs with 2–3 calls [model sizes unverified] |
| **Router + specialists** | TinyAgent's *Tool RAG* is a DeBERTa-v3-small classifier: 3.97 tools/query at 0.998 recall, prompt 2,762→1,397 tokens, accuracy *up*; plain embedding retrieval 0.949 recall. α-UMi (arXiv 2401.07324) planner/caller/summariser split at 7B/13B [figures unverified] |
| **Code-as-action** (smolagents CodeAgent, CodeAct) | Gains reported with strong models only; **no evidence at 0.3–4B**; needs a sandboxed interpreter |

**Where small models fail:** multi-step/multi-turn (steepest); long tool lists (arXiv 2411.15399);
long context [no specific study found]; **irrelevance** — knowing not to call — Llama-3.1-8B 42.7%,
Qwen3 77–85%. If code picks the tool, this failure cannot occur.

## 2. BFCL v4, small models (official CSV, fetched 2026-10-18)
Read **Non-Live AST** (single call) and **Irrelevance**; "Overall" mixes in web-search and memory.

| Model | Overall | Non-Live AST | Live | Multi-turn | Irrelevance |
|---|---|---|---|---|---|
| Nanbeige4-3B-Thinking (FC) | 51.4 | 81.6 | 79.4 | 51.1 | 83.1 |
| xLAM-2-8b-fc-r (FC) | 46.7 | 84.6 | 68.0 | **70.0** | 63.3 |
| BitAgent-Bounty-8B | 46.2 | 81.6 | 93.1 | 62.4 | 97.5 |
| Qwen3-8B (FC) | 42.6 | 87.6 | 80.5 | 41.8 | 79.1 |
| ToolACE-2-8B (FC) | 42.4 | 87.1 | 77.4 | 38.4 | 90.8 |
| xLAM-2-3b-fc-r (FC) | 41.2 | 83.0 | 62.9 | **58.4** | 63.5 |
| Qwen3-4B-Instruct-2507 (FC) | 35.7 | 87.9 | 76.4 | 22.1 | 84.9 |
| Arch-Agent-3B | 35.4 | 86.7 | 72.9 | 34.9 | 74.7 |
| Arch-Agent-1.5B | 32.1 | 82.7 | 67.7 | 26.6 | 74.8 |
| Hammer2.1-7b (FC) | 31.7 | 85.5 | 69.5 | 23.9 | 90.1 |
| xLAM-2-1b-fc-r (FC) | 30.4 | 69.0 | 55.1 | **45.5** | 64.5 |
| Hammer2.1-3b (FC) | 29.7 | 85.0 | 70.5 | 16.5 | 86.1 |
| Qwen3-1.7B (FC) | 28.4 | 82.9 | 74.6 | 11.0 | 76.5 |
| Hammer2.1-1.5b (FC) | 27.9 | 83.0 | 69.5 | 15.6 | 79.4 |
| Llama-3.1-8B (Prompt) | 25.8 | 84.0 | 70.8 | 11.1 | 42.7 |
| Qwen3-0.6B (FC) | 23.9 | 71.8 | 56.6 | 3.6 | 80.8 |
| Llama-3.2-3B (FC) | 22.0 | 82.7 | 58.3 | 4.0 | 52.1 |
| Hammer2.1-0.5b (FC) | 21.2 | 66.0 | 54.6 | 2.9 | 80.8 |
| Gemma-3-4b-it (Prompt) | 19.6 | 61.1 | 60.8 | 0.4 | 53.9 |
| Granite-4.0-350m (FC) | 19.0 | 67.9 | 46.1 | 2.5 | 60.8 |
| Llama-3.2-1B (FC) | 10.8 | 38.4 | 11.8 | 0.0 | 51.6 |
| Gemma-3-1b-it (Prompt) | 7.2 | 20.2 | 11.8 | 0.0 | 33.2 |

**Vendor-reported, not comparable:** LFM2.5-1.2B-Instruct card (custom handler) gives BFCLv3
LFM2.5-1.2B 49.1, Qwen3-1.7B 46.3, Granite-4.0-1B 52.4, Llama-3.2-1B 21.4, Gemma-3-1B 16.6.
LFM2.5 emits Pythonic calls between `<|tool_call_start|>`/`<|tool_call_end|>` by default; 32K ctx.

**Readings:** at 1.5–8B single-call accuracy is flat (83–88%); families differ on multi-turn;
multi-turn-trained models (xLAM-2) far outperform same-size generalists; Gemma 3 and Llama 3.2 at
1B are poor tool callers. **Phone-class picks (≤1.2B):** LFM2.5-1.2B, Granite-4.0-1B, xLAM-2-1b, or
a FunctionGemma-270M fine-tune (58→85% on Mobile Actions,
https://blog.google/technology/developers/functiongemma/).

**Not on the board:** Gemma 3n, Phi-4-mini, SmolLM3, LFM2/2.5, **Qwen3-VL-4B (today's laptop
model)**.

## 3. Constrained decoding
- **JSONSchemaBench** (ICML 2025, arXiv 2501.10868): Llama-3.1-8B GSM8K 80.1 unconstrained →
  82.4 llama.cpp, 83.7 XGrammar, 83.8 Guidance. Time per output token: 16.7 ms none, 29.1 ms
  llama.cpp grammar, **7.6 ms Guidance** (fast-forwards forced tokens), 46.6 ms Outlines (+8 s
  compile). Schema compile failures (Llama-3.2-1B): llama.cpp 37 categories, Guidance 25,
  XGrammar 3.
- **llguidance in llama.cpp:** `-DLLAMA_LLGUIDANCE=ON` (needs Rust); `%llguidance` grammars and
  JSON-schema requests route to it, ~50 µs/token mask. Set `additionalProperties: false`
  explicitly.
- **Counter-argument:** "Let Me Speak Freely?" (arXiv 2408.02442) — JSON mode hurts reasoning;
  rebuttals (dottxt; Castillo) blame prompting. Practice: put a `reasoning` field *before* the
  constrained fields when a task needs thought; keep schemas shallow.
- **Precedent:** Apple's ~3B on-device model (arXiv 2507.13575) is built around guided generation
  and post-trained on that format.
- **For Myrme's span-pointing:** a grammar can restrict output to `{"start":int,"end":int}` or an
  enum of candidate span ids — the model cannot invent a value at all.

## 4. Specialist small models
- **NVIDIA, "SLMs are the future of agentic AI"** (arXiv 2506.02153): most agent calls are narrow
  and repetitive; small by default, large by exception; conversion loop = log calls → cluster
  into tasks → fine-tune a small model per cluster → iterate. Position paper; "10–30× cheaper"
  [unverified].
- Consistent evidence: TinyAgent; xLAM-2; FunctionGemma; LFM2-350M-Extract (Liquid Nanos,
  single-turn, schema in the system prompt; vendor claim); LoRA Land (arXiv 2405.00732)
  [from memory].
- **Per-task LoRA:** llama.cpp loads adapters at runtime (`--lora`, server hot-swap); on Android
  [unverified].

## 5. Human-in-the-loop and evaluation
- No benchmark measures proposal UIs specifically; it is a design argument. Irrelevance rates of
  42–97% mean small models *will* call when they should not; accept/reject is the cheapest fix and
  produces labelled data.
- **Log every proposal with its outcome** (accepted / edited / rejected) plus input, prompt and
  model version — the dataset NVIDIA's conversion loop needs.
- **Eval practice:** per-task golden sets (50–200 real cases) scored by exact match on structured
  output (BFCL AST style), not an LLM judge; parse-valid rate measured separately; consistency over
  k repeats (τ-bench pass^k, arXiv 2406.12045); run on the real quantisation and device; include
  irrelevance cases.

## Recommendations (as given by the research)
| Pattern | Why | Risk |
|---|---|---|
| Keep the **code-selected workflow** | single call ~83–88%; multi-turn 0–22% at ≤4B | less open-ended flexibility, by design |
| **One prompt + schema + grammar per task** | specialisation wins small | per-task upkeep |
| **Grammar-constrain every output** via llguidance | 100% parseable, +0–3 pts, faster than GBNF | build needs Rust; `additionalProperties:false` |
| **Span pointing** constrained to offsets / ids | removes value hallucination | still must pick the right span |
| If a model ever selects: **≤4 candidates** from a classifier/rules router | shorter prompts, higher accuracy | silent router errors — measure recall |
| Multi-step: **plan once, execute in code**, never ReAct | small models lack multi-turn | validate the plan against a schema first |
| Phone tier LFM2.5-1.2B / Granite-4.0-1B / xLAM-2-1b / FunctionGemma-270M FT | best measured ≤1.2B | vendor numbers not comparable |
| **Proposal log → distillation → per-task LoRA** | turns accept/reject into training data | privacy: notes stay local; small sets overfit |
| **Per-task exact-match evals** on device | leaderboards do not predict your task | curation effort |

## Hype vs working at 0.3–8B
**Works:** single structured call per step; grammar-constrained output; small/retrieved tool lists;
plan-once execution; task fine-tunes and distillation; careful 4-bit quantisation; exact-match evals.
**Hype or unproven:** autonomous ReAct on ≤4B generalists; code-as-action at small size; swarms of
small agents; "one small generalist replaces cloud LLMs"; vendor leaderboards with custom handlers;
BFCL "Overall" as a selection metric.
