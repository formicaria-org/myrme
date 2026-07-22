//! Shared runner logic for the study agent: the fm-serve client and the "handle one discussion turn"
//! glue that both the chat REPL and the resident `@name` watcher use. All store I/O goes through a
//! running fm-serve (the single vault writer — no second FileStore), and retrieval is RAG over the
//! vault (FTS via fm-serve) plus optional web search through a local proxy.

/// In-app model downloader (resumable + checksum). Agent-only — behind the `download` feature so a
/// build that runs an already-provisioned model links no HTTPS/TLS stack.
#[cfg(feature = "download")]
pub mod fetch;
pub mod fmserve;
pub mod manifest;
/// Locate the Android native-library dir (where the bundled model runtime lives) in pure Rust.
pub mod nativelib;
pub mod watch;

use fm_agent::openai::OpenAiStep;
use fm_agent::search::SearxngSearch;
use fm_agent::{convo, AgentError, InputDoc, SearchHit, StudyAssistant, WebSearch};
use fmserve::VaultAccess;

/// A configured agent bound to a vault (via any [`VaultAccess`]) + a warm model (+ optional
/// web-search proxy). Generic over the vault seam so the same runner works over HTTP on the desktop
/// and in-process on a phone.
pub struct Agent<V: VaultAccess> {
    pub fm: V,
    pub model_port: u16,
    /// The model/agent name — also what a user `@name`-mentions, and the git author of its proposals.
    pub model: String,
    /// The local web-search proxy port, or `None` to run without the web.
    pub searxng_port: Option<u16>,
    pub max_reply_chars: usize,
    /// How many related notes to retrieve as RAG context.
    pub retrieve: usize,
    /// History budget (chars). When the discussion overflows it, `history` pins both ends — the first
    /// turn (the original ask) and the latest — and fills the middle with the most recent turns that
    /// fit (plain truncation, not summarized). Cheap, predictable, and it never drops the standing
    /// constraints the opener carries.
    pub history_budget: usize,
}

impl<V: VaultAccess> Agent<V> {
    /// Handle one already-posted user message on `note`'s discussion end to end: gather context (host
    /// note + RAG + web when `/search`), truncate old history if it overflows, run the turn, and POST
    /// the agent's reply (and a proposal when `allow_propose` and `/propose`). Returns the reply text.
    /// The user's own message is assumed already in the discussion (the REPL or the app posted it).
    /// Returns the reply text and, when one was posted, the id of the reply message — so a caller
    /// watching the discussion can mark it seen and never answer the agent's own message.
    ///
    /// `on_stage` is called on entering each pipeline stage (`reading your notes`, `searching the
    /// web`, `thinking`) so a caller can surface *what the agent is doing* — because in an agent the
    /// slow part is often the tools (web search, retrieval), not the LLM. Pass `&|_| {}` to ignore it.
    pub fn handle(
        &self,
        note: &str,
        intent: &convo::Intent,
        allow_propose: bool,
        on_stage: &dyn Fn(&str),
    ) -> Result<(String, Option<String>), String> {
        // In a plain discussion there is nothing to propose an edit to, so propose is off there.
        let intent = convo::Intent { propose: intent.propose && allow_propose, ..intent.clone() };

        on_stage("reading the conversation");
        let history = self.history(note)?;
        // The note being discussed, plus the notes it explicitly **links** to (text only) — bounded,
        // relevant context the user chose by linking, not a vault-wide search (that fed a tiny model a
        // pile of unrelated fragments it parroted back). `/search` still adds the web.
        on_stage("reading this note");
        let mut context = self.host_note(note)?;
        context.extend(self.linked_notes(note)?);
        // Web search is best-effort: if the proxy is unreachable, don't fail the whole turn — answer
        // from notes/memory and *tell the user* the answer isn't web-grounded (a wrong answer that
        // looks researched is worse than a flagged one).
        let mut web_unavailable = false;
        if intent.search {
            on_stage("searching the web");
            match self.web(&intent.ask) {
                Ok(docs) => context.extend(docs),
                Err(e) => {
                    web_unavailable = true;
                    on_stage("web search unavailable");
                    eprintln!("web search unavailable: {e}");
                }
            }
        }

        on_stage("thinking");
        let agent = StudyAssistant::new(self.llm(), NoWeb);
        let turn = agent
            .turn(&history, &intent, &context, Some(self.max_reply_chars))
            .map_err(|e| e.to_string())?;

        on_stage("writing the reply");

        let reply = turn.reply.clone().unwrap_or_default();
        // A tiny model sometimes returns nothing usable, or the echo-stripper cleans it to empty.
        // Never post an empty message — the store rejects it (a reply "needs something in it"), which
        // would surface as a 500; say so plainly instead.
        let reply = if reply.trim().is_empty() {
            "(I couldn't get a usable answer from the model — try rephrasing, or ask something simpler.)"
                .to_string()
        } else if web_unavailable {
            format!(
                "_(Web search was unavailable — answering from your notes and the model's own \
                 knowledge, which may be unreliable.)_\n\n{reply}"
            )
        } else {
            reply
        };
        let email = format!("{}@fm-agents.local", self.model);
        let mut reply_id = None;
        if turn.reply.is_some() {
            // Attributed to the model, so the discussion labels who said it.
            let meta = self.fm.reply_as(note, &reply, &self.model, &email)?;
            reply_id = meta["id"].as_str().map(|s| s.to_string());
        }
        if let Some(body) = &turn.proposal {
            self.fm.create_proposal(note, body, &self.model, &email)?;
        }
        Ok((reply, reply_id))
    }

