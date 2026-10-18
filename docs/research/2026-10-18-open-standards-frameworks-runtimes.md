# Open standards, frameworks and runtimes for Myrme (research, 2026-10-18)

> **Research, not a ruling.** About 22 searches and fetches by a research agent on 2026-10-18,
> condensed here. **[unverified]** = secondary source or memory only. Decisions it informs:
> `../context/plan.md`, `../context/decisions.md`.

**Bottom line (as given):** adopt **MCP** and **Agent Skills (SKILL.md)**; keep a **hand-rolled
loop in Rust**, no framework; **llama.cpp** as the runtime everywhere (in-process on iOS); watch
**Apple Foundation Models** and **LiteRT-LM** as pluggable backends; **copy** local-deep-researcher's
loop rather than depend on it.

## 1. Standards
### MCP (Model Context Protocol)
- **Current spec revision 2026-07-28**: no initialize handshake; every request carries its
  protocol version (`_meta`, and `MCP-Protocol-Version` on HTTP); mandatory `server/discover`;
  formal deprecation policy (≥12 months). 2025-11-25 servers covered by a compatibility section.
  https://modelcontextprotocol.io/specification/versioning
- **2025-11-25** added Tasks (async), OAuth 2.1, the registry [secondary].
- **Governance:** donated Dec 2025 to the **Agentic AI Foundation (AAIF)**, a Linux Foundation fund,
  with goose and AGENTS.md as founding projects. https://itbrief.com.au/story/anthropic-donates-mcp-to-new-agentic-ai-foundation
- **Registry** (registry.modelcontextprotocol.io): preview since 2025-09-08, API frozen v0.1;
  **no evidence of GA**; ~2,000 servers [unverified].
