# Making the fast phone model more accurate (research, 2026-10-19)

> **Status: research only; nothing adopted.** Nothing was run for this document: every number marked
> *measured* comes from `docs/research/2026-10-19-speed.md` (§2.3, §2.7) or from the answers recorded
> in `methods/*/eval/cases.json`. Every other claim is *reported* by the source named beside it, or
> marked as an *estimate*. A measure is adopted only by a dated entry in `docs/context/decisions.md`.

**The question.** The phone runs LiquidAI **LFM2.5-VL-450M** (llama.cpp b10081, CPU, Dimensity 7300;
moving from Q8_0 to Q4_K_M for speed). It is fast and often wrong on our methods. How can prompting
and fine-tuning make *this* model more accurate without making it slower?

**Scope.** Prompting and schema design (§2), fine-tuning (§3), training data (§4), licence (§5),
expected gains and risks from the literature (§6). §7 ranks the measures; §8 is a first experiment
for under a day with the existing evals.

## 1. What the 450M gets wrong (measured, ours)

Accuracy on the laptop GPU (speed doc §2.3 and §2.7; one run per row unless stated):

| method | 450M Q8_0 | 450M Q4_K_M | Qwen3.5-0.8B Q4_0 | qwen3-vl-4b |
|---|---|---|---|---|
| meetings, exact (of 17) | 5 (7 with layout B, 2 runs) | 5 | 13 | 16 (layout B, 3 runs) |
| research, exact / grounded (of 8) | 5 / 4 | 6 / 4 | 4 / 8 | 7 / — (exact, decisions.md 2026-10-18) |
| edits, exact (of 7) | 0 | 0 | 3 | 7 |
| recipe, exact (of 30) / found (of 257) / invented | 8 / 102 / 18 | 9 / 89 / 16 | 15 / 245 / 2 | — |
| image-text WER, typed / handwriting | 49% / 96% | — | 7.0% / 49.3% | — |

**Meetings: the failure is one field.** Reading the 450M's recorded constrained answers case by case
(`recorded["lfm2.5-vl-450m/constrained"]` in `methods/meetings/eval/cases.json`):
- It **copies the right sentence** as `quote` in most cases: `en-simple`, `en-moved`, `that-day`,
  `it-moved`, `en-cancel-then-new` (second item), `question-accepted`.
- It then labels every item `"cancelled"` in **15 of 17 cases**, including "Thursday at 3pm in B12 works for me,
  see you then."
