# Search and retrieval across every view, with small local models (research, 2026-10-29)

> **Research, not a ruling.** Three research agents surveyed the field on 2026-10-29 (retrieval
> methods; structured and multi-step search; what other apps and engines do). A fourth agent then
> checked the 16 claims the recommendations rest on against primary sources, and the main session
> checked the app's own behaviour locally. Every claim below is marked: **[verified]** (checked
> against the primary source by the fact-check, or measured here), **[source]** (seen in the primary
> source by a research agent, not re-checked), **[reported]** (second-hand only) or **[estimate]**.
> **Nothing here was measured on a real notebook**: every number is a public benchmark, a vendor
> card, or a synthetic micro-benchmark. The decisions this informs belong in `../context/plan.md`,
> `../context/decisions.md` and formicaria's `decisions.md`.

**Research on the internet** — where to search, reading pages safely, the research loop — is the
companion `2026-10-29-web-research.md`.

**The question.** People want to find things across every view: foods rich in a vitamin, a
particular exercise, a paper, a note. Can small local models with a careful design do this well,
and what have others built?

## TL;DR

1. **The biggest gains need no model.** Note search today is **unranked**, matches only whole
   leading words, and **cannot find Chinese words or the inside of German compounds** (measured,
   §1). Ranking with weighted BM25 over split fields, CJK bigrams and a trigram fallback are days of
   work and download nothing.
