# agents/ — the local study-assistant runtime (off by default, gitignored)

This folder holds everything the local agent needs to *run*, kept out of the core app and out of git
history. Only the **manifest and scripts** commit; the runtime binary and model weights are fetched
on demand and ignored (see `.gitignore`). Delete this folder and the app is byte-identical — the
agent's only durable output is an ordinary git branch + a proposal note (see
`docs/context/ai-agents-plan.md`).

## Get a model

```
pixi run fetch-model              # the default (lightest) model from models.toml
pixi run fetch-model lfm2.5-350m  # a named model
```

This downloads a pinned prebuilt `llama-server` into `runtime/` and the chosen GGUF into `models/`
(both gitignored), then prints the exact command to serve it. Everything lands **inside this folder**
— nothing is written elsewhere.

## Run it

The fetch prints the serve command; it's a bounded, localhost-only `llama-server`. The agent
(`crates/fm-agent`) then talks to it over `OpenAiStep::local(<port>, <model>)`, and its process is
meant to be run under the `SupervisedModel` launcher so the watchdog can stop it if the device is
pushed. See `crates/fm-agent/examples/smoke.rs` for a runnable end-to-end orchestration check.

## The philosophy

**Stay on the lightest model.** `models.toml`'s default is the smallest that works; quality is meant
to come from *context* (the user's own notes + clean web text — RAG) and a good search, not from a
bigger model. The larger entries are deliberate, measured step-ups, never the default.
