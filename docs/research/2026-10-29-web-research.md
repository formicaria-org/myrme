# Research on the internet with small local models (research, 2026-10-29)

> **Research, not a ruling.** The companion of `2026-10-29-search-and-retrieval.md`, which covers the
> notebook's own data. Three research agents surveyed the field on 2026-10-29 (where to search;
> reading pages safely; how to research with small models), a fourth checked the claims the
> recommendations rest on against primary sources, and the main session read Myrme's code and sent a
> few live requests. Marks: **[verified]** (checked against the primary source by the fact-check, or
> measured or read here), **[source]** (seen in the primary source by a research agent, not
> re-checked), **[reported]** (second-hand) and **[estimate]**. **No research loop was run with our
> models**; the phone timings are estimates from rates measured in `2026-10-25-minimum-speed.md`.

## TL;DR

1. **Today's `/research` is one pass over snippets.** One refined query goes to English Wikipedia,
   GitHub repository search and arXiv; the model reads at most 5 snippets of 700 characters and
   writes quoted claims that Rust checks. No page is read, no second search is made, nothing is
   searched in any language but English, and there is no source for health, nutrition or exercise.
2. **Four defects in it, all confirmed** (§1): arXiv queries are ORed (a nutrition question pulls
   physics papers); the User-Agent imitates a browser, against Wikimedia's policy; quotes are
   checked against more text than the model was shown; and the model's claim text enters the note as
   raw Markdown. Each is a small fix.
3. **There is no keyless, terms-safe general web search for an app shipped to many people** (§2).
   Brave forbids sharing a key and has no free tier; Bing's API is retired; Google's closes in 2027;
   public SearXNG instances block bots. General web search is the user's own key or the desktop's own
   SearXNG — never a default.
4. **The keyless sources that are allowed fit this app well**: Wikipedia in every language, with
   whole articles as plain text; Europe PMC (abstracts, open-access full text, preprints);
   MedlinePlus for health (English and Spanish); Crossref and arXiv; Stack Exchange. **Numbers about
   foods come from offline packs**, never from the web.
5. **Better retrieval beats a cleverer loop.** On a fixed corpus a better retriever tripled a 32B
   model's score; on FRAMES, perfect retrieval lifts a strong model from 47.4% to 72.9% **[verified]**.
   So the first improvement is code: read the whole text and choose the right passages, instead of
   handing the model snippets.
6. **Reading pages is feasible and can be made safe** in plain Rust with the HTTP client Myrme already
   has and about 150 lines of guards (§3). The AI-tool request-forgery bugs of 2024–2026 came from
   checking only the first URL and from IPv6 **[verified]**; the guards cover both.
7. **The small model stays a pointer.** Code runs a fixed loop (2 rounds on the phone, 3 on the
   laptop), chooses the sources, crosses languages through Wikipedia's interlanguage links, ranks
   passages and decides "nothing". The model quotes, and may name one follow-up entity copied word for
   word from a source (§4). Free tool use fails at this size: an untrained 0.5B agent scored 0.006
   exact match **[verified]**.
8. **Specialised learned models take no phone step** (§4.7). Code does language, credibility,
   conflicts of numbers and "nothing"; the one model worth measuring is an NLI cross-encoder on the
   laptop, checking that a claim restates its quote — on ONNX Runtime, since llama-server cannot
   return per-class scores, and with a licence question: every multilingual candidate was trained on
   non-commercial XNLI data.
9. **For health, the list of sources is the safety mechanism.** An 8B model given only the worst kind
   of misleading page was right 0.4% of the time, against 93.9% with helpful pages **[verified,
   §4.3]**. Health research reads only named public bodies.

## 1. What `/research` does today, and four defects

The pipeline (`fm-agent/src/lib.rs`, `research()`; `fm-agent-run/src/websearch.rs`): refine the ask
into one query → `DirectSearch` (Wikipedia en, GitHub repositories, arXiv; 4 hits each, 6 kept,
interleaved) → number the hits plus the host note and its linked notes → one call held to
`answer_schema` (`answerable`, then claims with `claim`, `quote`, `source`) → `grounding::verify`
drops any claim whose quote is not in its source → `assemble_body` writes the note with a Sources
list → a proposal. The research eval (`eval_research.rs`) hands the model the sources directly, so
it tests reading and quoting, not searching.

