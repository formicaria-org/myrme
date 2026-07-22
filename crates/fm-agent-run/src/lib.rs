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
    /// History budget (chars); older turns beyond it are summarized to fit a tiny model's window.
    pub history_budget: usize,
}

impl<V: VaultAccess> Agent<V> {
    /// Handle one already-posted user message on `note`'s discussion end to end: gather context (host
    /// note + RAG + web when `/search`), summarize old history if it overflows, run the turn, and POST
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
        on_stage("reading your notes");
        let mut context = self.host_note(note)?;
        context.extend(self.retrieve(&intent.ask, note)?);
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
    /// older turns are compressed with the model (**summarize-before-overflow**) and the recent tail
    /// is kept verbatim — so the conversation never blows a tiny model's context.
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
        if full.chars().count() <= self.history_budget {
            return Ok(full);
        }
        // Keep the most recent turns (up to half the budget) verbatim; summarize the older rest.
        let recent_budget = self.history_budget / 2;
        let mut recent = String::new();
        let mut older: Vec<String> = Vec::new();
        for m in msgs.iter().rev() {
            if recent.chars().count() + m.chars().count() < recent_budget {
                recent = format!("{m}\n{recent}");
            } else {
                older.push(m.clone());
            }
        }
        older.reverse();
        let older_text = older.join("\n");
        let summary = if older_text.trim().is_empty() {
            String::new()
        } else {
            StudyAssistant::new(self.llm(), NoWeb)
                .summarize(&older_text)
                .unwrap_or_else(|_| "[earlier conversation omitted]".into())
        };
        Ok(format!("(summary of earlier conversation) {summary}\n{}", recent.trim()))
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

    /// RAG: the most relevant vault notes to the ask (FTS via fm-serve), trimmed; excludes the host.
    fn retrieve(&self, ask: &str, host: &str) -> Result<Vec<InputDoc>, String> {
        if ask.trim().is_empty() {
            return Ok(Vec::new());
        }
        let hits = self.fm.search(ask)?;
        let mut docs = Vec::new();
        let Some(arr) = hits.as_array() else { return Ok(docs) };
        for h in arr.iter().filter(|h| h["id"].as_str() != Some(host)).take(self.retrieve) {
            let Some(id) = h["id"].as_str() else { continue };
            if let Ok(note) = self.fm.get(id) {
                let body = note["body"].as_str().unwrap_or_default();
                let title = note["title"].as_str().unwrap_or(id);
                docs.push(InputDoc { label: title.to_string(), text: body.chars().take(600).collect() });
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
            true
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
        let fm = FakeVault { thread: json!({ "messages": [{ "body": "hi" }, { "body": "there" }] }) };
        let history = agent(fm).history("note").unwrap();
        assert!(history.contains("hi") && history.contains("there"), "got: {history:?}");
    }
}
