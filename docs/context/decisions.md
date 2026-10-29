# Decisions — the *why*

An append-only log: a decision is superseded by a new dated entry, never edited away. Each heading
carries a `#subject`: `#contract` (what formicaria depends on), `#method` (a dedicated task),
`#model` (catalogue and runtimes), `#repo` (toolchain, CI, layout). Grep a subject.

## 2026-10-18 — Myrme lives in its own repo; formicaria keeps building throughout `#repo` `#contract`

**What prompted it.** The owner: *"move the agentic part out of the fm repo since I want to focus
more on it… dedicated methods for dedicated tasks"*, and *"make sure we do not break current main
fm. We need to act in parallel."* The repo is public so that CI runs free and so that formicaria,
itself public, can pin it.

**Decision.**
- `crates/fm-agent`, `crates/fm-agent-run` and `agents/` were extracted from formicaria at v0.9.1
  with their history (`git filter-branch --index-filter`, 78 commits). **The layout is kept
  exactly**, so the extraction changes no code and patches move between the two repos unchanged.
- `fm-fetch` stays in formicaria (`fm-app` and the updater use it too). Myrme takes it **by tag**,
  never by branch.
- The surface formicaria uses is the contract (`contract.md`), and part of it is compile-checked
  (`tests/fm_contract.rs`).
- Until formicaria's cutover (`plan.md`, Phase D), its in-tree copy takes fixes only, mirrored
  here the same day.

**Consequence.** All new agent work happens here. formicaria's `main` changes only by its own
commits, each checked by its own gate.

## 2026-10-18 — the plan's defaults are kept `#repo` `#method`

**The owner:** *"commit and push the scaffold, keep the defaults."* So:
- **The crates keep their `fm-agent*` names until formicaria's cutover** (`plan.md`, Phase D).
  Renaming would widen that commit.
- **formicaria's in-tree copy takes fixes only until the cutover**, and each fix is mirrored here
  the same day.
- **Pose coaching is not a Myrme method.** It is a real-time camera screen in formicaria that
  writes `log: workout` notes.

## 2026-10-18 — a method is scored by exact match on its own cases; the meeting pass is held to its schema `#method` `#model`

**What prompted it.** Phase B of `plan.md`: before any method changes, measure it. The first
measurement was M1, the meeting pass, on the laptop's two models.

**Decision.**
- **Every method gets `methods/<name>/eval/cases.json`**: invented inputs, the lines a careful
  person would expect, a hand-written perfect answer (`oracle`), and real models' answers
  (`recorded`).
  - **The gate replays them**: each oracle must resolve to its case's expectation, which checks the
    cases and the Rust half of the method together, and every recording is re-scored.
  - Live runs are `#[ignore]`d tests that can write their answers back.
  - Two numbers are kept apart: *well-formed* (exactly the JSON asked for) and *exact* (the right
    meetings). Cases whose right answer is nothing count like any other. Scoring lives in
    `fm_agent::eval`.
- **`LlmStep::complete_constrained`** holds an answer to a JSON Schema while it is generated
  (`response_format`, which `llama-server` compiles into a grammar).
  - It has a default that ignores the schema, so no implementor breaks and nothing in the contract
    changes.
- **The meeting pass uses it.** Measured on `methods/meetings/eval` (14 cases, 2026-10-18):

  | Model | Free | Held to the schema | Time |
  |---|---|---|---|
  | qwen3-4b-2507 | 8/14 | 14/14 | 15.6 s → 21.8 s |
  | qwen3-vl-4b (laptop default) | 11/14 | 12/14 | 58.3 s → 31.1 s |

  - Free answers were always valid JSON. They failed by **leaving out `quote`**, and a meeting with
    no pointer is dropped. Making every key required is the fix.
- **A quote of several sentences is read in the one the model's own date or time words point at**
  (`meetings::narrow_to_sentence`). A model quoted two meetings' sentences together for both, and
  both got the first one's date.
  - This keeps the rule that the date comes from the sentence and not from the model. It only
    chooses *which* sentence.

**Consequence.** Users get this only through formicaria's cutover (`plan.md`, Phase D). The phone
model is not measured yet. The schema's keys reach the server alphabetically (`serde_json` sorts
maps), so their order is not something this decision controls.

## 2026-10-18 (later) — the open items: questions, the phone model, llguidance, and two methods measured `#method` `#model` `#repo`

**The owner:** *"fix the open things first and then move on."*

**Decision.**
- **A question nobody answered fixes no meeting.** `meetings::resolve` drops a quote ending in `?`
  when no later email from anyone else exists (`Dropped::Unanswered`). Once someone has replied,
  reading the reply is the model's job. This was checked on three new cases (question accepted,
  declined, unanswered). The meeting eval is now 17 cases.
- **The meeting pass stays off the phone.** `lfm2.5-vl-450m` scores 5/17: it calls nearly every
  meeting cancelled. A phone-sized model must pass the eval before the pass runs there.
- **No llguidance build.** It was measured on the CPU at the same llama.cpp commit (b10076):
  16/17 in 92 s against the stock engine's 17/17 in 115 s. Every schema used here compiles with
  the stock engine. Revisit if one does not, or if phone speed needs it.
- **cargo-deny waives nothing.** Myrme has no YAML parser, and its lockfile resolved `zip`
  2.6.1, which formicaria's three `zip` waivers do not match.
- **Research (M2) gets an eval** (8 cases about invented things, scored *exact* and *grounded*).
  `grounding::verify` now:
  - reads a citation placed on the quote line;
  - credits an uncited claim to the single source holding its quote;
  - ignores a quote's closing punctuation.

  Both laptop models went from 5/8 to 7/8. A claim still ships only with a quote found word for
  word in its source.
- **Note edits (M5) are anchored edits, applied by Rust to the whole note** (`fm_agent::edits`).
  This replaces the model writing the whole new body:
  - With the old format both models returned only the changed part, so an accepted proposal would
    have deleted the rest of the note, and the model saw only the note's first 1200 characters
    anyway.
  - With anchored edits both models score 5/6, up from 3/6.
  - `Turn` now carries `edits` instead of `proposal`. The runner applies them, proposes only when
    something applied, and otherwise says it could not place the change.
  - `Turn` is not in the contract (`contract.md`); formicaria's copy is unaffected.
- **Methods are a closed `enum`** (`fm_agent::method::Method`), each with `methods/<name>/METHOD.md`
  in Agent Skills format. A test fails if a method has no folder or a folder is not a method.

## 2026-10-18 (evening) — the pin follows formicaria's release; its agent changes are mirrored; a cross-repo check moves to the integration run `#contract` `#repo`

- **The pin is v0.9.2** (the owner: *"it's already 0.9.2"*). There is one pin: `fm-fetch`'s tag in
  `crates/fm-agent-run/Cargo.toml`, which `pixi run integration` also reads. The Rust contract
  items did not change from v0.9.1.
- **v0.9.2's agent changes were mirrored** (`1db54e2`, applied with `git am -3`, its author and
  message kept). That is the parallel-work rule from `plan.md` working as intended: a change made
  in formicaria's copy arrives here unchanged.
- **The recipe-heading check is split across the two repos.**
  - That test read `ui/src/lib/templates.ts` from formicaria's tree, which does not exist here.
  - The headings are now `recipe::TEMPLATE_HEADINGS`, part of the contract (part 4).
  - The note is checked by a unit test here; formicaria's template is checked against the pinned
    checkout by `pixi run integration`.
