# Reputable sources only, reached with what Myrme already has (plan, 2026-10-29)

> **A plan, built the same day (§9).** It refines §2 of `2026-10-29-web-research.md` around one
> requirement from the owner: *research reads a good set of reputable sources, and only those, to
> avoid security issues*. Every "verified" below was checked live on 2026-10-29 with Myrme's own
> User-Agent and no browser. The owner settled §7 by following its proposals and asking no body for
> permission (`../context/decisions.md`, 2026-10-29 evening).

## TL;DR

- **The allowlist is the safety mechanism.** A small model cannot judge credibility: given a
  misleading health page it agreed with it far more often than with no page at all (web-research §4.3).
  So code reads **only named sources**, and every hit, fetch and redirect is checked against the
  list. Nothing outside it is ever opened.
- **No general search engine is needed to reach them.** Each reputable source is reached one of three
  ways, all keyless:
  1. its own API (Wikipedia, MedlinePlus, Europe PMC, arXiv, Crossref);
  2. **a sources pack**: the topic indexes of public health bodies (WHO fact sheets, NHS A–Z,
     gesund.bund.de, ISS EpiCentro, …), titles only, searched on the device like the food pack;
  3. the references a Wikipedia article cites, kept only when they point at the list.
- **DuckDuckGo stays out by default.** `duckduckgo.com/robots.txt` disallows `/html` and `/lite`,
  there is no API, and its terms reserve the "spirit" of use. A general engine is a
  bring-your-own-key option (W6), and its results are filtered to the same list.
- **Most of the machinery exists.** It includes the honest User-Agent, the 12-hour cache with
  `Retry-After`, Wikipedia whole-article extracts with BM25 passages (W1, uncommitted), the quote check
  `grounding::verify`, `keeps_to_its_quote`, `inert()` and the fixed pipeline. What is missing:
  - **source routing**;
  - **the sources pack**;
  - **a guarded fetch that only opens allowlisted hosts**.

## 1. The list, by what the person is doing

Code picks the row: the command, the view, or patterns in the question (a DOI, a nutrient name).
The model never picks a source.

