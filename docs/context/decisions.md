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
