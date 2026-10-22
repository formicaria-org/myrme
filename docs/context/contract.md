# The contract with formicaria

Everything formicaria depends on, in four parts. **Changing any of it breaks formicaria's `main`
unless formicaria changes first.** The Rust part is compile-checked by
`crates/fm-agent-run/tests/fm_contract.rs`. The other three are checked against a real `fm-serve`
built from the pinned tag by `pixi run integration` (`tests/integration_fm.rs`), so far for
proposals, replies and the recipe headings.

_Taken from formicaria at tag v0.9.2 (commit e670190) on 2026-10-18; the Rust items did not change from v0.9.1._

## Versions

Two numbers, kept apart:
- **The release version** (`fm_agent_run::VERSION`, `Cargo.toml`, the `v…` tag) changes with
  every release.
- **The contract version** (`fm_agent_run::CONTRACT_MAJOR`.`CONTRACT_MINOR`, now **1.7**) changes
  only when this document does.
  - **Major:** anything listed below is renamed, removed or changes meaning. formicaria checks the
    major at compile time, so an app written for another major refuses to build and says why. It
    never builds and misbehaves.
  - **Minor:** something is only added. An app written for 1.0 links 1.x unchanged.

So formicaria can take any newer Myrme release of the same contract major by changing its tag and
nothing else. That is how methods, prompts, evals, models and the runtime reach users without a
formicaria code change. `docs/context/releases.md` lists every release with its contract version.

The list in part 1 is compile-checked as contract 1.7 (`tests/fm_contract.rs`). Changing it means
changing the contract version in the same commit.

## 1. Rust items formicaria names

To refresh this list, run in a formicaria checkout (`../formicaria`):

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
| `fm_agent_run::label::{read_with, ready_json_with}`, `fm_agent_run::ocr::{shared, ReadPage}` (1.7) | desktop + phone: a label read by the OCR first |
| `fm_agent_run::manifest::{Manifest, Manifest::read, Manifest::platform_key, whisper_language}` | the catalogue |
| `Manifest::phone_choices`, `Model::{phone_label, bytes, mmproj_bytes}` (1.5) | phone: the model chooser (shown only with two or more choices; one since 2026-10-20) |
| `fm_agent_run::nativelib::native_lib_dir` (Android only) | phone: where the bundled runtime lives |
| `fm_agent_run::watch::{serve_loop, wait_ready}` | phone: the watch loop |

**Contract 1.7 adds labels read by the OCR** (Myrme 0.8.0):
- `ocr::shared(dir, manifest, bundle)`: the one OCR of the process, opened from the tools dir the
  first time it is asked for; the launchers' `Agent::ocr` comes from it too, so the models are loaded
  once. `ReadPage::read_turned`: the picture read turned (a provided method; a reader that cannot
  turn says so).
- `label::read_with(ocr, reader, image)` and `label::ready_json_with(ocr, reader)`: a label read by
  the OCR — turned when upright holds too few figures — and by a vision model fit for labels only
  when the OCR found too little. The answer has the shape `read`'s has (`text`, `model`), so the
  page reads it as before; `read` and `ready_json` are unchanged.

**Contract 1.6 adds the OCR of Myrme light** (Myrme 0.7.0):
- `host::BUNDLED_ONNXRUNTIME`: ONNX Runtime's name in the APK, which formicaria's packaging stages
  from the catalogue's `runtime_url_onnxruntime_android_arm64` (part 3).
- `Agent::ocr`: the OCR, which both launchers fill; formicaria builds no `Agent` itself.

**Contract 1.5 adds the phone's model choice and per-model settings** (Myrme 0.6.0):
- `Manifest::phone_choices` and `Model::phone_label`: the models a phone may choose between, each with
  the few words its chooser shows.
- `Manifest::add_template_flags` (`thinking = false`) and `Manifest::shared_mail_prompt`, with the
  field `Agent::shared_mail_prompt`: settings both launchers apply from the catalogue.
- Catalogue keys `phone_label`, `thinking`, `shared_mail_prompt` (part 3).

**Contract 1.3 adds the phone's host and the runtime refresh** (Myrme 0.4.0):
- `Bundled`, with `availability`, `BUNDLED_SERVER` and `BUNDLED_WHISPER`: a runtime an APK
  carries.
- `Tools::provision_weights`: weights and projector, no runtime.
- `Tools::installed_for`: against a phone's default.
- `Tools::run_in_process` with `InProcess` and `Hooks`: the phone's whole launch.
- `Tools::runtime_outdated` and `Tools::refresh_runtime` (1.2): an app update brings the runtime
  its catalogue pins.