| # | Defect | Evidence | Fix |
|---|---|---|---|
| D1 | **arXiv reads spaces as OR.** `search_query=all:{q}` with `%20` | `vitamin K deficiency` → `all:vitamin OR all:K OR all:deficiency`, **642,502** results, the first about the CKM angle in B → D K decays; with `+AND+`, 0 results **[verified, live, 2026-10-29]** | Join terms with `+AND+` (or quote the phrase); use `https://` |
| D2 | **The User-Agent imitates a browser** — `Mozilla/5.0 (compatible; formicaria-study-agent)`, no contact | Wikimedia's policy: "Do not copy a browser's user agent for your bot"; an identifying UA with a contact is expected **[verified]**. Its 2026 limits give an unidentified client 10 requests a minute and a compliant one 200 **[verified]** | `formicaria-myrme/<version> (+https://github.com/formicaria-org/myrme)` — a project address, never a person's |
| D3 | **Quotes are checked against text the model never saw.** The model is shown `pack_sources(&used, 700)`; `verify(&resp.content, &used)` checks the full `Source.text` | `fm-agent/src/lib.rs` ~284–312 **[verified]**. Today it matters for arXiv abstracts longer than 700 characters; with whole pages, a quote recalled from training would pass | Verify against exactly the text that was packed |
| D4 | **The claim is free text pasted into Markdown.** `answer_schema` makes `claim` an unrestricted string; `assemble_body` writes `- {claim} [\[n\]](url)` | **[verified]**. GitHub repository descriptions, which anyone can write, are sources today, so a description could steer the model into a claim carrying a link. Images are already blocked by both shells' CSP (`img-src 'self' data: blob:`) **[source]**, which removes the silent-beacon form | Escape Markdown and strip URLs in claims (or forbid `[ ] ( ) < > !` and `://` in the schema); show the quote under each claim |

## 2. Where to search

### 2.1 General web search: only the user's own key

| Service | Status for an app given to many people |
|---|---|
| Brave Search API | Terms §4(a): no sharing a key beyond affiliates and agents; §3(b)(i): no storing or caching results beyond transient use; **no free plan** — paid plans include $5 of credit a month (~1,000 requests) **[verified, terms of 2026-09-01]** |
| Bing Search API | retired 2025-08-11 **[source]** |
| Google Custom Search JSON | closed to new customers, ends 2027-01-01 **[reported]** |
| Mojeek, Kagi | paid, keyed **[source]** |
| Marginalia | key; the public test key is rate-limited and non-commercial **[source]** |
| DuckDuckGo | only the Instant Answer API (mostly Wikipedia); results need HTML scraping **[source]** |
| Public SearXNG instances | JSON off by default, bot challenges **[source]**; pointing many users at one is using it as a free proxy |
| Exa, Tavily, Serper, Jina | keyed; a US cloud sees every query **[reported]** |

What open-source research tools do **[source]**: Perplexica bundles its own SearXNG; GPT-Researcher
defaults to Tavily (a key); Khoj and Open WebUI search only once an administrator configures an
engine; local-deep-researcher scrapes DuckDuckGo. None ships a shared key.

