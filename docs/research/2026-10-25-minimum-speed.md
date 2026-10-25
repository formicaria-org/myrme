# A minimum acceptable speed, and the tools each method would need to meet it (research, 2026-10-25)

> **Research, not a ruling.** Asked for on 2026-10-25: set a minimum acceptable speed for formicaria
> and let it decide which tools and models Myrme uses, since chat on Qwen3.5-0.8B works but feels
> too slow. Speeds in §1–§4 come from Myrme's own earlier measurements (`2026-10-19-speed.md`,
> `2026-10-20-small-model-benchmark-summary.md`) unless marked. **§5 is what was tried the same
> day, on the laptop.** **[est]** = computed from measured rates, not measured as a whole;
> **[mem]** = prior knowledge, not re-checked; **[vendor]** = the maker's own figure.

## 1. The bar: one per kind of wait, in seconds

A single tok/s figure cannot be the bar: what a person waits for differs by method. The human
thresholds are old and stable:
- **0.1 s** feels instant; **1 s** keeps the train of thought; **10 s** is where attention leaves
  and a progress indicator plus a way to cancel are needed (Miller 1968, restated by Nielsen 1993).
- **Silent reading runs at about 238 words a minute** for non-fiction, most adults between 175 and
  300 (Brysbaert 2019, 190 studies). At ~1.33 tokens per English word that is **~5.3 tok/s**, and
  ~6.7 tok/s for a fast reader.

| Class | Methods | Minimum (fails below) | Target |
|---|---|---|---|
| **A. Conversation**, a person waiting to read | chat (@Myrme), `/search` replies | first words within **2 s**, then text arriving at **≥ 8 tok/s** (1.5× reading pace) | first words ≤ 1 s, ≥ 15 tok/s |
| **B. At the point of action** | dictation, a photo of a page, a food sentence | **≤ 2 s** per page or utterance | ≤ 1 s; dictation live |
| **C. Jobs a person started** | research, recipe from a recording, handwriting, edits | **≤ 10 s**, or progress plus cancel | ≤ 5 s |
| **D. Background**, nobody waiting | meetings from mail, titles, the OCR of a pass | no latency bar; budget is battery and heat, e.g. ≤ 5 s a thread | — |

**Why A is written as *first words + rate* and not *whole reply*:** a reply that streams at reading
pace is read as it arrives, so its total length stops mattering. A reply posted only when complete
must fit under ~10 s in total.

## 2. Where Myrme stands against the bar

**Chat replies are not streamed** (`openai.rs` sends `"stream": false`; the reply is posted as one
note), and each turn rereads the note, its links and the whole conversation. Chat has no eval and no
timing, so the chat rows are **[est]** from rates measured inside the other methods: a 1,500-token
context and a 300-token reply.

| Device · model | Reading | Writing | Chat now (whole reply) | With streaming, first words | Writing vs. 8 tok/s |
|---|---|---|---|---|---|
| Test phone (Dimensity 7300) · Qwen3.5-0.8B Q4_0 | ~105 tok/s | ~13–14 tok/s | **~36 s** [est] | ~14 s (rereading) | **passes** |
| Laptop CPU · Qwen3-VL-4B | ~117 tok/s | ~12 tok/s | **~38 s** [est] | ~13 s | passes |
| Laptop CPU · Qwen3.5-0.8B | ~591 tok/s | ~51 tok/s | ~8 s [est] | ~2.5 s | passes |
| Laptop RTX 3050 · Qwen3-VL-4B | — | ~52 tok/s | — | — | passes |

**What this shows:**
- **The phone already writes faster than people read.** 13–14 tok/s is 2.5× reading pace. Chat
  feels slow for two reasons, and neither is the model: the person sees nothing until the whole
  reply exists, and every turn pays again to read the whole context.
- **So, today, the bar picks streaming and prompt reuse first, not a new model.** A faster model
  only shortens a wait that streaming hides anyway.

The other methods, measured on the phone (s per request):

