# The best small on-device method for each task (research, 2026-10-18)

> **Research, not a ruling.** About 19 searches and fetches by a research agent on 2026-10-18,
> condensed here. **[mem]** = the agent's prior knowledge, not re-checked; **[unverified]** =
> secondary sources only. Sizes are approximate. Nearly every accuracy figure is vendor-reported.
> **None of this replaces a 30–50 item eval on the owner's own data.**

Budget assumed: a phone gives ~1–2 GB to a model; **one generative model resident at a time**;
non-LLM specialists (OCR, pose, ASR, embedder) are each under ~250 MB.

## (a) Email → meetings and tasks
Order: **ICS first, rules for dates, a small encoder for entities and intent, an LLM only for
leftovers.**
| Candidate | Size | License | Runtime | Phone | Note |
|---|---|---|---|---|---|
| ICS parse (`text/calendar` part) | — | — | native | yes | METHOD REQUEST/CANCEL, DTSTART, LOCATION, SEQUENCE cover invites, call-offs, moves with no model [mem] |
| dateparser | lib | BSD-3 [mem] | Python | laptop | 200+ locales |
| chrono-node | lib | MIT [mem] | JS | yes (WebView) | multi-locale, partial for some |
| Duckling | lib | BSD-3 [mem] | Haskell server | poor | known relative-date bugs |
| **GLiNER2** | 205–340M | Apache-2.0 | PyTorch; community ONNX | int8 [unverified on Android] | NER + classification + structure in one schema call, CPU (arXiv 2507.18546) |
| gliner_multi-v2.1 | ~209M | Apache-2.0 (**v0/v1 are CC-BY-NC**) | ONNX | yes | |
| **NuExtract-2.0-2B** GGUF | ~1.2–1.6 GB Q4 + mmproj | MIT | llama.cpp | borderline | multimodal (reads screenshots/PDFs) |
| NuExtract-2.0-4B | 4B | Qwen Research (**non-commercial**) | GGUF | no | avoid |
**Pick:** ICS → date rules → GLiNER2-base int8 with schema
`{event:[title,start,end,place,attendees], intent:[meeting,cancel,reschedule,task,none]}`.
**Fallback:** NuExtract-2.0-2B. GLiNER gives spans, not datetimes: a date parser always follows.
*(formicaria's current meeting pass already has Rust date reading — `fm_agent::meetings` — which
plays the dateparser role in-process.)*

## (b) Image → text
| Candidate | Size | License | Runtime | Phone | Note |
|---|---|---|---|---|---|
| **PP-OCRv5 mobile** det+rec | ~10–20 MB [mem] | Apache-2.0 | Paddle-Lite / ONNX | yes, fast | printed text, many scripts |
| **PaddleOCR-VL** 0.9B (v1.5) | <1 GB [unverified] | Apache-2.0 [mem] | transformers, vLLM; llama.cpp since ~b8110 [unverified] | Q4/Q8 | 109 languages, OmniDocBench v1.5 94.5 (arXiv 2601.21957) |
| Granite-Docling-258M | 258M | Apache-2.0 | transformers; community GGUF | yes | page → Markdown/DocTags |
| Qwen3-VL-2B-Instruct | ~1.5 GB total | Apache-2.0 [mem] | official GGUF, mtmd | borderline | only general "read my messy note" option |
| LFM2.5-VL-450M / 1.6B | 0.45/1.6B | LFM Open License (revenue-capped) | GGUF, LiteRT | yes | **on the owner's phone today** |
| TrOCR-small-handwritten | ~61M | MIT | ONNX | yes | line-level only, English |
| Florence-2 base/large | 0.23/0.77B | MIT [mem] | ONNX | yes | weak handwriting [mem] |
| **PP-FormulaNet-S** | small | Apache-2.0 | ONNX | yes | math → LaTeX (arXiv 2503.18382) |
| Texo | ~20M | check repo | ONNX | yes | math, matches PP-FormulaNet-S smaller (arXiv 2602.17189) |
**Pick by content:** printed → PP-OCRv5; structured pages → PaddleOCR-VL (or Granite-Docling where
size matters); math crops → PP-FormulaNet-S/Texo; handwriting/whiteboards → PaddleOCR-VL then
Qwen3-VL-2B. **No on-device handwriting benchmark exists for small VLMs** — build a 50-image eval
from the owner's notes.

## (c) Audio → text and dictation
| Candidate | Size | License | Runtime | Phone | Note |
|---|---|---|---|---|---|
| **whisper.cpp** base/small q5 | 57 MB / ~190 MB; RAM ~388/852 MB | MIT | GGML | yes to small | ~99 languages; **shipped today** |
| whisper large-v3-turbo | 809M | MIT | whisper.cpp | laptop | |
| Parakeet-TDT-0.6B-v3 | int8 enc ~622 MB | CC-BY-4.0 [mem] | sherpa-onnx | yes, RTF ~0.12 | **25 European languages only** |
| **Moonshine v2** streaming | from ~27M | MIT (legacy non-English weights non-commercial) | own runtime, Android/iOS | yes | lowest-latency dictation |
| SenseVoice-Small | ~234M | FunASR licence [unverified] | sherpa-onnx | yes | zh/en/yue/ja/ko |
| Vosk | ~50 MB | Apache-2.0 | Kaldi | yes | low-end fallback |
**Pick:** whisper (multilingual) stays; Moonshine for live English dictation; Silero VAD (~2 MB,
MIT [mem]) in front of all.