| Need | Sources, in order | How reached | Languages |
|---|---|---|---|
| **Health** (a symptom, a nutrient's role, a dose) | MedlinePlus → public-body pages from the sources pack → Wikipedia, labelled *encyclopedia, not guidance* | MedlinePlus API; pack + guarded fetch; Wikipedia API | en/es (MedlinePlus); en, fr, de, it via the pack |
| **Food** (foods rich in X, how much X in Y) | the app's food pack (Ciqual, BLS, CREA, Swiss table, USDA) → EFSA reference values from the pack → Wikipedia | local; pack + fetch | every language the tables cover |
| **Exercise** | the exercise pack, when it exists (S-roadmap) → Wikipedia | local; API | — |
| **Papers** (a DOI, an arXiv id, a PMID, "papers on …") | Europe PMC (includes PubMed and open full text) → arXiv → Crossref | APIs | en mostly |
| **Code** | GitHub → Stack Exchange | APIs | en |
| **Anything else** | Wikipedia in the person's language, then English | API (W1) | all |

### 1.1 Health bodies on the list, checked live

All were fetched with Myrme's User-Agent, no JavaScript and no cookies. Every robots.txt allows
`formicaria-myrme` on the page tested.

| Body | Country | Topic index (titles for the pack) | Entries on the index page |
|---|---|---|---|
| WHO fact sheets | — | `who.int/news-room/fact-sheets` | 244 fact sheets |
| NHS Health A–Z, incl. *Vitamins and minerals* | UK | `nhs.uk/conditions/` | 665 pages |
| CDC health topics | US | `cdc.gov/health-topics.html` | 112 links |
| MedlinePlus health topics (also an API) | US | `medlineplus.gov/healthtopics.html` | an A–Z of letters; topics via the API |
| gesund.bund.de (federal health ministry) | DE | `gesund.bund.de/themen` | 95 links |
| ISS EpiCentro A–Z | IT | `epicentro.iss.it/argomenti/a-z` | 56 links |
| EFSA topics (reference values, safety) | EU | `efsa.europa.eu/en/topics` | 385 links |
| Santé publique France, ANSES | FR | pages fetch; their index URLs need picking by hand (the first one tried was a 404) | — |
| BfR, DGE, BLV, salute.gov.it | DE, CH, IT | pages fetch; indexes to pick by hand | — |
| Cochrane (review summaries) | — | via Europe PMC rather than the site | — |

**Not on the list, because they cannot be read without a browser:**
- NIH Office of Dietary Supplements: a Cloudflare 403.
- BMJ: a 403. Its papers are reached through Europe PMC instead.

**Licences to confirm before shipping** (§7): NHS pages for personal reading on the device. The NHS
content *API* needs a syndication agreement; fetching a public page at a person's request is not
syndication, but this should be asked.

### 1.2 Wikipedia's citations as a bridge

A Wikipedia article already cites primary sources. Its external links (`prop=extlinks`, one API call)
filtered to the list gave:

| Article | Links | On the list |
|---|---|---|
| *Vitamin K deficiency* (en) | 19 | 12: PubMed, DOI, WHO ICD |
| *Eisenmangel* (de) | 21 | 10: NCBI, DOI, WHO ICD, BMJ, JAMA |
| *Vitamina D* (it) | 105 | 76: mostly NCBI and DOI |
| *Carence en fer* (fr) | 0 | 0 |

A cited PMID or DOI is then read through Europe PMC, an API on the list. So this is **not** "following
a link from a page" in the sense of web-research §4.5. The link only names an identifier, and code
opens it through an allowlisted API.

## 2. How each piece maps onto today's code

| Piece | Exists | To build |
|---|---|---|
| Honest User-Agent, `https` only | ✅ `websearch::UA` | — |
| Cache 12 h, 256 entries; rest a host on 429/503 | ✅ `Polite` | per-source cache lengths (MedlinePlus 12–24 h, Wikipedia days, papers forever); move to disk |
| Wikipedia in the person's language and English, whole extracts, BM25 passage | ⏳ W1, uncommitted | fix fr/de/it picking the wrong article (§6), Chinese returning nothing |
| arXiv, GitHub | ✅ | — |
| **Source routing** | — | `enum Need { Health, Food, Papers, Code, General }` chosen by `research()`'s caller (the view or command) or by patterns; each `Need` lists its sources. `DirectSearch::search` takes the `Need` |
| MedlinePlus, Europe PMC, Crossref | — | three parsers like `parse_arxiv`, each with a fixture test |
| **Sources pack** | the food pack's build and pin pattern (`ci/pack-foods.py`, `packs.rs` SHA-256 pin) | `ci/pack-sources.py`: fetch each index once at build time, keep `(title, url, body, language)`, FTS5 search on the device. Ships embedded and small (~2,000 rows, tens of KB), rebuilt with each release |
| **Guarded fetch** | — | W5, narrowed (§3) |
| Quote check, no-number-drift, no-negation-flip | ✅ `grounding::verify`, `keeps_to_its_quote` | — |
| Claims as plain text, markup made inert | ✅ `as_shown`, `inert()` | strip zero-width and bidi control characters |
| Domain shown on each claim | partly (the source link) | the body's name, e.g. "NHS (UK)", from the list itself |

## 3. The guarded fetch, narrowed to the list

Pages reached through the sources pack are HTML, so a fetch is needed. Because it only ever opens
listed hosts, it is much simpler than a general one. Each of the following rules is a test.

1. **Hosts are an exact list** (`www.who.int`, `www.nhs.uk`, …), matched after lower-casing and
   punycode. A suffix such as `*.nih.gov` is not matched.
2. `https` on port 443 only. No user-info in the URL, no IP literals.
3. **Redirects are followed by hand**, at most 3, and every hop is checked against the list. A
   redirect off the list ends the fetch.
4. Responses are capped at 2 MB, `text/html` or JSON only, with a 15 s timeout and no cookies.
5. **No JavaScript, no subresources** (no images or scripts), and no links followed from the page.
6. Text is extracted with dom_smoothie or a smaller readability pass, then passed through `inert()`
   and the passage chooser. The model sees only chosen passages.
7. **robots.txt is honoured for our product token**, cached a day per host. All hosts allowed it
   on 2026-10-29.

Private addresses (SSRF) are not reachable by construction, because the host must be on the list.
That still has to be checked on the resolved address in case a listed name ever resolves locally.

## 4. Health: the strict mode

With the health route selected:
- **sources** are MedlinePlus and the pack's public-body pages, plus the food pack for amounts;
- **Wikipedia** is shown only under a heading *Encyclopedia (not medical guidance)*, never mixed into
  the answer's claims;
- **papers** are read only when the person asks for them (a paper is evidence, not guidance);
- **the proposal says when the bodies disagree.** For example, NHS and EFSA give different reference
  values. Code shows both, each with its body's name, and the model is not asked to choose;
- **"I found nothing on the list" is a valid answer**, decided by code (no verified claim). It is
  never filled from outside the list.

This follows formicaria's standing ruling that Health carries only settled guidance.

## 5. DuckDuckGo and general search

| Option | Verdict |
|---|---|
| DDG HTML endpoint, by default | **No.** robots.txt on `duckduckgo.com` disallows `/html` and `/lite`; there is no API; one app on many phones scraping it is what the terms' "spirit" clause is about. It also returns any site, which the list would then discard. |
| DDG, opt-in by the person | possible, but it gains little: results are filtered to the list anyway, and the sources pack plus APIs already reach those sites. Not recommended |
| Bring-your-own-key engine (Brave, Mojeek, Kagi, SearXNG) | **Yes, later (W6).** It is the person's own account and quota, and results are filtered to the list with `site:` or afterwards. A setting can widen the list for that person. |

## 6. Known weaknesses to fix before shipping W1

The uncommitted W1 answers English well and the other languages badly. Measured live on 2026-10-29:
- French, German and Italian *vitamin K* questions found wrong articles;
- Chinese found nothing.

Two cheap fixes, before any model:
1. **Prefer a title match**: an article whose title contains a keyword over the search's first hit.
2. **For Chinese**, search with the question unsegmented in `zh.wikipedia` and take the interlanguage
   link, rather than the filler-word path, which assumes spaces between words.

Both get fixture tests and a line in the W3 eval.

## 7. Decisions for the owner

1. **The list itself** (§1.1). Should anything be added or removed? Candidates not yet checked
   include national bodies elsewhere, Health Canada, and the Chinese CDC.
2. **NHS pages.** Read them at the person's request, or ask NHS first?
3. **robots.txt** for pack-chosen pages: honour it (proposed; it costs nothing today, since every
   host allows us).
4. **DuckDuckGo**: leave it out (proposed), or offer it as an opt-in?
5. **Strict health mode as the default** for health questions (proposed), or as a setting?

## 8. Proposed order

| Step | What | Rough cost |
|---|---|---|
| R1 | Fix W1's two weaknesses (§6), then commit W1 | a day |
| R2 | Source routing (`Need`) plus MedlinePlus and Europe PMC parsers | 2 days |
| R3 | The sources pack (build, pin, on-device FTS) | 2–3 days |
| R4 | The guarded fetch for listed hosts (§3) | 3 days |
| R5 | Wikipedia's cited PMIDs and DOIs read through Europe PMC (§1.2) | a day |
| R6 | Strict health mode in the proposal (§4) | a day |
| R7 | The W3 eval over all of it, including "nothing on the list" cases | 3 days |

R1–R2 alone already give health questions MedlinePlus, and papers Europe PMC, without fetching a
single web page.

## How this was made

- **Pages, robots.txt and topic indexes:** fetched with `curl` and Python's `urllib`, using Myrme's
  User-Agent.
- **robots.txt rules:** parsed with `urllib.robotparser`.
- **Wikipedia's external links:** read through the Action API.

No personal data was used.

## 9. What was built (2026-10-29, evening)

All of R1–R7, in `crates/fm-agent-run/src/`: `sources.rs` (the list, `Need`, the nutrient words, the
sources pack's search, the food list's facts), `guarded.rs` (the fetch and its extractor), `apis.rs`
(MedlinePlus, Europe PMC, Crossref, Stack Overflow, Wikipedia's citations) and `websearch.rs` (the
routing). The decisions are in `../context/decisions.md`, 2026-10-29 evening; the app's food list is
contract 1.10.

### 9.1 How it differs from the plan

- **salute.gov.it is not on the list**: it answers a non-browser with a challenge page. **CDC is on the
  list but not in the pack**: its topic index is a script menu. MedlinePlus covers the US.
- **The pack**: 4,071 pages, 399 KB, from ten bodies (WHO in English, French and Chinese 721, BLV in
  German, French and Italian 1,233, NHS 650, gesund.bund.de 586, ANSES 283, Santé publique France
  187, DGE 127, ISS EpiCentro 108, BfR 98, EFSA 78). Every page was fetched once at build as the app
  would fetch it; its own `<h1>` is its title. French bodies have no page per vitamin; for those, the
  NHS page in English is offered.
- **Health strict mode** is a label, not a separate section: every source's title begins with its
  body's name, and Wikipedia's, for a health question, with *encyclopedia, not medical guidance*. Code
  does not detect two bodies disagreeing; each claim names its body, so both show.
- **The papers Wikipedia cites** are read for paper questions only (strict health reads papers only
  when they are asked for). A cited paper is kept when its title and abstract hold two thirds of the
  question's topic words: the *Iron* article cites geology.
- **The cache stays in memory**, as decided before (nothing about a search on disk).
- **The app's food list** answers "how much of a nutrient in a food" through `VaultAccess::food`. On a
  computer it works with any formicaria from 0.7 (checked against the pinned one by
  `pixi run integration`); a phone needs formicaria to answer `food` in its own vault access when it
  moves to this release. Of the list's hits, the food *named* so is taken (*Egg, whole, raw* for
  "eggs", not *Egg nog*, which the list ranks first).