| Method | Class | Phone now | Verdict |
|---|---|---|---|
| meetings | D | 3.7 | fine |
| edits | C | 2.8 | fine for speed (accuracy is the problem, 3/7) |
| research | C | 6.9 + the web | fine, with progress |
| **recipe** | C | **30.9** | **fails**: 405 tokens written at 14 tok/s |
| **handwriting page** | B | **median 28** | **fails** |
| typed page (OCR) | B | ~1 s on the laptop; phone not measured | likely fine |
| dictation (whisper base) | B | not measured on the phone | unknown; it is batch, not live |

## 3. What would meet the bar, per method

Ordered within each method from cheapest to most work. **Every row is to measure, nothing is
adopted.**

### Chat (class A): the bar asks for changes to how Myrme runs, not a faster model

1. **Stream the reply.** It would show first words at the reading cost and then text at
   13–14 tok/s on the phone. It **touches the contract**: formicaria would have to show a reply
   that is still being written (an updated note or a reply-in-progress event), so formicaria
   changes first and contract.md gets a new minor.
2. **Read once, not every turn.** llama-server keeps a slot's prompt cached
   (`cache_prompt`, `--slot-save-path` to keep it across restarts, already listed in `plan.md`).
   Edits already put the notes before the conversation so they stay a cached prefix
   (`assemble_turn_context`); chat would do the same. A later turn then reads only the new message
   (~50 tokens, ~0.5 s on the phone).
3. **Read while they type.** Typing a question takes longer than the ~14 s it costs to read the
   note. Opening a note's discussion (a UI surface, so code still picks the method) could fill the
   slot ahead of the question; the first turn's first words would then come in ~1 s.
4. **A reply-length budget per device.** `max_reply_chars = 2000` is ~500 tokens, ~37 s unstreamed
   on the phone. It matters less once replies stream.
5. **Speculative decoding, only for writing.**
   - **Retrieval / n-gram drafting** (`--spec-type ngram…` on upstream llama.cpp) is what works at
     small sizes. An EACL 2026 study of 1–2B models found separate draft models fail there
     (drafting costs too much), while retrieval-based drafting gives consistent gains. Chat that
     quotes notes, edits and recipes copy their input, which is that case.
   - **MTP:** Qwen3.5 was trained with multi-token-prediction heads, Unsloth publishes MTP GGUFs down
     to 0.8B, and upstream llama.cpp merged MTP drafting (beta, 2026) [unverified flag names:
     `draft-mtp` vs `mtp`]. All reported gains are for 27B and larger; **nothing is published for
     0.8B**. It needs a runtime newer than b10076, and `2026-10-19-speed.md` §3.2 warns that vision
     encoding regressed on Android master, so it would need the encode check before any upgrade.
