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

**Status:** moved out of the formicaria repo on 2026-10-18 with its history. formicaria still ships
its own copy until the cutover in [`docs/context/plan.md`](./docs/context/plan.md).

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
