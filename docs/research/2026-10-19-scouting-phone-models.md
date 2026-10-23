# Scouting a model to offer on the phone — the method, and a first leaderboard pass (2026-10-19)

> **Research, not a ruling.** How to shortlist an optional on-device model (a new phone choice, a
> sibling for the chooser's "Faster" / "More accurate, slower") before any of Myrme's own evals run.
> The leaderboard only shortlists; **Myrme's method evals decide** (design rule 5). Section 3 is a
> dated snapshot and will go stale; sections 1 and 2 are the part to reuse.

**The owner:** *"how can you pick one? You need now to pick one open and reliable benchmarking
website"*, then *"no more than 8GB of RAM"*, then *"make sure this comes out in myrme future research
about optional agents (particularly the methodology used)"*.

## 1. The source: one independent leaderboard

**Artificial Analysis, open-weights "Tiny (≤4B)" page** —
https://artificialanalysis.ai/models/open-source/tiny (and `/small` for 4–40B).

Why this one:
- **Independent.** It runs every model through its own evaluations; nothing is copied from model
  cards, which are nearly all vendor-reported (see `2026-10-18-dedicated-methods-per-task.md`).
- **It has a tier at phone size.** Most leaderboards rank frontier models and drop sub-4B ones.
- **Current and free to read**, with the methodology published.

What it is not:
- **Not open source.** The open, reproducible one, Hugging Face's Open LLM Leaderboard (code and
  `lm-evaluation-harness` runs public), was retired and archived in 2025, so it lists no current
  model: https://huggingface.co/docs/leaderboards/en/open_llm_leaderboard/archive
- **Not Myrme's tasks.** Its Intelligence Index (v4.3.2 on this date) is ten evaluations of
  agentic and knowledge work: AA-Briefcase, GDPval-AA, AutomationBench-AA, Terminal-Bench 4.0,
  SciCode, Humanity's Last Exam, GDP.pdf, CritPt, AA-Omniscience, AA-LCR. Myrme's methods are
  extraction, classification and short writing, single-shot and grammar-constrained. A higher index
  is a reason to measure a model, not a reason to ship it.
- **Not phone speed.** Its speed column is a cloud provider's tokens per second, and is empty for
  most tiny models. Time on the phone comes only from our own runs (`2026-10-19-speed.md`).

Rejected as the single source: LMArena (human preference votes; few sub-4B models), vendor hubs
(Qualcomm AI Hub, model cards: not independent), MLPerf Mobile/Client (hardware throughput, not
model quality).

## 2. The method, step by step

1. **Read the tiny page and record the index version.** Scores from different versions are not
   comparable: Phi-4 Mini shows 8 under v4.0 and 6 under v4.3.2 (16 on an older tier page). Note
   that the page shows only its top rows ("12 of 49 models" on this date) and that its scores are
   labelled *"Estimate (independent evaluation forthcoming)"*.
2. **Collapse variants.** Reasoning and non-reasoning rows of one model are one candidate.
3. **Filter by what the phone gives a model, not by the phone's RAM.** Myrme's budget is ~1–2 GB
   for one resident generative model (`2026-10-18-dedicated-methods-per-task.md`); an 8 GB phone
   leaves ~4 GB to apps at best. **Read the real GGUF file size on Hugging Face; do not estimate it
   from the parameter count.** The 0.6 GB-per-billion rule of thumb said ~2.3 GB for K2-Horizon-3.7B;
   its Q4_K_M is 3.16 GB (`2026-10-19-myrme-light.md` §4). Mixture-of-experts models count their
   *total* parameters, not the active ones (Qwen3.6 35B-A3B needs all 35B resident).
4. **Check each survivor's card before any download**, the gates that ended K2-Horizon:
   - **Reasoning cost:** a model that must think (its card asks for high effort, tens of thousands of
     output tokens) is minutes per answer on the Dimensity 7300. Myrme runs thinking off.
   - **Runtime:** the llama.cpp build it needs against the one we pin (b10076/b10081 today). An
     upgrade is its own measured change (the speed study notes a vision regression).
   - **Languages and modality:** English-only or text-only rules a model out of slots that need
     Italian or photos.
   - **Licence:** Apache-2.0/MIT; watch for non-commercial siblings (NuExtract-2.0-4B, GLiNER v0/v1).
   - **A GGUF exists** from the maker or a known quantiser.
5. **Gaps under ~3 index points are noise.** Treat the tier as bands, not ranks.
6. **Then Myrme's own evals decide**: the method evals (meetings /17, titles /12, recipes /257,
   edits /7, OCR WER) on the laptop first, then time on the phone. Offering it is the 2026-10-10
   rule: shown with its size and licence, never downloaded unasked; a new chooser entry is a
   `decisions.md` entry with the eval numbers.

## 3. Snapshot, 2026-10-19 (Index v4.3.2)