- **`pixi run integration` is a CI job.** It builds `fm-serve` from the pin and checks a
  `/propose` round trip, an unplaceable edit and the recipe headings, with a stand-in model.

## 2026-10-18 (night) — the faster grammar engine on every platform: Myrme builds its own runtime `#model` `#repo`

> Reverses the llguidance point of *the open items* (same day): "No llguidance build".

**The owner:** *"I would like also to adopt the faster options if possible for all the platforms."*

**What was measured.** qwen3-4b-2507, 17 meeting cases, CPU, llama.cpp b10076: llguidance took
92 s against the stock engine's 115 s, about 20% faster, and scored 16/17 against 17/17. The one
difference was a judgment the model made either way, not a grammar failure. Speed matters most on
the phone, which runs on the CPU.

**Decision.**
- **Myrme publishes its own model runtime.** `.github/workflows/runtime.yml` rebuilds llama.cpp's
  release archives with `-DLLAMA_LLGUIDANCE=ON`, using upstream's own flags for each platform.
  - Files: Linux x64 (CPU, Vulkan), macOS arm64, Windows x64 (CPU, Vulkan), Android arm64.
  - **Upstream's file names and layout** are kept, so the catalogue and formicaria's Android
    staging change only a URL.
  - Each platform keeps its current version: b10076 on the desktops, b10081 on Android.
  - The Vulkan archives are the CPU archive plus upstream's unchanged Vulkan backend from the same
    tag. That is upstream's own composition for Windows, and it works because a backend is a
    separate library under `GGML_BACKEND_DL`.
  - Each build fails unless the llguidance symbols are linked.
  - Android needs a two-line patch: llama.cpp's `cargo build` of llguidance names no target.
- A `runtime-*` tag publishes a GitHub release with `SHA256SUMS`.
- **The catalogue switches only after measuring.** The eval is re-run here on the published Linux
  archives, on the CPU and on the Vulkan GPU, against upstream's. `runtime_url_*` and
  `runtime_sha256_*` then point at the release. formicaria gets it at the cutover, or earlier by
  moving its own pin.

**Cost, stated.** One more thing to maintain: a llama.cpp bump is now a Myrme release, not a URL
change. The build machines are GitHub's free public runners.

## 2026-10-18 (late) — what formicaria needs to link Myrme from here, and the runtime switched to Myrme's build `#contract` `#model` `#repo`

**The owner, leaving for some hours:** *"I want the full port and all outstanding issues solved in your
recommended way. This should then also lead to a test of fm with our myrme outside the main repo to
go towards this new setting from next releases."*

**Decision.**
- **The catalogue lives beside the crate that embeds it**: `crates/fm-agent-run/models.toml`,
  exported as `fm_agent_run::CATALOGUE`.
  - `agent-serve --print-catalogue` writes it out, and `agent-serve` uses it when no
    `models.toml` is on disk.
  - The dev scripts find it in its new place.
  - formicaria's phone parses the constant; its desktop release writes the file beside the
    program.
- **`agent-serve`'s code is in the library** (`fm_agent_run::agent_serve::main`).
  - Cargo builds binaries only for a workspace's own members, so formicaria ships the same binary
    from a three-line crate of its own.
  - Both additions are in `contract.md`. Nothing existing was renamed or removed.
- **The runtime is Myrme's own** (release `runtime-llg-3`, all six archives). The catalogue's
  `runtime_url_*` and `runtime_sha256_*` now point there, after measuring:
  - The published Linux Vulkan archive against upstream's, qwen3-4b-2507, meeting eval held to
    its schema, twice each on the RTX 3050: **17/17 both**, **21.7 s against 26.1 s on average**
    (about 17% less time). The CPU build measured earlier saved about 20%.
  - `pixi run check-pins` downloaded every pinned archive and matched every checksum.
  - The link check in the workflow looks for llguidance's own code (`toktrie`) and for the
    absence of llama.cpp's "not enabled" abort, because macOS and Windows export no `llg_*`
    symbol.
  - Android pins b10081 to match formicaria's staging; the desktops pin b10076.
- **Testing formicaria from here:**
  - `FM_CHECKOUT=<path> pixi run integration` builds a local formicaria checkout instead of the
    pin.
  - `agents/e2e-live.sh` runs everything live: that checkout's `fm-serve` and `agent-serve`, a
    real model on a real runtime, and `@Myrme-laptop … /propose` posted as a person would, in a
    temporary home.

## 2026-10-18 (late) — a cut note is marked; research answers say first whether there is an answer `#method`

- **The live run found a fault the eval had not.** Asked to "add a final line" to a long note, the
  model replaced the last sentence it could see; the edit applied, and a sentence was lost.
  - Cause: the turn shows the model the note's first 1200 characters with nothing saying more
    follows.
  - **`fm_agent::shown_note`** now adds "[… the note goes on; only its beginning is shown here …]"
    when it cuts. The runner and the eval both use it.
  - `EDITS_INSTRUCTION` now says that adding at the end is `append`, never a replace, and that a
    replace changes only the words that change.
  - A case shaped like the live failure joined the eval (`long-repetitive-final-line`).
  - Both laptop models now score **7/7**, up from 5/6. The `latex` case that had kept the score
    there is fixed by the second sentence.
- **Research (M2) is held to a schema whose first field is `answerable`**
  (`grounding::answer_schema`, `GROUNDED_JSON_INSTRUCTION`). The claims are turned into the list
  `grounding::verify` checks, so the guarantee is unchanged.
  - Both laptop models now score **8/8**, up from 7/8: the cited "the sources do not say" is gone.
  - A free-text answer is still read.
- **Tried and not adopted:** "two meetings in one email are two items" in the meeting instruction.
  - qwen3-vl-4b went from 15/17 to 16/17 (the merge was fixed); qwen3-4b-2507 went from 17/17 to
    16/17 (it reported the moved meeting's old time as cancelled).
  - That is 32 of 34 either way, so the wording was reverted. The recordings are of the shipped
    wording.

## 2026-10-18 (night) — the image, speech and recipe methods measured on openly licensed data `#method` `#model`

**Data.** Every source is pinned by revision and every file checked by sha256
(`agents/eval-data.py`). The case files are committed; the media stay in `.eval-data/`. Licences and
attribution sit beside each case file.
- GNHK: 30 phone photographs of handwriting (CC BY 4.0).
- Multilingual LibriSpeech (Italian) and LibriSpeech (English): 30 clips each (CC BY 4.0).
- Wikibooks Cookbook: 30 recipes turned into speech-like text (CC BY-SA 4.0, kept under its own
  licence).

**Decision.**
- **The laptop vision reader is capped at 1024 image tokens.** `image_max_tokens` is a new
  catalogue key; adding a key is safe for formicaria (`contract.md`, part 3).
  - Uncapped, every photographed page took longer than the 120 s reading limit with the projector
    on the CPU.
  - Capped, the median word error is 17.9% (24 of 30 pages under 50%) at about 35 s a page.
- **The phone's vision model cannot read handwriting** (median 82.7% with the house instruction,
  96.4% with its own, 9 pages looping). It stays the phone's typed-text reader; handwriting on the
  phone waits for a dedicated reader (Phase C).