6. **Another model or engine: not justified by this bar.**
   - Gemma 4 E2B on LiteRT-LM: **47 tok/s CPU writing, 557 tok/s reading** [vendor], on a 2026
     flagship (S26 Ultra), Apache-2.0 per secondary sources. No mid-range figure is published, and
     it would bring a third runtime and its own file format. No published thinking-off scores were
     found, so it is not a download candidate yet (owner's rule, 2026-10-24).
   - LFM2.5-350M: 24 tok/s on a Galaxy A56 class phone [vendor, Cactus engine, INT8]. Fast, but the
     450M of the same family scored 0/5 on real mail.
   - LFM2.5-1.2B ran at about Qwen3.5-0.8B's speed on the test phone (`speed.md` §2.2), so it is not a
     faster option.
   - Gemini Nano through Android AICore is limited to supported (flagship) devices and is not open,
     so it does not fit "runs locally on any device".

### Recipe from a recording (class C, fails at 30.9 s)

The cost is the 405 tokens the model **retypes** from a transcript that already contains them.
- **Point instead of write:** number the transcript's sentences, have the model return sentence
  numbers for steps and notes, and have Rust copy the text. **Tried (§5): not adopted.** The
  estimate here (~40–60 tokens) was wrong: the ingredients are still written and are most of the
  answer, so it saved 31% of the tokens. It also lost ingredients.
- **n-gram speculative decoding** looked suited to copied output. **Tried (§5): no gain** on the
  free answer.

### Dictation and audio (class B)

- **Silero VAD** (~2 MB, MIT [mem]) in front of every recording: no time spent on silence.
- **Moonshine v2 streaming** for live English dictation: Tiny 34M, Small 123M, Medium 245M,
  sliding-window attention with ~80 ms lookahead, MIT per the repackages [unverified upstream
  licence text]. The paper claims accuracy on par with models 6× their size, running much faster
  [vendor]. **English only.**
- **Parakeet-TDT-0.6B-v3** for European languages, including Italian: RTF ~0.12 on Android
  (~150 ms per 1.5 s of audio) [vendor], CC-BY-4.0 [mem], INT8 encoder ~840 MB. It runs on
  **sherpa-onnx**, which sits on ONNX Runtime, the runtime Myrme already ships. It is
  the larger download. (Sources disagree on its language count: 25 European [mem] vs. 114 on one
  guide.)
- whisper stays for every other language.

### Handwriting (class B, fails at ~28 s a page)

**No small, fast page-level handwriting reader was found that is likely to beat the VLM's 44%.**
The candidates read one line at a time:
- **TrOCR-small-handwritten** (~61M, MIT, ONNX exports exist), English. PP-OCRv6's detector, already
  shipped, could cut the page into lines for it.
- Small HTR models per script (e.g. a 10 MB German one) exist, but one model per language does not
  scale.

Until a reader passes, handwriting should be treated as **class C**: shown with progress and a
cancel, rather than as a point-of-action wait.

### Meetings, titles, food (class D / B): already within reach

- Meetings (3.7 s, background) passes. **ICS first** (`plan.md` M1) removes the model from invites
  entirely.
- **GLiNER2 now runs from Rust**: community `gliner2-rs` with fragment ONNX exports (multi-v1, 2.5).
  This answers M6, *food from a sentence*, in milliseconds. Before relying on it, check
  maintainability: it is a community engine, not the authors'.

## 4. What this suggests deciding

1. **Adopt the bar in §1 as the speed rule for methods**, with a method's class in its `METHOD.md`
   and its timing in its eval run (`pixi run eval-speed` already reports s/request on the phone).
2. **Chat: stream + reuse the cached prompt, and leave the model alone.** Reuse already works; on
   Qwen3.5 it needs `-ub 128` to stop rereading (§5.1). Streaming needs a formicaria change (a
   contract minor).
3. **Recipe:** pointing was measured and loses ingredients (§5.2). Recipe stays as it is, and shows
   its progress.
4. **Still to measure:** MTP for the 0.8B. Its GGUF is a separate download, so it is offered first.
   n-gram drafting gave nothing on recipes.
5. **Dictation: Moonshine v2 (English, live) and Parakeet v3 (European) on sherpa-onnx**, with a
   60-clip eval first, the same as M4's.

## 5. Tried, 2026-10-25 (laptop CPU)

**Setup for every run:**
- Myrme's shipped runtime, release b10076 (before v0.1.0).
- Qwen3.5-0.8B Q4_0, the phone's file, at the phone's 4096-token window.
- i7-11800H, `-t 8 --device none`, thinking off.
- On mains power; one server on the laptop at a time.

The probes and the prototype stayed outside the repo.

### 5.1 Chat: prompt reuse, and when the first token comes

The setup:
- three turns on one note of about 1,500 tokens, built from the public eval sources;
- the request built as `turn` builds it (the provided notes, then the conversation, then the ask);
- streamed.

`prompt read` is what the server processed; `reused` is what it took from its cache.

| Server flags | Turn 1 read · first token | Turn 2 read / reused · first token | Turn 3 read / reused · first token |
|---|---|---|---|
| as shipped | 2006 · 4.5 s | 705 / 1490 · 1.7 s | 967 / 1679 · 2.4 s |
| as shipped, a 600-token mail request between turns | 1930 · 3.8 s | 705 / 1490 · 1.6 s | 1028 / 1679 · 2.3 s |
| `-cms 0` | 2006 · 4.5 s | 705 / 1490 · 1.7 s | 967 / 1679 · 2.4 s |
| **`-ub 128`** | 2006 · 4.6 s | **315 / 1874 · 0.9 s** | **319 / 2057 · 0.9 s** |

- **Reuse already works, and survives the mail pass.** The server keeps four slots and picks the
  chat's by its shared start, so a request in between does not evict the note.
- **Qwen3.5 is a hybrid model** (attention plus recurrent layers), and its recurrent state can only
  go back to a saved checkpoint. On these settings the checkpoint falls one 512-token micro-batch
  before the end of the prompt, so each turn rereads ~500 tokens it already had, on top of what is
  new.
- **`-ub 128` moves the checkpoint to within 128 tokens:** each later turn reads about 320 tokens
  instead of 700–970, and the first token comes 2–2.7× sooner. Turn 1 costs 3% more. Spacing the
  checkpoints (`-cms 0`) changes nothing.
- **Images under `-ub 128`:** five handwritten pages (`pixi run eval -- image`, cap 1024) read
  word-for-word the same (79.5% WER on these five, which loop to the 1500-token cap either way), in
  122 s against 116 s.
- **What the phone would see [est]:** a later turn's first token at ~3 s (320 tokens at ~105 tok/s)
  instead of ~7–9 s. The phone's own prefill at `-ub 128` has not been measured, and a smaller
  micro-batch could be slower on its CPU.
- **Streaming:** on the laptop, the first token came at 0.9–4.6 s and the whole reply at 4–11 s.
  On the phone [est], the first token would come at ~3 s (later turns), against ~25–40 s for the
  whole reply.

### 5.2 Recipe: pointing, and n-gram drafting

All 30 recipes, scored as the gate scores them (`recipe_outcome` after `keep_what_was_said`), plus
how many listed steps the kept steps cover. The prototype test is not committed.

| Variant | Exact /30 | Ingredients /257 | Invented | Steps covered /183 | Tokens written / recipe | s / recipe (laptop) |
|---|---|---|---|---|---|---|
| **today: free** | **17** | **245** | 1 | 171 | 314 | 6.2 |
| pointing (sentence numbers for steps and notes, schema) | 14 | 227 | 1 | 173 | 217 | 4.9 |
| free + `--spec-type ngram-mod` | 17 | 245 | 1 | 171 | 314 | 6.2 (draft acceptance 24%) |
| pointing + `ngram-mod` | 14 | 219 | 1 | 169 | 259 | 4.3 |

- **Pointing is not adopted.** It is 21% faster and finds fewer ingredients (−18). One recipe looped
  to the 1500-token cap.
- **n-gram drafting gives nothing on the free answer** and changed the pointed answers. It is
  dropped for recipes.
- The free score here (17/30) is below the 20/30 recorded on release b11731
  (`2026-10-20-small-model-benchmark-summary.md` §2). The runtimes differ.
- **So recipe meets its bar (class C) by showing progress, not by speed.** On the phone that needs
  the phone shell to report activity at all; see the plan.

## Sources

- Miller (1968) / Nielsen, response-time limits: https://www.nngroup.com/videos/3-response-time-limits-interaction-design/
- Brysbaert (2019), *J. Memory and Language* 109, 104047: https://www.Gwern.net/doc/psychology/linguistics/2019-brysbaert.pdf
- Mainardi, Sandikci, Vanschoren, *An Empirical Study of Speculative Decoding for Small Language Models*, EACL 2026: https://aclanthology.org/2026.eacl-long.255/
- Unsloth, MTP GGUFs for Qwen3.5: https://unsloth.ai/docs/models/mtp.md
- llama.cpp MTP merge (secondary): https://startupfortune.com/llamacpp-now-supports-multi-token-prediction-in-beta-and-the-implications-for-local-ai-tooling-are-bigger-than-the-pr-suggests/
- LiteRT-LM figures: https://developers.google.com/edge/litert-lm/overview
- LFM2.5-350M on phones (Cactus): https://cactuscompute.com/blog/lfm2-5-350m
- Moonshine v2: https://arxiv.org/abs/2602.12241
- Parakeet TDT on Android: https://soniqo.audio/guides/parakeet/android
- GLiNER2 ONNX for Rust: https://huggingface.co/jugaadsrl/gliner2-multi-v1-onnx · https://docs.rs/crate/gliner2/0.1.0
- TrOCR-small-handwritten ONNX: https://huggingface.co/Xenova/trocr-small-handwritten
