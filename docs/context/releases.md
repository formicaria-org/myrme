# Releases — which Myrme, which contract, which formicaria

One row per release, newest first. **Contract** is what formicaria checks
(`docs/context/contract.md`, *Versions*): an app built for contract major N links any release of
major N whose minor is at least the one it needs.

| Myrme | Contract | Linked by formicaria | What changed for users |
|---|---|---|---|
| **0.12.1** | **1.10** | formicaria `main` from 2026-10-29 (its next release) | The manual's assistant chapter says which sources `/research` reads; `fmserve::best_food` listed in the contract |
| 0.12.0 | 1.10 | — (superseded by 0.12.1 before formicaria linked it) | `/research` reads only a list of reputable sources (public health bodies, Wikipedia, Europe PMC, arXiv, Crossref, Stack Overflow, GitHub), chosen from the question in five languages; health questions read MedlinePlus and the bodies' own pages first, Wikipedia named as an encyclopedia; "how much X in Y" reads the app's food list; no general search engine |
| 0.11.1 | 1.9 | 0.22.0 – 0.22.1 | A meeting whose zone is misspelt, or written only in the invitation, lands at the right time; `/research` searches arXiv for every word and leaves fillers out, checks a claim only against what the model was shown and against its quote's numbers and negations, writes claims as plain text, names itself to the sources, reuses an answer for 12 h and lets a busy source rest |
| 0.11.0 | 1.9 | 0.20.0 – 0.21.0 | Four commands on every device (`/search` is read as `/research`); a chat reply shows as it is written, on the phone too; the phone's model rereads far less of a conversation each turn, so a later reply starts about twice as soon |
| 0.10.0 | 1.8 | 0.19.0 | The assistant follows formicaria's clock and never its computer's: a change of zone, or a phone, no longer puts the meetings it finds hours off or proposes one twice; "tomorrow" in an email is the sender's tomorrow; a chat knows what day it is |
| 0.9.0 | 1.7 | 0.18.0 – 0.18.1 | The meeting pass reads only what is new since its last read, so a long exchange with a meeting a week gets its next meeting proposed; an answer cut at the cap, or one that repeats itself, is read again instead of being taken for "no meetings"; a meeting restated without its time is the one already on that day |
| 0.8.1 | 1.7 | 0.17.0 – 0.17.1 | A laptop whose only GPU is integrated runs the model on the CPU, about 2.5× faster at writing there, and says so in the assistant's log; a discrete GPU is used as before |
| 0.8.0 | 1.7 | 0.15.0 – 0.16.0 | A packet's label is read by the OCR first, turned when upright reads too little, and a phone can read one; a meeting time written on another clock ("10am Berlin time", "15:00 CET") is moved onto the person's, on the meeting's own day |
| 0.7.1 | 1.6 | 0.9.9 – 0.14.0 | The mail verdict in plain words ("the meeting is still on Fri 23 Oct at 16:00", "has taken place"); a meeting that clashes with another says so, in the verdict and in the note proposed |
| 0.7.0 | 1.6 | 0.9.7, 0.9.8 | The phone runs Qwen3.5-0.8B and no longer offers the 450M; printed pages are read by an OCR (PP-OCRv6) on Linux, macOS and the phone, handwriting still by the vision model; a meeting that began before the newest email is past |
| 0.6.0 | 1.5 | 0.9.5, 0.9.6 | The phone's default writes ~65% faster (the same model at 4-bit); a more accurate phone model is catalogued as a choice ("More accurate, slower"); the laptop's mail pass names a conversation without reading it twice, and finds one meeting more. A title eval |
| 0.5.1 | 1.4 | formicaria 0.9.4 | The phone starts its model with the model's own context window (4096 for lfm2.5-vl-450m) instead of the global 2048, so a photo leaves room for the answer |
| 0.5.0 | 1.4 | — (superseded by 0.5.1 before formicaria released) | The assistant chapter of formicaria's manual is written here and rendered by formicaria from the pin; the catalogue carries the phone's runtimes, so formicaria keeps no copy of either. Versions are released by their published GitHub Release page |
| 0.4.1 | 1.3 | formicaria 0.9.3 | A model that fails to load is reported as such within seconds, on the desktop and the phone, instead of the assistant announcing itself warm after a 40-second wait; the phone also stops its transcriber then |
| 0.4.0 | 1.3 | — (superseded by 0.4.1 before formicaria released) | **The phone hosts Myrme through the facade too** (`Bundled`, `Tools::provision_weights`, `installed_for`, `run_in_process`): formicaria's phone host lost about 250 lines of Myrme's insides. **An app update brings the runtime its catalogue pins** (`runtime_outdated`, `refresh_runtime`): the faster runtime reaches people who already had the assistant |
| 0.3.0 | 1.1 | formicaria `main` from 2026-10-18 (after its push of the cutover) | The desktop host's facade, `fm_agent_run::host`: formicaria asks Myrme what is installed, fetches it, removes it and starts it, without reaching into Myrme's insides. Fixes removing the downloaded model from Settings, which an installed copy refused |
| 0.2.0 | 1.0 | formicaria `main` briefly, 2026-10-18 | The first versioned release: the contract is numbered, and formicaria checks it at build time. Everything below, unchanged |
| 0.1.2 | — | `main` after the cutover (2026-10-18) | Discussion history without Myrme's own mention and commands |
| 0.1.1 | — | — | An edit's kind is decided from the request alone |
| 0.1.0 | — | — | Every method measured; runtime with llguidance; research says whether it can answer; anchored note edits |

Releases before 0.2.0 had no contract number; their surface is what contract 1.0 describes.

**Cutting a release:**
1. Set `version` in `Cargo.toml`.
2. If `contract.md` changed, move the contract version: major for a removal, rename or change of
   meaning; minor for an addition.
3. Add a row here, and write the release notes in `docs/releases/v<version>.md`: a `# Myrme <version> — …`
   title, what changes for people using formicaria, what changes for formicaria, and how it was
   checked. CI publishes no page without it.
4. Run `pixi run ci` and `pixi run integration`.
5. Tag `v<version>`, annotated, and push it. CI refuses a tag that differs from `Cargo.toml`.
6. **CI publishes the GitHub Release page once the gate and the integration test pass on the tag**
   (`agents/release-pages.sh`). That page is what makes it a release: a tag without one is not
   linked by formicaria. A tag whose CI failed gets no page; delete it and tag again.
7. In formicaria, `pixi run myrme-bump` moves the pin to the newest release of the contract it
   builds for and runs its gate.
