# agents/ — the local study-assistant runtime (off by default, gitignored)

This folder holds everything the local agent needs to *run*, kept out of the core app and out of git
history. Only the **manifest and scripts** commit; the runtime binary and model weights are fetched
on demand and ignored (see `.gitignore`). Delete this folder and the app is byte-identical — the
agent's only durable output is an ordinary git branch + a proposal note (see
`docs/context/decisions.md`, `#agent`).

**The app does all of this by itself.** Turning the assistant on in Settings downloads the runtime
and a model, starts them, and searches the web in-process. What follows is the developer route: the
same pieces staged by hand in a checkout, which the app then prefers.

## Get a model

```
pixi run fetch-model              # the catalogue's `default`, the laptop's model (qwen3-vl-4b)
pixi run fetch-model qwen3.5-0.8b # a named model: here the phone's, the lightest default
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
@Myrme does mRNA change DNA? /search
```

`@Myrme` asks the copy on the device the message was written on; `@Myrme·phone` asks one by name,
and the model's own name still works. It replies in the thread. Commands: `/search` (web),
`/research` (grounded research), `/propose` (in a note's discussion, draft an edit — it lands as a
reviewable proposal branch, never `main`), `/transcribe` (images and recordings in the note),
`/recipe` (`/ricetta`). Everything goes through fm-serve's
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
- **Context, not size:** an answer draws on the host note, the notes it links to, and web results
  when asked. It does not search the rest of the vault.

## The philosophy

**The smallest model that does the job.** The catalogue (`crates/fm-agent-run/models.toml`) names one
default per kind of device, each chosen by measurement on that device (`agents/bench/results.md`,
the method evals and `docs/research/2026-10-20-small-model-benchmark-summary.md`). Changing a default is a
decision: after an update the app offers the new model and never downloads it unasked.
