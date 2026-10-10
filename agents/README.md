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
pixi run fetch-model              # the default (lightest) model from models.toml
pixi run fetch-model lfm2.5-1.2b  # a named model
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
@lfm2.5-1.2b does mRNA change DNA? /search
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
- **Context, not size:** an answer draws on the host note, the notes it links to, and web results
  when asked. It does not search the rest of the vault.

## The philosophy

**The smallest model that does the job.** `models.toml` names one default per kind of device, each
chosen by measurement on that device (`agents/bench/results.md`). Changing a default is a
decision: after an update the app offers the new model and never downloads it unasked.
