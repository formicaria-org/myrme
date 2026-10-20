# Packet labels with the OCR, an ingredient vocabulary and the small LLM (research, 2026-10-20)

> **Status: research for Myrme 0.8.0; nothing adopted.** The owner: *"Using the ocr model for
> ingredients and label scanning with a photograph, we can improve it with the llm by doing some RAG
> with real known ingredients that one can expect in food (we should make sure it is available
> because sometimes products have ingredients that a normal person does not). This will be in the
> next myrme release."*

## 1. Where labels stand today

`label.rs` reads a nutrition label with the vision model (2026-10-13, `agents/bench/results.md`):

| Model | Labels wholly right | Wrong figures |
|---|---|---|
| qwen3-vl-4b | 7 of 8 | none |
| The phone's 450M | 2 of 8 | many |

So `reads_labels` is set for the laptop model only, and **a phone cannot read a label at all.** The
OCR of Myrme 0.7.0 reads printed text, so it can change that.

## 2. A packet eval from Open Food Facts (local, not redistributed)

**The set:**
- `~/bench-scratch/labels-tools/fetch.py` fetched 30 products: the 20 most-scanned in Italy and 10 in
  the UK whose ingredient and nutrition photos were selected (cropped by volunteers);
- each has both photos, the ingredient text in the product's language, and the figures per 100 g;
- it was found through `search.openfoodfacts.org`, because the classic search API returned 503.
- **Licences:** images CC BY-SA, data ODbL. They are kept local, used for evaluation only, and never
  committed or compiled into the vocabulary. Only product codes and the fetch script may be
  committed.

**Open Food Facts' references are not reliable enough as they stand.** At least 6 of 30 are in
another language than the photo, or belong to another photo:
- a Spanish list for a British Lindt bar;
- a French list for an Italian Pringles tube;
- an English list for an Italian Nutella jar;
- one "ingredients" photo that is the nutrition table.

References must be hand-checked against the photos; `~/bench-scratch/labels/corrections.json` has
the first two.

## 3. The OCR on real packets (PP-OCRv6 small, laptop CPU, first numbers)

**Time:** 60 photos at 0.45 s each.

**Ingredient lists** (recall of the reference's words in the OCR text, since the photos hold the list
in several languages):
- 15 of 30 at 100%;
- mean 68.7% before correcting the references; the 0% cases are reference errors or the two below.

**Genuine OCR failures:**
- **A curved, glossy tube (Pringles), clear to a person:** "INGREDITIT: Pat dirae" for "INGREDIENTI:
  Patate disidratate", "estrato d levit" for "estratto di lievito", "livit in pove" for "lievito in
  polvere". These are exactly what a vocabulary of known ingredients can repair.
- **A sideways photo (a Coca-Cola can):** nothing read. Turned upright it reads most of the list, two
  lines lost to glare. Turned upside down it returned 67 characters of junk at confidence 0.98.
  - **A short reading at high confidence can be wrong.** The 0.7.0 routing should also ask for a
    minimum amount of text, and the OCR should try the four orientations when a page reads poorly.
    The upright eval pages are unaffected.

**Nutrition tables:** 152 of 209 reference figures (73%) appear in the OCR text. Most tables read
below 0.97 confidence (columns, small print), so the figures need the table read back into rows, not
the routing.

### 3a. With hand-checked references, and the first correction prototypes (2026-10-20)

**The references.** 14 of 30 references were checked against the photos
(`~/bench-scratch/labels/corrections.json`):
- seven were wrong in Open Food Facts (another language, another recipe, or garbled);
- one "ingredients" photo is the nutrition table, so that packet is left out.

**What the OCR got,** of 494 reference words in 29 packets:

| | Words |
|---|---|
| Read correctly (recall **86.4%**) | 427 |
| Misread, with a near miss in the text | 37 |
| Never seen (lost lines, a sideways photo) | 30 |

**What corrects the misreads,** each judged on the same words. The vocabulary is a hand-made seed
list of ~170 packaged-food terms in en/it/fr/de (`~/bench-scratch/labels/vocab/seed.tsv`, our own
work) plus Wikidata's 604 additives (CC0).