- `meetings::resolve` already reads the day and time **from the quote** first (`find_day(&nquote…)`,
  `find_time(&nquote)`), and uses the model's `date`/`time` words only as a fallback. So the model's
  `date` errors (it writes the email's sent date, `"2026-10-13"`) mostly do not matter; its
  `status` errors do.
- If `status` alone were right on those six cases, the old layout would score about **11/17**. This is
  our reading of the recorded answers, not a run.

**The grammar writes the keys in alphabetical order** (measured: every recorded answer starts with
`"date"`). `serde_json` sorts map keys, and both llama.cpp's GBNF converter and the llguidance path
keep the order they are given (llama.cpp `common/json-schema-to-grammar.cpp`, master; the repo's own
note in `meetings::answer_schema`). So today:

| method | order the model must write in | what it decides before it has pointed |
|---|---|---|
| meetings | date, end, place, quote, status, time, title | the day and end time, before the quote |
| edits | **new, old, op** | the new text, before the anchor and the kind of change |
| research | answerable, then per claim: **claim, quote, source** | the claim sentence, before the quote and the source |
| recipe | ingredients, notes, servings, steps, times, title | (fine: the copying comes first) |

The order is the opposite of design rule 2 ("the model points; Rust computes") for edits and research:
the free text comes first and the pointer last. For a large model this matters little; for a 450M it
plausibly matters more. Reported evidence that field order changes accuracy: Tam et al., *Let Me
Speak Freely?* (arXiv 2408.02442; the answer-before-reasoning figures are quoted second-hand, not
checked here) and D. Castillo's test (https://dylancastillo.co/posts/llm-pydantic-order-matters.html).

**No 450M recordings exist for edits and recipe**, so their failure modes are unknown. The first step
there is to record them (`pixi run eval -- edits …` writes `recorded[...]`).

## 2. Prompting and schema design for a sub-1B model

### 2.1 Levers, and what each costs in time on the phone

Phone speed (measured, speed doc §1–§2.2): reading ~350 tok/s (Q4_K_M `llama-bench`: 397 at 512
tokens, 356 at 2048); writing 23–26 tok/s in the Q8_0 method runs, 59 tok/s in `llama-bench` at
Q4_K_M. **Writing dominates text methods**, so anything that shortens the answer is a speed gain,
and anything that lengthens the prompt costs ~0.3 s per 100 tokens when not cached.

| lever | cost on the phone (estimate) | evidence |
|---|---|---|
| **Pointer-first key order** (quote/anchor/source before judgements and free text) | none | §1; 2408.02442 (reported) |
| **Lean schema per model** (drop fields Rust already computes) | **faster**: fewer tokens written | §1 (Rust reads day/time from the quote) |
| **Clearer enum words** (e.g. `status` values that do not prime "cancelled") | none | §1 diagnosis; untested |
| **Split a judgement into its own tiny call** (like edits' "kind" step) | one extra call: ~0.3 s reading (mostly cached prefix) + ~5 tokens written ≈ 0.2–0.5 s | decisions.md 2026-10-18 (night), the kind step |
| **Shorter instructions** | slightly faster reading | owner's own finding for images: house prompt 1.00 → 0.63 on typed notes (`models.toml`, `read_prompt`) |
| **One worked example (one-shot)** | ~150–200 tokens ≈ +0.4–0.6 s per call uncached; ~0 when the system prompt is the cached prefix | unmeasured for us |
| **Three examples** | ≈ +1.5 s uncached (≈ +35% on a 4.3 s meetings call) | unmeasured |
| **Sampling per the model card** | none | card: text `temperature=0.1, min_p=0.15, repetition_penalty=1.05`; LFM2-350M-Extract card: greedy, temperature 0 (both reported) |

Notes on these:
- **Sampling.** `OpenAiStep::local` sends `temperature 0.2, seed 0` and nothing else, so llama-server's
  own defaults apply to `top_k`/`top_p`/`min_p`. For grammar-held answers, temperature 0 (Liquid's
  advice for its extraction model) is the obvious first test. A repetition penalty is risky on our
  methods: they copy text verbatim, and a penalty pushes against repeating the source's tokens (our
  reasoning; not measured).
- **Few-shot and the cache.** llama-server reuses only an identical prefix (speed doc §2.7). An example
  placed in the system prompt is free on the second call of the same method, and paid again after
  any other method's call evicts it. The example must not come from an eval case (§4.3).
- **Prompt overfitting.** The evals are small (17, 8, 7, 30 cases). Choosing among many prompt variants
  by their score on the same cases selects for those cases. §8 fixes the variants before the run and
  requires the laptop model not to lose; §4.3 proposes a separate dev set for anything larger.

### 2.2 Per-model prompts: make `read_prompt` general

The catalogue already gives one model its own image instruction (`read_prompt`, passed to
`OpenAiStep::with_image_instruction`). The same pattern generalises in two ways:
- **A profile key** in `models.toml`, e.g. `prompt_profile = "small"`, that selects, in Rust, the
  small-model variant of each method's instruction *and schema* (`prompts.rs` holds both texts;
  `meetings::answer_schema(profile)` etc.). Adding a key is safe under the contract (contract.md §3:
  "Adding a key is safe; renaming or removing one is a contract change") and would be a minor release.
- **Not** free prompt text per method in the catalogue: prompts would then live in two places, and
  the evals (which call the Rust methods) would not see them.

Because the instruction and the schema move together, a profile is also how a lean schema (§2.3)
ships for the 450M while qwen3-vl-4b keeps its full one.

### 2.3 Concrete changes per method (all testable with the existing evals)