    fn llm(&self) -> OpenAiStep {
        OpenAiStep::local(self.model_port, &self.model)
    }

    /// The discussion so far as text, oldest first, within `history_budget`. When it overflows, the
    /// **first turn and the latest turn are both pinned** and the middle is filled with the most recent
    /// turns that fit — so the conversation never blows a tiny model's context *and* the original ask
    /// (the constraints every later round must still honour) is never the thing that drops. Plain,
    /// deterministic truncation — no summarize-model call (the owner cut that orchestration; a tiny
    /// model's summary of a chat is unreliable anyway).
    fn history(&self, note: &str) -> Result<String, String> {
        let view = self.fm.thread(note)?;
        let msgs: Vec<String> = view["messages"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|m| m["body"].as_str().map(|b| format!("- {}", b.trim())))
                    .collect()
            })
            .unwrap_or_default();
        let full = msgs.join("\n");
        if full.chars().count() <= self.history_budget || msgs.len() <= 1 {
            // Fits, or a lone over-long turn there is nothing to drop from — keep it whole (never
            // return empty, the old tail-only bound's edge case).
            return Ok(full);
        }
        // Over budget. Pin BOTH ends: the FIRST turn (the original ask + standing constraints) and the
        // LATEST turn (the current state), then fill the middle with the most recent turns that fit.
        // The old "keep the recent tail, drop the oldest" bound is what let a long refinement silently
        // lose an early requirement and re-break it (oscillation); pinning the opener fixes that.
        let last = msgs.len() - 1;
        let gap = "- […earlier turns omitted…]";
        // Reserve room for both ends and (if the kept turns aren't contiguous) the gap marker, so the
        // result stays within budget.
        let mut used = msgs[0].chars().count() + msgs[last].chars().count() + gap.chars().count() + 2;
        let mut middle: Vec<usize> = Vec::new();
        for i in (1..last).rev() {
            let cost = msgs[i].chars().count() + 1;
            if used + cost > self.history_budget {
                break;
            }
            used += cost;
            middle.push(i);
        }
        middle.reverse(); // back to chronological order
        let mut out = vec![msgs[0].clone()];
        // A gap marker only when a real hole was left between the opener and what follows it.
        let hole = match middle.first() {
            Some(&i) => i > 1,
            None => last > 1,
        };
        if hole {
            out.push(gap.to_string());
        }
        out.extend(middle.into_iter().map(|i| msgs[i].clone()));
        out.push(msgs[last].clone());
        Ok(out.join("\n"))
    }

    /// The host note itself — what the discussion is about, so always included.
    fn host_note(&self, note: &str) -> Result<Vec<InputDoc>, String> {
        let v = self.fm.get(note)?;
        let body = v["body"].as_str().unwrap_or_default();
        if body.is_empty() {
            return Ok(Vec::new());
        }
        let title = v["title"].as_str().unwrap_or("this note");
        Ok(vec![InputDoc { label: format!("{title} (this note)"), text: body.chars().take(1200).collect() }])
    }

    /// The notes the host note **links to** (`[..](note:<id>)` in its body) — their *text only*, as
    /// context. Bounded (deduped, host excluded, capped) and deliberately shallow: only what the user
    /// chose to link, one hop, no vault-wide search — the safe half of RAG the owner asked for.
    fn linked_notes(&self, host: &str) -> Result<Vec<InputDoc>, String> {
        let v = self.fm.get(host)?;
        let body = v["body"].as_str().unwrap_or_default();
        let mut docs = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for id in note_refs(body) {
            if id == host || !seen.insert(id.clone()) {
                continue;
            }
            let Ok(note) = self.fm.get(&id) else { continue };
            let text = note["body"].as_str().unwrap_or_default();
            if text.trim().is_empty() {
                continue;
            }
            let title = note["title"].as_str().unwrap_or("linked note");
            docs.push(InputDoc { label: format!("{title} (linked note)"), text: text.chars().take(1200).collect() });
            if docs.len() >= 5 {
                break; // a note with many links must not flood a tiny model
            }
        }
        Ok(docs)
    }

    /// Web search via the local proxy (text-only), as context. `None` proxy ⇒ no web.
    fn web(&self, ask: &str) -> Result<Vec<InputDoc>, String> {
        let Some(port) = self.searxng_port else { return Ok(Vec::new()) };
        let hits = SearxngSearch::local(port).search(ask).map_err(|e| e.to_string())?;
        Ok(hits
            .into_iter()
            .map(|h| InputDoc { label: format!("web: {} ({})", h.title, h.url), text: h.text })
            .collect())
    }
}

