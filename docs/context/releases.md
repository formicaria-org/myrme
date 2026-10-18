# Releases — which Myrme, which contract, which formicaria

One row per release, newest first. **Contract** is what formicaria checks
(`docs/context/contract.md`, *Versions*): an app built for contract major N links any release of
major N whose minor is at least the one it needs.

| Myrme | Contract | Linked by formicaria | What changed for users |
|---|---|---|---|
| **0.3.0** | **1.1** | formicaria `main` from 2026-10-18 (after its push of the cutover) | The desktop host's facade, `fm_agent_run::host`: formicaria asks Myrme what is installed, fetches it, removes it and starts it, without reaching into Myrme's insides. Fixes removing the downloaded model from Settings, which an installed copy refused |
| 0.2.0 | 1.0 | formicaria `main` briefly, 2026-10-18 | The first versioned release: the contract is numbered, and formicaria checks it at build time. Everything below, unchanged |
| 0.1.2 | — | `main` after the cutover (2026-10-18) | Discussion history without Myrme's own mention and commands |
| 0.1.1 | — | — | An edit's kind is decided from the request alone |
| 0.1.0 | — | — | Every method measured; runtime with llguidance; research says whether it can answer; anchored note edits |

Releases before 0.2.0 had no contract number; their surface is what contract 1.0 describes.

**Cutting a release:**
1. Set `version` in `Cargo.toml`.
2. If `contract.md` changed, move the contract version: major for a removal, rename or change of
   meaning; minor for an addition.
3. Add a row here.
4. Run `pixi run ci` and `pixi run integration`.
5. Tag `v<version>`, annotated. CI refuses a tag that differs from `Cargo.toml`.
6. In formicaria, `pixi run myrme-bump` moves the pin to the newest release of the contract it
   builds for and runs its gate.
