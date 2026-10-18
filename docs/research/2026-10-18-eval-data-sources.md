# Openly licensed data for the image, speech and recipe evals (research, 2026-10-18)

> **Research, not a ruling.** One research agent, about 30 lookups, on 2026-10-18. Licences were
> checked against the original sources (HF API, GitHub, owners' pages, papers) unless marked
> **UNVERIFIED**. Pin every Hugging Face file by commit (`/resolve/<sha>/<file>`) and keep a
> sha256. **Mirror licence tags lie.** Teklia tags IAM and RIMES as MIT, and RecipeNLG mirrors are
> tagged CC-BY, but all three are restricted at their source.

## Handwriting, OCR, handwritten math (M3)

| Dataset | Licence (original) | What it is | Fetch | Commit samples? |
|---|---|---|---|---|
| **GNHK** (GoodNotes) | **CC-BY-4.0** | 687 **phone photos of real paper** (lists, sticky notes, diaries), word polygons and text | official: goodnotes.com/gnhk (click-through); HF mirror `aryaman27/goodnotes-handwritten-dataset` @ `904297760a9fec813fdc1ecb5f8055cb1ca03550` (not checked against the original) | yes, with attribution |
| **danielrosehill/Whiteboards** | **CC-BY-4.0** | 21 phone photos of a whiteboard, with transcription and category | HF @ `4b31e86e15743ffea20204a8f6992b0173ecbdf9`, `data/*.webp` + `metadata.jsonl` | yes |
| **MathWriting** (Google, 2024) | **CC BY-NC-SA 4.0** | online ink + LaTeX (renders, not photos) | `mathwriting-2024-excerpt.tgz` (1.5 MB) or HF `deepcopy/MathWriting-human` @ `d0a406be8326d3b482b5fa8c3a5a2144e7723e13` (test parquet ~50 MB) | **no**: download at eval time only |
| CROHME 2019 | CC BY-NC-SA 3.0 | ink + LaTeX | HF `Neeze/CROHME-full` @ `6d03ee8f86df1d0d93f3de00df75303d53b75f7d` | no |
| HME100K | **UNVERIFIED**, probably research-only | camera photos of handwritten math | HF `ConstantHao/HME100K` | assume no |
| IAM, RIMES, FUNSD, SROIE | restricted or unclear; scanned, not photographed | | | no |

- **Pick:** 50 pages from the GNHK test split, plus the 21 Whiteboards images, plus 30 MathWriting
  test items fetched at eval time.
- **Gap:** no openly licensed set of modern Italian handwriting exists. 20–30 of the owner's own
  pages would be the most realistic data, and could ship under CC-BY.

## Speech to text (M4)

| Dataset | Licence | What it is | Pinned slice |
|---|---|---|---|
| **Multilingual LibriSpeech** | **CC-BY-4.0** | read audiobooks, 8 languages incl. Italian | `facebook/multilingual_librispeech` @ `2e83e61823b4c47dcbcb1980bb88601274127609`, `italian/test-00000-of-00001.parquet` (**83 MB**) |
| **LibriSpeech test-clean** | **CC-BY-4.0** | English audiobooks | `openslr/librispeech_asr` @ `71cacbfb7e2354c4226d01e70d77d5fca3d04ba1`, or OpenSLR `test-clean.tar.gz` (346 MB, md5 published) |
| FLEURS | CC-BY-4.0 | the same sentences in 102 languages | `google/fleurs` @ `70bb2e84b976b7e960aa89f1c648e09c59f894dd`; the Italian test tar is 658 MB |
| VoxPopuli | CC0 | Parliament speech, less scripted | `facebook/voxpopuli` @ `42f01879c780b4a2e90ec0b4f616c2ece526e4f1`; the Italian test file is 868 MB |
| Common Voice | — | only through Mozilla Data Collective (account, API key; mirrors not allowed) | **avoid** |

- **Pick:** 30 Italian MLS clips plus 30 LibriSpeech test-clean clips.
- All of these are read speech. 10–20 of the owner's own voice memos would cover casual speech.

## Recipes spoken aloud (M7)

No openly licensed recording of a recipe spoken aloud, together with the recipe as ground truth,
was found:
- YouCook2's annotations are MIT, but its videos are YouTube links only, re-hosting them is
  legally grey, and many have no narration.
- Tasty is mostly silent.
- RecipeNLG and Recipe1M are non-commercial, research-only.

- **Pick:** build the set.
  - Take 30 English recipes from **Wikibooks Cookbook** (CC BY-SA 4.0, HF
    `gossminn/wikibooks-cookbook` @ `49fb73ea07d2a1380c15197db5589934937457ed`) and about 20 from
    Italian Wikibooks (*Libro di cucina*).
  - Rewrite each as a spoken script and synthesise it with a TTS voice whose licence permits it.
  - The ground truth is the structured recipe.
  - Keep this data in its own folder under BY-SA, not MIT.
- **Better:** 10–20 recipes recorded by the owner, in English and Italian.
- **Fallback:** YouCook2's MIT step annotations with live narrated videos, fetched locally and
  never redistributed. That gives steps only, no ingredients.

## Licence caveats

- CC-BY data (GNHK, MLS, FLEURS, LibriSpeech) needs attribution, kept in a NOTICE file.
- NC-SA data (MathWriting, CROHME) may be downloaded at eval time only; re-check if Myrme is ever
  sold.
- BY-SA data (Wikibooks) and anything derived from it stays BY-SA.