The tiny page's top rows, variants collapsed. Parameters as the page lists them. All are ≤4B, but
only the smallest fit the ~1–2 GB budget at Q4.

| # | Model | Maker | Params | Index | Context |
|---|---|---|---|---|---|
| 1 | K2 Horizon 3.7B | Institute of Foundation Models | 3.7B | 16 | 524k |
| 2 | MiniCPM5-2B | OpenBMB | 2.6B | 12 | 131k |
| 3 | G9v3-3B | AI9Stars | 3B | 11 | 131k |
| 4 | Granite 4.2 3B | IBM | 3B | 9 | 131k |
| 5 | MiniCPM5-1B (both variants) | OpenBMB | 1B | 9 | 128k |
| 6 | Nanbeige4.1-3B | Nanbeige | 3.9B | 8 | 256k |
| 7 | LFM2.5-2.6B | Liquid AI | 2.7B | 8 | 128k |
| 8 | Qwen3.5 2B (reasoning) | Alibaba | 2.3B | 7 | 262k |
| 9 | Phi-4 Mini Instruct | Microsoft | 3.8B | 6 | 128k |
| 10 | Qwen3.5 0.8B (reasoning) | Alibaba | 0.9B | 6 | 262k |

Read against Myrme:
- **K2-Horizon-3.7B** was already considered and not taken (`2026-10-19-myrme-light.md` §4).
- **Qwen3.5-0.8B** is in the catalogue as the phone's "More accurate, slower" choice (Q4_0, thinking
  off; `decisions.md`, 2026-10-19).
- **Worth a card check next (step 4), by size first:** MiniCPM5-1B (1B, the only other ~1 GB entry),
  then LFM2.5-2.6B and MiniCPM5-2B if a ~1.6 GB slot is acceptable. The LFM family already runs as
  the default (`lfm2.5-vl-450m-q4`).
- **Not on the visible page:** Gemma 4 E2B/E4B, and the 37 lower rows. The small page (4–40B) has
  no entry at or under 4B.

Sources: https://artificialanalysis.ai/models/open-source/tiny ·
https://artificialanalysis.ai/models/open-source/small ·
https://artificialanalysis.ai/models/phi-4-mini ·
https://huggingface.co/docs/leaderboards/en/open_llm_leaderboard/archive

## 4. Myrme's evals on the shortlist (2026-10-19, laptop)

