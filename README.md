# Myrme

The on-device assistant of [formicaria](https://github.com/formicaria-org/formicaria), a
local-first research notebook. Myrme is built for **small models that run on your own laptop or
phone**: nothing leaves the device unless you ask it to search the web.

Each task gets a dedicated method instead of one general agent:

- meetings and tasks from email
- handwriting, boards and photographed pages to text
- speech to text and dictation
- web research with checked citations
- note edits
- food and recipes

Whatever Myrme produces arrives in formicaria as a **proposal** you accept or reject.

**Status (2026-10-18):** `v0.1.0`.
- Every method is measured on its own cases (`methods/*/METHOD.md`).
- The model runtime is built here with llguidance for every platform (release `runtime-llg-3`).
- formicaria's switch to linking this repository by tag is rehearsed and green on a branch. It
  ships from the formicaria release that merges it; until then formicaria ships its own copy. See
  [`docs/context/plan.md`](./docs/context/plan.md).

## Build and test

Everything runs through [pixi](https://pixi.sh):

```
pixi run ci          # fmt, clippy, tests (with and without the download feature), cargo-deny
pixi run build       # release build of agent-serve and agent-chat
```

## Run it against formicaria

Running it by hand needs four processes; see [`agents/README.md`](./agents/README.md).
formicaria's own app starts and supervises Myrme itself when the assistant is turned on in Settings.

## Licence

MIT. The model weights Myrme downloads carry their own licences, which are stated before anything
is fetched (`agents/models.toml`).
