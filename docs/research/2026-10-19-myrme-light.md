# Myrme light — dedicated tools for dedicated tasks, and a very small LLM (architecture plan, 2026-10-19)

> **Status: proposed; nothing adopted, nothing built.** A plan to try, gated phase by phase on the
> method evals. It extends `docs/context/plan.md` (Part 2, *the `Encoder` seam*; Phase C, *the
> dedicated methods*) and uses the candidates of `2026-10-18-dedicated-methods-per-task.md`.
> Adopting any part is a dated entry in `docs/context/decisions.md`; a new runtime is one by itself.

**Refined the same day** (the owner: *"refine plan and start, test it on laptop only for now and then
we go and try it on the phone"*, *"but only the best option"*): **one pick per tool**, no candidate
lists, and every phase runs on the laptop first; the phone is its own phase once the laptop shows the
tools win.

**The owner:** *"I feel like we can do many of the things without an LLM and with targeted models
that would run there and be very accurate"*, then *"start architecting a plan to try what I would
call the light version of myrme, with dedicated tools that can solve dedicated tasks and a very small
LLM"*.

## 1. Why: what the measurements say

The phone's fast model (`lfm2.5-vl-450m`) fails mostly at tasks that are **extraction or
classification**, not writing (speed study `2026-10-19-speed.md`, improving-the-fast-model study
`2026-10-19-improving-the-fast-model.md`):

| Method | Phone's fast LLM today | What the task really is |
|---|---|---|
| Meetings from mail | 5/17; finds the right sentence, then answers "cancelled" in 15 of 17 | find date sentences (Rust), label each one's status (classification) |
| Conversation title | 9/12; under other layouts names the mailbox's owner | find the correspondent's name and role (named entities) |
| Recipe from text | 102 of 257 ingredients found, 18 invented | label the lines and the words in them (tagging) |
| Photo → text, typed | 49% word error, ~39 s a page | optical character recognition |
| Photo → text, handwriting | 96% word error | recognition; the hardest case for any small model |
| Note edits | 0/7 | pick the kind (classification), find the anchor (a span), write the new text (generation, often quoted in the request) |
| Research | 4/8 grounded | retrieval and quoting (extraction), then a short summary (generation) |
| Chat | — | generation |

A targeted extractor **points by construction**: it returns spans of the input or labels from a
closed set, so it cannot invent a name or an ingredient, and it is small and fast. That is design
rule 2 (*the model points; Rust reads and computes*) taken to its end. An LLM stays only where the
output is open text.

## 2. The shape of Myrme light

```
trigger (command · file type · schedule)          ← code chooses the method, as today (rule 1)
   │
method pipeline (Rust, deterministic)
   ├── tools that point: OCR · entity/span extractor · classifier · embedder     (ONNX Runtime)
   ├── Rust: dates, numbers, units, quote checks, layout, ICS parsing             (no model)
   └── the very small LLM, only for a free-text slot: a reply, a summary,
       an edit's new text when the request does not quote it                    (llama.cpp)
   │
proposal (the only output, as today)
```

- **Two runtimes, no more** (plan.md, *the seams*): llama.cpp for the LLM, **ONNX Runtime** for every
  tool. Embeddings may stay on llama.cpp (it serves embedding GGUFs) if that keeps a tool off ONNX.
- **Tools are small and co-resident.** Together under ~0.5 GB of RAM; the LLM is the one large
  resident. A tool is loaded on its method's first use and kept while the assistant runs.
- **Each tool is an offer**, like a model today: its size and licence shown, downloaded only when
  the person says so, removed with the assistant (the 2026-10-10 rule; `installed.rs`).
- **"Light" is a phone choice**, beside *Faster* and *More accurate, slower* (`phone_label`), until
  the evals say it should be the default.

### The picks — one per role

| Role | The pick | Why this one |
|---|---|---|
| Read text (`ReadText`) | **PP-OCRv6 small**, through the `oar-ocr` crate | the newest PaddleOCR generation (Apache-2.0); ~30 MB (detector 9.4 MiB, recogniser 20.2 MiB) with one 18,708-character dictionary for Chinese and Latin scripts; a Rust engine on ONNX Runtime that loads models from memory |
| ~~Extract spans and labels (`Extract`)~~ *dropped 2026-10-20* | **GLiNER multi v2.1** (`urchade/gliner_multi-v2.1`), through `gline-rs` | multilingual, Apache-2.0, the mature Rust engine. GLiNER2 is a later upgrade, taken only if it beats this on our evals |
| ~~Embed (`Embed`)~~ *dropped 2026-10-20* | **multilingual-e5-small** (118M, MIT) | small enough for a phone, covers Italian and Chinese |
| The very small LLM | **Qwen3.5-0.8B Q4_0**, thinking off | the best small model we measured (§1 of the speed study), already a phone choice |

Nothing else is tried unless a pick fails its bar; then the plan says why, and picks again.

**What a pick is judged on** (the owner, 2026-10-19: *"we might have space for the models, so they can
be a little bigger, it is their runtime cost that matters"*): **time per call and memory while
running**, not download size. So a larger variant of a pick (PP-OCRv6 *medium* instead of *small*, a
larger GLiNER or embedder) is tried wherever an eval says the pick falls short, and kept only if its
time on the device is acceptable. Vocabularies cost next to nothing at run time and may be as large as
helps.

### The seams, concretely

In `fm-agent` (no runtime dependency, as today), three traits beside `LlmStep`:

| Trait | Returns | First implementation |
|---|---|---|
| `ReadText` | lines of text with boxes and confidences | PP-OCRv6 small through `oar-ocr` |
| `Extract` | labelled spans of the input (`person`, `job title`, `organisation`, `quantity`, `unit`, `food`, …) and labels for a whole text from a closed set | GLiNER multi v2.1 through `gline-rs` |
| `Embed` | vectors for retrieval and for nearest-example classification | multilingual-e5-small on ONNX Runtime |

The implementations live in `fm-agent-run` behind a feature (`tools`), like `download` today.
`Agent` gains the tools a device has, as it has `image_specialist` (a slot that exists, unfilled).
The methods take the traits, so a test fills them with fakes, as `LlmStep` is faked today.

### Runtime packaging (the one-off cost)

- **`ort` with `load-dynamic`**: ONNX Runtime is opened at run time (`dlopen`), with no link-time
  dependency — the same pattern as the phone's bundled `llama-server`. The library
  (`libonnxruntime.so`, Android arm64) is staged into the APK's `jniLibs` by formicaria's packaging,
  from a catalogue pin with its hash (`runtime_url_onnx_android_arm64`, `runtime_sha256_…`), exactly
  as `ci/android-stage-runtime.sh` does for llama.cpp (contract 1.4's pattern). Desktop: an archive
  per platform, fetched like the llama runtime.
- **APK size** grows by the library (to measure; the risk register below).
- **Catalogue**: a tool is a `[[models]]` entry with `kind = "ocr" | "extract" | "embed"` and its
  files. PP-OCRv5 is three files (detector, recogniser, dictionary), so an entry needs a list of
  files with one hash each — a format addition (contract minor), to design in phase L0.

### Kept simple

**Status, 2026-10-20: Myrme light is Qwen3.5-0.8B and Rust, plus at most one tool, the OCR for
typed pages.**
- The owner: *"a minimal set of absolutely necessary tools instead of one per subtask (getting the
  title is well within LLM territory)"*.
- **Qwen3.5-0.8B is the phone's model as of 2026-10-20** (`decisions.md`). With it, every method
  runs on the phone with no tool at all.
- **A tool earns its place only by doing what the LLM cannot do acceptably on the phone.**
  - GLiNER (titles) and e5 (research) were measured and left out.
  - **The OCR is the one candidate.** Typed pages: 0.8% WER against Qwen's 2.3%, about 1 s a page
    on the laptop CPU. It is kept only if the phone confirms the time gain: Qwen reads a
    handwritten page in a median 28 s there, and typed pages have not been timed on the phone.
  - The OCR also brings ONNX Runtime into the app, so its size counts as well.
- **Shipped in Myrme 0.7.0** (owner, 2026-10-20: *"ship also the OCR tool already"*), before
  its phone timing, on Linux, macOS and the phone. `decisions.md`, 2026-10-20, has the routing and
  the costs: plain lines for tables and code, and no Windows yet.

The owner, 2026-10-19: *"lets keep the architecture simple instead of adding blocks that are not sure
to improve."* So the architecture is the three tools, Rust and the one small LLM — **a block joins
only when an eval shows it wins clearly**. Vocabularies (correction against a word list, Word Beam
Search, linking to the food table) were tried for that reason and left out: about 5 points on
handwriting, nothing on typed text (results below).

## 3. Each method, light

| Method | Light pipeline | LLM? | Bar to beat (our evals) |
|---|---|---|---|
| **M3 Photo → text** | PP-OCRv5 detect + recognise; Rust orders lines into text. Low confidence or a page judged handwritten → the VLM (Qwen3.5-0.8B, capped at 1024) if installed, else say so | only as fallback | typed pages **≤ 2.3% WER** (capped Qwen3.5-0.8B) in **< 5 s** a page on the phone; handwriting: not worse than the fallback |
| **Conversation title** | `Extract` (`person`, `job title`, `organisation`) over the thread; Rust picks the person who is not the owner and writes most, and the role span nearest their name or signature | none | **12/12** (qwen3-vl-4b), at least the 450M's 9/12 |
| **M1 Meetings** | ICS part first (invites, moves, call-offs, no model). Else Rust finds sentences with a day or time; `Extract`'s classifier labels each (`proposed`, `confirmed`, `moved`, `cancelled`, `not a meeting`); Rust resolves, as `meetings::resolve` does today | none | **≥ 13/17** (Qwen3.5-0.8B) at well under a second |
| **M7 Recipe from text** | Rust splits sections by headings and list shape; `Extract` tags `quantity`, `unit`, `food`, `preparation` in each ingredient line; steps are the numbered or remaining lines | none | **≥ 245/257 ingredients, ≤ 2 invented** (Qwen3.5-0.8B) |
| **M5 Note edits** | the kind (`append`, `insert_after`, `replace`, `delete`) by nearest-example classification on `Embed`; the anchor by matching the request's quoted or named words to the note's sentences (Rust, then `Embed`); the new text copied when the request quotes it | only for unquoted new text | **≥ 3/7** (Qwen3.5-0.8B, 1.2B) |
| **M2 Research** | retrieval: BM25 in Rust + `Embed`, a reranker later; claims kept only with a verified quote (`grounding::verify`, as today) | yes, a short summary | **8/8 grounded** |
| **M4 Audio → text** | whisper, as today; a voice-activity filter in front | none | unchanged |
| **Chat** | — | yes | — |

The very small LLM is then left with three slots — a chat reply, a research summary, an edit's new
text — all short, all free text, and none of them extraction.

## 4. The very small LLM

- **Role:** writes short free text from material Rust and the tools prepared. Never points, never
  extracts, never routes.
- **The pick: Qwen3.5-0.8B Q4_0, thinking off** — the best small model measured (meetings 13/17, recipes
  245/257, typed pages 2.3% WER capped), already in the catalogue. Left with only the free-text slots,
  its slower writing matters less: the long answers it wrote (recipes) move to the tagger.
- **Its evals:** the chat, research-summary and edit-text slots (chat has no eval yet; L5 writes one).
- **Considered and not taken: K2-Horizon-3.7B** (IFM, Apache-2.0; Artificial Analysis index 16 against
  a median of 6 for ≤4B models; the owner pointed to it, 2026-10-19). It is a reasoning model whose card
  asks for `reasoning_effort="high"` and at least 32,768 output tokens (mean ~2.3k on IFEval); its
  Q4_K_M is 3.16 GB; it needs llama.cpp **b11454** or later (we pin b10076/b10081); it is English and
  text only. On the phone that is minutes per answer and most of its memory, the opposite of a model
  that writes a short reply. Its 0.9B sibling scores 3 (median 6). If ever, it is a laptop text model,
  after a runtime upgrade measured for the vision regression the speed study notes.
  Sources: https://huggingface.co/IFM/K2-Horizon-3.7B · https://huggingface.co/IFM/K2-Horizon-3.7B-GGUF ·
  https://artificialanalysis.ai/models/k2-horizon-3-7b · https://artificialanalysis.ai/models/open-source/tiny

## 5. Phases, each gated

Every phase ends in a measured result written into this document, and a `decisions.md` entry if
anything is adopted. Laptop first for accuracy, then the phone for time, with the usual guards.

| Phase | What (laptop unless it says phone) | Done when |
|---|---|---|
| **L0 spikes** | in a scratch project outside the repo: `oar-ocr` with PP-OCRv6 small, `gline-rs` with GLiNER multi v2.1, multilingual-e5-small — each built and run on the laptop's CPU (`ort` downloads its own ONNX Runtime) | each runs and gives a first number |
| **L1 OCR** | M3 typed and handwritten pages through PP-OCRv6 small, scored with Myrme's own `eval::wer`; then confidence routing to the VLM | typed **≤ 2.3% WER**; time per page on the laptop CPU recorded |
| ~~L1b vocabularies for reading~~ | tried, not adopted: about 5 points on handwriting, no gap closed (results below) | — |
| **L2 extraction** | titles, then meetings' status, through GLiNER | titles 12/12; meetings ≥ 13/17 — *titles 11/12, then **not adopted** (a 1.16 GB model for one field, 2026-10-20); meetings stay with the LLM (results below)* |
| **L3 recipes** | the ingredient tagger and section split | ≥ 245/257, ≤ 2 invented — *GLiNER 206/257, 5 invented, 0.8 s: not met; recipes stay with the LLM* |
| **L4 edits** | kind, anchor and copied text without an LLM; Qwen3.5-0.8B only for unquoted text | ≥ 3/7 — *not pursued: edits stay with the LLM (below)* |
| **L5 the LLM's slots** | evals for chat, research summary, edit text; Qwen3.5-0.8B on them | its evals |
| **L6 research retrieval** | BM25 + multilingual-e5-small | 8/8 grounded — *extractive 4/8: research stays with the LLM (below)* |
| **P phone** | ONNX Runtime in the phone shell (`load-dynamic`, `libonnxruntime.so` in `jniLibs`), the tools that won on the laptop, timed on the Dimensity 7300 with the usual guards; APK size | the laptop bars hold on the phone, and each tool's phone time is recorded |
| **L7 ship "Light"** | the tools into `fm-agent-run` (feature `tools`), a phone choice; formicaria stages ONNX Runtime | a Myrme release, a formicaria release |

L1 is first because it is the largest measured gap, the smallest model (~20 MB) and the clearest
eval. The laptop proves accuracy cheaply; the phone phase (P) then proves time, size and the runtime
path, once, for every tool that won. Each later phase can stop the plan
early without waste: what was built is a method that measurably beats today's.

## Results so far

### L0 (OCR part) and L1 on the laptop — 2026-10-19

**Setup.** A scratch project outside the repo (`~/bench-scratch/light-spike`, kept in
`keep/scripts`): `oar-ocr 0.8.1` with PP-OCRv6 small (detector 9.4 MiB, recogniser 20.2 MiB,
dictionary 18,708 characters), scored with Myrme's own `fm_agent::eval::wer` against the image
eval's references. Laptop CPU (i7-11800H, **on battery**), one page at a time.

**What the runtime path needed** (each is a finding for the phone phase too):
- `ort` must be the release `oar-ocr` was written against (`=2.0.0-rc.12`); Cargo otherwise takes
  `rc.15`, whose API changed.
- **`load-dynamic` is the way**, here as on the phone: `ort`'s downloaded static ONNX Runtime needs
  a newer glibc (`__isoc23_strtoll`) than pixi's toolchain links against. Opened at run time
  (`ORT_DYLIB_PATH`), Microsoft's own build works.
- **ONNX Runtime 1.23 is the version for `ort` rc.12** (with the `api-22` feature): 1.22 aborts in
  its device enumeration (`OrtEpDevice … !this->empty()`).

**Accuracy and time** (`light-L1-*.log`):

| 30 pages | PP-OCRv6 small | Qwen3.5-0.8B, capped (VLM) | lfm2.5-vl-450m (VLM) |
|---|---|---|---|
| typed (Wikibooks recipes) | **0.8% WER, 0.98 s a page** | 2.3% | 49.0% |
| handwriting (GNHK) | 61.3% WER, 0.66 s a page | **43.4%** | 96.3% |

- **Typed text: the OCR wins outright** — about three times fewer errors than the best small VLM, in
  about a second on a laptop CPU, from a 30 MB model. 28 of 30 pages under 3.5%; one at 13.5%
  (`wb-021`), to read before the phone phase.
- **Handwriting: the VLM stays.** Sending low-confidence pages to Qwen3.5-0.8B never beats sending
  all of them (61.3% → 43.4% as the threshold rises); the OCR is not the better reader of any
  handwritten page here.
- **Confidence tells the two apart.** Every typed page reads at a mean confidence of 0.99–1.00;
  handwritten pages mostly 0.63–0.95. **The routing rule for M3: confidence ≥ 0.97 → the OCR's text;
  below → the VLM if installed, else the OCR's text, marked as uncertain.**
- **L1's typed bar is met** (≤ 2.3%). Phone time is the phone phase's question.

### L1b on the laptop — vocabularies for reading, 2026-10-19

**Vocabularies** (none built from the eval pages): `en_50k`, the 50,000 most frequent English words
of hermitdave's FrequencyWords (OpenSubtitles, CC-BY-SA-4.0); its top 10,000; and `en_50k_food`,
the 50k plus a standard list of cooking units and the words of **formicaria's food table** (9,965
words from `crates/fm-app/packs/foods.sqlite.gz`).

**(a) Correction after reading** (`correct.rs`): a word of three letters or more that the vocabulary
lacks becomes the nearest vocabulary word (edit distance 1, or 2 from five letters; the more frequent
on a tie), only in lines read below a confidence (the gate).

| Vocabulary, gate | typed (OCR 0.8%) | handwriting (OCR 61.3%) |
|---|---|---|
| top 10k, 0.97 | 1.1% | 56.9% |
| 50k, 0.97 | 1.0% | 55.3% |
| 50k + units + food table, 0.97 | 0.9% | **55.3%** |
| **50k + units + food table, 0.95** | **0.8%** (no word changed) | **56.8%** |
| 50k + units + food table, 0.90 | 0.8% | 59.4% |

- **The domain vocabulary is what keeps typed recipes intact**: with the general list alone, words
  it lacks (`tbsp`, `cassava`, `groundnut`) were "corrected". **The larger general list did better
  than the smaller one** here; *specific* matters more than *small*.

**(b) Word Beam Search** (`wbs.rs`, after Scheidl et al.): the recogniser run alone on each detected
line (`CRNNModel::infer`, public in `oar-ocr`), its probabilities decoded twice on the same crops.

| | best path | Word Beam Search |
|---|---|---|
| handwriting, `en_50k` | 64.1% | **58.4%** |
| typed, `en_50k_food` | 0.8% | 4.9% |

- On handwriting it gains about as much as (a) (−5.7 points); ungated, it **damages typed text**,
  forcing names and rare words into dictionary words. Axis-aligned crops cost 2.8 points against the
  pipeline's own (64.1% vs 61.3% best path).

**Decided on these numbers: neither is adopted** (the owner: *"looks like vocabularies is not adding
much"*, *"keep the architecture simple"*). The better of the two, (a) at gate 0.95, gains 4.5 points
on handwriting and nothing on typed text — not enough for a block of its own. (b) gains no more,
needs its own decoding path, and damages typed text without a gate. A VLM + vocabulary run (the VLM's
transcript corrected the same way) was started and stopped for the same reason.
- **Neither closes the handwriting gap** to the VLM (43.4%): the recogniser itself is the limit
  (PP-OCR is trained for print). With a VLM installed, low-confidence pages go to it (L1); the
  correction is what a light-only phone gets.
- **Untested, and likely better:** the person's own vocabulary (their vault), smaller and more
  specific than any general list. It needs an eval from the owner's own handwriting (plan.md, Phase C).

### A bigger OCR? PP-OCRv6 small against medium, laptop — 2026-10-19

The same pipeline with PP-OCRv6 **medium** (detector 59 MiB, recogniser 73 MiB) in place of small,
run back to back on the laptop CPU on battery (absolute times noisy, ratios not; `light-ocr-*.log`):

| 30 pages | small | medium |
|---|---|---|
| typed | 0.8% WER, 1.6 s a page | 0.8% WER, 5.7 s a page |
| handwriting | 61.3%, 1.0 s | 54.4%, 3.0 s |

**Small stays the pick.** Medium is 3–3.5× slower for nothing on typed text, and its gain on
handwriting still leaves it short of the VLM (43.4%), which reads the handwritten pages anyway.

### L2 on the laptop — GLiNER for titles and meetings, 2026-10-19

**Setup.** A second scratch project (`~/bench-scratch/gliner-spike`): `gline-rs 1.1.0` with
`onnx-community/gliner_multi-v2.1` (commit `6ddaeb94`; upstream `urchade/gliner_multi-v2.1`,
Apache-2.0), ONNX Runtime **1.20** opened at run time. Laptop CPU, 4 threads, on battery.

**Runtime findings for integration:**
- **`gline-rs` pins `ort =2.0.0-rc.9`; `oar-ocr` pins `=2.0.0-rc.12`.** One binary cannot hold both:
  the two engines must be aligned (one patched, or GLiNER's pipeline written against rc.12) before
  either joins `fm-agent-run`.
- `gline-rs`'s tokenizer builds a C++ dependency (`esaxx-rs`): a C++ compiler at build time (the
  Android NDK has one; this laptop needed conda's `gxx`).
- **The int8 export is unusable**: names came back as job titles, 0/12 at the default threshold. The
  full-precision export (1.16 GB on disk) is the one that works; its run-time cost is what matters
  (0.04 s a conversation, 2.2 s to load). A working smaller export is a phone-phase question.

**L2a — titles** (Rust picks the correspondent from the thread's headings; GLiNER reads their
messages; Rust writes "job title, organisation"), scored as `eval_titles.rs` scores:

| | titles | time |
|---|---|---|
| labels `job title`, `organization`, threshold 0.5 | 10/12 | 0.04 s |
| + label `person` (names get a place to go; its spans unused), threshold 0.3 | **11/12** | **0.04 s** |
| qwen3-vl-4b (LLM, for comparison) | 12/12 | seconds |
| lfm2.5-vl-450m (the phone's LLM) | 9/12 | seconds |

- The `person` label is what made the descriptions right ("Sales engineer, ATI Industrial
  Automation", not the person's own name). Left: Pavel Novak, whose role is only implied ("I lead
  the Soft Robotics Lab"); and one place read as an organisation ("equipment officer, front desk").
- ~~Adopted for the light path~~ **Not adopted (2026-10-20).** The owner: *"we cannot load entire
  models just to write titles, that's absurd"*. The working export is 1.16 GB, and it would be
  loaded for one short field, while the small LLM is already loaded for the meetings question on
  the same emails. Titles stay with the LLM: one more short call, no second model. This also
  removes the `ort` version conflict, the C++ build dependency and an unknown phone memory cost.
  Qwen3.5-0.8B's own title score is not yet measured.

**L2b — meetings: GLiNER is not the tool.** Probed on every sentence of the 17 threads
(`light-L2b-probe.log`): days, times and places are found reliably ("Thursday" 0.94, "3pm" 0.82,
"room B12" 0.94, "mercoledì" 0.83), but what happened to a meeting is not — "I have to cancel our call
on Thursday", "Thursday doesn't work for me" and "mercoledì non riesco" carry no cancellation span,
and "deadline" was found once at 0.45. That judgement (moved, cancelled, declined, accepted, only a
deadline) is what the pass needs, and Rust already reads the dates once a sentence is pointed at.
**Decided (simple): meetings stay a pointing task for the small LLM under the grammar**, as today;
no cue lists, no classifier block.

### L3 on the laptop — recipes from a transcript with GLiNER, 2026-10-19

The method's 30 transcripts (`methods/recipe/eval`), GLiNER multi v2.1 full precision with the one
label `ingredient`, scored with `fm_agent::eval::recipe_outcome` (`light-L3-recipe-*.log`). No rule
reads the transcripts' template phrases ("For this you need", "First"); steps are simply the other
sentences of four words or more.

| 30 recipes | exact | found / 257 | invented | time |
|---|---|---|---|---|
| A: every `ingredient` span, threshold 0.3 | 2/30 | 231 | 81 | 0.8 s (laptop CPU) |
| **B: the spans of the densest sentence**, threshold 0.3 | **13/30** | **206** | **5** | **0.8 s** (laptop CPU) |
| B, threshold 0.5 | 9/30 | 198 | 1 | 0.8 s |
| Qwen3.5-0.8B (the plan's LLM), constrained | 15/30 | 245 | 2 | 30.9 s a recipe on the phone |
| lfm2.5-vl-450m (the phone's LLM) | 8/30 | 102 | 18 | 14.0 s on the phone |

- **Every span** picks up ingredients named in the steps ("a little water") — 81 invented.
- **The densest sentence** — where a speaker lists them — is far better than the phone's LLM today
  (206 against 102 found, 5 against 18 invented) and about 40× faster than the small LLM, but **misses
  the bar** (≥ 245, ≤ 2). It also assumes people list ingredients together, which these generated
  transcripts do and real recordings may not.
- **Not adopted as the recipe method**: the small LLM stays, being right more often. Kept as a
  finding: if phone time decides, GLiNER is the fast option at a measured cost in accuracy.

### L4 — note edits stay with the small LLM (decided without a spike), 2026-10-19

The 7 edit cases split in two: four are mechanical (the new text is quoted, or the line is named —
"add 'book the room' to the list", "remove the line about milk", "add a final line: '…'") and three
need understanding and rewriting (fix a spelling mistake, turn a paragraph into bullets, write a
formula as LaTeX). A tool path for the first four would be a router plus rules on how requests are
phrased — brittle against how people really ask, and seven cases cannot show it generalises. Under
the plan's rule (a block joins only on a clear win), **edits stay as they are: the small LLM picks the
kind, then writes the edits held to the schema; Rust applies them.**

### L6 on the laptop — research without an LLM, 2026-10-19

The research eval hands the method numbered sources, so it measures picking and quoting, not
retrieval. multilingual-e5-small (`intfloat/multilingual-e5-small`, MIT, ONNX full precision, 470 MB)
scored every source sentence against the question (`query:`/`passage:`); the sentences within 0.02 of
the best, at most three, became claims quoting themselves, checked by `grounding::verify` and scored
by `eval::research_outcome` as the LLM's are (`light-L6-research-*.log`).

| | exact | time |
|---|---|---|
| e5-small extractive, abstain below 0.85 | 4/8 | 0.04 s a question |
| e5-small extractive, abstain below 0.80 | 3/8 | 0.04 s |
| Qwen3.5-0.8B | 4/8 (grounded 8/8) | seconds |
| MiniCPM-V 4.7 1B (b11731, CPU, thinking off) | 7/8 (grounded 7/8) | seconds |

- **What it cannot do is judgement.** An answer spread over two sentences or two sources loses its
  second fact; and the off-topic case's best match (0.835) sits beside a real answer's (0.836), so no
  threshold says "the sources do not answer".
- **Decided: research stays with the small LLM**, its quotes checked by Rust as today. Embeddings may
  still choose which fetched pages to read — not measured here.

### The small LLM: MiniCPM-V 4.7 1B against Qwen3.5-0.8B, laptop — 2026-10-19

The owner pointed to MiniCPM-V (OpenBMB). Its 4.7 release (2026-10-12) has a **1B** variant:
Apache-2.0, built on the Qwen3.5 language backbone, with a vision encoder that compresses image
tokens 16× by default. It needs **llama.cpp b11729-era or later** (support merged in PR #29416 on
2026-10-10); Myrme pins b10076/b10081 (2026-07-21), so it was measured with the release **b11731**
(prebuilt, `~/bench-scratch/minicpm`), and Qwen3.5-0.8B was measured again on the same build for a
like-for-like comparison. Laptop CPU, 8 threads, on battery, thinking off for both (MiniCPM-V thinks
by default too; `enable_thinking=false` works).

| | MiniCPM-V 4.7 1B Q4_K_M | Qwen3.5-0.8B Q4_0 |
|---|---|---|
| meetings, constrained | 7/17 | **14/17** |
| research exact / grounded | **7/8** / 7/8 | 5/8 / **8/8** |
| edits (one run at temperature 0.2; noisy) | **4/7** | 1/7 (3/7 on b10076) |
| recipe, free (as production) | 21/30 · 227/257 · 1 invented | 20/30 · **248/257** · 1 invented |
| recipe, constrained | 15/30 · 249/257 · 2 | 17/30 · 225/257 · 1 |
| handwriting, 30 pages (house instruction) | 45.4% WER, **733** prompt tokens a page on average, 16.1 s a page (laptop CPU) | 43.4% capped at 1024 (GPU, b10076), 1,135 prompt tokens |
| download | 672 MB + **1,110 MB** vision projector | 507 MB + 205 MB |

- **Neither is better across the board**: Qwen3.5-0.8B is far better at meetings and finds more
  recipe ingredients; MiniCPM-V is better at research and edits. On handwriting they are within two
  points. MiniCPM-V's photos are cheaper in tokens, its projector five times larger to load.
- **Correcting the VLM's transcript against a word list did not help either** (45.4% → 45.9%): the
  VLM + vocabulary question is answered.
- **Not adopted on these numbers**; it would also need the runtime upgrade. Kept in the shortlist
  (`2026-10-19-scouting-phone-models.md`).

### The small LLM: the shortlist and a user's real mail — 2026-10-19

The full numbers are in `2026-10-19-scouting-phone-models.md` §4. The shortlisted models are
MiniCPM5-1B and 2B, Qwen3.5-2B, K2-Horizon-3.7B and LFM2.5-2.6B, all on b11731. The headline:
- **Real mail (the owner's five conversations, private):**

  | Model | Exact /5 | 5 threads |
  |---|---|---|
  | Qwen3.5-0.8B | **3** | 17 s |
  | Qwen3-VL-4B | 4 | 90 s |
  | K2-Horizon | 4 (2 before the past fix) | 101 s |
  | Qwen3.5-2B | 2 (1 before the fix) | 73 s |
  | MiniCPM family | 1 | |
  | the 450M | 0 | |

- **The light LLM stays Qwen3.5-0.8B.** Nothing smaller than 2 GB beats it, and nothing at any
  size beats it per second.
- **A Rust gap found on the way, and fixed:** `resolve` judged "past" by day, so a meeting earlier
  the same day passed as fixed. It now judges by the newest email's time, which is
  model-independent.

## 6. Risks and open questions

| Risk | Why it matters | How the plan meets it |
|---|---|---|
| ONNX Runtime on Android from Rust is not documented by `ort`'s maintainers | the plan's foundation on the phone | the phone phase (P) does it first; the laptop phases do not depend on it |
| APK and download size | every phone carries the runtime; tools add downloads | measured in L0; tools are per-feature offers |
| GLiNER2's ONNX export is several graphs, run by young community engines | L2 might need the older GLiNER, or our own orchestration | start with `gline-rs` (GLiNER); GLiNER2 only if it beats it on our eval |
| Handwriting | no small tool reads it well; the capped VLM reaches 43–45% | handwriting keeps the VLM fallback; a dedicated reader is a later question |
| Languages | a user's mail is English, Italian and Chinese | every eval keeps its Italian and Chinese cases; `gliner_multi`, PP-OCRv5's multilingual dictionaries |
| Licences | some popular tools are non-commercial | each `[[models]]` entry carries its licence; the 2026-10-18 watch-list applies; GLiNER's v0/v1 weights are CC-BY-NC, v2.1 is the pick |
| Two runtimes to maintain | update risk, as the vision encoder's 2.7× slowdown on llama.cpp master showed | pin both; measure before any upgrade |
| iOS | no assistant there today | out of scope; ONNX Runtime ships for iOS if it is ever wanted |

## 7. What changes for formicaria, if L7 is reached

- Its Android packaging stages ONNX Runtime from Myrme's catalogue pin, as it stages llama.cpp.
- Settings shows a third phone choice ("Light"), and offers each tool with its size and licence
  through the flow it already has.
- A Myrme contract minor (new catalogue keys and, if formicaria names them, new items). Nothing is
  removed.

## Sources

- The `ort` crate (ONNX Runtime for Rust), `load-dynamic`: https://docs.rs/ort
- `oar-ocr` (PP-OCRv5 through ONNX Runtime, in Rust): https://docs.rs/crate/oar-ocr/0.8.1
- `gline-rs` (GLiNER inference in Rust): https://docs.rs/crate/gline-rs/latest
- GLiNER2 ONNX exports are split into several graphs run by the host:
  https://huggingface.co/jugaadsrl/gliner2.5-multi-v1-onnx ; a production GLiNER2-in-Rust report:
  https://users.rust-lang.org/t/building-a-production-multilingual-pii-detection-system-in-rust-with-gliner2/139964
- Word Beam Search, CTC decoding with a dictionary: https://repositum.tuwien.ac.at/handle/20.500.12708/924 ;
  code https://github.com/githubharald/CTCWordBeamSearch
- OCR post-correction with lexicons and confusion candidates: https://arxiv.org/pdf/1604.06225 ;
  https://lhncbc.nlm.nih.gov/system/files/pub2010020.pdf
- Lexicon size against accuracy in handwriting: https://cedar.buffalo.edu/~govind/faisalThesis.pdf
- TASTEset (recipe entities): https://arxiv.org/abs/2204.07775 ; ingredient-parser:
  https://ingredient-parser.readthedocs.io/en/stable/resources/index.html
- Candidates, sizes and licences per task: `2026-10-18-dedicated-methods-per-task.md`.
- Every "today" number: `2026-10-19-speed.md` (§2.2, §2.3, §2.7) and the method evals.
