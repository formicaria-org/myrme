# Releases — which Myrme, which contract, which formicaria

One row per release, newest first. **Contract** is what formicaria checks
(`docs/context/contract.md`, *Versions*): an app built for contract major N links any release of
major N whose minor is at least the one it needs.

| Myrme | Contract | Linked by formicaria | What changed for users |
|---|---|---|---|
| **0.6.0** | **1.5** | — (not yet linked) | The phone's default writes ~65% faster (the same model at 4-bit); a more accurate phone model is catalogued as a choice ("More accurate, slower"); the laptop's mail pass names a conversation without reading it twice, and finds one meeting more. A title eval |
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