## (d) Web research with citations
Retrieval, not generation, makes the citations: allowlisted search → main-content extraction
(trafilatura / Readability) → chunks → BM25 + embedder → rerank → small LLM writes **only from top
chunks citing chunk ids** → Rust checks each cited span exists.
| Candidate | Size | License | Phone | Note |
|---|---|---|---|---|
| **EmbeddingGemma-300M** | <200 MB quantised | Gemma Terms (not OSI) | yes | best open multilingual <500M on MTEB (~61) |
| Qwen3-Embedding-0.6B | 0.6B | Apache-2.0 [mem] | yes | Apache alternative |
| bge-small / multilingual-e5-small | 33M / 118M | MIT [mem] | yes | fallback |
| **Qwen3-Reranker-0.6B** | 0.6B | Apache-2.0 | tight | llama-server `--reranking` (some GGUFs need fixes) |
| ms-marco-MiniLM-L6 | 22M | Apache-2.0 | yes | English phone reranker |
| jina rerankers | — | CC-BY-NC [mem] | — | avoid |

## (e) Note edits and summaries — the one task needing a general LLM
Structured output under a grammar: `{edits:[{op, anchor, old, new, reason}]}`; never whole-note
rewrites. Picks: Qwen3-1.7B (thinking off) phone, Qwen3-4B / SmolLM3-3B laptop; LFM2.5-1.2B,
Gemma 3n E2B (Gemma Terms) as alternatives; TextRank as a model-free fallback.

## (f) Food from a sentence; recipe from a recording
No off-the-shelf small parser exists. Research framing: slot-filling then database lookup
(Korpusik & Glass, https://sls.csail.mit.edu/publications/2017/ASLP17_Korpusik.pdf).
GLiNER2 with labels `food, quantity, unit, preparation` (same model as (a)) → rules for numbers and
units → lookup in the food table **formicaria already carries** (18,338 foods). Recipe: ASR → the (e)
LLM with grammar `{title, ingredients[], steps[]}` → a per-line ingredient parser. **The portion
defaults table ("toast" = 1 slice) matters more than the model.**

## (g) Pose coaching from camera
Coaching = **rules on joint angles** (depth, alignment, rep counting by angle hysteresis), not a
model.
| Candidate | Size | License | Phone | Note |
|---|---|---|---|---|
| **MediaPipe Pose Landmarker** lite/full/heavy | ~3/6/26 MB [mem] | Apache-2.0 | real-time | 33 landmarks + 3D world coords |
| MoveNet Lightning/Thunder | ~3/12 MB [mem] | Apache-2.0 | yes | 17 keypoints, 2D |
| RTMPose-t/s | ~3–6M params | Apache-2.0 | ONNX, ncnn | best AP/FLOP [mem] |
Fixed, guided camera position per exercise; form guidance, never injury claims.

## Router + specialists, and what they cost
1. **Deterministic first:** explicit command; MIME (`audio/*`, `image/*`, `text/calendar`,
   `message/rfc822`); UI surface.
2. **Inside images:** PP-OCRv5 first; high confidence → done, low → escalate to a VLM; formula
   regions → formula model.
3. **Free-text intent:** a tiny classifier (GLiNER2's head, or EmbeddingGemma centroids over ~20
   examples per intent with a confidence threshold); **below threshold, ask the person** (a
   picker); an LLM router last.

| Specialist (Q4/int8) | Disk |
|---|---|
| GLiNER2 | ~0.2–0.3 GB |
| PP-OCRv5 mobile | ~0.02 GB |
| PaddleOCR-VL | ~0.6–1.0 GB |
| formula model | ~0.05 GB |
| whisper small q5 | ~0.2 GB |
| Moonshine | ~0.1 GB |
| EmbeddingGemma | ~0.2 GB |
| MiniLM reranker | ~0.03 GB |
| Qwen3-1.7B | ~1.1 GB |
| MediaPipe pose | ~0.01 GB |
| **All installed** | **~2.5–3 GB** |
Fetch per feature on first use, checksum, size shown first, deletable per feature; pins explicit.
RAM: one generative model resident; encoders co-reside under ~500 MB; mmap'd GGUF; evict on
Android `onTrimMemory`; 1–3 s per swap [unverified]. **Runtimes: keep to two — llama.cpp and ONNX
Runtime — plus MediaPipe only for pose**; prefer ONNX exports of Paddle models.

## Licence watch-list
Avoid (non-commercial): NuExtract-2.0-4B, GLiNER v0/v1, jina rerankers, Moonshine legacy
non-English. Non-OSI but usable: Gemma Terms (EmbeddingGemma, Gemma 3n), LFM Open License
(revenue-capped), SenseVoice [unverified]. Clean: whisper, GLiNER2, gliner_multi-v2.1,
NuExtract-2B, PaddleOCR(-VL), Granite-Docling, Qwen3 small, MediaPipe, MoveNet, RTMPose, Parakeet
(CC-BY-4.0, attribution).