Step 6 for every row that passed the size and card gates, plus K2-Horizon-3.7B, which the owner asked
to look at again. All runs use **one runtime: llama.cpp b11731**. That is newer than the pinned
b10076/b10081: MiniCPM-V 4.7 needs llama.cpp PR #29416 (merged 2026-10-10) and K2 ≥ b11454. Other settings:
- laptop CPU (`-dev none -t 8`), temperature 0 (edits: 0.2, its eval's own setting, one run, so noisy);
- meetings grammar-constrained;
- thinking off (`enable_thinking: false`; K2: `reasoning_effort: low`, its lowest);
- Q4_K_M, except Qwen3.5-0.8B and LFM2.5-2.6B, which are Q4_0 (the file type read from each GGUF).

On a laptop CPU, Q4_0 and Q4_K_M run at about the same speed, so the encoding does not explain the
time differences. Parameter count and answer length do.

### 4.1 The artificial evals (`methods/*/eval`)

| Model | GGUF | Meetings /17 | Titles /12 | Edits /7 | Research /8 | Recipe, free /30 |
|---|---|---|---|---|---|---|
| Qwen3.5-0.8B (shipped, "More accurate") | 0.5 GB | **14** · 19 s | — | 1 · 7 s | 5 | 20 |
| MiniCPM-V 4.7 1B | 0.67 + 1.1 GB | 7 · 47 s | — | 4 · 56 s | **7** | **21** |
| MiniCPM5-1B | 0.69 GB | 10 · 50 s | 6 · 8 s | 3 · 64 s | — | — |
| Qwen3.5-2B | 1.40 GB | 11 · 54 s | **12** · 20 s | 3 · 27 s | — | — |
| MiniCPM5-2B | 1.56 GB | 7 · 10 s | **12** · 16 s | 4 · 25 s | — | — |
| K2-Horizon-3.7B | 3.16 GB | 12 · 84 s | 11 · 30 s | **5** · 183 s | — | — |
| LFM2.5-2.6B | 1.59 GB | out: always reasons (below) | | | | |

These sets are small and invented: 17 meetings, 7 edits, 8 research cases. One case moves a score
by 6–14 points, so a gap of two or three cases is noise.

**LFM2.5-2.6B** has no switch to stop reasoning: its chat template knows only `preserve_thinking`.
One meeting question took 17 s and 1,074 characters of hidden reasoning, then was cut off before
any answer. On the real mail below it gave 0/5 well-formed answers in 322 s. It fails step 4's
reasoning-cost gate, and its screen was stopped.

### 4.2 Real mail: the owner's own meetings

The artificial meetings are 62–227 characters, one or two emails each. Real exchanges are longer,
and they are where a larger model should show its worth.

**The real set:** the five mail conversations in a user's vault on 2026-10-19:
- **four accepted meetings**, scored on the time the owner kept;
- **one conversation not accepted**, where the right answer is no meeting;
- each 3–7 emails, 1.5–4.5k characters, with links as places.

**Handling.** The cases are private: built in `~/bench-scratch/realmail/` and never committed.
`eval_meetings.rs` reads them through `FM_EVAL_CASES`, the switch `eval_image.rs` already had, and
never records answers back when that switch is set.

**Caveats:**
- **Five cases is too few to rank close models.** They show what breaks on real mail.
- **The set favours Qwen3-VL-4B.** The four meetings are the ones it found and the owner accepted,
  so a meeting it missed is in neither set.

Times were measured with the CPU otherwise idle. Each run was repeated once, and the scores
repeated exactly.

| Model | Exact /5, day rule | **Exact /5, time rule** | Artificial /17, time rule | 5 threads, laptop CPU |
|---|---|---|---|---|
| Qwen3-VL-4B (laptop model, layout b) | **4** | **4** | **16** | 84–90 s |
| K2-Horizon-3.7B | 2 | **4** | 12 | 98–101 s |
| **Qwen3.5-0.8B** | 3 | 3 | 14 | **15–17 s** |
| Qwen3.5-2B | 1 | 2 | 11 | 69–73 s |
| MiniCPM-V 4.7 1B | 1 | 1 | 7 | 32 s |
| MiniCPM5-1B / MiniCPM5-2B | 1 / 1 | 1 / 1 | 10 / 7 | 49 / 21 s |
| LFM2.5-VL-450M Q4_K_M (phone default, "Faster") | 0 | 0 | 5 | 14 s |
| LFM2.5-2.6B | 0 well-formed | — | — | 322 s |

- **Day rule:** `resolve` as released in 0.6.0.
- **Time rule:** with the fix described in §4.3. Every model was run again on both sets with it.
- **The artificial set does not move.** Every recorded answer in `cases.json` scores the same
  under both rules.
- Qwen3-VL-4B's 16/17 here, against 15/17 recorded, comes from the newer runtime on CPU. Its
  recorded answers score the same under both rules.

### 4.3 What the real mail shows

- **The larger models also report the meeting that already happened.** K2-Horizon and Qwen3.5-2B
  find the next meeting, then also list the one held earlier on the day of the last email (e.g.
  17:00, with the last email at 17:31).
  - `meetings::resolve` drops a meeting as past only when its *day* is before today
    (`day < today`). A same-day meeting that is already over passes as fixed, and the phone would
    propose it.
  - **Fixed (2026-10-20, uncommitted).** A meeting that is not a call-off is past if it began
    before the newest email was sent (`stamp(day, start) < newest heading`). A call-off stays under
    the day rule, since it still records what happened to a meeting.
  - K2 rose from 2/5 to 4/5 and Qwen3.5-2B from 1/5 to 2/5; nothing else moved. Qwen3.5-2B's
    remaining miss is an invented cancellation, left alone by design.
  - Test: `a_meeting_already_begun_when_the_newest_email_was_sent_is_past`.
- **Qwen3.5-0.8B holds up.** It scores 3/5, the best below 2 GB, and is 4–6× faster than every
  larger candidate. Its two misses:
  - it reads a later "can't make it" as cancelling the wrong meeting;
  - it takes an older time.
- **The phone's default (450M) fails on real mail:** 0/5, with invented cancellations. The
  artificial set did not show this.
- **The MiniCPM family answers "no meeting"** on real mail, all three models. That rules them out
  for the mail task, whatever their research and recipe scores.

### 4.4 Reading

Nothing on the shortlist beats Qwen3.5-0.8B at a phone's runtime cost.
- **K2-Horizon** is the strongest candidate here: it leads on edits (5/7) and, with the time rule,
  matches the laptop model on real mail (4/5) in about the same time.
  - But it costs a 3.16 GB file, about 6× the 0.8B's time per call on the laptop, and a runtime
    upgrade.
  - It is a candidate for the **laptop**, against Qwen3-VL-4B, not for the phone.
- **Qwen3.5-2B** is the only shortlisted model that also reads images (with its 0.67 GB mmproj).
  - Its text scores do not beat the 0.8B, so its handwriting run was not worth spending.

**Next, in order:**
1. ~~The time-aware past check in `resolve`~~: done and re-scored (§4.2).
2. **A bigger real-mail set**: more labelled conversations, including ones with no meeting.
3. **K2-Horizon against Qwen3-VL-4B on the laptop**, on every method, if a laptop upgrade is wanted.
4. **Revisit the phone default.** The 450M's real-mail result argues for Qwen3.5-0.8B as the
   default, or for a plainer "Faster" label.