/// The note ids linked in a body — every `note:<ULID>` (from `[..](note:id)` or a bare ref). A ULID is
/// exactly 26 Crockford-base32 chars, so take the 26 after each `note:` and keep only alphanumeric
/// runs; a shorter/malformed match is skipped. Order-preserving; duplicates handled by the caller.
fn note_refs(body: &str) -> Vec<String> {
    body.match_indices("note:")
        .filter_map(|(i, _)| {
            let id: String = body[i + 5..].chars().take(26).collect();
            (id.len() == 26 && id.chars().all(|c| c.is_ascii_alphanumeric())).then_some(id)
        })
        .collect()
}

#[cfg(test)]
mod ref_tests {
    #[test]
    fn note_refs_extracts_linked_note_ids() {
        let body = "See [Bayes](note:01KY42HKAM9EZNFMGCS4A99V4C) and an embed \
                    ![x](note:01KY3ZBV913R3AA06FW7DBWQB0). A bare note:tooShort is ignored.";
        let ids = super::note_refs(body);
        assert_eq!(ids, vec![
            "01KY42HKAM9EZNFMGCS4A99V4C".to_string(),
            "01KY3ZBV913R3AA06FW7DBWQB0".to_string(),
        ]);
        assert!(super::note_refs("no links here").is_empty());
    }
}

