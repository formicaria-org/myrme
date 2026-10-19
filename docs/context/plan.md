# Myrme — the plan (2026-10-18)

> **A plan, not a ruling.** The rulings it asks for go in `decisions.md` when the owner makes them.
> Evidence is in `../research/` (three reports, 2026-10-18). Status lines are updated as work lands.

## The owner's direction

> *"Move the agentic part out of the fm repo since I want to focus more on it, specifically by
> thinking about dedicated methods for dedicated tasks… we target small models that run on chip to
> allow people to use them locally."* — *"make sure we do not break current main fm. We need to act
> in parallel in the dev phase."*

## Part 1 — What the research says, and what Myrme takes from it

The three reports agree, and they agree with how formicaria already built the assistant:

| Finding | Evidence | What Myrme does |
|---|---|---|
| **Small models do single calls well and multi-step loops badly.** At 1.7–8B, single-call accuracy is 83–88%; multi-turn is 0–22% at ≤4B for general models | BFCL v4, official CSV | **Workflows, not autonomous agents.** A method is a fixed pipeline in Rust with at most a few single-shot model calls. No ReAct loop on a device |
| **Fewer visible tools, shorter prompts** raise accuracy (TinyAgent: ~4 tools at 0.998 recall from a tiny classifier) | TinyAgent; arXiv 2411.15399 | **Code selects the method** (kept from formicaria, 2026-10-10). If free-text intent is ever routed, it goes through a small classifier with an *ask-the-person* fallback, never an LLM over a list |
| **Constrained decoding** guarantees a parse and slightly helps quality; llguidance covers far more schemas than llama.cpp's GBNF and is faster | JSONSchemaBench (ICML 2025) | **Every model output is grammar-constrained**, to span offsets, ids or enums wherever possible. That is "the model points, Rust computes", enforced |
| **Specialists beat generalists on narrow tasks** | xLAM-2, TinyAgent, FunctionGemma, LFM2-Extract; NVIDIA arXiv 2506.02153 | **Dedicated methods**, each with the cheapest method that works: no model, then a small encoder, then a small LLM |
| **Fixed research loops work at 3–8B**; multi-agent research does not | local-deep-researcher; judgement | Research is a fixed loop: query → search → extract → rank → write from chunks → check citations in Rust |
| **Frameworks don't fit** a Rust, std-only, mobile codebase (tokio, churn, built for big models) | framework survey | **Keep the hand-rolled loop.** Borrow designs from Pydantic AI and goose; depend on none of them |
| **MCP and Agent Skills are the open standards** the community builds on (MCP is under the Linux Foundation's AAIF; `rmcp` needs tokio) | standards survey | **Later, at the edge, never in the core.** An optional `myrme-mcp` binary, and method prompts kept as `SKILL.md`-style folders (Part 2) |
| **llama.cpp is the runtime that reaches every platform**, in-process on iOS; Apple Foundation Models and LiteRT-LM are credible later backends | runtime survey | A **`Backend` seam**: llama-server over HTTP now; others plug in behind it |
| **Logging accept/reject turns usage into eval and training data** | NVIDIA's conversion loop | Read formicaria's supervision records (`supervision-datasheet.md` there) into per-method eval sets |

What the research **does not** change: proposals, nothing downloaded unasked, the live memory
reading that decides support, and the notes-only build of formicaria.

## Part 2 — The shape of Myrme

### A method

**A method is one dedicated task, end to end:**
- a **trigger** (command, MIME type, schedule or UI surface);
- the **capabilities** it needs (`llm`, `vision`, `asr`, `encoder`, `web`);
- a **pipeline** of deterministic steps and constrained model calls;
- a **proposal** as its only output.

Methods are a **closed `enum`**. Adding one is a `decisions.md` entry, the same discipline as
formicaria's `Renderer` and `Command`, so there is still no registry.

Each method also has a folder of **data**, not code:

```
methods/<name>/
  METHOD.md      SKILL.md-compatible: frontmatter name + description, then the prompt(s)
  schema.json    the output schema the grammar is built from
  eval/          golden cases: input, expected structured output, "nothing" cases
```

Keeping the prompt in Agent Skills' format costs nothing. It lets anyone who knows that format read
and improve a method, and it lets the Phase E MCP server expose methods to other agents unchanged.
The Rust stays the authority on what runs.

### The methods, with the dedicated approach each gets

The order is the rough priority. "Today" means already shipped inside formicaria.

| # | Method | Today | Dedicated approach (from the per-task report) |
|---|---|---|---|
| M1 | **Meetings from mail** | ✅ the model points, Rust reads dates | **ICS first.** A `text/calendar` part settles invites, moves and call-offs with no model. Then Rust dates, then the LLM pointing under a span grammar. Later, a GLiNER2-class encoder for intent and entities |
| M2 | **Research** | ✅ `/search`, `/research` | **local-deep-researcher's fixed loop**, plus main-content extraction, BM25 plus a small embedder, and **a Rust check that every cited span exists in its source** |
| M3 | **Image → text** | ✅ LFM2.5-VL-450M on the phone | **Route by content.** Printed text: PP-OCRv5 (ONNX, ~20 MB), escalating to the VLM only on low confidence. Math crops: PP-FormulaNet-S or Texo. Handwriting: VLM. Needs a 50-image eval from the owner's notes first |
| M4 | **Audio → text, dictation** | ✅ whisper, any language | Add **VAD** (Silero) in front. Try **Moonshine** for live English dictation; whisper stays for multilingual |
| M5 | **Note edits** | ✅ `/propose` | Output constrained to `{edits:[{op, anchor, old, new, reason}]}`, never a whole-note rewrite |
| M6 | **Food from a sentence** | ❌ (the tracker exists in fm, no assistant path) | An encoder for `food, quantity, unit` spans, rules for numbers, then a lookup through formicaria's own food tool via `dispatch`. **The portion-defaults table matters more than the model** |
| M7 | **Recipe from a recording** | ✅ `/recipe` | ASR, then the LLM under a `{title, ingredients[], steps[]}` grammar, then a per-line ingredient parser |
| M8 | **Pose coaching** | ❌ | Probably **not a Myrme method**: it is a real-time camera loop (MediaPipe in formicaria's UI) that writes `log: workout` notes. Decide separately |

### The seams inside Myrme

- **`Backend`**: `complete(prompt, constraint) -> text`.
  - First implementation: llama-server's OpenAI-compatible API, with `json_schema` for the
    constraint. Test a llama.cpp build with **llguidance** for the coverage and speed the
    benchmarks show.
  - Later implementations: in-process libllama (iOS), Apple Foundation Models, LiteRT-LM.
- **`Encoder`**: span or label extraction and embeddings. **ONNX Runtime** is the second and last
  general runtime. Keep to two runtimes, plus MediaPipe only if pose ever lands here.
- **`Vault`**: the existing `VaultAccess` over formicaria's `/api`. Unchanged.

### Which models

The catalogue keeps its rule: a live memory reading decides, and nothing is fetched unasked. Each
specialist joins `models.toml` with its size and licence.

| Tier | Candidates |
|---|---|
| Phone (≤1.2B) | LFM2.5-1.2B, Granite-4.0-1B, xLAM-2-1b |
| Laptop (3–8B) | Qwen3-4B-Instruct-2507 (87.9 single-call), xLAM-2-3b/8b (best multi-turn) |

**Licences to avoid:** CC-BY-NC weights (GLiNER v0/v1, jina rerankers), NuExtract-4B, and
Moonshine's legacy non-English weights.

## Part 3 — Phases

Each phase leaves both repos green. **formicaria's `main` changes only in Phase A and Phase D**, each
time by an ordinary commit that its gate checks.

### Phase A — the port (status: **done in Myrme; rehearsed in formicaria**, 2026-10-18)

In Myrme:

| Step | Status |
|---|---|
| Repo created; `crates/fm-agent`, `crates/fm-agent-run` and `agents/` extracted from formicaria v0.9.1 with their history (78 commits) | ✅ 2026-10-18 |
| Standalone workspace; `fm-fetch` from formicaria pinned by tag `v0.9.1`; pixi toolchain | ✅ |
| `pixi run ci` locally: fmt, clippy, 122 + 44 tests (56 with `download`), cargo-deny | ✅ green |
| `.github/workflows/ci.yml` (push to main and PRs), with the integration job | ✅ green on GitHub |
| Contract written down (`contract.md`) and compile-checked (`tests/fm_contract.rs`) | ✅ |
| `CLAUDE.md`, `AGENTS.md`, `README.md`, `docs/context/`, `docs/research/` | ✅ |
| First push to `github.com/formicaria-org/myrme` | ✅ 2026-10-18 |
| **Port the agent work formicaria shipped since the extraction**: v0.9.2's `1db54e2` mirrored with `git am` (its author and message kept); the pin moved to v0.9.2 | ✅ |

In formicaria. **Done as one rehearsed change on a local branch, `myrme-cutover` in a worktree
`~/fm-myrme` (not pushed), instead of four commits on `main`**: the owner asked for the full move
tested outside the repository. The four steps below are all in it.
1. ✅ (formicaria `decisions.md#agent`, 2026-10-18, on the branch) **Freeze rule recorded.** A `decisions.md#agent` entry says Myrme lives in its own repo, and the
   in-tree copy takes fixes only, mirrored the same day.
2. ✅ **The catalogue travels with the crate.** Move `agents/models.toml` to
   `crates/fm-agent-run/models.toml`, export it as `fm_agent_run::CATALOGUE`, and make the three
   places that copy it (`desktop.yml`, `packaging/formicaria.sh`, `ci/android-stage-runtime.sh`)
   take it from `agent-serve --print-catalogue`.
   - Do it in Myrme first and mirror it, so the paths stay identical.
3. ✅ (as `deny.toml`'s single allowed git source, plus the cross-crate guards reading Myrme at the pin)
   **A guard in `ci/checks.sh`.** Every `fm-agent*` dependency is either in-tree or a git
   dependency pinned by `tag` on a public URL.
4. ✅ (the existing no-default-features guards pass on the branch) **An agent-off build** of `fm-serve` and of the mobile shell in CI, so a broken Myrme can always
   be shipped around.

### Working in parallel (from now until Phase D)

- **formicaria `main`** ships its in-tree copy. Fixes only.
  - Mirroring a fix: `git format-patch -1 <sha> -- crates/fm-agent crates/fm-agent-run agents` in
    formicaria, then `git am` in Myrme. The paths are identical, so no rewriting is needed.
- **Myrme `main`** is where all new agent work happens. Its CI must be green. It never needs
  formicaria to change.
- **Trying Myrme inside the real app** takes two routes. Neither touches formicaria's checkout:
  - **Out of process (the usual case):** `pixi run serve` in formicaria, then
    `pixi run agent-serve` here, pointed at `127.0.0.1:8765`. The HTTP door is all it needs.
  - **Linked (when the host side changes):** a separate formicaria worktree on a local branch:
    ```
    cd ../formicaria && git worktree add ../fm-lab -b lab
    cd ../fm-lab && sed -i 's|path = "../fm-agent-run"|path = "../../../myrme/crates/fm-agent-run"|' crates/fm-serve/Cargo.toml
    ```
    - Apply the same edit to `mobile/src-tauri/Cargo.toml` for the phone.
    - Build with `pixi run serve` in that worktree.
    - The `lab` branch is never pushed.
    - Before Phase D, formicaria depends on these crates by *path*, and Cargo's `[patch]` cannot
      override a path dependency. That is why it takes a worktree and not a config file.

### Phase B — foundations (in Myrme only; formicaria unaffected) — status: **started**, 2026-10-18

| Step | Status |
|---|---|
| Eval harness: `fm_agent::eval`; cases, oracle and recordings per method; replayed in the gate; `pixi run eval` | ✅ M1 (17 cases), M2 (8), M5 (6) |
| Constrained calls: `LlmStep::complete_constrained`, `OpenAiStep` sends `response_format` | ✅ |
| llguidance compared with the stock grammar engine | ✅ not adopted (`decisions.md`) |
| M1 held to its schema; a question nobody answered fixes nothing | ✅ 17/17 qwen3-4b-2507, 15/17 qwen3-vl-4b |
| M2 parser: misplaced citations, closing punctuation | ✅ 5/8 → 7/8 on both laptop models |
| M5 as anchored edits applied to the whole note | ✅ 3/6 → 5/6 on both laptop models |
| `Method` enum and `methods/<name>/METHOD.md` for all seven | ✅ |
| Cases for M3 (images), M4 (speech), M7 (recipe), from openly licensed data (`agents/eval-data.py`) | ✅ GNHK 30 photos, MLS + LibriSpeech 60 clips, Wikibooks 30 recipes |
| M3 laptop reader capped at 1024 image tokens (catalogue `image_max_tokens`) | ✅ every page timing out → 17.9% median WER, ≈35 s a page |
| M4 measured: `ggml-base` 6.3% EN / 36.1% IT; `ggml-small` 4.1% / 23.1% | ✅ recorded; suggesting *better listening* by language is the owner's call |
| M7 measured: 28/30 on the laptop default; the schema bought nothing and stays off | ✅ |
| The runtime with llguidance on every platform (`runtime.yml`, release `runtime-llg-3`), in the catalogue | ✅ ~17% faster on the GPU, ~20% on the CPU, as accurate |
| M2 held to a schema with `answerable` first | ✅ 7/8 → 8/8 on both laptop models |
| M5: a cut note is marked; append and replace rules (found by the live run) | ✅ 5/6 → 7/7 on both laptop models |
| Integration test against a real formicaria (`pixi run integration`, a CI job): `/propose` round trip, an unplaceable edit, the recipe headings | ✅ against v0.9.2 |

1. **An eval harness per method.**
   - Grow `agents/bench` into `methods/<name>/eval/`, scored by exact match on structured output.
   - Report the parse-valid rate separately and include "nothing" cases.
   - Recorded model outputs run in `pixi run ci`; live model runs stay `#[ignore]` (as
     `tests/live_*.rs` already are).
   - Seed the cases from formicaria's supervision records.
2. **The `Backend` seam and constrained calls.**
   - Route every model call through one function that takes a schema.
   - Measure llama-server's `json_schema` against an llguidance build on the meeting eval before
     choosing one.
3. **The `Method` enum and `methods/` folders.** The existing four commands and the meeting pass
   become methods M1–M7, with no behaviour change, proven by the evals from step 1.
4. **An integration test against a real formicaria.**
   - Build `fm-serve` from the pinned formicaria tag, start it on a temporary vault, and drive one
     proposal end to end over HTTP.
   - This makes contract parts 2–4 machine-checked.

### Phase C — the dedicated methods (in Myrme; reaches users only through Phase D)

**Myrme light (proposed 2026-10-19):** the dedicated methods as a whole — tools that point on ONNX
Runtime, a very small LLM only for free text — planned in phases L0–L7, each gated on the evals:
`docs/research/2026-10-19-myrme-light.md`. One pick per tool; the laptop first (L0–L6), the phone after (P).
**Status 2026-10-20:** the laptop phases are done, and the light version ships in **Myrme 0.7.0**:
Qwen3.5-0.8B (the phone's model), Rust, and **one tool, the OCR for printed pages** (Linux, macOS,
phone; not Windows yet). GLiNER and e5 were measured and dropped: one tool per subtask is not the
aim. Still open: the OCR's time on the phone (`examples/ocr_page.rs` is the run).

**Next, by what the evals show matters most:**
1. **Handwriting on the phone.** The phone's vision model cannot read handwriting (82.7% median WER).
   Try a dedicated reader (PaddleOCR-VL, or printed-text OCR first). This adds a runtime, so it needs
   a `decisions.md` entry.
2. **Italian speech** with `ggml-base` (36.1% WER). Decide whether *better listening* is suggested by
   language, and measure the owner's own recordings.
3. The items below.


In order, each gated on its eval beating today's behaviour:

1. M1 ICS-first
2. M2 research loop with checked citations
3. M3 OCR routing (adds ONNX Runtime; a `decisions.md` entry, since it widens what ships)
4. M6 food from a sentence
5. M4 VAD and dictation

### Phase D — cutover in formicaria (one revertible commit) — status: **rehearsed, green**, 2026-10-18

On branch `myrme-cutover` (worktree `~/fm-myrme`, local only):
- formicaria's whole `pixi run ci` is green with Myrme from GitHub at every pin it took
  (`v0.1.0-rc.1`, `v0.1.0`, `v0.1.1`, **`v0.1.2`**).
- **Live, end to end, on `v0.1.2`:** both laptop models, two runs each, all pass. Each run used the
  branch's own `fm-serve` and `agent-serve`, Myrme's llguidance runtime and a real model; a
  person's `@Myrme-laptop add a final line … /propose` became a proposal that keeps the whole
  2,576-character note and ends with the line (`agents/e2e-live.sh`).
  - The first live runs found two faults the evals had not: a model replacing a sentence that
    shared a number with the request, and a command copied out of the history. Both are fixed, and
    the edits eval now runs as production does.
- formicaria's Android staging takes Myrme's llguidance build and checks its SHA-256.
- Myrme's `FM_CHECKOUT=~/fm-myrme pixi run integration` passes 3/3.
- `agents/e2e-live.sh` runs the branch's own binaries with a real model on Myrme's runtime.
- **What is left is the owner's:** merge the branch into `main` for the next release, and run the
  `android` workflow by hand on it (the phone shell was not compiled here; this machine has no
  NDK).

1. Tag Myrme (`v0.1.0`), with the contract test and integration test green against formicaria
   `main`.
2. In formicaria, in one commit:
   - delete `crates/fm-agent`, `crates/fm-agent-run` and `agents/`;
   - depend on `{ git = "https://github.com/formicaria-org/myrme.git", tag = "v0.1.0" }` in
     `fm-serve` and the mobile shell;
   - add a `[patch]` so Myrme's `fm-fetch` resolves to formicaria's in-tree crate (one copy);
   - update `THIRD-PARTY.md`.
3. **Before it lands:**
   - `pixi run ci` green;
   - the release archive's file list matches the previous release (`agent-serve` and the
     catalogue present);
   - the `android` and `cross` workflows run by hand on that commit.
4. **From then on, a pin bump that changes what Myrme does is a formicaria `decisions.md` entry**,
   with the eval numbers.
5. After the cutover, the linked dev route becomes a gitignored `.cargo/config.toml` `[patch]`
   pointing at `../myrme/crates/*`.
6. **Then narrow the contract.** Replace the ~20 items with a facade crate (provision, status,
   serve loop, device word). Each item struck from `contract.md` is one fewer way to break
   formicaria.

### Phase E — outward, optional, and each its own ruling

- `myrme-mcp`: a separate binary with tokio confined to it. It is an MCP server exposing notes
  search and read plus Myrme's methods, so goose, Claude Code and Codex can use formicaria. It could
  also act as an MCP client on the desktop for community tools.
- An iOS backend: in-process libllama or Apple Foundation Models. iOS forbids subprocesses, so this
  is the only way the assistant reaches an iPhone.
- Per-method LoRA adapters distilled from accepted proposals (NVIDIA's loop). This needs enough
  records first; formicaria counted 23 in August.

## Phase D is done (2026-10-18)

**Its last step, the facade:**
- **Desktop: done** (Myrme 0.3.0, contract 1.1, `fm_agent_run::host`).
- **Phone: done** (Myrme 0.4.0, contract 1.3, `Tools::run_in_process`). It was built here with
  formicaria's NDK; it was not run with a model, because the emulator is x86_64 and the runtime is
  arm64.
- **Contract 2.0 (next breaking release):** remove the 1.0 items nothing uses (`contract.md`).
- **Speed study** (2026-10-19; the phone's faster default and accurate choice adopted, layouts still
  the mail layout per model): `docs/research/2026-10-19-speed.md`,
  measured on the owner's Dimensity 7300 phone and the laptop. Its §4 lists the owner's decisions
  (phone quant Q4_K_M, KleidiAI, prompt layout, Qwen3.5-0.8B for text); §5 is the resume plan.
- **No copy in formicaria** (Myrme 0.5.0, contract 1.4, 2026-10-19): the manual's assistant chapter
  and the phone's runtime pins are read from Myrme. formicaria 0.9.3, the first release on the
  cutover, links 0.4.1 and is published for every platform.

formicaria `main` links Myrme from this repository; the owner merged the cutover on 2026-10-18.
From then on:
- **formicaria pins a Myrme release** and checks its contract at compile time
  (`MYRME_CONTRACT`; contract 1.0 since Myrme 0.2.0).
- **`pixi run myrme-bump`** in formicaria moves to the newest compatible release.
- **formicaria's release notes name the Myrme they carry.**
- **A Myrme release** follows `releases.md`. A change formicaria must make first (a new contract
  major) is a formicaria change before it is a Myrme release.

## Hand-off: what is the owner's (2026-10-18)

1. **Merge `myrme-cutover` into formicaria `main`** for the next release.
   - It is in the worktree `~/fm-myrme`, local and not pushed, at `v0.1.2`.
   - It is five commits, **rebased onto `main` at `f7f08ab`** (the only conflict was both sides
     appending to `decisions.md`, resolved by keeping both).
   - On that state, 10 of formicaria's 11 gate steps pass. The 11th, `third-party-check`, fails on
     `main` itself since `f7f08ab`: `THIRD-PARTY.md` carries the NIA text that `ci/third-party.sh`
     does not yet generate. That is the fitness work's to finish. Before the rebase, at `v0.1.2`,
     all 11 passed.
2. **Run formicaria's `android` workflow by hand on it.** The phone shell was not compiled here (no
   NDK). Its only change is that the catalogue is `fm_agent_run::CATALOGUE`, and that the staging
   script fetches Myrme's Android runtime and checks its checksum.
3. **Decide** whether formicaria suggests *better listening* to people who speak Italian (36.1% →
   23.1% WER).
4. Real recordings and photographs of your own, when convenient. They are the realistic eval sets
   still missing.

## Questions for the owner

Answered 2026-10-18 (*"commit and push the scaffold, keep the defaults"*); recorded in
`decisions.md`:
- **Pushed:** yes.
- **Crate names:** keep `fm-agent*` until after Phase D.
- **formicaria's in-tree copy:** fixes only until Phase D.
- **Pose coaching:** not a Myrme method.

Still open:

| Question | Default if unanswered |
|---|---|
| Phase E's MCP server: wanted, and on which side (Myrme or formicaria)? | not started |