- **Speech:** `ggml-base` gives 6.3% WER in English and 36.1% in Italian; *better listening*
  (`ggml-small`) gives 4.1% and 23.1% at about three times the time. Recorded for the owner, who
  decides whether formicaria suggests it by language.
- **Recipes stay free-form.** qwen3-vl-4b scored 28/30 (256 of 257 ingredients found, 1 invented)
  both free and held to `recipe::answer_schema()`, and the schema was slower. qwen3-4b-2507 scored
  26/30 both ways.

## 2026-10-18 (night) — an edit's kind is decided from the request alone; the evals run as production runs `#method`

**What the live run showed, the second time.** After the cut-note fix, `agents/e2e-live.sh` still
got a proposal that replaced "A sentence of field notes …, round 14." with "Next survey: 14
November". The model had matched the request's *14* to a sentence in the note, not to its end.

**Why the eval had missed it.** It gave the turn no discussion history and ran at temperature 0.
Production gives the person's message as history (`- @Myrme-laptop … /propose`) and runs at the
client's default, 0.2. With both mirrored, the eval reproduced the failure, and temperature 0 did
not cure it (5/7 for qwen3-4b-2507).

**Decision.**
- **The evals run as production runs.**
  - The edits eval passes the history line the runner builds, and uses the runner's sampling
    unless `FM_EVAL_TEMPERATURE` says otherwise.
  - The others will be brought to the same rule as they are next touched.
- **M5 first decides the kind of change from the request alone** (`EDIT_KIND_INSTRUCTION`,
  `edits::kind_schema`): `append`, `insert_after`, `replace`, `delete` or `several`.
  - The edits are then held to that op (`edits::answer_schema_for`); `several`, or no answer,
    allows all four.
  - It costs one short extra call. Without the note in view, the request is easy to classify.
  - At production settings: qwen3-4b-2507 **7/7** (6/7 without), qwen3-vl-4b **6/7**. Only `latex`
    is left on qwen3-vl-4b, the reformatting judgment it makes once the history is in view.
- **History carries messages without Myrme's own tokens** (`convo::plain`): no `@Myrme…` mention,
  model alias or command; anyone else mentioned stays.
  - The live run with qwen3-vl-4b appended "Next survey: 14 November **/propose**", copying the
    command from the history line.
  - With this, both laptop models score **7/7** at production settings, `latex` included.

## 2026-10-18 (night) — Myrme has a release version and a contract version; formicaria checks the contract `#contract` `#repo`

**The owner:** *"keep even a small backward compatibility as I envision a modular system: fm can work
with myrme versions that are different (unless new features are added and or old removed and
severely changed) … the user should not need to think about it and have the features available."*

**Decision.**
- **Two numbers** (`contract.md`, *Versions*):
  - The **release version** is `Cargo.toml`'s `version`, now 0.2.0. A `v…` tag must equal it, and
    CI refuses one that does not.
  - The **contract version** is `CONTRACT_MAJOR.CONTRACT_MINOR`, now 1.0. The major changes when
    anything the contract lists is renamed, removed or changes meaning; the minor when something
    is only added.
- **formicaria checks the contract at compile time.** It states the major it was written for and
  the least minor it needs. A Myrme of another major does not build into it, and says so; any
  newer release of the same major does, with no change but the tag. That is the compatibility the
  owner described, enforced where a mismatch cannot reach a user.
- **Why not more** — a runtime handshake, or a Myrme updated apart from formicaria: the app links
  Myrme, and `agent-serve` is built in the same release, so a shipped app always holds one
  matching pair. A runtime check becomes worth having only if Myrme is ever installed or updated
  on its own (for instance, a newer catalogue fetched by itself); it is noted in `plan.md`, not
  built.
- **`releases.md`** lists every release, its contract version, the formicaria that links it, and
  what changed for users.

## 2026-10-18 (night) — the desktop host's facade, contract 1.1 `#contract`

**The owner:** *"do the facade now."*

**Decision.**
- **`fm_agent_run::host`** answers what an app asks to host Myrme on a desktop, over the one folder
  it is given: what is installed, what it takes on disk, fetching it (with the app's progress and
  cancel), removing it, staging speech, and building the `agent-serve` command. There is also
  `unsupported()` for whether the machine can run it at all.
  - The logic and the messages a person reads were moved from formicaria's `fm-serve/src/agent.rs`
    unchanged, and its tests moved with it.
  - formicaria keeps what is its own: where the folder is, its settings file, the progress it
    shows, its cancel counter, the catalogue its release ships, and when it starts the assistant.
- **Contract 1.1, an addition.** The 1.0 items stay, because **the phone still uses them**. Its host
  is not moved yet: the phone shell cannot be built on this machine (no NDK), and 937 lines
  rewritten without a build would be a guess.
  - When the phone moves to a facade of its own, checked by formicaria's `android` workflow, the
    items nobody uses are removed. That is contract 2.0.
- **A bug found on the way.** formicaria's "remove the downloaded model" refused on every installed
  copy. It took a `models.toml` in the folder for a source checkout, but provisioning copies the
  catalogue into the per-user tools folder. `Tools::remove` decides "checkout" the way formicaria
  finds the folder: is it the per-user tools folder. A test holds it
  (`a_tools_folder_with_a_catalogue_copy_can_still_be_cleared`).
- **`pixi run integration` builds formicaria from inside its checkout**, so that checkout's own
  Cargo configuration applies, a developer's local `[patch]` of Myrme included.

## 2026-10-18 (night) — the phone's facade, and the runtime an update brings, contract 1.3 `#contract` `#model`

**The owner:** *"make sure formicaria is as lean as possible regarding the agentic features and relies
on myrme; make sure that we can update the formicaria easily for users as before … make sure it
works for all the OS and fix missing points."*

**Decision.**
- **The phone hosts Myrme through `host` too.** The phone shell can be built here after all:
  formicaria's pixi `android` environment carries the NDK in `.android/`.
  - `Bundled` is the APK's runtime and what Settings says about it.
  - `Tools::provision_weights` fetches the weights without a runtime.
  - `Tools::installed_for` resolves against the phone's default.
  - `Tools::run_in_process` is the whole launch: weights, best-effort projector, sweep, the model
    under the watchdog, speech beside it, the run loop. Its log lines are unchanged.
  - formicaria keeps its generation, stop switches, presence board and settings, passed as `Hooks`.
- **An app update brings the runtime its catalogue pins.**
  - A runtime on disk was never fetched again, so the llguidance runtime would never have reached
    anyone who had the assistant before it.
  - The model runtime now carries a record of its archive (`runtime/.runtime-sha256`). One with
    none, or another, is outdated (`Tools::runtime_outdated`).
  - `Tools::refresh_runtime` fetches and checks the new archive into `runtime-next/`, and only
    then copies it over and writes the record. A failure leaves the old runtime working.
  - formicaria runs it before the assistant starts. Checked live: a real download, the server
    replaced, the log kept.
- **Contract 1.3**, still an addition. The 1.0 items formicaria no longer uses are listed in
  `contract.md` for contract 2.0.

## 2026-10-19 — a version is released when its GitHub Release page is published `#contract`

**The owner:** *"I still see no release of myrme"* — *"which is wrong if one wants to ship based on
its releases."*

