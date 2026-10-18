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