- **"Foods rich in X"** is not a ranking of the food list (that is S3 of the search-and-retrieval
  plan); the bodies' pages and Wikipedia answer it.

### 9.2 Faults found by measuring, fixed at their class

| Seen | Class | Fix |
|---|---|---|
| "à quoi sert la vitamine K" → *Feijoa* articles | a verb steers Wikipedia's search; *à* not a filler (fillers compared before accents were folded) | fillers compared folded; when the best article shares no word with the question, its **focus** (the nutrient or the topic it names) is searched |
| "Wie äußert sich Eisenmangel?" → *Herzinsuffizienz* | the same | the same |
| "studies on vitamin K and bone" → vegan diets | English read as French (*on*); *K* searched apart from *vitamin* | English's own small words counted in language detection; asking words dropped; *vitamin K* sent as one phrase |
| MedlinePlus second hits off topic (*Uterine fibroids* for vitamin D) | its search ranks loosely | a hit must share a topic word with the question |
| 维生素K → *抗生素耐药性* | Chinese character pairs overlap by chance | a Chinese question's page must hold half its pairs; mixed-script words split where the script changes |
| "vitamine D" → *Hépatite D*; "borrow checker" → *Check-up* | a lone letter, or a loose prefix, taken as the topic | a lone letter requires every word; prefixes match only a plural or a compound |
| "is measles dangerous" → general sources | a disease name is no health word | a question naming a topic a body has a page on (every word of its title) is a health question |

### 9.3 Measured

- **Offline eval** (`tests/eval_sources.rs`, exact match): routing **29/29** in five languages;
  the pack's first page **17/17**, including four cases whose right answer is *no page*.
- **Live** (`websearch::tests::live_sources`, 2026-10-29, twelve questions): every health question
  in en, fr, de, it and zh was answered by MedlinePlus, a body's page and Wikipedia in the question's
  language and in English, in 1.6–4.2 s; the Italian measles question by ISS EpiCentro and the NHS; a
  DOI by its Europe PMC abstract in 0.3 s; a Rust question by Stack Overflow; a general question as
  before. No source off the list appeared.
- **Not measured**: whether the model's grounded answers improve (the W3 answer eval with the model);
  how often a body's robots.txt or markup changes (the pack is rebuilt by hand, `pixi run
  pack-sources`).