/// `turn`/`summarize` never call web (they take pre-assembled context), so this stub satisfies the
/// generic; the runner does web search itself and folds it into the context.
struct NoWeb;
impl WebSearch for NoWeb {
    fn search(&self, _query: &str) -> Result<Vec<SearchHit>, AgentError> {
        Err(AgentError::new("unused"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};

    /// A `VaultAccess` with no HTTP and no fm-serve — proof the runner is decoupled from the desktop
    /// transport. The mobile in-process impl is just another one of these.
    #[derive(Default)]
    struct FakeVault {
        thread: Value,
        alive: bool,
    }
    impl VaultAccess for FakeVault {
        fn get(&self, _: &str) -> Result<Value, String> {
            Ok(Value::Null)
        }
        fn thread(&self, _: &str) -> Result<Value, String> {
            Ok(self.thread.clone())
        }
        fn discussions(&self) -> Result<Value, String> {
            Ok(json!([]))
        }
        fn alive(&self) -> bool {
            self.alive
        }
        fn search(&self, _: &str) -> Result<Value, String> {
            Ok(json!([]))
        }
        fn reply(&self, _: &str, _: &str) -> Result<Value, String> {
            Ok(Value::Null)
        }
        fn reply_as(&self, _: &str, _: &str, _: &str, _: &str) -> Result<Value, String> {
            Ok(Value::Null)
        }
        fn create_proposal(&self, _: &str, _: &str, _: &str, _: &str) -> Result<Value, String> {
            Ok(Value::Null)
        }
        fn activity(&self, _: &str, _: &str, _: &str) {}
        fn activity_done(&self, _: &str) {}
        fn present(&self, _: &str) {}
    }

    fn agent(fm: FakeVault) -> Agent<FakeVault> {
        Agent {
            fm,
            model_port: 0,
            model: "test".into(),
            searxng_port: None,
            max_reply_chars: 600,
            retrieve: 3,
            history_budget: 4000,
        }
    }

    #[test]
    fn the_runner_reads_the_thread_through_the_vault_seam_not_fm_serve() {
        // No fm-serve, no socket — Agent runs against a plain in-memory VaultAccess, which is exactly
        // what makes the Android in-process impl a drop-in.
        let fm = FakeVault { thread: json!({ "messages": [{ "body": "hi" }, { "body": "there" }] }), alive: true };
        let history = agent(fm).history("note").unwrap();
        assert!(history.contains("hi") && history.contains("there"), "got: {history:?}");
    }

    // --- history() budgeting: the multi-round refinement fix (pin both ends, trim the middle). ---

    #[test]
    fn history_returns_the_whole_thread_when_it_fits_the_budget() {
        // The common case: under budget, nothing is dropped and the order is oldest-first.
        let thread = json!({ "messages": [{ "body": "one" }, { "body": "two" }, { "body": "three" }] });
        let h = agent(FakeVault { thread, alive: true }).history("note").unwrap();
        assert_eq!(h, "- one\n- two\n- three");
    }

    #[test]
    fn history_pins_the_original_ask_and_the_latest_turn_when_it_overflows() {
        // The refinement fix: over budget, the FIRST turn (the standing constraints an early round set)
        // and the LATEST turn (the current state) are both kept, a dropped middle is marked, and it is
        // a *middle* turn that gives way — not the opener. Without this, a long back-and-forth loses its
        // earliest requirement and the model re-breaks it (oscillation).
        let thread = json!({ "messages": [
            { "body": "FIRST: always keep the action items" },
            { "body": "second turn — some middle padding here" },
            { "body": "third turn — more middle padding here" },
            { "body": "LATEST: and fix the title" },
        ]});
        let mut a = agent(FakeVault { thread, alive: true });
        a.history_budget = 120; // room for both ends + the gap marker, but not every middle turn
        let h = a.history("note").unwrap();
        assert!(h.contains("FIRST: always keep"), "the original ask is pinned, never dropped: {h:?}");
        assert!(h.contains("LATEST: and fix"), "the most recent turn is always kept: {h:?}");
        assert!(h.contains("omitted"), "a dropped middle is marked as a gap: {h:?}");
        assert!(!h.contains("second turn"), "a middle turn is what gives way to fit the budget: {h:?}");
        // The opener leads and the latest closes — order preserved.
        assert!(h.find("FIRST").unwrap() < h.find("LATEST").unwrap(), "chronological: {h:?}");
    }

    #[test]
    fn history_keeps_both_short_turns_without_a_marker_or_duplication() {
        // Two turns are both ends, so even under an absurdly tight budget both survive, in order, with
        // no gap marker and no duplicated opener.
        let thread = json!({ "messages": [{ "body": "the ask" }, { "body": "the answer" }] });
        let mut a = agent(FakeVault { thread, alive: true });
        a.history_budget = 5;
        let h = a.history("note").unwrap();
        assert_eq!(h, "- the ask\n- the answer");
    }

    #[test]
    fn history_keeps_a_lone_over_long_turn_rather_than_returning_nothing() {
        // One turn longer than the whole budget: there is nothing to drop, so it is kept whole — never
        // the empty string the old tail-only loop could return.
        let thread = json!({ "messages": [{ "body": "a single very long turn that exceeds the budget" }] });
        let mut a = agent(FakeVault { thread, alive: true });
        a.history_budget = 5;
        let h = a.history("note").unwrap();
        assert!(h.contains("single very long turn"), "a lone turn is kept whole, not dropped to empty: {h:?}");
    }

    #[test]
    fn serve_loop_stops_the_model_when_formicaria_is_gone() {
        // The "no orphaned agent" guarantee, checked with NO real model or network so it holds on both
        // the desktop and the phone: when the vault — fm-serve, i.e. formicaria — is gone (`alive()` is
        // false), the watch loop must trip `stop_model` and RETURN, rather than keep the model (and its
        // memory / GPU VRAM) loaded. This is the seam that failed when a restarted fm-serve was mistaken
        // for the original: the agent must go out after formicaria does.
        let agent = agent(FakeVault { alive: false, ..Default::default() });
        let stopped = std::cell::Cell::new(false);
        // `finished` stays false (the model itself is fine); only the missing vault should end the loop.
        crate::watch::serve_loop(&agent, "test", std::path::Path::new("/tmp"), 1, &|| false, &|| stopped.set(true));
        assert!(stopped.get(), "serve_loop must call stop_model when the vault (formicaria) is not alive");
    }
}