- **Rust SDK `rmcp`** (https://github.com/modelcontextprotocol/rust-sdk): ~4.0k stars, 3.1.0
  implements 2026-07-28; stdio, child-process, streamable-HTTP server (Tower) and client;
  **requires tokio** — conflicts with the std-only stance unless confined to its own module/thread.
  Alternative: `rust-mcp-sdk` 2.0.
- **Transports:** stdio is natural on desktop; **impossible on iOS** (no processes). The stateless
  2026-07-28 revision makes a hand-rolled JSON-RPC-over-HTTP server small.
### Agent Skills (SKILL.md)
- Open spec at agentskills.io (Dec 2025); reference skills at https://github.com/anthropics/skills.
  Read by Codex CLI, Gemini CLI, Cursor, Copilot/VS Code and others (27–30+ tools [unverified]).
- A folder: SKILL.md (frontmatter `name`, `description`, then instructions) + optional scripts;
  **only descriptions sit in context until triggered** — the property a short-window model needs.
- Portability is imperfect (a July 2026 report: Cursor skipped bundled assets).
### AGENTS.md
Repo instructions for coding agents (an AAIF project). Helps contributors, not the product.
### A2A
Agent-to-agent federation, v1.0 March 2026, in AAIF since ~2026-08-17. **Irrelevant** to a
single-user local app.

## 2. Frameworks
| Name | License | Signal | Small-local fit | Rust/mobile fit |
|---|---|---|---|---|
| smolagents | Apache-2.0 | 29.9k★ | partial; CodeAgent risky | no |
| LangGraph | MIT | ~25–31k★ | heavy | no |
| Pydantic AI | MIT [memory] | ~14k★ | OK | no — design reference |
| DSPy | MIT [memory] | wide research use | offline prompt tuning only | no |
| LlamaIndex | MIT | 46k★ | heavy | no |
| CrewAI | MIT | ~42–51k★ | poor (many calls) | no |
| OpenAI Agents SDK | MIT | ~18k★ | OK via compat API | no |
| Google ADK | Apache-2.0 | ~17k★ | Gemini-tilted | no |
| **goose** (Block) | Apache-2.0 | ~55.4k★ | OK with Ollama | **Rust**; MCP-host reference; a whole app |
| **rig** | MIT [memory] | ~7.9k★, breaking releases (v0.44, 2026-10-07) | OK | Rust, tokio |
| mistral.rs | MIT | ~7.8k★ | grammar-enforced tools, MCP client | Rust; mobile undocumented |

Star counts mostly January 2026 (https://dev.to/ialijr/top-10-most-starred-ai-agent-frameworks-on-github-2026-3d4o).
**Verdict for a Rust codebase: hand-roll the loop** (~300–600 lines over llama-server's
OpenAI-compatible API). Frameworks add tokio/async and churn and do not help with what makes small
models work — per-model templates, grammars, tool subsets. Borrow designs from Pydantic AI (typed
tools) and goose (MCP host).

## 3. Runtimes
- **llama.cpp / llama-server** (~132k★, MIT): OpenAI-style tool calls with `--jinja`, native
  parsers for Llama 3.x, Hermes, Qwen 2.5+, Mistral Nemo, Functionary, Command R7B, DeepSeek R1,
  GPT-OSS, generic fallback otherwise; `parallel_tool_calls` off by default; **aggressive KV quant
  (`-ctk q4_0`) hurts tool calling**. GBNF and `json_schema`; multimodal via mtmd. Android builds
  with Hexagon/Adreno backends; **iOS: link libllama in-process** (e.g. `llama-cpp-2` crate;
  `common_chat_parse` for tool calls). https://github.com/ggml-org/llama.cpp/blob/master/docs/function-calling.md
  Streaming tool calls today [unverified].
- **Ollama** (MIT): daemon; fine as an optional desktop endpoint, not embeddable on mobile.
- **LM Studio**: closed app, OpenAI-compatible server, MCP host [unverified]; desktop only.
- **mistral.rs**: all-Rust GGUF, grammar-enforced tools, agent loop, MCP client; no documented
  Android/iOS.
- **candle**: Rust ML, no tool layer. **MLC LLM**, **ExecuTorch**: mobile runtimes, little or no
  tool layer [not checked].
- **LiteRT-LM** (Google, ~6.7k★, Apache-2.0, v0.18): Android, iOS, web, desktop; Kotlin/C++/Python
  stable, Swift/JS preview, **C API** (versioned prebuilts since v0.16) → callable from Rust over FFI
  in-process on both phones; constrained tool use; Gemma 4 E2B/E4B, Gemma 3n, FunctionGemma, Qwen,
  Phi-4; GPU/NPU. https://github.com/google-ai-edge/LiteRT-LM
- **Apple Foundation Models** (iOS/macOS 26+, Apple Intelligence devices): ~3B on-device model
  [size from memory], Swift `Tool` protocol, guided generation, no download. WWDC26: pluggable
  `LanguageModel` protocol, image input, vision tools [beta vs release unverified]. Swift-only, so
  Rust needs a shim; device-gated. **The only path to an assistant on iOS that needs no subprocess
  and no weights download.**

## 4. Open local research agents
- **local-deep-researcher** (langchain-ai, ~9.4k★, MIT, LangGraph): query → search → summarise →
  reflect on gaps → new query, N times; markdown with sources; DuckDuckGo/SearXNG/Tavily; needs
  JSON (small DeepSeek R1 struggles; there is a fallback). **Best template: each step a narrow
  prompt, no open tool selection.** https://github.com/langchain-ai/local-deep-researcher
- GPT Researcher (~28k★, Apache-2.0 [memory]): planner/executor, tuned for large models.
- open_deep_research: multi-agent supervisor, frontier models [not checked].
- Judgement: fixed pipelines work at 3–8B; free-form multi-agent research does not below ~14B.

## Recommendations (as given)
**Adopt:** MCP both ways (client over stdio on desktop; server exposing notes so goose / Claude Code /
Codex can use formicaria), 2026-07-28 revision, small curated tool subset per turn; Agent Skills
folders with descriptions only in the prompt; AGENTS.md in the repo; a hand-rolled Rust loop over the
OpenAI-compatible API with `--jinja` tool calls and grammar fallback, KV cache f16/q8, an eval set
per model family, any OpenAI-compatible endpoint as a desktop backend; llama.cpp everywhere,
in-process on iOS behind a `Backend` trait; deep research as local-deep-researcher's fixed loop.
**Watch:** Apple Foundation Models, LiteRT-LM, mistral.rs, rig, goose, the MCP registry.
**Avoid:** Python frameworks as runtime dependencies; A2A; code execution on mobile; multi-agent
research at 0.3–8B.
