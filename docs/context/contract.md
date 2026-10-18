# The contract with formicaria

Everything formicaria depends on, in four parts. **Changing any of it breaks formicaria's `main`
unless formicaria changes first.** The Rust part is compile-checked by
`crates/fm-agent-run/tests/fm_contract.rs`. The other three are checked against a real `fm-serve`
built from the pinned tag by `pixi run integration` (`tests/integration_fm.rs`), so far for
proposals, replies and the recipe headings.

_Taken from formicaria at tag v0.9.2 (commit e670190) on 2026-10-18; the Rust items did not change from v0.9.1._

## 1. Rust items formicaria names

To refresh this list, run in `~/formicaria`:

```
git grep -hoE "fm_agent(_run)?::[A-Za-z_]+(::[A-Za-z_]+)*(::\{[^}]*\})?" <tag> -- crates/fm-serve/src mobile/src-tauri/src | sort -u
```

| Item | Used by |
|---|---|
| `fm_agent::convo::{MYRME, device_word}` | desktop + phone: Myrme's name and the device's word |
| `fm_agent::launch::SupervisedModel` | desktop: the supervised model server |
| `fm_agent::preflight::Need` | desktop: what a launch needs |
| `fm_agent::transcribe` | desktop + phone: audio and image capability |
| `fm_agent::watchdog::{Limits, StopFlag, SystemMonitor, ResourceMonitor::sample}` | the live memory reading that decides support |
| `fm_agent_run::{Agent, ImageReader}` | phone: the in-process agent |
| `fm_agent_run::fetch::{ensure_model, ensure_mmproj, ensure_runtime}` (feature `download`) | first enable, both platforms |
| `fm_agent_run::fmserve::{Origin, VaultAccess}` | phone: the agent's way into the vault |
| `fm_agent_run::installed::{Installed, resolve, sweep, sweep_whisper, whisper_on_disk}` | Settings: what is on disk, handing disk back |
| `fm_agent_run::label::{read, ready_json}` | Settings: the model's label |
| `fm_agent_run::manifest::{Manifest, Manifest::read, Manifest::platform_key, whisper_language}` | the catalogue |
| `fm_agent_run::nativelib::native_lib_dir` (Android only) | phone: where the bundled runtime lives |
| `fm_agent_run::watch::{serve_loop, wait_ready}` | phone: the watch loop |

**Plan:** collapse these into a small facade (`plan.md`, Phase D) so the insides can change freely.

## 2. The HTTP door Myrme calls

`POST http://127.0.0.1:8765/api/<cmd>`, sending `X-Formicaria-Agent: 1` so formicaria's idle
watchdog ignores the agent's polling. The commands are whatever `fmserve.rs` posts; grep
`post(`/`"/api/` there for the current list. formicaria's `dispatch.rs` match is the truth on its
side. The host-side routes (`/api/agent_status`, `agent_models`, `agent_activity`,
`agent_activity_poll`, `agent_present`, `agents`) belong to formicaria; Myrme's runner reports to
some of them.

## 3. The catalogue

`agents/models.toml`, parsed by `Manifest::parse`. formicaria's release copies it into every
desktop archive (`.github/workflows/desktop.yml:141`, `packaging/formicaria.sh:42`) and stages the
Android runtime from it (`ci/android-stage-runtime.sh`). **Its keys are a format formicaria reads.**
Adding a key is safe; renaming or removing one is a contract change. The plan moves it next to the
crate (`plan.md`, Phase A), so formicaria stops naming the path `agents/`.

## 4. The notes Myrme writes

- **Replies** in discussion threads, under the device's Myrme name.
- **Proposals:** a branch plus a proposal note. Only allow-listed properties may be proposed (in
  formicaria, `apply_props`: `start`, `due`, `location`, tags, and `title` since 2026-10-15).
- **Meeting notes** from the mail pass.
- **Recipe notes** (`/recipe`) carry the recipe template's sections: `## Ingredients`, `## Method`,
  `## Times`, `## Notes` (`fm_agent::recipe::TEMPLATE_HEADINGS`).
  - formicaria defines them in `ui/src/lib/templates.ts`.
  - Myrme's unit test checks the note; `pixi run integration` checks formicaria's template at the
    pin.

formicaria's rulings (`decisions.md#agent`, `#git`) govern how these are reviewed and merged.
