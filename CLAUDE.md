# CLAUDE.md — read before working in this repo

**Myrme is formicaria's on-device assistant, built for small models that run locally.** It left the
formicaria repo on 2026-10-18 with its history, so the owner can work on it at its own pace. The
work is **dedicated methods for dedicated tasks**. formicaria (`../formicaria`, beside this checkout in `formicaria-org/`,
https://github.com/formicaria-org/formicaria) is the notebook Myrme serves. It links these crates
behind its `agent` feature and must keep building at every step.

**First action in any session:** read [`docs/context/overview.md`](./docs/context/overview.md).
Its router names the file to open before you touch each area. The working plan is
[`docs/context/plan.md`](./docs/context/plan.md).

## The one rule that protects formicaria

**Nothing here may break formicaria's `main`.** In practice:
- **The contract** is [`docs/context/contract.md`](./docs/context/contract.md) and its compile check
  [`crates/fm-agent-run/tests/fm_contract.rs`](./crates/fm-agent-run/tests/fm_contract.rs).
  - Renaming, moving or hiding an item listed there breaks formicaria.
  - So does changing the `/api` calls, the `models.toml` format or the note conventions the
    contract lists.
  - Such a change needs a matching formicaria change landed first, then an entry in
    `docs/context/decisions.md`.
- **formicaria carries no copy of these crates any more** (cutover, 2026-10-18). It links a
  Myrme release by tag and checks the contract version at compile time; `releases.md` says which
  formicaria links which release.
- **A release** follows `docs/context/releases.md`. An addition to the contract is a new minor; a
  removal, rename or change of meaning is a new major, and needs formicaria's change ready first.
- **formicaria pins Myrme by tag, never by branch.** A tag formicaria pins is never moved or
  deleted.

## House rules

- **Toolchain is pixi-only.** Use `pixi run <task>`: `test`, `test-download`, `clippy`, `fmt`,
  `deny`, `build`. **`pixi run ci` is the single gate**, and GitHub runs exactly that. Keep it
  green.
- **Commit only when the owner asks.** Work on `main`, with no branch or PR ceremony.
- **In this environment, shell commands need `dangerouslyDisableSandbox: true`.** Sandboxed Bash
  fails with a seccomp/`setgroups` error.
- **Remote (cloud) sessions cannot push or open PRs here.** The GitHub account in a remote run is
  not the one that owns this repo. Commit locally if useful, finish with a patch
  (`git format-patch origin/main -o <scratchpad>`), and hand back its path. Publishing is the
  owner's local step.
- **The agent cap from the owner's global notes applies here too:** at most 5 subagents and 1M
  tokens per run, stated before launch.

## Design rules (from formicaria's rulings, kept until a dated entry here reverses one)

1. **Code selects the method.** A command, a file's type, a schedule or a UI surface picks it, never
   a model choosing among tools (formicaria `decisions.md#agent`, 2026-10-10).
2. **The model points; Rust reads and computes.** Model output is grammar-constrained to spans, ids
   or enums wherever the task allows.
3. **Everything the assistant produces is a proposal** that a person accepts. A person's own action
   writes directly.
4. **Nothing is downloaded unasked.** A model or pack is offered with its size and licence first.
5. **A method ships with an eval:** exact-match on its structured output, including cases where the
   right answer is "nothing".

## Before you add anything

Ask the same four questions formicaria asks:
1. Which subject is it? Grep `docs/context/decisions.md`.
2. Read the entries the subject names, and read past them.
3. Classify it honestly: permitted, extends an exception, or contradicts a standing decision.
4. Does it widen what ships to formicaria's users?

If it contradicts a decision, **write the dated reversal first**.

## Keep the context current

When you finish substantive work:
- update `plan.md`'s status lines;
- append to `decisions.md` if you decided or reversed something;
- update `contract.md` if the surface formicaria uses changed.

Research goes in `docs/research/` with a date. It is evidence, not a ruling.