**So:** general web search is an option for a person who brings a key (Brave, Mojeek, Kagi; Exa or
Tavily with a "your query goes to a US company" line) or runs SearXNG (the desktop's current proxy).
Keys belong in the system keychain, never in a vault.

### 2.2 The keyless sources to use

| Source | What it gives | Limits and terms | Live |
|---|---|---|---|
| **Wikipedia** (Action API, every language) | search; **whole articles as plain text** (`prop=extracts&explaintext`, e.g. 28,303 characters for *Vitamin K*); titles in other languages (`prop=langlinks`) | 2026 limits: unidentified 10/min per IP; a compliant UA 200/min — **counted by the contact in the UA, not by IP, so one shared bucket for every install** — "subject to experimentation and change"; ≤3 concurrent; honour `Retry-After` **[verified]**. CC BY-SA: show title and link. TextExtracts "isn't actively maintained" **[source]** | fr, de, it, zh **[source]** |
| **Europe PMC** | abstracts (`resultType=core`), **open-access full text** (`/{PMCID}/fullTextXML`, JATS), preprints including medRxiv (`SRC:PPR`), a language filter | keyless **[verified, live]**; rate limit and commercial use **unverified** (a forum post only); full-text reuse per article licence, some CC BY-NC-SA | **[verified]** |
| **MedlinePlus** web service | ranked health topics with summaries | keyless, **85 requests a minute per IP**, cache 12–24 h, say the information "is from MedlinePlus.gov", no logo or endorsement; **English and Spanish only** **[verified]** | **[verified]** |
| **arXiv** | title, abstract; `arxiv.org/html/<id>` full text for most papers | 1 request every 3 s, one connection, "all of the machines under your control as a whole"; a mobile app is a listed use **[verified]** — whether users' phones are "under your control" is unstated: ask arXiv | **[verified]** |
| **Crossref** | metadata, sometimes an abstract | polite pool with a project `mailto`: 10/s, 3 concurrent **[source]** | **[source]** |
| **PubMed** E-utilities | abstracts (two calls) | 3/s per IP keyless, 10/s with a personal key **[source]** | **[source]** |
| **Unpaywall** | where a DOI is open access, with its licence (may be empty) | email parameter (the project's) **[source]** | **[source]** |
| **OpenAlex** | metadata, abstracts, full-text search | keyless works but costs ~$0.001 a search from **$0.10 a day shared by everyone on the IP** (~100 searches); a free personal key gives $1 a day; single-record lookups are free; "keep the key out of code you share" **[verified]** | **[verified]** |
| **Stack Exchange** | questions and answers with bodies | 300 a day per IP keyless **[verified]**; keys are now rotatable and to be "stored securely" — do not assume one can be embedded **[verified]**; CC BY-SA attribution | **[verified]** |
| GitHub | repository search | 10 searches a minute unauthenticated; code search needs a token **[source]** | **[source]** |

**Not usable as live sources** **[source]**: USDA FoodData Central (its key may not be published —
use the CC0 bulk data in a pack); NIH ODS fact-sheet API (a Cloudflare challenge to non-browsers);
the NHS content API (a syndication agreement and key); WHO GHO (deprecated, errors); Semantic
Scholar keyless (a pool shared by all, answered 429 at once; bring a key); Nominatim (1 request a
second across all of an app's users). No keyless health API from a public body was found for French,
German, Italian or Chinese.

### 2.3 Choosing sources by code

1. **The command or the surface decides.** A health surface (or `/research health`) reads MedlinePlus,
   Wikipedia labelled *encyclopedia, not guidance*, and Europe PMC/PubMed only on request. A paper
   note (a DOI, arXiv id or PMID in it) reads Europe PMC, arXiv, Crossref. A food or training view
   reads the local packs first, then Wikipedia. Code reads Stack Exchange, GitHub. Otherwise:
   Wikipedia.
2. **Then topic words, by dictionary**, reusing the nutrient and exercise dictionaries of the
   companion doc (§3.2 there), and patterns for DOIs and paper ids.
3. **Language by code.** Detect the script (Han → Chinese) and the language; search that language's
   Wikipedia, then follow the article's **interlanguage link** to the English one, which is usually
   longer and is what MedlinePlus, Europe PMC and arXiv speak. The 0.8B model does not translate
   (§4.2).
4. **A few sources in parallel, fail-soft, a fixed number from each** — as today. The model never
   chooses a source.
5. **Cache every answer on the device** (12–24 h for MedlinePlus; days for Wikipedia; versioned arXiv
   and PMC items for ever), honour `Retry-After`, one request at a time per source, and say "source
   busy" rather than retrying in a loop. With Wikipedia's bucket shared worldwide, the cache is what
   keeps it working.

## 3. Reading pages

### 3.1 Prefer an API that gives text

Wikipedia's plain-text extracts, arXiv's HTML pages and Europe PMC's JATS XML need no HTML
extraction **[source]**. Most research questions this app sees can be answered from those.

### 3.2 A guarded fetch for everything else

The 2024–2026 request-forgery bugs in AI tools share two causes **[verified]**: Open WebUI checked
only the first URL and followed redirects to private addresses (CVE-2026-45401); its IPv6 private
check was a no-op and IPv4-mapped addresses slipped through (CVE-2026-45331); LangChain.js's
recursive loader followed redirects (CVE-2026-27795). The app itself listens on `127.0.0.1:8765`
with a loopback-trusted API, so this is not theoretical for us.

ureq 2.12.1, already in the tree, calls its pluggable `Resolver` for every connection including
each redirect hop and connects only to the addresses it returns **[verified, source read]** — so a
filtering resolver is "resolve, check, connect to what was checked" by construction. **Except
through a proxy**: then it resolves the proxy, and the guard checks nothing **[verified]**. Build
without `proxy-from-env` and never set a proxy on this agent.

| Guard | Stops |
|---|---|
| `https` only (`https_only(true)`); `http` only for a URL the person pasted | other schemes, downgrades |
| no user-info in the URL; ports 443 and 80 only | `http://user@127.0.0.1:8765/`, odd services |
| a filtering resolver: resolve on a thread with a timeout, **fail the host if any address is non-public**, return the list | loopback, LAN, cloud metadata, DNS rebinding |
| the non-public list, unit-tested: IPv4 0/8, 10/8, 100.64/10, 127/8, 169.254/16, 172.16/12, 192.168/16, 192.0.0/24, the documentation and benchmark ranges, 224/4, 240/4; IPv6 ::, ::1, fc00::/7, fe80::/10, fec0::/10, ff00::/8, 100::/64, 2001::/23, 2001:db8::/32; **unwrap and re-check** IPv4 inside `::ffff:0:0/96`, NAT64 `64:ff9b::/96`, 6to4 `2002::/16` | the IPv6 class of bugs |
| redirects off; follow ≤5 by hand, re-checking each | the redirect class of bugs |
| 15 s total, 5 s to connect; no idle pooling | slow or endless responses |
| a cap on **decompressed** bytes (2 MiB phone, 3 MiB laptop; the largest probe page was 626 KB) | huge pages, gzip bombs |
| a content-type allow-list; refuse binary | non-text input |
| no cookies, no `Authorization`, no `Referer`; an agent of its own | leaking anything |
| ≤3 fetches at once, 1 per host | hammering a site |
| parser limits; extraction behind `catch_unwind` | pathological HTML |
| **stop at a bot wall** (403, 429, a challenge page); never disguise the client | terms and reputation |

No JavaScript runs: the parser builds a tree and drops scripts. A browser engine (including the
Android WebView) is the wrong tool: it runs third parties' code inside the app, and Open WebUI's
browser-based loader is where several of its 2026 bugs clustered **[reported]**. In a probe of 16
typical research pages without JavaScript, **13 served their whole text**; the other 3 were bot
walls, not script-rendered pages **[source, measured by the agent]**.

### 3.3 Extracting the text

| Crate | Licence | Evidence | Fit |
|---|---|---|---|
| **dom_smoothie** 0.19 | MIT | ScrapingHub article benchmark F1 0.865 (precision 0.785, measured at v0.14) **[verified]**; whole text found on 13/13 probe pages; +1.7 MB to a stripped x86 binary **[source, measured]** | the pick: maintained, Readability-faithful, high recall (BM25 then drops the rest) |
| rs-trafilatura 0.2 | MIT/Apache-2.0 | F1 0.970, best of all including Python's trafilatura 0.958 **[verified]**; one author, who also built the WCXB benchmark it leads | measure against dom_smoothie on our own pages |
| trafilatura-rs 0.6 | Apache-2.0 | F1 0.913 against Python's 0.920 on trafilatura's own set **[source]** | heaviest |
| html2text, htmd | — | converters that keep all the boilerplate **[source]** | no |

Small-LLM extractors do not win (ReaderLM-v2 1.5B 0.741 on WCXB) **[source]**. A third-party reader
service (Jina Reader, Firecrawl) sees every URL a person researches — not a default.

PDFs: formicaria runs `pdftotext` on the desktop and has no extractor on Android **[source]**. Prefer
the HTML or XML forms above, so PDFs are rare.

### 3.4 Choosing the passages

Small readers do badly with long or noisy context: models of 7B and below failed to extract the
answer 85–100% of the time on questions they could not answer alone, even with the passage given,
and added context destroyed 42–100% of answers they knew **[verified: arXiv 2603.11513]**; more
passages first help, then hurt **[source: arXiv 2410.05983]**. Quote-first output is an easier task
than free answering, but the lesson stands: **fewer, cleaner passages, best first**.

- **BM25 in Rust** (no crate) over chunks of 2–4 sentences with their heading path, folded like
  `textmatch::fold`, with CJK bigrams; at most 2 chunks a page and 1 a section; a page whose best
  score is below a threshold contributes nothing. Optionally fused with the embedder of the companion
  doc.
- **Budget by time, not characters**: target seconds × the measured prompt-reading rate. Phone
  (Qwen3.5-0.8B, ~105 tokens/s read, `ctx = 4096`) ~1,300 tokens of passages; laptop CPU (~70 tokens/s
  at 4k) ~2,000; laptop GPU ~4,000 **[estimate from measured rates]**. Count tokens with
  llama-server's `/tokenize`: Chinese is about one character a token.

## 4. The research loop

### 4.1 What others do, and why it does not transfer

| System | Loop | Cost |
|---|---|---|
| local-deep-researcher | **fixed**: query → search → summarise → reflect → new query, 3 rounds; its README says 1.5B and 7B models struggle with the JSON **[source]** | ~7 calls |
| GPT-Researcher | planner writes sub-questions, parallel crawls, filter, report **[source]** | 5–15 calls |
| Vane (ex-Perplexica) | model picks tools; 2 / 6 / 25 iterations by mode **[source]** | 3–27 calls |
| Khoj research | model picks tools; 5 iterations, ≤3 searches and 1 page each **[source]** | ≤6 calls |
| open_deep_research (LangChain, archived 2026-08) | supervisor plus tool-calling researchers; **~580k tokens a task** **[source]** | — |
| STORM | five model roles, dozens of calls **[source]** | — |

Small search-trained models — Jan-nano (4B), Lucy (1.7B), II-Search-4B, AgentCPM-Explore,
MiroThinker-8B — all choose their own tools, and their scores are measured by their makers. In II's
own table, **an untrained Qwen3-4B with search already scores 76.8 on SimpleQA**, against 80.1 for
Jan-4B and 91.8 for II-Search-4B **[verified]**: much of the gain is searching at all, which a loop
run by code also gives. The one fixed-loop result, s3 — a trained 7B only writes queries for 3
turns — matched Search-R1-7B (58.9 against 57.8), **but with Claude-3-Haiku writing the answers**,
and its artifacts are non-commercial **[verified]**. It shows that query-writing alone can carry the
gain; it does not show a small answerer benefits.

**Why the model must not drive.** An untrained 0.5B agent averaged 0.006 exact match **[verified:
DGPO]**. Qwen3.5-0.8B's card gives 25.3 on BFCL-V4 and 11.6 on TAU2 — **in thinking mode**; the
model runs non-thinking by default and its card warns it loops when thinking **[verified]**, so
these are an upper bound. Small models that decide when to search under-search and hallucinate
(companion doc §3.7).

### 4.2 What a small model may do, and what code does

| Step | Evidence | Who |
|---|---|---|
| sub-queries | no study isolates a ≤1B decomposer; for single-fact questions one query does as well **[reported]** | **code** searches the person's own words always; the model's refined query is **added** (union), kept only if the eval shows a gain |
| translating the query | Qwen3.5-0.8B scores 27.2 on WMT24++ (2B: 45.8), in thinking mode **[verified]** | **code**, through interlanguage links; on the laptop a 4B translation may be shown as an editable term |
| which results to open | at 0.5B, scoring one result at a time beat ranking a list (0.714 against 0.622 nDCG@10) **[reported]** | **code** on the phone (engine rank plus BM25); the laptop model may point at ≤3 ids, if the eval says so |
| "enough" or "nothing" | small models abstain erratically and context raises their confidence **[source]** | **code**: a fixed number of rounds; "nothing" ⇔ no verified claim |
| reading long pages | 0.8B: 4.7 on AA-LCR, 26.1 on LongBench v2 (thinking mode) **[verified]** | **code** picks passages (§3.4) |
| contradictions | "all models struggle" (WikiContradict); more compute did not help on conflicting results (SealQA) **[source]** | **never resolved**: the model points at the two claims; the note shows both quotes under *sources disagree* |
| quoting | select-then-generate holds quality **[source]** | **the model points, Rust verifies** — as now |

### 4.3 Health and credibility

On TREC health questions, Llama-3-8B agreed with the right answer 93.9% of the time given a helpful
page, 38.3% given a harmful one, 81.6% with no retrieval — and **0.4% against the single worst kind
of misleading page** ("Liar"; other attacks left it 15–55%) **[verified: arXiv 2509.03787, Table 5]**.
Good retrieval raises small models' health accuracy by up to 30% **[source: arXiv 2407.12468]**. So
for health the **source list is the safety mechanism**, chosen by code: named public bodies, then
encyclopedia text labelled as such; the model never rates credibility. This matches formicaria's
standing ruling that Health carries only settled guidance.

### 4.4 Recommended loops

**Phone** (Qwen3.5-0.8B; ≤2 rounds; ~3k model tokens; ≤120 s **[estimate: ~25 s a grounded write]**):
1. *Code*: the command or surface picks the method and the sources; the language is detected.
2. *Code*: search the person's words; *model* (the existing refine step, ≤8 words) adds a second
   query; *code* searches both and merges.
3. *Code*: open the top 3 through their text APIs or the guarded fetch; extract; choose passages
   (~1,300 tokens, best first).
4. *Model*: the existing quote-first answer, plus one optional field, `follow`: **a span of ≤6 words
   copied from a source**, naming what still needs looking up (the bridge in a two-step question).
   Rust checks it is really in a source, or ignores it.
5. *Code*: if `follow` holds and rounds remain, search it with the question's keywords and repeat
   3–4, keeping the first round's verified quotes.
6. *Code* stops at 2 rounds, an empty or unchecked `follow`, no new URL, or 120 s (showing what it
   has). **"Nothing" is decided by code: no verified claim.**
7. *Code* assembles the proposal with its sources and the domain of each claim.

**Laptop** (qwen3-vl-4b; ≤3 rounds; ~10k tokens; ~3 min on CPU, ~1 min with a GPU **[estimate]**):
the same, plus — each only if the eval shows it helps — a translation shown as an editable term,
the model pointing at which results to open, and `conflicts: [[i, j]]` pointing at disagreeing claims.

**Experiments, laptop, downloads only on request:** Lucy-1.7B writing the queries and `follow`
(s3-style); a free tool-use model (II-Search-4B or Jan-nano) capped at 8 calls, on the same cases.

### 4.5 Prompt injection in what is read

Already in place: the model has no tools; code chooses every URL and never follows a link found in a
page; output is held to a schema; every quote is checked; everything is a proposal; remote images are
blocked. Still to do before reading arbitrary pages: **D4** (claims as plain text); a lexical check
that a claim restates its quote, or the quote shown under it; **never let fetched text shape a later
outbound query** (the `follow` span is copied from a source, so it is searched only together with
the person's own keywords and shown in the proposal); strip zero-width and bidirectional control
characters; show the domain on each claim — a verified quote from a hostile page is still that
page's claim.

### 4.6 Politeness

A URL the person pasted is a user-triggered fetch, like Google's own user-triggered fetchers, which
"generally ignore robots.txt" **[verified]**. URLs that *code* picked from results could honour
robots.txt for our own product token, cached a day per host **[source: RFC 9309]** — a choice for the
owner. Cache extracted text per vault in the disposable index (final URL, title, text, date, ETag),
a week for the general web.

### 4.7 Each step: code, a specialised model, or the LLM

A third pass (2026-10-29, one research agent and one fact-check) asked, step by step, whether a small
**specialised learned model** — not a general LLM — should take a step from Qwen3.5-0.8B. It builds
on Myrme's own measurements: multilingual-e5-small picking sentences scored 4/8 exact, and an
off-topic match (0.835) sat beside a real answer (0.836), so **similarity can rank but never decide
"nothing"**; GLiNER finds typed spans but cannot judge which matters; and its int8 mDeBERTa export
failed outright (0/12), so **every int8 file must be scored against full precision before it counts**
(`2026-10-19-myrme-light.md`, L2, L2b, L6).

**Runtime facts, checked at llama.cpp b10076** **[verified]**: the converter registers XLM-R,
BERT, RoBERTa and ModernBERT *sequence classifiers* and T5, but **no DeBERTa, no question-answering
head and no token classifier**. And llama-server has **no route that returns per-class scores**:
`/rerank` returns one logit, and `/embeddings` normalises what it returns. So NLI, QA readers,
LettuceDetect and GLiNER run on **ONNX Runtime**, not llama.cpp.

| Step | Phone | Laptop | Why |
|---|---|---|---|
| Language of the question | **code**: `whichlang` (MIT, 285 KB, 16 languages incl. Chinese; ~85% on two-word queries) or `lingua` limited to our languages; the app's locale when unsure | same | a crate, not a model; every detector fails on bare proper nouns **[verified]** |
| Queries | **code** (the person's words) ∪ the **LLM**'s refined query | same; TongSearch-QR-1.5B (MIT) only as an experiment — its 1.5B scores 24.6 on BRIGHT, below GPT-4o's 26.5; only the 7B beats it **[verified]** | no small permissive query writer shows a gain here |
| Sources, fetching, passages | **code** (§2.3, §3) | same; an embedder per the companion doc if the eval earns it | — |
| The quoted answer | **LLM points**, Rust verifies | same | a QA reader returns one span, the same weakness as e5; trained on English SQuAD only (German MLQA F1 falls to 44 for the base model) **[verified]** |
| Does the claim restate its quote? | **code**: every number, date, unit and negation in the claim, and most of its content words, must be in the quote; the quote shown under the claim | + an **NLI cross-encoder on ONNX**, flagging, never blocking: mDeBERTa-v3-base-xnli (279M; int8 338 MB; XNLI accuracy en .871, de .824, fr .823, zh .803) **[verified]** — only if it beats the lexical check on the eval | a short quote against a one-sentence claim is exactly what NLI is trained for, and the 0.8B judging its own claim is self-grading |
| `follow` | the **LLM** points — at a span, or better at one of the page's **own links**, which code offers as candidates | same | choosing the bridge is judgement, which GLiNER could not do |
| Conflicts | **code**: numbers, dates and units parsed and normalised ("2.1 million" = "2,100,000") | + the same NLI model's contradiction class, measured on our lengths | large NLI models beat Llama-3-8B on short-evidence conflicts (88.8 against 75.2 F1), but base-size NLI on long passages is untested **[verified: arXiv 2410.04068]** |
| "Nothing" | **code**: no verified claim | experiment: a SQuAD-2 reader's no-answer score (mdeberta-v3-base-squad2, MIT, NoAns F1 82.1) against the same cases | — |
| Credibility | **code**: the named-source list; for papers, journal and retraction lookups by DOI (PaperQA2's idea) | same | **no learned "quality" score**: such classifiers measure educational style, and a classifier exists precisely to catch misinformation that passes them **[verified]** |

**Extra download: phone 0 MB** (one small crate); **laptop ~340–560 MB** (one NLI file, int8 or fp16),
and only if the eval earns it. A phone NLI fallback, multilingual-MiniLMv2-L6 (int8 107 MB, from an
onnx-community conversion with no licence field), only if the lexical check fails its cases.

**Licences — a decision for the owner.** XNLI and ANLI are CC BY-NC 4.0 **[verified]**, and they are
in the training data of **every multilingual NLI candidate** (both MoritzLaurer models, tagged MIT)
and of MiniCheck (tagged MIT). Whether non-commercial training data binds the weights is unsettled.
If it excludes them, the clean path is **LettuceDetect v2** (Apache-2.0 weights, CC-BY-4.0 data,
Chinese included, 1.23 GB; ONNX export by us — it is a ModernBERT token classifier llama.cpp cannot
convert) **[verified]**. Also: deepset's QA weights are CC-BY-4.0 (attribution), SQuAD 2.0 is
CC BY-SA 4.0, and PaperQA2's journal list (from Finland's JUFO portal) and its retraction data (fetched
from Crossref Labs at run time) carry no licence in that repo **[verified]** — check before copying.

**Worth taking from open research agents, as ideas** (GPT-Researcher and II-Researcher are
Apache-2.0; Khoj is AGPL-3.0, so ideas only) **[verified]**: skip passage compression when the text
is already short; score a passage against the question **and the page title**; and II-Researcher's
compressor has the LLM return **segment numbers** — the pointing Myrme already uses. Not their
similarity thresholds as a gate (Myrme's L6 result).

**Corrections from the fact-check:** gliner-x-small covers 20–22 languages, not 23, and is **weaker
than gliner_multi-v2.1 on Chinese** (0.579 against 0.641); the "NLI loses 18 F1 on long evidence"
was the xlarge model — the xxlarge gained.

**Eval cases this adds to §5:** 30 claim–quote pairs (10 English, 5 each French, German, Italian,
Chinese: a paraphrase, a changed number, a flipped negation, a swapped entity, an added clause, a
claim carrying an instruction), scored for Rust's check, MiniLM-L6, mDeBERTa at fp32/fp16/int8, and
the 0.8B asked yes or no, with latency per pair; the "nothing" cases for the SQuAD-2 reader; 5 conflict
near-misses that are not conflicts ("1.8 m" and "180 cm", "in 2019" and "since 2019", the same fact in
two languages); `follow` as a free span against pointing among link candidates; and 40 short queries
for language detection (bare names, loanwords, mixed scripts). First, three runtime checks: the
mDeBERTa ONNX file on ONNX Runtime 1.23 through `ort` rc.12 on Android arm64; whether the `tokenizers`
crate builds without its C++ dependency; and int8 against full precision for each file.

## 5. The eval

The current research eval (8 invented cases, scored *exact* and *grounded*) tests reading and
quoting. Extend it to the whole loop, offline and deterministic:

- **A small invented web in the real APIs' shapes** — Wikipedia search, extract and langlinks JSON,
  arXiv Atom, MedlinePlus XML, a few HTML pages — stored as recorded responses keyed by the
  normalised request URL. A replay implementation of `WebSearch` and the fetcher **fails on any
  unrecorded URL**, so the gate never touches the network. A separate, ignored live test re-records
  only to catch API drift. Invented facts mean the model's memory cannot answer: what is measured is
  retrieval and grounding.
- **~40 cases:** 10 single-step; 8 two-step (tests `follow`); **8 "nothing"**, including near-miss
  pages about a similar name; 5 conflicts (both claims and the pair expected); 4 credibility (a wrong
  answer on a page outside the health list); 3 non-English questions whose page exists only in
  English (tests the language bridge); 2 pages with injected instructions (output must not change).
- **Metrics:** exact (the set of fact and source pairs; empty for "nothing"); grounded; "nothing"
  false positives and negatives apart; citation precision and recall; **retrieval recall per round
  with no model at all** (cheap enough for every gate run); `follow` exact; calls, tokens and seconds
  per case. **Run the model cases more than once and report which cases flipped.**
- **Page extraction:** ~30 saved pages with the passage that answers each question marked; score
  passage recall within the token budget; dom_smoothie against rs-trafilatura.

## 6. Recommended order

| Step | What | Cost |
|---|---|---|
| **W0** | Fix D1–D4 — **done 2026-10-29** (`../context/decisions.md`); still to do: send searches with the person's words as well as the refined query | hours; no dependency |
| **W1** | Wikipedia in the person's language plus English through interlanguage links, with **whole plain-text extracts** in place of snippets; passages chosen by BM25; verify against the packed passages | ~2 days; no dependency |
| **W2** | Sources by command and surface (§2.3): MedlinePlus and Europe PMC added; Crossref and Unpaywall for papers; an on-device cache with `Retry-After` handling — **the cache and `Retry-After` done 2026-10-29** (in memory, `../context/decisions.md`) | ~3 days |
| **W3** | The offline eval of §5 | ~3 days |
| **W4** | The fixed two-round loop with `follow` (phone) and three rounds (laptop) | ~3 days + eval |
| **W5** | The guarded fetch and dom_smoothie, for URLs a person pastes first, then for results; `cargo deny` on the html5ever stack | ~1 week |
| **W6** | Bring-your-own-key general search (Brave, Mojeek, Kagi, SearXNG), keys in the keychain | ~2 days |
| later | laptop experiments (§4.4); offline packs for nutrient intakes and exercises (companion doc) | — |

**Open for the owner:**
1. **robots.txt** for URLs that code picked from results: honour it (the cautious reading) or treat
   every `/research` fetch as user-triggered (as Google and OpenAI do).
2. **Wikipedia's shared bucket.** One project contact means every install shares 200 requests a
   minute. A per-user contact in the UA is allowed by the policy, but Wikimedia says the keying "may
   have to change if this mechanism is abused". Which contact goes in the UA?
3. **Health outside English and Spanish.** No keyless public-body API was found for French, German,
   Italian or Chinese; until one is, those questions read Wikipedia labelled as an encyclopedia.
4. **arXiv's "machines under your control"**: worth an email to arXiv before shipping fetches from
   every phone.

## 7. The fact-check (2026-10-29)

Sixteen claims were checked against primary sources. **None was invented.** What changed:

| As first reported | After the check |
|---|---|
| Wikimedia limits are per IP | the compliant-UA class is counted **by the contact in the UA, not by IP** — one bucket for all installs |
| OpenAlex keys required since 2026 | keyless still works; only ~100 searches a day per IP; a key is needed only for deep paging |
| Brave free tier | **no free plan**; $5 credit a month on paid plans; the clause is §4(a) |
| ureq's resolver guards every connection | yes — **except through a proxy** |
| Llama-3-8B 0.4% on adversarial pages | 0.4% is the single worst attack; others 15–55% |
| Qwen3.5-0.8B card scores | all in **thinking mode**; the model runs non-thinking |
| s3 shows small models gain from a fixed loop | the answers were written by Claude-3-Haiku |
| BrowseComp-Plus: retriever tripled the score | true, and the model averaged under one search a question |
| FRAMES 47.4 → 72.9 | measured with Gemini-Pro-1.5, not a small model |
| Stack Exchange app keys may be embedded | the 2025 keys are rotatable and to be stored securely — do not assume |
| Europe PMC 500/min, commercial use OK | unverified (forum only) |
| 360M–8B models fail 85–100% at extraction | the abstract scopes it to 7B and below |

## 8. Not measured

- No research loop run with our models; every phone and laptop time is an estimate from measured
  token rates; network time on mobile data is unmeasured.
- dom_smoothie's size and speed on Android arm64; extraction quality on our own pages.
- Whether a 0.8B model can point at a `follow` span reliably — the two-step cases will tell.
- Whether the refined query helps at all against the person's own words.
- Europe PMC's rate limit and terms; CORE and Unpaywall licences; arXiv's 75% HTML coverage.
- The non-public address list on a real carrier network (NAT64, CGNAT) — test before shipping the
  fetcher on phones.

## How this was made

Three research agents ran in parallel (about 131k, 182k and 168k tokens; ~250 searches, fetches and
small live requests), then a fact-check agent (about 122k tokens, 54 calls) checked sixteen claims
against primary sources — about 600k tokens and four agents in all, under the owner's cap of five
agents and 1M tokens a run. The main session read Myrme's research code and confirmed D1 with two
live arXiv requests and D2–D4 in the code.

A third pass on specialised learned models (§4.7) — one research agent (about 151k tokens) and one
fact-check of its twelve claim groups — was run separately the same day, under the same cap. The agents' full
reports are not kept in the repo.