**Decision.**
- **A published GitHub Release page is what makes a Myrme version a release**, not the tag alone.
- CI publishes it (`agents/release-pages.sh`, the `release` job in `ci.yml`) only after the gate and
  the integration test pass on the tag. A tag whose CI fails gets no page.
- formicaria's `myrme-bump` and its release check take only versions with a published,
  non-prerelease page.
- The pages for v0.1.0 to v0.4.1, which were tags only, are filled in by the same job on `main`.
  Every one of those tags had passed CI.

**Why.** Tags alone left nothing visible on the Releases tab, and nothing to tell a version that
passed from one that was pushed and then failed.

## 2026-10-19 — the manual's assistant chapter and the phone's runtimes live here; formicaria keeps no copy `#contract`

**The owner:** *"make sure myrme releases are proper, with documentation and proper description"*;
*"the manual of ai part should be coming from there, not copied doubly on two sides"*; *"no code
duplication"*.

**Decision.**
- `docs/manual/assistant.md` is the assistant chapter of formicaria's user manual. formicaria
  renders it from the release it pins and keeps no copy (contract 1.4, part 3a).
- The phone's runtime archive with its hash, and the whisper.cpp tag the phone compiles, are
  catalogue keys. formicaria's Android packaging reads them instead of its own copy.
- Each release has notes in `docs/releases/<tag>.md`. The release page is those notes plus the
  compatibility row and links to the documentation at that tag. CI publishes no page without notes,
  and brings an existing page up to date when its notes change.

## 2026-10-19 — the phone gets a faster default and an accurate choice `#model` `#contract`

**The owner:** *"lets ship the speed plan for now and give as an option to the user this tradeoff.
Avoid adding massive text to explain, few words that a user can understand."*

**Decision.**
- **The phone's default is `lfm2.5-vl-450m-q4`**: the same 450M at Q4_K_M. On the Dimensity 7300 it
  writes 59 tok/s against Q8_0's 36, with the same scores on the four text evals
  (`docs/research/2026-10-19-speed.md` §2.2–2.3). It is a new catalogue entry, not a new file under
  the old name: a phone recognises its model by the file on disk, so a phone running
  `lfm2.5-vl-450m` keeps it and is *offered* the new default (the 2026-10-10 rule, `installed.rs`).
- **A phone may choose `qwen3.5-0.8b` instead** (Q4_0, Apache-2.0): meetings 13/17 against 5/17,
  edits 3/7 against 0/7, recipes 245 of 257 ingredients against 102. Slower on text (a recipe
  30.9 s against 14.0); photos capped at 1024 tokens read a handwritten page in a median 28 s.
- **Two catalogue keys (contract: additions).**
  - `phone_label`: the few words the phone's chooser shows — "Faster", "More accurate, slower".
    `Manifest::phone_choices` lists the models that carry one.
  - `thinking = false`: `Manifest::add_template_flags` passes `--chat-template-kwargs
    {"enable_thinking":false}` on the phone and the desktop alike. Qwen3.5 thinks by default and
    otherwise answers nothing.
- **The phone's chooser is formicaria's next change.** Until it lands, a phone runs the default and
  the choice exists only in the catalogue.
- **Not shipped as a global change: prompt layout B** for the mail pass (see the evening entry: it
  became a per-model setting). It helped qwen3-vl-4b (15 → 16/17) and the Q8_0
  450M, but cost the two phone models meetings cases (450M Q4_K_M 5 → 3, Qwen3.5-0.8B 13 → 8). The
  layouts are studied further on those two baselines (speed study §2.7).
- **The meeting pass stays off on the phone** (2026-10-18).

## 2026-10-19 (evening) — the mail pass's prompt layout is the model's `#method` `#model`

**The owner:** *"Fixed a model we can try different layouts. Pick only the laptop model and the
fastest phone one as baselines"*, then *"go with option 1"* (the layout as a per-model setting).

**Decision.**
- **A catalogue key, `shared_mail_prompt`.** Set, the mail pass's two questions (meetings, title)
  share one system prompt holding both instructions by name, the emails follow, and the user message
  ends `Question: meetings` or `Question: title` (`fm_agent::meetings::prompt`). The title call then
  reuses the meetings call's cached emails: 516 tokens read instead of 4,041 on a ~4000-token
  exchange. Unset, each question's instruction is its own system prompt, as before.
- **Set for qwen3-vl-4b only.** Measured (speed study §2.7, 2 runs each, identical):
  qwen3-vl-4b 16/17 meetings and 12/12 titles shared, against 15/17 and 12/12 apart;
  lfm2.5-vl-450m Q4_K_M 3/17 and 5/12 shared, against 5/17 and 9/12 apart — shared, it names the
  mailbox's owner or the instruction's example roles. Qwen3.5-0.8B fell from 13/17 to 8/17 meetings.
- **The title has an eval** (`methods/meetings/eval/titles.json`, 12 cases, run at the runner's own
  settings). The meetings and title evals take `FM_LAYOUT`, and default to the model's own layout, so
  an eval measures what ships.
- `Agent` gains `shared_mail_prompt`, filled from the catalogue by both launchers (an addition).

## 2026-10-20 — a meeting already begun when the newest email was sent is past `#method`

**The owner:** *"yes, do the past fix and re-score"*, after the shortlist was run on their own mail
(`../research/2026-10-19-scouting-phone-models.md` §4).

**Decision.**
- **What changed.** `meetings::resolve` drops a meeting as `Dropped::Past` when it starts before
  the newest email in the conversation was sent, compared by the email headings' stamps. Before,
  only a day before today counted.
- **Why.** On real mail, a follow-up at 17:31 named the 17:00 call it followed, and larger models
  (K2-Horizon, Qwen3.5-2B) reported that call beside the next meeting. Under the day rule it passed
  as fixed, and would have been proposed.
- **Call-offs keep the day rule**: they still record what happened to a meeting.
- **Measured.**
  - Real mail: K2-Horizon 2/5 → 4/5, Qwen3.5-2B 1/5 → 2/5, every other model unchanged.
  - Every answer recorded in `methods/meetings/eval/cases.json` scores the same.
  - Test: `a_meeting_already_begun_when_the_newest_email_was_sent_is_past`.
- **No contract change**: same functions, same signatures.

## 2026-10-20 — the phone runs Qwen3.5-0.8B; the 450M is no longer offered `#model`

**The owner:** *"ok, drop the 450M and make qwen the phone model"*, then *"lets go on with qwen 0.8
as default for the light version and devices like smartphones"*.

**Decision.**
- **What changed.** `default_mobile = "qwen3.5-0.8b"`. It is the phone's only `phone_label`, so
  formicaria shows no chooser: it shows one only for two or more choices.
- **The 450M entries stay catalogued, without a label.** A phone running either 450M keeps it and is
  offered Qwen3.5-0.8B with its size (`installed.rs`, the 2026-10-10 rule).
- **Why** (`../research/2026-10-20-small-model-benchmark-summary.md`): the 450M was faster only.

  | | The 450M | Qwen3.5-0.8B |
  |---|---|---|
  | A user's real mail: meetings found | 0 of 4 | 2 of 4 |
  | Invented meetings: meetings found | 1 of 11 | 8 of 11 |
  | Recipes: ingredients found / invented | 89–102 / 16–18 | 245–248 / 1–2 |
  | Handwriting / typed pages (WER) | 96% / 49% | 43% / 2.3% (photos capped at 1024 image tokens) |
  | Phone time: meetings, research | 4.3 s, 6.0 s | 3.7 s, 6.9 s |

  - Its card does not recommend it for fine-grained OCR. Its language model is 350M parameters.
