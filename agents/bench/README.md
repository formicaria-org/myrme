# formicaria model-benchmark

A **repeatable benchmark on our own case** — not synthetic `llama-bench`. It drives a running
`llama-server` through formicaria's real study-agent workload (our system prompts + a fixed offline
case battery) and reports only measured numbers. Used to pick the biggest *usable* model per device,
and kept so that decision is a re-run, not a re-argument.

**Where the results live (all measured, all in-repo):**
- **`results.md`** — the raw per-case blocks, one per run (throughput, RSS, VRAM, MemAvailable, per-case quality). Newest first.
- **`../../docs/context/device-resources.md`** — the per-model resource-cost matrix + device specs.
- **`resource-report.html`** — a self-contained, offline-openable visual report of the full sweep (open in a browser; no external assets). Regenerate it from the two files above when new runs are added.

## What it measures

Per case, from llama.cpp's own response: **decode tok/s** (`timings.predicted_per_second`), prompt
tok/s, token counts, wall time, and a **VERIFIED quality signal** (correctness where we can check it
deterministically — not just counts). Around the run it samples the model process **RSS/VmHWM**,
**VRAM** (GPU), and **MemAvailable** (so a phone block proves the admission gate had room).

The quality checks by case type (`check` field), grounded in agentic-eval methodology (BFCL for
tool-calls, FACTS/RAGAS-style faithfulness — see `docs/context/model-selection-research-2026-07-24-grounded.md`):
- **`research`** — VERIFIES grounding: each `> quote` must be a **verbatim substring of the source its
  claim cited** (the case carries structured `sources` to match against). Reports `N claims /
  **V/Q verbatim** / MIS-CITED / HALLUCINATED-quote`. A model that emits quote-*shaped* lines it did
  not copy scores 0 verbatim — the thing the old counting harness could not tell apart.
- **`toolcall`** — the BFCL-style "decide whether to search" axis: the request carries a `web_search`
  tool; scored on the emitted `tool_call` (well-formed call, or correctly *not* calling when `expect_tool`
  is null), never the prose.
- **`abstain`** — junk/novel sources that don't answer the question; the model must abstain, not
  fabricate cited claims. Reports `abstained` vs `FABRICATED`.
- default (`chat`/`write`) — programmatic format checks (KaTeX `$…$` not `\(`/`\[`; no whole-answer fence).

## The cases (`cases/*.json`)

Mirror our real system prompts (`crates/fm-agent/src/{lib,grounding}.rs`) — keep them in sync if those
change. Each carries a `check` type (above) + any `sources`/`tools`/`expect_tool` the check needs:
- `1-chat.json` — a discussion reply (`CHAT_INSTRUCTION`).
- `2-research.json` — grounded synthesis + a `sources` map for verbatim-quote verification (`GROUNDED_WRITE_INSTRUCTION`); offline/deterministic.
- `3-write.json` — a house-format note write with a table (`OUTPUT_FORMAT_INSTRUCTION`).
- `4-toolcall-search.json` — needs a current external fact → should emit a `web_search` call.
- `5-toolcall-nosearch.json` — answerable from the note → should **not** search (relevance detection).
- `6-abstain.json` — novel sources that don't answer → must abstain, not fabricate.

**Deferred hardening (methodologically noted, not yet built):** a cross-vendor LLM **entailment judge**
for claim↔quote support (needs a judge model + a hand-labelled gold set), and **pass^k reliability**
(N seeds at temperature > 0). The deterministic checks above cover the highest-value correctness signal
first.

## Run it

**Laptop** — against the model `agent-serve` is already running (or launch `llama-server` on any GGUF
with the app's args: `runtime/llama-server -m <gguf> --host 127.0.0.1 --port 8081 -c <ctx> -t 8 -ngl 99 --no-warmup`):

```
python3 agents/bench/run.py --url http://127.0.0.1:8081 --model <label> --device laptop \
    --pid $(pgrep -x llama-server) --gpu --date $(date +%F) >> agents/bench/results.md
```

**Phone** — push the arm64 `llama-server` + GGUF to `/data/local/tmp` (the only allowed scratch),
launch it there, adb-forward its port, then:

```
adb forward tcp:18081 tcp:8081
python3 agents/bench/run.py --url http://127.0.0.1:18081 --model <label> --device phone \
    --adb-name llama-server --date $(date +%F) >> agents/bench/results.md
```

Set `ADB` in the env if `adb` isn't at `.android/platform-tools/adb`. Temperature is pinned to 0 for
repeatability. Not run in `pixi run ci` (it needs a live model + weights); it is a manual measurement
tool, like the mid-run-kill harness.

## Every model is different — special tokens + chat templates

`llama-server` applies **the GGUF's own chat template** (from its metadata), so BOS/EOS/`<|im_start|>`
and friends are handled per-model — you do not hand-write a prompt string. But **model-specific knobs
are not universal**, and assuming one config fits all is how a fair model gets a wrong verdict:

- **Thinking models** (Qwen3, …) emit `<think>…</think>` and put the answer in a *separate* field.
  Default here is **thinking OFF** (`enable_thinking:false`, `--think` to keep it on) — a study
  assistant wants a direct answer, and otherwise a small token budget is spent entirely reasoning and
  `content` comes back **empty**. The runner also reads `reasoning_content`/`finish_reason` so an empty
  answer is reported as `THINK-ONLY`/`EMPTY(length)`, never silently.
- `enable_thinking:false` is **harmlessly ignored** by templates that don't use it (lfm2.5, Llama-3.2).
  Do not assume the reverse — a new model may need its own knob. When a result looks broken, suspect the
  template/config before the model.
- **To actually ship a thinking model**, the app's `OpenAiStep` (`crates/fm-agent/src/openai.rs`) needs
  the same handling, or it hits the identical empty-answer bug.
