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

## Use it (talk to the assistant inside formicaria)

Four processes — the model and its helpers run *out of process*; formicaria never runs them.

```
pixi run serve                               # formicaria (fm-serve) — the single vault writer
pixi run search-proxy                         # web search (keyless, another terminal)
pixi run agent-serve -- --searxng-port 8888   # the agent: model (warm) + @name watcher
```

Then **in any discussion in the app**, mention the agent:

```
@lfm2.5-230m does mRNA change DNA? /search
```

It replies in the thread. Commands: `/search` (web), `/propose` (in a note's discussion, draft an
edit — it lands as a reviewable proposal branch, never `main`). Everything goes through fm-serve's
API, so there is **no second store**; the agent's messages + proposals appear in the app instantly.

For a terminal chat against a specific note instead:

```
pixi run agent-chat -- --note <ULID> --searxng-port 8888
```

### How it behaves

- **Warm session, fastest on a weak device:** the model loads once and stays warm; each turn is a
  fast call. It is **tied to formicaria's life** — when the app closes (fm-serve stops answering),
  `agent-serve` stops the model and exits. No orphan, zero idle cost.
- **Watchdog-guarded:** the model runs under preflight + a resource watchdog that kills it if the
  device is pushed. `Ctrl-C` on `agent-serve` stops everything.
- **Off = pure formicaria:** don't run `agent-serve` and the app is byte-identical, super-light.
- **RAG on the tiny model:** quality comes from context — the host note + relevant notes (FTS) + web
  results — not model size. The 230M default is a *helper*; step up via `models.toml` if you must.

## The philosophy

**Stay on the lightest model.** `models.toml`'s default is the smallest that works; quality is meant
to come from *context* (the user's own notes + clean web text — RAG) and a good search, not from a
bigger model. The larger entries are deliberate, measured step-ups, never the default.
