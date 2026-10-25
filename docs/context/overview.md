# Overview — read this first

This is the short map of Myrme's repo. When it disagrees with the code, the code wins; fix this file.

## Router: read before you touch an area

| Touching… | Read first |
|---|---|
| Anything formicaria imports, the `/api` calls, `models.toml`'s format, the notes Myrme writes | `contract.md` (with its *Versions* rules), and `crates/fm-agent-run/tests/fm_contract.rs` |
| Cutting a release; which Myrme a formicaria links | `releases.md` |
| What to build next, the phases, working in parallel with formicaria | `plan.md` |
| Why something is the way it is | `decisions.md`. Rulings before 2026-10-18 are in formicaria's `docs/context/decisions.md` under `#agent`; the index of them is at the bottom of `decisions.md` here |
| Choosing a pattern, a framework, a runtime or a protocol | `../research/2026-10-18-small-model-agent-patterns.md` · `../research/2026-10-18-open-standards-frameworks-runtimes.md` |
| Choosing a model or a non-LLM method for one task | `../research/2026-10-18-dedicated-methods-per-task.md` |
| Scouting a new optional model for the phone (shortlist from a leaderboard, then our evals) | `../research/2026-10-19-scouting-phone-models.md` (the method in §2) |
| A bigger model for the laptop (K2-Horizon-7B against qwen3-vl-4b, reasoning off and low) | `../research/2026-10-25-k2-horizon-7b-laptop.md` (not adopted: fewer exact, half the speed) |
| What was measured for the phone model and Myrme light, how, and the results in one place | `../research/2026-10-20-small-model-benchmark-summary.md` |
| **Myrme light** — dedicated tools (OCR, extraction, embeddings on ONNX Runtime) and a very small LLM | `../research/2026-10-19-myrme-light.md` (the plan, phases L0–L7; the laptop phases are done, and its one tool, the OCR, shipped in Myrme 0.7.0) |
| **The speed bar**: the minimum speed per kind of wait, what meets it per method, and what was tried (prompt reuse with `-ub`, streaming, recipe pointing) | `../research/2026-10-25-minimum-speed.md` · `plan.md` (*The speed bar*) |
| Making the small model faster: runtime settings, kernels, quantisation, other engines | `../research/2026-10-19-speed.md` · `../research/2026-10-21-low-bit.md` (below Q4_0: answered, none faster) · `../research/2026-10-23-integrated-gpu-fallback.md` (`gpu = "auto"` on an integrated GPU: slower than the CPU; fixed 2026-10-23) · `plan.md` (*Faster engines*) |
| Running the assistant by hand, benchmarks | `../../agents/README.md` · `../../agents/bench/` |

## What is here

```
crates/fm-agent       the pipeline: deterministic orchestration, the methods and their evals,
                      meeting pass, transcription, watchdog, launcher. No fm crate and no network
                      crate. ~8.7k lines with tests, 168 tests (15 of them live, `#[ignore]`d).
crates/fm-agent-run   the runner: agent-serve (supervisor + @Myrme watcher), agent-chat, the host
                      facade (`host`), the mail pass, the OCR (feature `ocr`, on by default), the
                      model catalogue (`models.toml`, exported as `CATALOGUE`), the downloads
                      (feature `download`, which uses formicaria's fm-fetch pinned by tag).
                      ~8.1k lines with tests, 93 tests (10 of them live).
agents/               scripts (fetch, serve, eval, integration, release pages), the bench harness.
                      runtime/ and models/ are fetched and gitignored.
methods/<name>/       each method's METHOD.md and its eval cases (chat has none).
docs/manual/          formicaria's assistant chapter.   docs/releases/   each version's notes.
docs/context/         this working memory.   docs/research/   dated research, not rulings.
```

The crate names keep the `fm-` prefix for now, so formicaria's cutover changes as little as
possible. Renaming them was put off until after Phase D (`plan.md`, *Questions for the owner*);
Phase D is done, and no rename has been decided.

## How Myrme meets formicaria

- **Over HTTP, for vault data.** The runner reads and writes vaults only through formicaria's
  `fm-serve` at `127.0.0.1:8765/api/<cmd>`, sending `X-Formicaria-Agent: 1` (`fmserve.rs`,
  `VaultAccess`). It never touches a vault's files.
- **As a library, for hosting.** formicaria's `fm-serve/src/agent.rs` and the phone's
  `mobile/src-tauri/src/agent.rs` link these crates behind the `agent` feature, to provision models,
  start `agent-serve` and run the watch loop. The items they name are the contract.
- **Through notes.** Replies in discussion threads, proposals (a branch plus a proposal note), and
  meeting notes. formicaria's UI renders them; Myrme owns no UI.