| method | change | why | speed effect |
|---|---|---|---|
| **meetings** | Key order `quote, status, place, date, time, end, title` (serde_json `preserve_order`, or build the schema as a string) | point first, judge second | none |
| | **Lean schema for the small profile:** `quote, status, place` only | Rust already takes day and time from the quote and the title from the quote when empty (`resolve`) | **fewer tokens**: about half of each item (estimate) |
| | **Status words**: try `"on"/"off"`, or a boolean `called_off`, with the instruction saying a reply that agrees or a new time means *on* | the model answers only "cancelled" in 15 of 17 cases (§1) | none |
| | **Status as a second call**: for each quote, "Read with the emails after it, does this sentence set the meeting now, or call it off?" (enum of two) | same pattern as edits' kind step; the judgement gets the whole context with one question | +0.2–0.5 s per meeting (estimate) |
| | Layout B (speed doc §2.7) as the base | measured 5 → 7/17 on the 450M | faster (cache) |
| **edits** | Key order `op, old, new` | the model now writes the new text before choosing what to anchor | none |
| | Record the 450M's answers first | failure mode unknown (0/7) | — |
| | Small profile: for `append`, a schema with only `new` (the kind step already knows it is an append; Rust sets `old = ""`) | removes the anchor the model must not write | fewer tokens |
| **research** | Key order per claim `source, quote, claim` | quote-first is the house contract (`GROUNDED_WRITE_INSTRUCTION`); the schema currently asks for the claim first | none |
| | Small profile: drop `claim`; the verified quote is the claim | the claim sentence is the unverified part; grounded 4/8 | **fewer tokens** |
| **recipe** | Record the 450M's answers first; then try the small profile without `notes` and `times` | 102/257 found: whether it stops early or skips is unknown | fewer tokens |
| | Two calls: ingredients only, then steps only | each call has one copying job | about equal tokens, one more prefill |
| **image-text** | Keep `read_prompt`; no prompt lever is likely to move 96% handwriting WER | card: "not recommended for … fine-grained OCR" (reported) | — |

## 3. Fine-tuning LFM2.5-VL-450M

### 3.1 The model (reported, model card and `config.json`)