- **Not yet measured:** LFM2.5-VL-1.6B, Liquid's vision model in Qwen's speed class. If it beats
  Qwen3.5-0.8B on the same evals and on phone time, this entry is revisited.
- **No contract change**: the catalogue's format and every function stay as they are.

## 2026-10-20 — printed pages are read by an OCR first (Myrme light) `#method` `#contract`

**The owner:**
- *"lets go on with the best options of the light version that make sense (minimal set of
  absolutely necessary tools instead of one per subtask)"*;
- *"ship also the OCR tool already for the light version"*;
- then, of the release, *"One release, 0.7.0 with OCR"*.

**Decision.**
- **The one tool of Myrme light is the OCR**: PP-OCRv6 small through `oar-ocr` on ONNX Runtime 1.23
  (`ocr.rs`). Every other method stays with the small LLM; GLiNER and e5 were measured and dropped
  (`../research/2026-10-19-myrme-light.md`).
- **Routing.** It reads every page first; its text stands at a mean confidence of 0.97 or above, or
  when no vision model is loaded; otherwise the vision model reads the page. Measured through the
  install path: printed pages 0.8% WER, 30 of 30 confident; handwriting 0 of 30 confident.
- **Where it runs.**
  - Linux and macOS fetch ONNX Runtime and the OCR's files with the model runtime.
  - The phone carries ONNX Runtime in the APK (formicaria's packaging, contract 1.6) and fetches
    the files with the weights.
  - **Windows has none for now**: Microsoft's archive is 78 MB for a 14 MB DLL.
- **Accepted cost:** the OCR gives plain lines, so a printed table or code block loses its layout
  when the OCR is confident. Written in the manual.
- **Not measured before release:** the OCR's time on the phone. The release does not wait for a
  phone test (formicaria's rule, 2026-10-15); the phone timing run is ready
  (`examples/ocr_page.rs`).

## 2026-10-20 — the mail verdict in words, and a meeting that clashes says so `#method`

**The owner:**
- *"some wrong words there"*: "the meeting stays at 2026-10-16 16:30" for a meeting already held,
  raw timestamps, and "left out: not found in the emails as written";
- *"it put two meetings at the same time in the same day and I had to catch the conflict by
  myself"*.

**Decision.**
- **Dates in words** (`when`): "Fri 23 Oct at 16:00".
- **A meeting that began before the newest email "has taken place"**, by the rule of 0.7.0; one
  ahead "is still on …".
- **A dropped answer is said plainly:** "one more possible meeting was not proposed: its words are
  not in the emails".
- **A clash** (`clash`) is another meeting starting less than an hour away, among:
  - each conversation's own meeting, and those linked to it, named by the conversation;
  - any other meeting note.

  It is added to the verdict, and to the note proposed, as a **Two meetings at once** line, so it
  is seen before the meeting is accepted. formicaria gives starts, not ends, so the measure is an
  hour, the same as formicaria's calendar.

## 2026-10-21 — a label is read by the OCR first; a phone can read one `#method` `#contract`

**The owner:**
- *"Using the ocr model for ingredients and label scanning with a photograph … This will be in the
  next myrme release"*;
- of the plan, *"I think it looks overly complex"*, then *"stay with only 1 and 2 and drop the
  vocabulary"*: the rotation retry with a minimum-text rule, and labels on the phone through the
  OCR's text. The vocabulary and the LLM's correction of ingredients are dropped (they gained one
  point, `../research/2026-10-20-packet-labels.md` §3a).

**Decision.**
- **`label::read_with` has the OCR read a label first** (contract 1.7). Measured on thirty packets
  against figures checked by hand on the photos (§3c–3e of the research): the OCR's text, read by
  formicaria's `labelText.ts`, got **155 of 210 figures right and 14 wrong, in 0.67 s** on a laptop
  CPU; the phone's vision model got 85 right in 28 s, the laptop's 88 in 40 s.
- **Turned only when upright reads too little.** "Too little" is the **minimum-text rule** for labels:
  fewer than [`ENOUGH`] = 4 figures (a number with a unit, or with decimals, never a percentage).
  Then the three other turns are tried, stopping at the first that is enough; the reading with the
  most figures is kept. Pages of writing are not turned: there it quadrupled the time of every
  handwritten page for nothing (§3b).
- **A vision model reads only when the OCR found too little**, and only one fit for labels (the
  2026-10-13 rule stands: the phone's is not). Where there is none, what the OCR read is given if it
  holds any figure, for the person to check and complete; nothing at all is said plainly.
- **So a phone reads labels**, with the OCR it already has for pages: `ready_json_with` is ready
  wherever the OCR is.
- **One OCR per process** (`ocr::shared`): the launchers and the app's label reading share it.

**Not measured before release:** the OCR's time on the phone (the timing run is ready).

## 2026-10-23 — a meeting time written on another clock is moved onto the person's `#method` `#contract`

> SUPERSEDED IN PART (2026-10-24, later, *Myrme follows formicaria's clock*): the person's clock is
> formicaria's only — never the computer's, which Myrme no longer asks.

**Asked:** an email fixed a meeting *"at 10am <city> time"*; the pass proposed 10:00, it was
accepted, and an agenda in another zone showed 10:00 instead of the hour it was there. *"we need to
adjust for: laptop time zone, the time zone in the calendar and the one of the meeting."*
formicaria's side is its `decisions.md` entry of the same date.

**Decision.**
- **Rust reads the zone, never the model** (design rule 2): `fm_agent::zone::after_time` reads the
  words right after a time the sentence holds — a place and "time" (any city in the zone database,
  plus countries, adjectives, US regions and Italian/French city names), an abbreviation (`CET`,
  `SGT`, `PT`; not `ET`, which is French "and"), or an offset (`UTC+2`). Nowhere else: "I'm in Berlin
  this week" names a place, not a clock. No zone written → as written.
- **`meetings::resolve` takes the person's clock** (`Option<&Here>`) and moves day and time onto it
  with the zone database (`jiff`), on the meeting's own day — so the 25 October change is counted.
  `Meeting::written` keeps the email's own time (`10:00 Berlin time`) for the box.
- **The person's clock** comes from formicaria's existing `/api/calendars` (`zone`, or `offset` from a
  formicaria before 2026-10-23), else the computer's zone. "Today" is that clock's day, not UTC's.
- **`PASS_VERSION` 7**: every conversation is read once more, so a meeting proposed on the clock it was
  written on is proposed moving.

**Eval:** three cases (`zone-berlin`, `zone-new-york-next-day`, `zone-unwritten-stays`); a case's
`here` names the reader's clock. Recorded answers score exactly as before on the seventeen others.

**Contract:** additive — a new dependency (`jiff`, MIT OR Unlicense) and one more existing formicaria
route read. `resolve`/`meeting_lines` gained a parameter; neither is in the contract.

## 2026-10-23 (later) — an integrated GPU is not "a GPU" for `gpu = "auto"` `#model`

**What prompted it.** A laptop whose discrete GPU had dropped out ran the 4B laptop model on its
integrated GPU, because `auto` passed `-ngl 99` and llama.cpp offloads to any Vulkan device. Measured
on one laptop with the shipped runtime (`docs/research/2026-10-23-integrated-gpu-fallback.md` §2): the
integrated GPU writes 5.7 tok/s, the CPU 14.4; it reads 80 against 95. The phone's Mali showed the same
(speed study §2.4). The owner: *"implement the iGPU fix in Myrme"*.

**Decision.**
- **`auto` means a discrete GPU when there is one, else the CPU.** Before launch `agent-serve` asks the
  staged runtime (`llama-bench --list-devices`), and when every Vulkan device reports `uma: 1` it starts
  the server with **`--device none`** (not `-ngl 0`, which still sends large batches to the GPU), and
  says so once in the agent log (`crates/fm-agent-run/src/gpu.rs`).
- **Every failure keeps the old launch**: no probe binary, a probe slower than 20 s, output it cannot
  read. A machine with a working discrete GPU never loses it to this.
- **`on` still uses any GPU** (the way to force a strong integrated one, none of which is measured yet);
  `off` and an explicit `--ngl` are unchanged. macOS (Metal) is left alone.

**Test:** the decision and the parser are unit-tested; `the_staged_runtime_still_reports_devices_in_this_shape`
pins the line's shape against a staged runtime (`MYRME_RUNTIME` or `agents/runtime`; skipped where none is
staged), so a runtime bump that changes it fails there. Checked end to end on the laptop above: the server
started with `--device none` and the note in the log.

**Contract:** none. `contract.md` does not name the `gpu` key; its values and the `models.toml` format are
unchanged, and the new module is private. Whisper (`whisper-server`) is not covered: it was not measured.

## 2026-10-24 — the meeting pass reads what is new since its last read, not the whole exchange again `#method`

**What prompted it.** A meeting fixed in a long exchange was never proposed: a conversation with a
meeting a week, where the newest emails set the next one and the other side accepted. The owner: *"there should not be a re-read of the full conversation at each check, only
of the new information from the last check … plus conversation and context compacting, we can have
a faster system"*, and *"one can compact the conversation up to the last meeting in a few lines"*.

**What was measured** (on the user's mail, locally, never copied here; qwen3-vl-4b held to the
schema, with the pass's own prompt and settings):
- **The answer, not the input, broke.** The prompt was well inside the window. The answer hit the
  600-token cap (`finish_reason: length`), so its JSON was cut, `parse` read it as `[]`, and the pass
  said "no change" and recorded the conversation as read. No later check would look at it again until
  another email arrived.
- **A larger cap does not fix it.** Uncapped, the model wrote the same meeting, a past one, eight
  times, the schema's `maxItems`, and never the new one: a repetition loop over the history. The
  other conversations answered with one item each.
- **The same model, shown only the emails since the last meeting,** answered the new meeting
  correctly, in a fraction of the answer and about half the prompt.
- **Why the history is the cost:** the model is asked where the meetings stand *now*, but it is
  shown every email, and lists what every email fixed. `resolve` then drops each past meeting and
  `plan` only weighs the ones to come, so the history costs answer tokens and adds nothing.
  The answer grows with the number of meetings ever arranged, and so does each read's prompt.

**Decision.**
1. **The model reads a window, not the conversation** (`meetings::window`): the emails that arrived
   since the last read, the email before the first of them (what it answers), and the emails whose
   sentences fix a meeting still to come or ask a question still unanswered (`Ledger::keep`, by the
   email's sent stamp). Emails in between are compacted to one line written by Rust, saying how many
   there were and that none fixes a meeting still to come, which the ledger makes true. The line holds
   no time and no sentence, so it cannot be quoted: an echo of it is not in the emails and is dropped.
2. **Rust still checks against the whole conversation.** `resolve` dates each quote from its own
   email, checks whether a question was answered, and finds a place in any email. Only what the model
   sees has changed. `plan` is unchanged.
3. **What the pass remembers is a ledger per conversation** (`meetings::Ledger`, in
   `mail-meetings.json`): the newest email the model has read, the emails kept in view with the start
   each fixes, and a fingerprint of the emails read. **The email headings are the version and the
   diff**: a conversation note only gains emails at its end, each under `### Sender — sent`. When the
   emails already read no longer match the fingerprint (an email rewritten, a re-import), the ledger
   is dropped and the conversation is read from its start. git would also show what changed, but it
   would need a new formicaria route (a contract change) and is not on the phone, for no more than the
   headings already say.
4. **A conversation never read is read in steps, oldest first** (`meetings::step`), each step at most
   `CHUNK_TOKENS` of new emails, with the ledger carried between steps. **One step per call of the
   pass**, so a question in a discussion never waits behind a long first read. Only the last step plans
   and proposes; the ones before only move the ledger.
5. **A cut answer is a failure, never "nothing"** (`LlmResponse::truncated`). The step is retried at
   once with half as many new emails; a single email whose answer is still cut is passed over, and the
   mail panel's verdict says so. **So is an answer that repeats an item**: the loop above is chaotic
   (with the conversation's real title and owner's name it looped; with either replaced by a
   placeholder it answered correctly), and four copies of a past meeting fit inside the cap, where
   they would read as "nothing new".
6. **The answer is sized for a window:** `maxItems` 4 (an item quoting one sentence takes about 105
   tokens on `weekly-long`, more when the quote carries a meeting link) and
   `ANSWER_TOKENS` 720; `TEXT_TOKENS` 6000, so the prompt and the answer still fit an 8192 window.
7. **`PASS_VERSION` 8.** Every conversation is read once more, in steps, under the new reading.
   What was already proposed is kept, so nothing is asked twice.
8. **The eval runs the same reader** (`meetings::read_all`, every step at once) and gains a long
   exchange of the same shape, with invented people.

**What it costs, said.** A meeting the model missed in an earlier read is not looked for again in
an email that has left the window, where a full read gave it another chance at each new email. A
place sent once in an email that has left the window cannot be copied by the model, so a later
meeting at the same link is proposed without it. A first read of a long conversation takes several
calls of the pass.

## 2026-10-24 (later) — Myrme follows formicaria's clock; times are kept as instants `#method` `#contract`

> Reverses, in part, *a meeting time written on another clock is moved onto the person's*
> (2026-10-23): "the person's clock is formicaria's setting, **else the computer's zone**" — Myrme no
> longer asks the computer.

**What prompted it.** formicaria's time-zone audit (`formicaria/docs/context/time-zones-audit-2026-10-24.md`
§2b, §6.2): Myrme and formicaria each kept their own idea of the person's zone and of today. Myrme took
the zone the readers used, not the store's; fell back to its own system lookup when formicaria did not
answer (always, on the phone); computed today itself; and remembered what it proposed as naive stamps,
so after a change of zone the same meeting could be proposed twice. formicaria 0.19 made one app clock
and serves it (`clock`).

**Decision.**
1. **The clock comes from formicaria only** (`VaultAccess::clock`, contract **1.8**, defaulted): its
   zone and its `today`. With an older formicaria, the zone `/api/calendars` answers, today counted on
   it. **Never this process's own zone**: with neither, the meeting pass does not run (said once in
   the log) and chat says no date. `pixi run clock` (part of `ci`, `agents/clock.sh`) fails on any
   system-zone or local-time call in `crates/*/src`.
2. **One clock inside a pass.** A heading or `mail_last` written with its offset (formicaria 0.20) is
   read onto the clock as it is parsed; one written without (older notes) is taken as on it. Every
   comparison in the pass is then between times on the same clock, and `plan` is unchanged.
3. **What is remembered is instants**: the starts proposed (`proposed_at`) and the ledger's kept
   starts are written as UTC instants and read back onto the clock in force, so a change of zone does
   not make an old meeting look new. The naive copies (`proposed`) are still written for a downgrade,
   and a file without `proposed_at` is read on the clock in force at the upgrade. **`PASS_VERSION`
   stays 8**: the loader reads both forms, so nothing is read again for this. The ledger's read point
   stays a heading stamp: after a change of zone the offset headings are rewritten onto the new clock,
   the ledger's fingerprint no longer matches, and the conversation is read again from its start — one
   costly read, never a skipped email.
4. **Times are proposed as instants** when formicaria says it takes them (`instants: true`): RFC 3339
   with the clock's offset, which formicaria converts onto the vault's clock once.
5. **Chat is told today's date**, on formicaria's clock, with its zone — one line of provided context
   (`today` in `conversation_turn`), in keeping with the minimal chat prompt (2026-07-22). An edit
   (`/propose`) is not told: it points at the note's words, and more to point at is more to point
   wrong at (2026-10-18).
6. **Only a device that reads mail runs the meeting pass** (`mail_novelty` is `Some`): two desktops
   syncing one vault would otherwise both propose.
7. **A day an email names is counted from the sender's own day.** On the reader's clock, an email
   written late on the 7th at −05:00 is already on the 8th, and its "tomorrow" would be the 9th.
   `on_clock` keeps the sender's stamp beside the converted one when the two differ
   (`### Ana — 2026-10-08 12:30 (sent 2026-10-07 22:30 -05:00)`); stamps are compared on the reader's
   clock, and `heading_day` counts from the sent day. The hour, with no zone written, still stays as
   written (2026-10-23). Eval: `offset-headings-sender-day`, `offset-headings-weekday`; the eval
   reads each body through `on_clock`, as the pass does.

**What it costs, said.** A heading written before formicaria 0.20, after a change of zone, is still
read on the clock in force, which can put it hours off; formicaria 0.20 gives those headings their
offset from Gmail.

## 2026-10-24 (evening) — one release-pages run at a time, and one page per tag `#release`

**What happened.** A release pushes `main` and its tag together (owner, 2026-10-24), and both runs
reach the `release` job. For 0.9.0 the two ran side by side and each created the page: GitHub held
two pages for one tag, every later run failed on that tag, and 0.10.0's page was never published,
so formicaria's `myrme-bump` could not take it.

**Decision.** The `release` job runs in one concurrency group (`release-pages`, never cancelled), so
the second run finds the page; and `agents/release-pages.sh` first removes any second page of a tag,
keeping the oldest. Tags are never touched. The release order itself is unchanged.

## 2026-10-25 — four commands on every device; a speed bar per method; the reply as it is written `#method` `#contract` `#model`

**Asked.** The owner: *"we have so many cmds for myrme and the user might be confused, can we
shorten the list to the minimal set in all devices?"* They chose four. And on speed: *"improving on
the laptop is sufficient for now, I will check the phone results myself when we ship the new
release."* Evidence: `../research/2026-10-25-minimum-speed.md`.

**Decision — commands.**
- **Four: `/research`, `/propose`, `/transcribe`, `/recipe`**, the same on every device.
  - `/search` was a second way to the web that a person could not tell from `/research`.
  - It is still read, as `/research`, so a habit or an old message keeps working.
- **In a first-class discussion** (no note to research into), `/research` answers there from the
  web, as `/search` did. Nothing else changes.
- `Command::Chat` and `Command::Propose` lose their `search` field. This is not in the contract.

**Decision — the speed bar.**
- Each method names its wait class in `METHOD.md` (*Wait*), and `agents/eval-speed.sh` prints the
  bar beside the method's time. Chat is timed by `agents/chat-speed.py`.
- **qwen3.5-0.8b runs with `-ub 128`** (catalogue key `ubatch`). It is a hybrid model, so a stored
  prompt is reused only back to its last checkpoint. Measured on the laptop CPU:
  - a later chat turn rereads ~320 tokens instead of 705–967;
  - its first token comes in 0.9 s instead of 1.7–2.4 s;
  - the first turn costs +3%, and images read the same.
  - The phone is checked by the owner at release.
  - **Kept after a second look** (the owner, 2026-10-25): at `-ub 512` the same model scored 1–4
    cases higher on three of four evals, inside the 15–19 /30 recipe, 14–16 /23 meetings and
    3–4 /7 edits spread of runs that should not differ (research §5.3).
- **MTP drafting is not adopted** (research §5.3): on the laptop CPU it writes chat, meetings and
  edits no faster and reads 7–15% slower; only recipe writes ~15% faster. The phone is untested.
- **The chat reply streams** to `VaultAccess::activity_draft` at most once a second (contract 1.9).
  - It is display only; the reply is still posted whole.
  - The desktop sends it to `/api/agent_activity` as `draft`, with the stage "writing the reply",
    so an older formicaria keeps showing that stage.
- **Not adopted:**
  - recipe by pointing (14/30 against 17/30);
  - n-gram drafting (no gain).
  - MTP waits on an offer of its download.

## 2026-10-29 — `/research` fixes: arXiv words ANDed, an honest User-Agent, quotes checked against what was shown, claims as plain text `#method`

**Found** by the research of `../research/2026-10-29-web-research.md` (§1, D1–D4), each confirmed in
the code or with a live request; the owner asked for the four to be fixed.

- **D1 — arXiv requires every word.** A space in `search_query` is OR at arXiv: *vitamin K
  deficiency* matched 642,502 papers, a particle-physics paper first. `websearch::arxiv_query` joins
  the words with `+AND+` (the same words then matched none — the truth); no words, no request. The
  request is `https` now. The desktop proxy (`agents/search-proxy.py`) does the same. **Fillers are
  left out** (`ARXIV_FILLERS`, 87 English function and question words): required, they made a search
  find nothing — *how does attention work in transformers* matched 0 papers, and 5,198 without *how*,
  *does* and *in*. A test reads the proxy's copy of the list so the two cannot drift.
- **D2 — the client says who it is.** `formicaria-myrme/<version> (+https://github.com/formicaria-org/myrme)`,
  replacing a string that began `Mozilla/5.0`. Wikimedia's policy forbids a bot to copy a browser's
  user agent and asks for a contact; a project address, never a person's. The desktop proxy, which
  sent a whole Chrome string, sends `formicaria-myrme-search-proxy (+…)`: **its DuckDuckGo engine
  scrapes HTML and may now be refused** — accepted, it is fail-soft, and disguising the client is
  what the research says never to do. Which contact goes in the UA for Wikimedia's shared bucket is
  still the owner's question (§6 of the research).
- **D3 — a quote is checked against the text the model was shown.** `pack_sources` cut each source to
  700 characters (450 on the retry), but `verify` was given the full texts, so a quote from past the
  cut — never shown, recalled or lucky — passed. `grounding::as_shown` makes one cut used by both.
- **D4 — a claim reaches the note as text.** The claim is the model's words and a source can steer
  them (a GitHub repository's description is anyone's). `assemble_body` now escapes `\ [ ] < >` in
  claims and source titles and **drops any word carrying an address** (`://`, `www.`, `mailto:`); the
  only link on a claim's line is the citation's, which code writes. Images were already blocked by
  both shells' CSP.

**Not done here** (the research's later steps): showing the quote under each claim, and a check that a
claim restates its quote. Tests: `a_quote_from_past_the_cut_is_not_supported_by_the_source_as_shown`,
`a_claim_and_a_title_reach_the_note_as_text_that_cannot_link`,
`research_checks_a_quote_only_against_what_the_model_was_shown`, `an_arxiv_query_requires_every_word`,
`an_arxiv_query_leaves_out_the_words_no_paper_is_found_by`, `the_desktop_proxy_leaves_out_the_same_fillers`,
`the_user_agent_names_the_project_and_is_not_a_browser`. The contract is unchanged.

## 2026-10-29 (later) — a claim keeps to its quote's numbers and negations; web answers are kept and busy sources rest `#method`

- **`grounding::keeps_to_its_quote`**: a claim whose number is not a number of its quote (`1,450` and
  `1450` are one), or that negates when its quote does not (en, fr, de, it, zh), is dropped like an
  unsupported quote. Measured: over the recorded answers of four model variants it dropped exactly two
  claims, both "there is no information about …" written over an unrelated quote on a case whose right
  answer is nothing — exact rose 7/8 → 8/8 for both; Qwen3.5-0.8B live on CPU, four runs, 5/8 exact
  and 8/8 grounded with and without it, the same cases each time. Showing the quote under each claim is
  left to the owner.
- **`DirectSearch`** keeps each answer 12 h in memory (256 at most, oldest first), and leaves a source
  that answered 429 or 503 alone for its `Retry-After` (60 s when it names none, an hour at most) —
  never a retry loop. Nothing is written to disk.

## Rulings inherited from formicaria (before 2026-10-18)

These still hold. Their full text is in formicaria's `docs/context/decisions.md`; grep the heading
there. A reversal of one is written *here*, dated, and names the formicaria heading it supersedes.

- `/transcribe` reads writing too — one verb, two specialists (2026-08-30, `#agent` `#toolchain`)
- A proposal's outgoing commit is kept, so the *accepted* label can be true (2026-08-30, `#git` `#agent` `#data`)
- A feature the app cannot deliver must not offer itself (2026-08-28, `#agent` `#ui`)
- 2026-08-29 — the assistant is not a user, and must not keep the app alive `#agent` `#ui`
- 2026-08-29 — a capability is a *reason*, and it is not `preflight::admit` `#agent`
- 2026-08-29 — a pinned checksum needs a pinned revision, or it is a bomb on a timer `#agent`
- The study agent's model warm-up is deferred a few seconds after launch (2026-07-24, `#agent`)
- 2026-09-02 — the phone answers the same status shape as the desktop, and AI availability is stated per OS `#agent` `#track-m`
- 2026-09-02 — the assistant provisions itself, so a downloaded copy can run it `#agent` `#toolchain`
- 2026-09-02 — the assistant asks the machine, not a list of operating systems `#agent` `#toolchain`
- 2026-09-02 — a platform arm nobody can compile gets a tested core and a cross-check `#toolchain` `#agent`
- 2026-09-02 — advice that cannot succeed is worse than none, and a feature that takes 3 GB can give it back `#agent` `#ui`
- 2026-09-02 — iOS: the subprocess consequence is reversed for Android and held for iOS, which ships agent-free `#track-m` `#agent`
- 2026-09-03 — the shell's agent gate is the feature **and** Android, derived in `build.rs` `#track-m` `#agent`
- 2026-09-04 — a proposal its author turned down is never merged `#git` `#agent`
- 2026-09-12 — an adjunct's idempotency key is a source key, not only a blob hash `#agent`
- 2026-09-12 — a city digest is two halves: the events are a command, the prose is the assistant `#agent` `#data`
- 2026-09-12 — a subscription is not a crawl `#toolchain` `#agent`
- 2026-10-09 — mail and your own calendars become notes on their own; the person only answers proposals `#agent` `#data` `#vault`
- 2026-10-09 — a repeating meeting is one note per occurrence, laid out two months ahead `#data` `#agent`
- 2026-10-09 — the email exchange lives in the conversation note `#data` `#vault` `#agent`
- 2026-10-09 — the assistant proposes meetings from email, and a proposal may now date a note or add one `#agent` `#data`
- 2026-10-09 — the model points at the sentence; Rust reads the date from it `#agent`
- 2026-10-09 — the laptop model gets an 8192-token window, and its image projector moves off the GPU `#agent` `#toolchain`
- 2026-10-10 — an image is read by a dedicated reader if there is one, else by the main model if it can see, else by nobody `#agent`
- 2026-10-10 — an update offers a newer assistant model; it never downloads one unasked, and the catalogue follows the app `#agent` `#ui`
- 2026-10-11 — a tool is a deterministic command in a non-agent crate; the assistant calls it like everyone else `#seams` `#agent`
- 2026-10-13 — the program on a computer stays up while a reminder is waiting, and the assistant does not `#toolchain` `#agent` `#ui`
- 2026-10-13 — a label is read from a photograph, and the person checks it `#agent` `#data` `#ui`
- 2026-10-13 — a food keeps its pictures, and a label can be settled later `#data` `#ui` `#agent`
- 2026-10-15 — a meeting's details are corrected in the review; a new email can move or place it `#agent` `#ui`
- 2026-10-15 (later) — the label chooses conversations; the assistant tracks where a meeting stands `#agent` `#data`
- 2026-10-15 (evening) — mail and calendars read on opening; mail every 5 minutes; the meeting pass drains its backlog `#agent`
- 2026-10-15 (night) — a near-copied quote stands as the email's own sentence `#agent`
- 2026-10-15 (night) — calendars read every 5 minutes, an unchanged feed skipped `#agent`
- 2026-10-15 (night) — a conversation is titled "Person — who they are"; `title` joins the proposal allowlist `#agent` `#ui`
- 2026-10-15 (night) — on a re-read, a call-off that is the latest word is proposed `#agent`
- 2026-10-16 — new mail wakes the meeting pass; its verdict is shown, "nothing to change" included `#agent`
- 2026-10-16 — quoted history is trimmed in Chinese mail too `#agent`
- 2026-10-16 — an email read late is placed by when it was sent; the existing notes' quotes cut once `#agent`
- 2026-10-16 (evening) — a stopped mail reader says so in Collaboration, with the sign-in beside it `#agent` `#ui`
- 2026-10-16 (evening) — the website gains a privacy page and a terms page, for publishing the Gmail client `#toolchain` `#agent`
- 2026-10-16 (evening) — a quote shortened with "…" is checked piece by piece; another conversation's meeting is not "taken" `#agent`
- 2026-10-18 — the assistant is Myrme, on every device; each device answers its own messages `#agent` `#ui`
- 2026-10-18 — a recipe from a recording: `@Myrme /recipe` `#agent` `#data`
- 2026-10-18 — each device names its Myrme `#agent` `#ui`
- 2026-10-18 — every device hears any language; *better listening* is a choice `#agent` `#ui` `#toolchain`
- 2026-10-18 — dictation: what is said is written at the caret `#agent` `#ui` `#seams`