2. **"Foods rich in vitamin K" is a structured question, not a text search.** The amounts are
   already in the food pack. A dictionary parser (nutrient aliases in each language, cue words such
   as *rich*/*riche*/*reich*/*low*) and a fixed SQL template answer most such questions. The model
   is a fallback that **picks from a short list**; it never writes a query (§3.2).
3. **Small models must not write queries, invent ids, decide when to search, or say "nothing".**
   A 0.5B model wrote correct SQL 13.9% of the time *with* a schema grammar; a prompted LLM invented
   79 of 128 filter values and never returned an empty filter; small models that choose when to
   search under-search and hallucinate **[verified]**. Code decides; the model points.
4. **Papers are where meaning matters.** On LitSearch, dense retrieval beats BM25 by 24.8 points of
   recall@5 **[verified]**. An embedder pays for itself there first.
5. **The small embedders became much better in 2026.** granite-embedding-97m-multilingual-r2 (97M,
   Apache-2.0) scores 60.3 on multilingual retrieval against 50.9 for multilingual-e5-small, the
   model this project dropped on 2026-10-20 — and **it loads in the llama.cpp build Myrme already
   ships (b10076)** **[verified]**. That earlier decision was about judging answers, not ranking
   notes; it does not settle this.
6. **No vector database.** 10,000 vectors of 384 numbers scan in about a millisecond in plain Rust
   (measured, §3.9). The vectors fit in the disposable SQLite index.
7. **Rerankers and graph RAG are not free wins.** A weaker reranker *lowered* a strong retriever
   from 64.64 to 58.36 **[verified, vendor table]**; LightRAG's 72–28 win over plain RAG reversed
   once judge bias was removed **[verified]**. Both must earn their place on our own eval.
8. **First build the eval** (§5): an invented fixture corpus with known-item, paraphrase and
   "nothing" queries, exact asserts, and a lexical oracle. Every step after it is gated on it.

## 1. What the app searches today (checked in code and measured)

| View | How it is searched | What fails |
|---|---|---|
| Notes (and papers' PDF text) | One FTS5 table, `fts(id UNINDEXED, text)`, tokenizer `unicode61 remove_diacritics 2` (`fm-core/src/file.rs`). Every word of the query becomes a prefix term and all must match (`fts_match_expr`); `commands::search` sorts by **`updated` descending** | No relevance ranking at all (its doc comment says "ranked"; the code sorts by date). One column, so a title cannot outweigh a body. No synonyms, typo tolerance or meaning |
| Foods (18,338) | FTS5 `names(name, alt)` with `prefix='1 2 3'`, `ORDER BY (src='usda'), bm25(names)` (`fm-core/src/pack.rs`) | Names only. The amounts (`amount.per100` by `nutrient.key`) are stored and cannot be searched, so "rich in vitamin K" has no path |
| Exercises (117 carried cards) | A substring filter in the UI over `name`, `group`, `area`, `equipment`… (`ExercisesScreen.svelte`) | No aliases ("RDL", "quads", "lats"), no muscle vocabulary |
| Myrme `/research` | Web snippets through the local search proxy; quotes checked by `grounding::verify` | Does not search the vault |

**Measured on SQLite 3.46.1 with the app's tokenizer (2026-10-29):**

| Query | Note text | unicode61 (today) | trigram |
|---|---|---|---|
| `世界` | 你好世界 我喜欢学习 | **no match** | no match (2 characters) |
| `学习` | same | **no match** | — |
| `喜欢学` | same | — | match |
| `beratung` | Ernährungsberatung für Sportler | **no match** | match |
| `legumes` | vitamine K et légumes verts | match | match |
| `cafe` | Café crème | match | match |

So accents already work; Chinese words and the inside of compounds do not. `unicode61` keeps a run
of CJK characters without spaces as one token, and most Chinese words are two characters, below the
trigram minimum of three.

## 2. The rules any design must keep

From `CLAUDE.md` and formicaria's rulings: **code selects the method**; **the model points, Rust
reads and computes**; everything the assistant produces is a proposal; **nothing downloaded
unasked**; every method ships with an exact-match eval including "nothing"; two runtimes only
(llama.cpp and ONNX Runtime); no CC-BY-NC weights. A food search or a lexical search must work with
the assistant off, so those belong in formicaria, not in Myrme.

**On subagents versus one loop** (Anthropic, *Building agents: when to fold subagents into your main
agent loop*, 2026-10-22): its single open-ended tool loop assumes Opus-class models and does not
transfer to 0.8B–4B. Two parts do transfer. Tools grouped *by user intent* with fixed-shape results
(one search per view, returning ids) is what §3.2 proposes. And its eval advice — start with about
ten golden cases, compare **which cases flipped**, and run each case **more than once** — goes into
§5.

## 3. Findings

### 3.1 Lexical search done well (formicaria, no model, no download)

- **Rank.** FTS5's `bm25()` takes one weight per column, and `ORDER BY rank` with weights stored
  once (`INSERT INTO fts(fts, rank) VALUES('rank', 'bm25(10.0, 5.0, 3.0, 1.0)')`) is the fast form
  **[source: https://www.sqlite.org/fts5.html]**. It needs the index split into `title, tags,
  headings, body` columns first. The index is disposable, so this is a version bump and a rebuild.
- **Prefixes.** `prefix='2 3'` makes `term*` fast; a prefix also stands in for stemming in French,
  German and Italian **[source]**.
- **CJK.** Split CJK runs into overlapping character bigrams in Rust, at index and at query time
  (the idea of Lucene's CJK bigram filter); a one-character query falls back to `LIKE`
  **[reported]**. Our measurement in §1 is the failure this fixes.
- **Partial words.** A second table with `tokenize='trigram remove_diacritics 1'`, asked only when
  the main table returns little, finds compound parts and identifiers (measured, §1).
  `remove_diacritics` on trigram needs SQLite 3.45 or later **[reported]**; the bundled 3.46 has it.
- **Typos.** `fts5vocab` lists the index's terms; an edit-distance pass in Rust gives "did you mean"
  **[source]**.
- **Stemming.** `porter` is English only. Snowball stemmers for 18 languages (not Chinese) exist in
  Rust (`rust-stemmers`, used by tantivy) and could feed a stemmed shadow column, which needs
  per-note language detection **[source: https://docs.rs/tantivy/latest/tantivy/tokenizer/enum.Language.html]**.
  Moving to tantivy itself is not justified: the gaps are in the schema and tokenizer, not the
  engine (and tantivy's Android issues #2480/#2635 are open **[reported]**).
- **Document expansion** (Doc2Query--: up to +16% after filtering hallucinated questions
  **[reported: https://arxiv.org/abs/2301.03266]**) costs one LLM pass per note; laptop-only and
  late, if ever.

### 3.2 Questions that are really filters: foods, exercises, papers

**Evidence on letting a model build the query:**

- Grammar-constrained text-to-SQL, Qwen2.5 on Spider (*Beam Search, Self-Consistency, and the Limits
  of Inference-Time Scaling for Grammar-Constrained Text-to-SQL in Small Language Models*, 2026)
  **[verified: https://arxiv.org/abs/2608.25761]**:

  | Model | No grammar | Grammar, greedy | Grammar, beam 8 |
  |---|---|---|---|
  | 0.5B | 13.5% | 13.9% | 24.5% |
  | 1.5B | 32.6% | 35.1% | 50.5% |
  | 3B | 47.7% | 44.5% | 56.2% |
  | 7B | 64.3% | 60.0% | — |

  A grammar guarantees valid output, not correct output, and at 3B and 7B it *hurt* greedy decoding.
- Filter extraction against a closed list of 49 values (Multi-Meta-RAG, 2,556 queries, set-exact)
  **[verified: https://arxiv.org/abs/2607.03929]**: a linear probe on 135M–1.5B models 90.9%, a
  **plain substring match 88.0%**, prompted GPT-3.5 80.9%. GPT-3.5 emitted 128 distinct values, 79 of
  them not on the list, and never predicted an empty filter; all of its deficit came from queries
  that needed no filter. On the rest the three were within 1.1 points.
- Strict output formats can hurt *reasoning* ("Let Me Speak Freely?",
  https://arxiv.org/abs/2408.02442 **[source, abstract]**; disputed for classification by dottxt's
  reply **[reported]**). So: constrain short classification slots, keep reasoning out of the JSON.

**What other tools do without a model.** Nutrient rankings (USDA FoodData Central's nutrient search,
MyFoodData's lists, Open Food Facts' `<nutrient>_100g` comparisons) all work the same way: choose a
nutrient, sort, filter by food group, switch between per 100 g, per serving and per calorie
**[reported]**. Exercise databases filter on closed muscle and equipment vocabularies **[reported]**.

**The design this points to** — deterministic first, the model only on what is left, and only
pointing:

1. **Foods by nutrient** (formicaria, works with the assistant off).
   - An alias table per nutrient key in each language the food tables use, seeded from
     `fm-app/src/nutrients.rs`'s display names plus published synonyms (for vitamin K1, MeSH lists
     phylloquinone, phytomenadione, phytonadione **[reported]**). About 60 keys × ~6 aliases.
   - Cue words for direction (*rich, high, lot / riche / reich / ricco* → descending; *low, without,
     pauvre, arm* → ascending), basis (*per 100 kcal*, *per portion*; default per 100 g), and a number
     with a unit converted to the key's unit in Rust.
   - Leftover words → FTS on `names` or the `grp` group. A fixed SQL template:
     `… WHERE n = ? [AND grp = ?] [AND per100 >= ?] ORDER BY per100 DESC LIMIT 50`.
   - **A missing amount is unknown, not zero**: say how many foods have the nutrient measured. Offer
     per 100 kcal, because per 100 g puts dried herbs and spices on top.
   - Only when no nutrient is recognised: one Qwen3.5-0.8B call under a schema,
     `{answerable, nutrient ∈ shortlist, direction ∈ {high, low}, basis ∈ {…}, group ∈ grp | null,
     threshold | null}`, with `answerable` first (the field that moved research from 7/8 to 8/8).
     Rust builds the query. The model never states a value.
2. **Exercises.** Aliases for names (RDL → Romanian deadlift), a lay-to-anatomical muscle table
   (quads, lats, chest), the closed equipment list including *none*. Rank by primary muscle, then
   text. A model fallback may only point at one of the top candidate ids.
3. **Papers.** Patterns for DOI, arXiv id and ISBN → exact lookup; author and year → a property
   filter; date phrases ("in March") → a date range in code (LongMemEval found time-aware expansion
   worth +6.8–11.3% recall on temporal questions **[reported: https://arxiv.org/abs/2410.10813]**).
   Then BM25 plus an embedder, fused (§3.5). SPECTER2 (Apache-2.0, has a short-query adapter) is a
   paper-specific option **[source: https://huggingface.co/allenai/specter2]**, to measure against a
   general multilingual embedder.
4. **One search box for everything: fan out, do not route.** Each view's deterministic searcher runs;
   results come back in sections (Notes, Foods, Exercises, Papers), ordered by which view's
   dictionary matched the query. Nothing is hidden, so a wrong guess costs a scroll, not a wrong
   answer. A search box inside a view searches that view. This keeps "code selects the method" by
   construction.

### 3.3 Small embedders

Fact-checked rows are marked; others are from the cards as read by the research agent.

| Model | Params | File | Licence | MMTEB multilingual retrieval | Notes |
|---|---|---|---|---|---|
| **granite-embedding-97m-multilingual-r2** (IBM, 2026-04-29) | 97M (28M without the vocabulary table) | ONNX `quint8_avx2` 98 MB (**x86 only**); fp32 390 MB | Apache-2.0 | **60.3** | 384-d, 32k context, no Matryoshka, CLS pooling. **Loads in llama.cpp b10076/b10081** (pre-tokenizer from PR #22716, 2026-06-02); no official GGUF — convert ourselves, not with a community file made before that PR **[verified]** |
| multilingual-e5-small (dropped 2026-10-20) | 118M | ONNX int8 118 MB | MIT | 50.9 | the baseline **[verified, via granite's card]** |
| bekko-embedding-v1-a8m (2026-07-28) | 106M total, 7.7M active | ONNX 130 MB | MIT | 56.2 | Fastest on a Pi 5 (33 docs/s vs 19 e5-small, 10 granite) — **but measured with OpenVINO; its card calls ONNX Runtime 1.9× slower on a Pi 5** and marks its quantised ONNX files `_not_recommended`. Languages "broad but uneven" **[verified]** |
| EmbeddingGemma-300M (2025-09) | 308M | GGUF Q8_0 334 MB | **Gemma Terms of Use** (not OSI); the google/ repo is **gated** | 62.5 **[source]**; MMTEB mean over all tasks 61.15 at 768-d, 59.68 at 256-d **[verified]** | 2k context; document prompt `title: {title or "none"} \| text: …` **[verified]** — notes have titles, which is free context |
| Qwen3-Embedding-0.6B | 596M | GGUF Q8_0 639 MB | Apache-2.0 | 64.64 **[verified]** | Matryoshka 32–1024, 32k; laptop |
| BGE-M3 | 568M | — | MIT [reported] | 54.6 (MIRACL 69.6) | strong on MIRACL, weaker on MMTEB-R |
| harrier-oss-v1-270m (Microsoft, 2026-03) | 268M | bf16 only | MIT on the card, built on Gemma 3 | 66.4 | weak BEIR/MIRACL in another table — possibly tuned to MMTEB; licence lineage unclear |
| potion-multilingual-128M (static) | — | 512 MB | MIT | 37.9 (MTEB retrieval) | too weak for retrieval |
| jina-embeddings-v5 nano/small | — | — | **CC-BY-NC — excluded** | — | — |

- **Leaderboards disagree**: on LightOn's BEIR/MIRACL table granite-311m and harrier fall below
  EmbeddingGemma **[source: https://huggingface.co/lightonai/mDenseOn]** — and LightOn is itself a
  competitor. Only our own eval settles the choice.
- **Shrinking vectors.** EmbeddingGemma loses 1.5 points going 768 → 256 dimensions **[verified]**.
  int8 vectors keep ~97% of quality, ~99% with re-scoring; binary ~92.5% / ~96% **[source:
  https://huggingface.co/blog/embedding-quantization]**. The brute-force test (§3.9) saw int8 change
  the top result in 1 of 5 queries at 100k × 768: re-score the int8 top-50 in full precision.
- **Learned sparse and late interaction** (OpenSearch multilingual sparse, mSlideOn, mLateOn +
  the Rust `next-plaid` engine) are credible on a laptop, especially mLateOn for long papers (MLDR
  77.9), but have gaps in German/Italian or Chinese and cost more storage **[source]**. SPLADE-v3 is
  CC-BY-NC-SA. Not first.

### 3.4 Rerankers

- **They can make results worse.** In Qwen's own table, reranking Qwen3-Embedding-0.6B's top 100
  (64.64 alone) with bge-reranker-v2-m3 gives 58.36 and with gte-multilingual-reranker 59.44;
  Qwen3-Reranker-0.6B gives 66.36 **[verified; vendor-run]**. FutureHouse found the same for
  PaperQA2: a weak reranking model was worse than none **[source:
  https://arxiv.org/abs/2409.13740]**.
- Qwen3-Reranker-0.6B (Apache-2.0, GGUF Q8_0 639 MB) runs on llama.cpp's `--reranking` /
  `/v1/rerank` **[verified]**.
- Horizon-Labs multilingual-reranker-small (141M, Apache-2.0, int8 ONNX 268 MB, MIRACL 0.779) exists,
  but its score is **in-domain** (it trains on MIRACL), on 60 queries per language, author-run; and
  **ModernBERT rerankers need llama.cpp changes merged after b10076**, so it is ONNX-only for us
  **[verified]**.
- **CPU cost is the real limit.** AnythingLLM's source notes mxbai-rerank-xsmall at 6 s for 18
  documents and ms-marco-MiniLM-L6 at 1.6 s on a desktop CPU, and caps batches at 10 because
  onnxruntime-node does not return memory **[source]**; Khoj made reranking opt-in for the same
  reason **[verified]**. A reranker, if any, is a laptop "search harder" step over the top 20–30.

### 3.5 Fusion and context

- **Hybrid.** Start with Reciprocal Rank Fusion (k = 60). Bruch et al. find a convex combination of
  normalised scores beats RRF and can be tuned from few labelled queries, and that RRF is sensitive
  to k **[verified: https://arxiv.org/abs/2210.11934]** — which suits a project that keeps
  exact-match evals.
- **Context on each chunk.** Anthropic's contextual retrieval cut top-20 retrieval failures by 35%
  (5.7% → 3.7%), 49% with contextual BM25, 67% with a reranker (→ 1.9%) **[verified:
  https://www.anthropic.com/news/contextual-retrieval]** — with a large model writing the context.
  The free, deterministic version for notes: prefix every chunk with the note's title, heading path
  and tags in Rust. Logseq's semantic-search author found bare blocks "work poorly" without their
  page and parents **[source]**; Khoj keeps the heading with each 256-token chunk **[source]**.
- **Chunks.** Embed most notes and cards whole; split long notes and papers at headings or paragraphs
  (~256–512 tokens). Short chunks suit fact lookup, longer ones broad questions **[source, abstract:
  https://arxiv.org/abs/2505.21700]**.
- **No HyDE or query rewriting.** "A strong negative correlation between retriever performance and
  gains from expansion" **[verified: https://arxiv.org/abs/2309.08541]**, and a 0.8B model knows
  little. At most the model may point at keywords in a long question; Rust runs the searches.

### 3.6 Graph RAG: not at this scale

LightRAG's reported 72–28 win over plain RAG becomes a slight plain-RAG advantage once the LLM
judge's order and length biases are removed **[verified: https://arxiv.org/abs/2506.06331]**. Plain
RAG is better at single-hop and detail questions **[source: https://arxiv.org/abs/2502.11371]**.
HippoRAG 2's gains used a 70B extractor and a 7B embedder **[reported]**. No published evidence
shows an LLM-built graph beating hybrid retrieval on a personal-size corpus. **The notebook already
has an exact graph** — links, backlinks, tags, properties, dates. Use it in code: a one-hop
expansion from the top hits, a small boost for notes linked to them, tags as facets. That needs its
own eval case.

### 3.7 Research over the vault, and grounding

- **Keep the loop fixed.** Small models search less and hallucinate more when they decide for
  themselves, and adaptive search often hurts **[verified: https://arxiv.org/abs/2604.04651]**
  (vanilla Qwen3-0.6B scores 19.4 on HotpotQA there; Qwen3-4B 53.4; a Qwen3-1.7B trained always to
  search 58.2). IRCoT — retrieve, write one sentence, retrieve with it — gave +11–21 recall points
  with untrained 0.7–11B models **[reported: https://arxiv.org/abs/2212.10509]** and fits a loop that
  code runs for K steps.
- **Code decides "nothing".** Retrieval reduces a model's ability to abstain, and small models
  abstain erratically **[reported: https://arxiv.org/abs/2411.06037]**. Every app sets a score
  threshold, and every one differently: Khoj 0.18 distance **[verified]**, Jan 0.3 **[verified]**,
  ML Kit 0.5 **[source]**, Open WebUI 0 (off) **[source]**. Ours must be calibrated on the eval's
  "nothing" cases and stored with the model's signature.
- **Whole document when it fits.** Jan inlines a file that fits in 75% of the context instead of
  retrieving from it **[source]**; LM Studio, Open WebUI and AnythingLLM do the same. For a question
  about one note or paper, Myrme should pass the whole text.
- **Grounding** stays as now: quote first, check in Rust. MiniCheck (MIT encoders, ~GPT-4 on
  LLM-AggreFact) could check paraphrased claims as a second, laptop-only signal **[reported]**.

### 3.8 What others built

| App | Retrieval | Lesson |
|---|---|---|
| Jan (Tauri, Rust; Apache-2.0) | MiniLM GGUF **served by llama.cpp**, rusqlite, sqlite-vec if present **else linear search in Rust** **[verified]** | The closest template to formicaria + Myrme |
| Copilot for Obsidian | v4 **retired its in-plugin embedding index**; semantic search moved to a separate local service ("Miyo"); keyword search otherwise **[verified]** | An embedding index inside the app is a maintenance burden |
| Obsidian Omnisearch | pure BM25, the most-downloaded search plugin (~1.3M) **[reported]** | Lexical done well satisfies most people |
| Smart Connections | dense, note and block level, JSON store in the vault | Top issues: re-creating embeddings (#496), "no result found", freezes on large vaults **[source]** → cache by content hash |
| Khoj | gte-small, optional mxbai reranker, threshold 0.18, 256-token chunks with heading **[verified]** | Reranking opt-in for CPU cost; changing the model forces a full re-index |
| AnythingLLM | MiniLM, LanceDB | its own docs say RAG returns 4–6 chunks and "would not know what you mean" **[source]** |
| paperless-ngx 3.x | tantivy for text; sqlite-vec for AI, off by default | had to write a compaction step because sqlite-vec deletes never free space (#54, open) **[verified]** |
| PaperQA2 | text search → dense top-k → LLM rerank and summary → answer | a weak reranking model was worse than none **[source]** |
| Screenpipe (Rust) | added embeddings in 2025, then went **FTS5 only**; benchmarks FTS recall against `LIKE` as ground truth **[source]** | a cheap lexical-oracle test worth copying |
| Android ML Kit + AppSearch | keyword while typing, semantic once the query is complete; int8 vectors; **each vector tagged with the model's signature, re-indexed on mismatch** **[source]** | the storage design to copy |
| Reor | Electron + LanceDB + JS embedder | archived **[source]** |

### 3.9 Engines, and why plain Rust is enough

- **Brute force, measured** (a throwaway Rust benchmark: i7-11800H, one thread, `-O3`, random data):

  | Vectors | f32 memory | f32 ms/query | int8 memory | int8 ms/query |
  |---|---|---|---|---|
  | 10k × 384 | 15 MB | 1.1 | 3.8 MB | 0.4 |
  | 100k × 384 | 154 MB | 12 | 38 MB | 3.7 |
  | 100k × 768 | 307 MB | 22 | 77 MB | 7.4 |

  A phone is perhaps 2–4× slower **[estimate]**. A notebook of 10k notes, papers and the food table
  is roughly 30–60k chunks **[estimate]**. Store vectors as BLOBs in the disposable index.
- **sqlite-vec**: pre-v1, stable 0.1.9 (2026-03-31), last commit 2026-05-18, delete leak #54 and the
  Android tracking issue #68 both open **[verified]**. No speed gain at our size. Not now.
- **fastembed-rs** pins `ort =2.0.0-rc.15`/api-24, which conflicts with Myrme's `=2.0.0-rc.12`/api-22
  whatever its features, and downloads from Hugging Face by default **[verified]**. Not usable; its
  model list is a useful catalogue.
- **LanceDB, qdrant-edge, tantivy, candle**: too heavy, a third runtime, or a duplicate of FTS5.
- **llama.cpp** (already shipped): `--embedding`, `--pooling`, `--reranking` → `/v1/rerank`, all
  present at b10076. **Router mode** (since late 2025) serves several models on one port, but
  **starts a child process per model** — so chat and embedding are two resident models either way
  **[verified]**.

### 3.10 The phone

- Published per-passage phone numbers are scarce. EmbeddingGemma-300M on a Snapdragon X Elite
  laptop: 15.2 chunks/s on the NPU against 1.7 on the CPU **[source:
  https://arxiv.org/html/2606.11257v1]**. Estimate for a MiniLM-class model on a mid-range phone CPU:
  ~100–250 ms per 256-token chunk, so a first index of 30k chunks takes 1–2 hours and a saved note
  well under a second **[estimate — measure before promising anything]**.
- **Embed on save, never on start; cache by (model signature, sha256 of the chunk) in a file that
  survives the index rebuild** — re-embedding is the most-reported failure (§3.8). A phone could take
  vectors built on the laptop when the signature matches, as derived data.
- Without a downloaded embedder, search is lexical — which, after §3.1, is good.

## 4. Recommended order (each step gated on §5's eval)

| Step | Where | What | Cost | Download |
|---|---|---|---|---|
| **S0** | formicaria | The eval (§5) and a lexical oracle test | ~2 days | none |
| **S1** | formicaria `fm-core` | Ranked lexical: split `fts` into `title, tags, headings, body`; stored `bm25` weights; `ORDER BY rank`; `prefix='2 3'`; fix the "ranked" comment | ~1 day | none |
| **S2** | formicaria `fm-core` | CJK bigrams; trigram fallback table; `fts5vocab` typo suggestions; optionally Snowball shadow column | ~1–2 days | none |
| **S3** | formicaria `fm-app` + UI | Foods by nutrient (alias table, cue words, SQL template, coverage, per-100-kcal); exercise aliases and muscle table; paper identifiers and date phrases; one box that fans out into sections | ~1 week | none |
| **S4** | Myrme | Model fallback for S3's leftovers: one constrained 0.8B call that picks from a shortlist | ~2 days + eval | none (model already there) |
| **S5** | Myrme + formicaria | Opt-in semantic search: granite-97m-r2 converted to GGUF by us (parity-checked against the official output), served by the existing llama-server; int8 BLOBs; content-hash cache; RRF, then a tuned convex combination; context prefix per chunk; papers first | ~1–2 weeks | ~100 MB, offered with size and licence |
| **S6** | Myrme, laptop | Research over the vault: a fixed IRCoT-style loop over S1/S5 results; whole-note inlining when it fits; quotes checked as now | later | none |
| **S7** | laptop, conditional | A reranker over the top 20–30 (Qwen3-Reranker-0.6B GGUF), only if the eval shows a gain | later | ~640 MB |

**Not recommended now:** HyDE or LLM query rewriting; a model writing SQL; LLM-built knowledge
graphs; static embeddings; learned sparse; ColBERT on the phone; sqlite-vec, LanceDB, fastembed-rs.

**Open for the owner** (decisions, not research):
1. **Where semantic search lives.** The embedder runs in Myrme's llama-server, but search must work
   with the assistant off. Options: a second, small "search" model in Myrme's catalogue that can be
   enabled without the assistant; or vectors only while the assistant is on, lexical otherwise.
2. **Which embedder.** granite-97m-r2 (Apache-2.0, smallest, loads today) versus EmbeddingGemma
   (stronger on some tables; Gemma terms; gated upstream). The eval decides; the licence may decide
   first.
3. **The phone first index**: an hour or two of CPU on battery. Charging only? Or vectors from the
   laptop?

## 5. The eval to build first

Fully invented fixtures (the repo is public), committed and frozen:

1. **Corpus.** ~150 notes on invented topics with near-duplicates and synonyms; ~20 invented paper
   abstracts with fictional authors and DOIs under a reserved prefix; an invented food slice with
   nutrient amounts; ~20 exercise cards. Traps: the same word in two senses, a number shared by two
   notes, accented and non-English spellings, Chinese text, German compounds.
2. **Queries** (~150–250), each with `expected: [ids]` graded (2 = the target, 1 = acceptable) or
   `expected: []`:
   - known item (title words, a code, a number); paraphrase with no shared words; a heading inside a
     long note; cross-type (a paper and a note);
   - structured: "foods rich in vitamin K", "aliments riches en fer", "low-sodium cheese", "more than
     10 mg of vitamin C", a chemical name (phylloquinone), a misspelling; "a back exercise without
     equipment", "RDL"; DOI, author-year, "the paper about … I added in March";
   - **"nothing" (≥ 20%)**: absent topics, near-misses sharing words but not meaning, a nutrient not
     in the pack, a muscle and equipment pair with no card.
3. **Generation.** A local model may draft queries from ~8 hand-written examples
   (InPars/Promptagator style, https://arxiv.org/abs/2202.05144, https://arxiv.org/abs/2209.11755);
   a person reviews them and they are frozen. **Never keep only the queries today's search already
   answers**: that bakes its blind spots into the test. "Nothing" and paraphrase cases by hand.
4. **Metrics per query class and per mode** (today's search / S1 / S2 / hybrid / +rerank):
   Recall@5, Recall@10, MRR@10, nDCG@10; **null-precision** (share of "nothing" queries that return
   nothing); **false-empty rate**. For the structured parsers: exact match on the parsed record, and
   parser-only coverage reported apart from parser + model.
5. **Gates.** Exact asserts on a "must" subset (known-item top-1, every "nothing" case); aggregates
   with a small tolerance. Compare ranks, not raw scores. **Run model-dependent cases more than once
   and report which cases flipped**, not only the total.
6. **Pin** the model file's sha256, pooling, normalisation, chunker version and threshold with the
   model signature. Set the threshold between the 95th percentile of "nothing" scores and the 5th
   percentile of target scores.
7. **Lexical oracle** (Screenpipe's idea): FTS results against a substring scan over the fixtures,
   to catch tokenizer regressions (accents, CJK, compounds).
8. **No LLM judge as a gate**: such judgments can be gamed and favour their own family
   **[source: https://arxiv.org/abs/2412.17156]**.
9. Runs on a real notebook stay on the machine; only aggregate numbers may enter this repo.

## 6. The fact-check (2026-10-29)

Sixteen claims were checked against primary sources (model cards and the Hugging Face API, arXiv,
GitHub at the b10076 tag, crates.io). **None was invented**; every paper and model exists and the
headline numbers match. What changed the recommendations:

| Claim as first reported | After the check |
|---|---|
| granite-97m-r2 needs ONNX Runtime; llama.cpp support unconfirmed | **Loads in b10076** (PR #22716); no official GGUF; community GGUFs older than 2026-06-02 may tokenise wrongly |
| granite-97m-r2 is 98 MB on a phone | the 98 MB file is an **x86 AVX2** quantisation; on ARM, quantise ourselves or use fp32 (390 MB) |
| bekko is 3× faster than granite on a Pi 5 | **with OpenVINO**; ONNX Runtime is 1.9× slower there by its own card |
| Horizon reranker MIRACL 0.779 | in-domain, 60 queries a language, author-run; **not loadable by b10076** |
| one llama-server could hold chat and embeddings | router mode: one port, **one process per model** |
| EmbeddingGemma downloadable | google/ repo **gated**; ggml-org mirror ungated, Gemma terms either way |
| "distilled Qwen3-1.7B 58.2 on HotpotQA" | 58.2 is the always-search-trained variant; plain distilled 47.9 |

## 7. Not measured, and still open

- **Nothing on a real notebook.** Every retrieval number above is a public benchmark or a vendor
  card.
- No phone measurement of any embedder or reranker under llama.cpp or ONNX Runtime.
- No measurement of Qwen3.5-0.8B filling a filter record; no independent number exists for any ≤1B
  model on this. S4's eval answers it.
- granite-97m-r2 GGUF parity against the official model: to check after converting.
- Whether one-hop link expansion helps on a notebook: untested anywhere.
- SPECTER2 against a general embedder on non-English papers: unknown.
- Licences seen only second-hand: bge-reranker-v2-m3, gte rerankers (Apache), BGE-M3 (MIT); the LFM
  licence was not read; harrier's Gemma lineage is unresolved.

## How this was made

Three research agents ran in parallel (about 121k, 135k and 152k tokens; ~190 searches and fetches
in all), then one fact-check agent (about 109k tokens, 42 calls) read their reports and checked the
load-bearing claims; one agent (about 21k) fetched the subagent article. About 540k tokens and five
agents in all, under the owner's cap of five agents and 1M tokens. The main session measured the
tokenizer behaviour in §1 and read the app's search code. The full agent reports are
not kept in the repo; this document is their condensed, checked form.