| Method | Recall | Correct items broken |
|---|---|---|
| OCR alone | 86.4% | — |
| Word by word, nearest vocabulary word | 86.2–86.8% | 7–14 words changed that were right |
| Whole item (between commas), nearest vocabulary phrase, cutoff 0.75 / 0.85 | 88.3% / 89.5% | about half of all changes ("farina di riso integrale" → "farina di segale", "Italia" → "titania") |
| **Whole item, top 3 candidates, Qwen3.5-0.8B chooses (keep as read or one candidate), grammar-held** | 87.4% | **1 of 27** |

On the 66 items the LLM was asked about (0.21 s an item, laptop CPU):
- kept right: 22;
- fixed: 6;
- changed but still wrong: 9;
- left misread: 24, mostly because no candidate was right;
- broke a right item: 1.

**Reading:**
- **The LLM is what makes correction safe.** Its gain is small until the vocabulary is broad enough
  to hold the right candidate.
- **Word-level correction does not help**, as for handwriting (L1b).
- **The never-seen 6% needs another reader** (a rotation, or the vision model where there is one).
  No vocabulary reaches it.

### 3b. The rotation retry, timed (2026-10-20)

On the laptop CPU, a page a time:

| Pages | Upright only | With the retry |
|---|---|---|
| Handwritten (30) | 0.46 s | 1.77 s |
| Packet photos (60) | 0.46 s | 1.32 s |
| Typed (30) | 0.68 s | 0.67 s |

**Every handwritten page reads "weak", so it was read four times** before going to the vision model
anyway. It was left out of Myrme 0.7.1 and kept as a patch
(`~/bench-scratch/labels-tools/ocr-rotation.patch`). For 0.8.0, it should run only for a label, or
use an orientation model.

## 4. The vocabulary: sources and licences (checked 2026-10-20)

| Source | What | Languages | Licence, when compiled into the app |
|---|---|---|---|
| **Wikidata** (P628, E-number) | 604 additives; industrial ingredients as ordinary items | en 604, de 494, fr 481, zh 363, it 354 labels; many aliases | **CC0: no obligation** |
| **EU food additives database** (Reg. 1333/2008) and **EUR-Lex** (Reg. 1169/2011 Annex II: the 14 allergens; Reg. 231/2012: synonyms) | 395 E-numbers, the authoritative list | 24 EU languages in the legal texts | CC BY 4.0 / Decision 2011/833/EU: **one attribution line** |
| Open Food Facts taxonomies | 4,862 ingredients (~80k names, real label spellings), 750 additives | 207 | **No licence of their own**: AGPL-3.0 repo, ODbL data. Copyleft either way; **avoid** unless OFF grants it in writing |
| Codex INS (CXG 36-1989) | INS numbers | en/fr/es | CC BY-NC-SA 3.0 IGO: **avoid** (non-commercial) |
| EFSA FoodEx2 | exposure classification | en | unclear, and the wrong shape |

**Recommendation:**
- **Wikidata + EU** for additives, industrial ingredients and allergens. Wikidata needs a curated seed
  list for industrial ingredients (modified starches E14xx, glucose-fructose syrup, hydrolysed
  proteins, carriers), which it has no clean class for.
- **formicaria's food list** (already in the app) for ordinary foods.

## 5. The plan for 0.8.0, each step judged on the eval

1. **The eval:** hand-checked references for the 30 packets, scored on ingredient recall and the
   seven nutrition figures.
2. **OCR robustness:**
   - try the four orientations when a page reads poorly, and keep the best;
   - add a minimum-text rule to the routing.
3. **The vocabulary** (Wikidata + EU + the food list), compiled in; attribution in the notices.
4. **Retrieval and the LLM:** Rust splits the list into items and retrieves the nearest vocabulary
   entries per item. Qwen3.5-0.8B picks one or "not in the list, copied as written", its answer held
   to that format; Rust writes the result. It is compared against Rust matching alone, and the LLM
   stays only if it clearly wins.
5. **Labels on the phone:** the OCR's table text through `labelText.ts`, the reader the page already
   uses. It is enabled where it reads the figures right.

Sources:
- https://www.wikidata.org/wiki/Wikidata:Licensing
- https://commission.europa.eu/legal-notice_en
- https://www.legislation.gov.uk/eudn/2011/833
- https://world.openfoodfacts.org/data
- https://world.openfoodfacts.org/terms-of-use
- https://github.com/openfoodfacts/openfoodfacts-server/blob/main/LICENSE
- https://openfoodfacts.github.io/openfoodfacts-server/api/
