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

## Releases and documentation

- **[Releases](https://github.com/formicaria-org/myrme/releases)**: each version's page says what
  changed for people using formicaria, what changed for formicaria, and how it was checked. A version
  is released once CI has published its page, after the gate and the integration test pass on its
  tag. Notes are written in [`docs/releases/`](./docs/releases/).
- **[Which formicaria links which Myrme](./docs/context/releases.md)**, with the contract version
  each needs. formicaria links Myrme by release tag and checks the contract when it builds, so people
  only ever update formicaria.
- **[The user manual's assistant chapter](./docs/manual/assistant.md)** is written here and rendered
  in [formicaria's manual](https://formicaria-org.github.io/formicaria/manual/user/assistant.html).
- **[The contract with formicaria](./docs/context/contract.md)**: what formicaria may rely on.
- **[The methods](./methods/)**, each with its evaluation and results (`methods/*/METHOD.md`).
- The model runtime is built here with llguidance for every platform (`runtime-llg-*` releases,
  [`.github/workflows/runtime.yml`](./.github/workflows/runtime.yml)).

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
