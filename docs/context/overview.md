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
| **Myrme light** — dedicated tools (OCR, extraction, embeddings on ONNX Runtime) and a very small LLM | `../research/2026-10-19-myrme-light.md` (proposed plan, phases L0–L7) |
| Running the assistant by hand, benchmarks | `../../agents/README.md` · `../../agents/bench/` |

## What is here

```
crates/fm-agent       the pipeline: deterministic orchestration, meeting pass, transcription,
                      watchdog, launcher. No fm crate and no network crate. ~6.1k lines, 122 tests.
crates/fm-agent-run   the runner: agent-serve (supervisor + @Myrme watcher), agent-chat,
                      the model catalogue, the downloads (feature `download`, which uses
                      formicaria's fm-fetch pinned by tag). ~5.1k lines, 44 tests (56 with download).
agents/               models.toml (the catalogue formicaria ships), scripts, the bench harness.
                      runtime/ and models/ are fetched and gitignored.
docs/context/         this working memory.   docs/research/   dated research, not rulings.
```

The crate names keep the `fm-` prefix for now, so formicaria's cutover changes as little as
possible. Renaming them is an open question in `plan.md`.

## How Myrme meets formicaria

- **Over HTTP, for vault data.** The runner reads and writes vaults only through formicaria's
  `fm-serve` at `127.0.0.1:8765/api/<cmd>`, sending `X-Formicaria-Agent: 1` (`fmserve.rs`,
  `VaultAccess`). It never touches a vault's files.
- **As a library, for hosting.** formicaria's `fm-serve/src/agent.rs` and the phone's
  `mobile/src-tauri/src/agent.rs` link these crates behind the `agent` feature, to provision models,
  start `agent-serve` and run the watch loop. The items they name are the contract.
- **Through notes.** Replies in discussion threads, proposals (a branch plus a proposal note), and
  meeting notes. formicaria's UI renders them; Myrme owns no UI.