- Language model **LFM2.5-350M**: 16 layers (10 gated short-convolution, 6 grouped-query attention),
  hidden size 1024, vocabulary 65,536. Vision encoder **SigLIP2 NaFlex, 86M** (12 layers, hidden 768),
  plus a projector. Context 32,768. Licence `lfm1.0`.
  (https://huggingface.co/LiquidAI/LFM2.5-VL-450M, its `config.json`; arXiv 2511.23404 for LFM2.)
- The card says: *"We recommend fine-tuning LFM2.5-VL-450M model on your use cases to maximize
  performance"*, and links LoRA SFT notebooks for Unsloth and TRL.

### 3.2 What Liquid documents (reported)

| route | what it supports | notes |
|---|---|---|
| **Liquid Finetune** (https://github.com/Liquid4All/liquid-finetune; docs.liquid.ai/lfm/fine-tuning/leap-finetune) | SFT, DPO, GRPO; VLM SFT/DPO/GRPO; LoRA and full; local GPUs, SLURM, Kubernetes, Modal; GGUF export | config YAML (`peft_config`, `use_peft`, `extends: DEFAULT_LORA`); no VLM example config or hardware figures published |
| **TRL** (docs.liquid.ai/lfm/fine-tuning/trl) | SFT, DPO; VLM SFT with `AutoModelForImageTextToText` + `AutoProcessor` + custom collator | VLM LoRA example: r 8, alpha 16, lr 5e-4, 1 epoch; `target_modules=["q_proj","v_proj","fc1","fc2","linear","gate_proj","up_proj","down_proj"]` |
| **Unsloth** (unsloth.ai/docs/models/tutorials/lfm2.5) | text LoRA for LFM2.5-1.2B; VL LoRA for LFM2.5-VL-1.6B (`finetune_vision_layers = False`); export `save_pretrained_gguf(..., quantization_method="q4_k_m")` | text target modules `q_proj, k_proj, v_proj, out_proj, in_proj, w1, w2, w3`; VL-450M not named |
| **Liquid's guidance** (docs.liquid.ai/lfm/fine-tuning/overview) | start with LoRA; **500 to 5,000 task examples**; freeze a held-out set before training; train with the production chat template | "a 1.2B LoRA run can take minutes to tens of minutes on a single modern GPU" |

**Keeping the vision tower intact.** The TRL example's `fc1`/`fc2` are SigLIP's MLP names, so that list
would put adapters on the vision encoder too. For our text methods the adapter should target only the
language model, by a regex on the module path (`model.language_model.*` with `q_proj|k_proj|v_proj|
out_proj|in_proj|w1|w2|w3`), leaving the encoder and projector frozen. Then the shipped
`mmproj-LFM2.5-VL-450m-F16.gguf` stays valid byte for byte (check by SHA-256 against `models.toml`).

### 3.3 LoRA or full fine-tune

- **LoRA** is what every Liquid route starts with. It changes ~1–2% of the weights (TRL page,
  reported). *LoRA Learns Less and Forgets Less* (arXiv 2405.09673): LoRA underperforms full
  fine-tuning on the target domain at ~100K examples, but **keeps the base model's other skills
  better**. For narrow schema tasks with a few thousand examples, the target-domain gap is the smaller
  concern; forgetting is the larger one (chat, image reading).
- **Full fine-tuning** of 450M parameters with AdamW needs roughly 16 bytes per parameter (weights,
  gradients, two optimiser moments, master copy) ≈ **7 GB** before activations (estimate): it does not
  fit the RTX 3050's 4 GB. 8-bit optimisers bring it near 3 GB plus activations: possible, fragile, and
  it changes every weight the chat and vision paths use. Not recommended.

### 3.4 Compute: is the RTX 3050 (4 GB) enough?

**For LoRA on the language model, yes (estimate, not tried).**
- bf16 weights of the whole VL model ≈ 0.9 GB; with gradient checkpointing, sequences of 1–2k tokens
  and batch 1–4, LoRA training should stay well under 4 GB. Ampere supports bf16.
- Time: training costs ~6 × N × tokens FLOPs. 2,000 examples × 500 tokens × 3 epochs = 3M tokens ×
  6 × 3.5e8 ≈ 6e15 FLOPs. At an effective 3–5 TFLOP/s on a laptop 3050 that is **~20–35 minutes**
  (order of magnitude; to measure).
- Image training (handwriting) is heavier: a full page is ~1,000–1,900 image tokens per example
  (speed doc §1), plus the encoder in memory. Likely still possible at batch 1; otherwise cloud.
- **Cloud if needed** (reported, https://modal.com/pricing, 2026-10-19): T4 ~$0.59/h, L4 ~$0.80/h,
  A10 ~$1.10/h, A100 80 GB ~$2.50/h; the Starter plan includes $30/month of credits. A run of the size
  above costs **under $1**. Liquid Finetune has a Modal launcher.
- Training needs PyTorch with CUDA on the laptop (the inference runtime is Vulkan). It is a
  development tool: keep it in its own pixi environment under `agents/`, outside `pixi run ci`.

### 3.5 Back to GGUF, and how it runs on the phone

Two ways, both supported by llama.cpp (master source, checked 2026-10-19):

1. **Merged model.** `merge_and_unload()` → save the HF checkpoint → `convert_hf_to_gguf.py`
   (`Lfm2VlForConditionalGeneration` is mapped to the `lfm2` text architecture and the
   `language_model.` prefix is stripped, `conversion/__init__.py`, `conversion/base.py`) → F16 GGUF
   → `llama-quantize … Q4_K_M`. Reuse the original mmproj unchanged. **No speed cost at all.** But the
   one model now *is* the fine-tune, for chat and photos too.
2. **Runtime adapters.** `convert_lora_to_gguf.py --base <hf dir>` → one adapter GGUF per method.
   llama-server loads them with `--lora a.gguf --lora b.gguf --lora-init-without-apply`, and a request
   can carry `"lora": [{"id": 0, "scale": 1.0}]`, unlisted adapters defaulting to 0
   (`tools/server/README.md`; documented under `/completion`, **check it is honoured on
   `/v1/chat/completions`**, which `OpenAiStep` uses). The base stays untouched for chat and photos.
   - Speed cost (estimate): r = 16 on a 1024-wide projection adds ~3% to that matmul, plus reading the
     F16 adapter. **To measure on the phone**; it must not eat the Q4_K_M writing gain.
   - Cache: a different adapter changes every cached key and value, so the meetings→title prefix
     reuse of layout B is expected to break unless both calls use the same adapter (to verify).
   - Memory: an r = 16 adapter on the 350M is a few MB to tens of MB (estimate).

A fine-tuned model shipped through the catalogue needs a hosted file pinned by revision and SHA-256
like every other entry (e.g. a formicaria-org Hugging Face repo), the licence copy and modification
notice (§5), and the usual "offered with size and licence first" (design rule 4).

## 4. Training data

### 4.1 Where labelled examples can come from, per method

The cheapest labels are the ones **code generates with the answer known**: no teacher, exact labels,
and full control over overlap with the evals.

| method | source | labels from | volume |
|---|---|---|---|
| **meetings** | A generator of email threads in English and Italian: propose / accept / decline / move / cancel / question answered or not / deadline / no meeting, with random names, days, times, rooms, links | the generator (which sentence fixes it, and its status) | 1,000–2,000 |
| | qwen3-vl-4b writing more natural threads and answering them (distillation) | the teacher, **kept only when** `meetings::resolve` accepts it and it agrees with the generator's intent | 500+ |
| **edits** | Notes (Wikibooks text, generated notes) with programmatic changes: typo fixes, appended lines, deleted lines, paragraph → bullets, formula → LaTeX | the program that made the change (op, old, new) | 1,000+ |
| | qwen3-vl-4b on generated requests (measured 7/7 on the eval) | the teacher, kept only when Rust applies it and the result matches | 500+ |
| **research** | Invented-entity sources (the eval's own design: things a model cannot know) generated by qwen3-vl-4b | teacher answers kept only when every quote passes `grounding::verify` | 500–1,000 |
| **recipe** | **Wikibooks Cookbook pages beyond the eval's 30** (`agents/eval-data.py` takes the first 30 parsable files `recipe_00_000…`), turned into speech by the same `spoken()` | the page's own ingredient list and procedure | 1,000+ |
| **image-text** | **GNHK train split** (the eval uses the test split); typed pages rendered from text | the dataset's annotation / the rendered text | 1,000+ pages |

Distillation is from **our own local models**, whose licence (Apache-2.0 for qwen3-vl-4b) puts no
restriction on using their outputs. The teacher's measured accuracy bounds the labels: 16/17 meetings
and 7/7 edits are good teachers; for recipes the page itself is better than any teacher.

### 4.2 How many examples

- Liquid: **500–5,000** task examples (reported, fine-tuning overview).
- arXiv 2509.08381 (ETLCH): a ~1B LLaMA with LoRA on **a few hundred to one thousand samples per
  task** (JSON extraction, knowledge graphs, NER) beat its baselines, "with substantial gains observed
  even at the lowest data scale" (reported).
- LIMA (arXiv 2305.11206): 1,000 curated examples taught a 65B model response formats (reported; a
  different scale).
- So: **~1,000 per method** to start, more for the methods with most variation (meetings, recipe).
  Train with the production chat template and the **production schema's key order** (whatever §2
  settles), with the loss on the answer tokens only.

### 4.3 Keeping the evals out of training

1. **Generate from disjoint pools.** Name, place and link lists for generators exclude every name,
   address and link in `methods/*/eval/cases.json`; recipe training files exclude the 30 eval files
   by file name; images come only from GNHK's train split.
2. **An automatic overlap check** before every training run: reject any training example sharing an
   8-word sequence with any eval case input (a script beside the data, its report kept with the run).
3. **A separate dev set** (~100 generated cases per method) for choosing checkpoints and prompt
   variants. The golden evals are read once per candidate, at the end, as the decision gate.
4. **Never turn a failed eval case into a training example**, nor paraphrase one. Failed cases go to
   the generator's *kinds* (e.g. "question accepted"), with new content.
5. Record the training data's provenance (dataset, revision, generator commit, seed) next to the
   adapter, as the eval data already does.

## 5. Licence: LFM Open License v1.0 (reported, https://www.liquid.ai/lfm-license; not legal advice)

- **Fine-tuned weights are Derivative Works.** §1 defines a Derivative Work as a work "based on (or
  derived from) the Work"; §2 grants a perpetual, worldwide, royalty-free licence to "reproduce,
  prepare Derivative Works of, publicly display, publicly perform, sublicense, and distribute the
  Work", **subject to §5**.
- **No copyleft.** The licence's FAQ: fine-tuning does not require public release ("The license does
  not have a 'copyleft' requirement"); fine-tunes may stay proprietary. §4 allows "additional or
  different license terms" for your modifications, provided use of the Work still complies.
- **The commercial threshold (§5).** "Commercial Use" is "any use of the Work for direct or indirect
  commercial advantage or monetary compensation" (§1). Rights for Commercial Use "are conditioned
  upon You or Your Legal Entity not exceeding the Threshold" (§5(a)); the Threshold is "annual
  revenue of 10 million United States dollars ($10,000,000) or more" (§1). "Any Commercial Use of the
  Work or a Derivative Work by a Legal Entity that exceeds the Threshold is not licensed" (§5(b)).
  - "Legal Entity" includes everything that controls, is controlled by or is under common control
    with the acting entity (≥50% ownership among the tests), so revenue counts at group level.
  - The licence does **not** say how revenue is measured (gross or net, which year).
  - Qualified non-profits are exempt for non-commercial or research use (§5(c)).
  - Non-commercial use is not conditioned on the threshold by §5's wording.
- **What this means for shipping a fine-tune inside formicaria.**
  - Fine-tuning and redistributing the derivative are allowed for us, under the same licence terms.
  - **The threshold travels with the model:** each person or organisation that *uses* it commercially
    must be under $10M, exactly as for the base model today. Fine-tuning does not change what the
    app already discloses before download.
  - §4 obligations on redistribution: give recipients a copy of the licence (§4(a)); mark modified
    files with prominent notices (§4(b)); keep the copyright notices (§4(c)); carry any NOTICE
    attributions (§4(d)). In practice: the licence file and a "fine-tuned from LFM2.5-VL-450M by
    formicaria; changed" notice in the model repo, and the licence name in `models.toml`.
  - §7: no trademark rights. A descriptive name ("fine-tuned from LFM2.5-VL-450M") is customary use;
    a name that looks like a Liquid product is not.
  - §11: the licence ends automatically on non-compliance.
- **Training data licences**, separately: Wikibooks is CC BY-SA 4.0 (`methods/recipe/eval/
  LICENSE.md`), GNHK CC BY 4.0. Whether weights trained on share-alike text are an "adaptation" is
  unsettled; record the sources and attribute them with the adapter either way.

## 6. Expected gains and risks from the literature (reported unless stated)

| source | setting | result |
|---|---|---|
| arXiv 2509.08381 | ~1B LLaMA, LoRA, a few hundred–1,000 samples per task, JSON extraction / KG / NER | beats strong baselines on most metrics, gains even at the lowest data scale |
| LoRA Land, arXiv 2405.00732 | 310 LoRA fine-tunes, 10 base models (mostly ~2–7B), 31 tasks | +34 points over the base models on average, +10 over GPT-4 |
| TinyAgent (BAIR, 2024; via our 2026-10-18 patterns doc) | TinyLlama-1.1B, LoRA on ~80K plans | 12.7% → 80.1% on its task |
| FunctionGemma-270M (via the same doc) | one task fine-tune | 58% → 85% on Mobile Actions |
| LFM2-350M-Extract (https://huggingface.co/LiquidAI/LFM2-350M-Extract) | Liquid's own extraction fine-tune of the 350M class, mostly synthetic data | vendor claim: beats Gemma 3 4B on its extraction benchmark; no numbers on the card |
| arXiv 2405.09673 | LoRA vs full, code and maths | LoRA forgets less of what it was not trained on |

**What to expect for us** (our judgement, to be measured): narrow structured methods with exact
labels are the case where small-model fine-tunes do best. Meetings and edits, where the 450M's
failures are about the task's rules rather than reading, are the most likely to reach the 1.2B /
Qwen3.5-0.8B level (13/17, 3/7) or above. Recipe recall is a copying-length problem and may gain less.
Handwriting at 96% WER is a reading problem; a text-side adapter will not fix it, and a vision
fine-tune is a larger project with an uncertain result.

**Risks.**
- **Other skills get worse.** A merged fine-tune changes chat and image reading too. Runtime adapters
  avoid that; a merged model must pass every method eval plus a chat check before shipping.
- **Quantization after fine-tuning.** The base model lost nothing at Q4_K_M on our evals (measured);
  a fine-tuned one must be re-measured at Q4_K_M, not assumed.
- **Overfitting to small evals** (§2.1, §4.3).
- **Maintenance.** Each base-model update (LFM2.5 → a later release) means re-training and re-hosting.
  The alternative already measured, Qwen3.5-0.8B, needs no training but writes at 22 tok/s against
  the 450M's 59 at Q4_K_M (speed doc §2.2): fine-tuning is the route that keeps the speed.

## 7. Ranked measures

Gain is for the 450M on our evals. "Effort" is working time; "risk" is to accuracy elsewhere or to
the contract.

| # | measure | expected gain | speed | effort | risk |
|---|---|---|---|---|---|
| 1 | **Meetings status fix**: clearer enum words, or status as its own small call | high: up to ~+6 cases (from 5–7 toward ~11–13/17) by our reading of the recorded answers | none, or +0.2–0.5 s per meeting | hours | low; shared prompt also changes qwen3-vl-4b, so measure both |
| 2 | **Pointer-first key order** in all schemas (meetings, edits `op,old,new`, research `source,quote,claim`) | medium, unknown per method | none | hours | low; changes the laptop model's behaviour too |
| 3 | **Lean small-model schemas** behind a `prompt_profile` catalogue key | medium accuracy (fewer things to get wrong), sure speed gain | **faster** | 1 day | low; a catalogue key is a minor contract addition |
| 4 | **Temperature 0** for grammar-held calls (Liquid's advice for extraction) | small, possibly none (seed is already fixed) | none | minutes | low |
| 5 | **Record 450M answers** for edits and recipe, then diagnose | enables 2–3 for those methods | — | 1 hour | none |
| 6 | **One-shot example** per method for the small profile | uncertain | +0.4–0.6 s per call uncached | hours | overfitting if built from eval cases |
| 7 | **Per-method LoRA adapters** (meetings first), language model only, programmatic + distilled data, runtime `--lora` | high (literature: large gains at 0.3–1B with 500–2,000 examples) | ≈ equal; overhead to measure; layout-B cache may break | 2–4 days per method incl. data | medium: adapter loading on Android unmeasured |
| 8 | **One multi-task LoRA merged** into the phone model, re-quantized Q4_K_M | high on trained methods | none | 1 week | higher: chat and photos change; full regression needed |
| 9 | **Vision fine-tune** for handwriting (GNHK train) | uncertain | none | 1–2 weeks; probably cloud | high; the card discourages fine-grained OCR |
| — | Full fine-tune | — | — | — | not recommended: does not fit 4 GB, forgets more |

Measures 1–6 are cheap and should run before any training: they also fix the schema the training
data must match. Measure 7 is the main lever for "more accurate at the same speed".

## 8. First experiment (under a day, existing evals)

**Meetings on the 450M Q4_K_M: status and schema, with qwen3-vl-4b as the guard.**

Setup:
- Laptop RTX 3050, the b10076 runtime, `pixi run eval -- meetings <gguf> <name> 99`, with
  `EVAL_NO_RECORD=1` for the variants and per-case logs kept (speed doc, *Mistakes*). Re-fetch the
  450M Q4_K_M by `~/bench-scratch/keep/candidate-models.sha256` and send one request by hand first.
- Each variant is a patch in a worktree, as for layout B. Base: the layout-B patch
  (`~/bench-scratch/keep/prompt-layout-B-2026-10-19.patch`), measured 7/17 (450M Q8_0) and 16/17
  (qwen3-vl-4b).

Variants, fixed before running:

| id | change on top of the previous |
|---|---|
| M0 | layout B as is |
| M1 | key order `quote, status, place, date, time, end, title` |
| M2 | lean schema `quote, status, place` (instruction trimmed to match) |
| M3a | M2 with status as `"on"`/`"off"` and one sentence on what *on* means |
| M3b | M2 with status asked in a second call per quote (two-value enum) |
| T | the best of the above at temperature 0 |

Per variant: two runs on the 450M Q4_K_M (about 15–25 s each on the GPU, measured order for 17
cases), one on qwen3-vl-4b. Record exact score, well-formed count, generated tokens per request and
the failing case ids. About 4–6 hours including the patches.

**Decision rule.** A variant goes forward only if the 450M gains **at least 2 cases in both runs** and
qwen3-vl-4b stays **≥ 16/17**; if the laptop model loses, it goes forward only as a small-profile
variant (§2.2). The winning variant is then the schema the meetings training data uses (§4).

**If time remains**, the same day: record the 450M on edits and recipe (measure 5), and try key order
`op, old, new` on edits.

**Next, the fine-tuning pilot (2–3 days, not part of this experiment):** a meetings generator
(§4.1, ~1,500 threads, overlap check §4.3), a LoRA (r 16, language model only) on the RTX 3050,
converted with `convert_lora_to_gguf.py`, run on the laptop with `--lora`, then on the phone over
USB (`EVAL_ADB_DIR`) for speed with the adapter on and off.

## Sources

- Ours: `docs/research/2026-10-19-speed.md` §1, §2.2, §2.3, §2.7; `methods/*/eval/cases.json`
  (recorded answers); `crates/fm-agent/src/{prompts,meetings,edits,grounding,recipe,openai,lib}.rs`;
  `crates/fm-agent-run/models.toml`; `docs/context/{decisions,contract}.md`;
  `docs/research/2026-10-18-small-model-agent-patterns.md`.
- Model: https://huggingface.co/LiquidAI/LFM2.5-VL-450M (card and `config.json`, fetched
  2026-10-19); https://huggingface.co/LiquidAI/LFM2-350M-Extract; LFM2 Technical Report, arXiv
  2511.23404.
- Fine-tuning: https://docs.liquid.ai/lfm/fine-tuning/overview;
  https://docs.liquid.ai/lfm/fine-tuning/trl.md; https://docs.liquid.ai/lfm/fine-tuning/leap-finetune.md
  (repo https://github.com/Liquid4All/liquid-finetune); https://unsloth.ai/docs/models/tutorials/lfm2.5.md.
- llama.cpp (master, 2026-10-19): `convert_hf_to_gguf.py`, `conversion/lfm2.py`,
  `conversion/__init__.py`, `conversion/base.py`, `convert_lora_to_gguf.py`,
  `common/json-schema-to-grammar.cpp`, `tools/server/README.md` (`--lora`,
  `--lora-init-without-apply`, per-request `lora`).
- Licence: https://www.liquid.ai/lfm-license (LFM Open License v1.0 and its FAQ).
- Papers: arXiv 2509.08381 (ETLCH, low-resource LoRA extraction); 2405.00732 (LoRA Land); 2405.09673
  (LoRA Learns Less and Forgets Less); 2408.02442 (Let Me Speak Freely?); 2305.11206 (LIMA).
- Other: https://dylancastillo.co/posts/llm-pydantic-order-matters.html (field order);
  https://modal.com/pricing (GPU prices, 2026-10-19).