With these, formicaria uses no 1.0 item for downloads, installs, launches or the run loop. What it
still names is the catalogue (`Manifest`), its vault seam (`fmserve`), the label reader and image
reader record, dictation (`transcribe`), `device_word`/`MYRME`, `StopFlag` (in `Hooks`), and one
memory-reading test. The 1.0 items nothing uses now — `fetch::*`, `installed::{sweep, sweep_whisper,
whisper_on_disk}`, `watch::*`, `nativelib`, `launch`, `preflight` — can go in **contract 2.0**,
the next breaking release.

**Contract 1.1 adds the desktop host's facade, `fm_agent_run::host`** (feature `download`):
- `Tools` over the one folder the app gives Myrme: `catalogue`, `installed`, `is_provisioned`,
  `bytes`, `remove`, `provision`, `provision_speech`, `speech_staged`, `speech_fetchable`,
  `speech_wanted` and `launch`;
- `Stage`, `Progress` and `Launch`;
- `unsupported()`, `whisper_runtime_key()`, `SERVER_BIN`, `WHISPER_BIN`, `WHISPER_PORT`.

formicaria's desktop host uses these from Myrme 0.3.0. The 1.0 items it used before stay, because
the phone still uses them. When the phone moves to a facade of its own, the ones nobody uses are
removed, and that is contract 2.0.

**Versions (contract 1.0):** `fm_agent_run::{VERSION, CONTRACT_MAJOR, CONTRACT_MINOR}`. formicaria
checks the contract at compile time and reports the version.

**Added for formicaria's cutover (2026-10-18)**, used from the release after it:
- `fm_agent_run::CATALOGUE`: the catalogue, embedded. The phone parses it, and the desktop writes
  it beside the program.
- `fm_agent_run::agent_serve::main`: `agent-serve`'s entry point, so formicaria's own three-line
  crate ships the same binary.
- `agent-serve --print-catalogue`: prints the catalogue.

**Plan:** collapse these into a small facade (`plan.md`, Phase D) so the insides can change freely.

## 2. The HTTP door Myrme calls

`POST http://127.0.0.1:8765/api/<cmd>`, sending `X-Formicaria-Agent: 1` so formicaria's idle
watchdog ignores the agent's polling. The commands are whatever `fmserve.rs` posts; grep
`post(`/`"/api/` there for the current list. formicaria's `dispatch.rs` match is the truth on its
side. The host-side routes (`/api/agent_status`, `agent_models`, `agent_activity`,
`agent_activity_poll`, `agent_present`, `agents`) belong to formicaria; Myrme's runner reports to
some of them.

## 3. The catalogue

`crates/fm-agent-run/models.toml` (it was `agents/models.toml` until 2026-10-18), embedded as
`fm_agent_run::CATALOGUE` and parsed by `Manifest::parse`. formicaria's release copies it into every
desktop archive (`.github/workflows/desktop.yml:141`, `packaging/formicaria.sh:42`) and stages the
Android runtime from it (`ci/android-stage-runtime.sh`). **Its keys are a format formicaria reads.**
Adding a key is safe; renaming or removing one is a contract change. Added since the
extraction: `image_max_tokens` (per model, 2026-10-18); `phone_label`, `thinking` and
`shared_mail_prompt` (per model, 2026-10-19), read through `Manifest::phone_choices`,
`Manifest::add_template_flags` and `Manifest::shared_mail_prompt`. `Agent` gained the field
`shared_mail_prompt` (2026-10-19), an addition. **Contract 1.4** (Myrme 0.5.0) adds the
phone's runtimes, which formicaria's packaging reads from here instead of keeping its own copy:
`runtime_url_android_arm64` with `runtime_sha256_android_arm64` (staged into the APK by
`ci/android-stage-runtime.sh`) and `whisper_android_source`, the whisper.cpp tag
`ci/android-stage-whisper.sh` compiles. **Contract 1.6** (Myrme 0.7.0) adds the OCR's ONNX Runtime
for the phone: `runtime_url_onnxruntime_android_arm64` with `runtime_sha256_onnxruntime_android_arm64`,
Microsoft's AAR, whose `jni/arm64-v8a/libonnxruntime.so` formicaria's packaging stages into the APK
under `host::BUNDLED_ONNXRUNTIME` (`ci/android-stage-onnxruntime.sh`). The other OCR keys
(`ocr_*_<part>`, the desktop's `runtime_url_onnxruntime_<platform>`, `onnxruntime_lib_<platform>`) are
read by Myrme only.

## 3a. The user manual's assistant chapter (contract 1.4)

`docs/manual/assistant.md` is **the** chapter on the assistant in formicaria's user manual. formicaria
keeps no copy: its `ci/docs.sh` renders this file, read at the release it pins, as
`user/assistant.md` of its book. So:
- its links to other pages of the book (`./collaboration.md`) are relative to formicaria's
  `docs/src/user/`;
- formicaria's app links into it by heading (`<ManualLink page="user/assistant.html"
  section="turn-it-on">`, checked by formicaria's `ci/checks.sh`): **renaming a heading formicaria
  links to is a contract change**, like renaming an item in part 1.

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
