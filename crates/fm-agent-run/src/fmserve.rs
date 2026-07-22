//! A tiny client for a **running `fm-serve`** — the agent's one and only store. It POSTs
//! `/api/<command>` (the exact JSON surface the browser uses), so **`fm-serve` stays the single vault
//! writer** and the agent never opens a second `FileStore` — no double storage, safe alongside the
//! live app. A non-browser client sends no `Origin` header, which passes `fm-serve`'s CSRF guard.
//!
//! Hand-rolled over the shared `fm_agent::http` plumbing (no framework, no TLS — localhost only).

use fm_agent::http;
use serde_json::{json, Value};
use std::time::Duration;

pub struct FmServe {
    host: String,
    port: u16,
    timeout: Duration,
}

impl FmServe {
    pub fn local(port: u16) -> Self {
        Self { host: "127.0.0.1".into(), port, timeout: Duration::from_secs(60) }
    }

    /// POST one command and return its JSON answer. A non-200 carries `fm-serve`'s error body.
    fn call(&self, cmd: &str, args: Value) -> Result<Value, String> {
        let body = args.to_string();
        let request = format!(
            "POST /api/{cmd} HTTP/1.1\r\nHost: {host}:{port}\r\n\
             Content-Type: application/json\r\nContent-Length: {len}\r\nConnection: close\r\n\r\n{body}",
            host = self.host,
            port = self.port,
            len = body.len(),
        );
        let raw = http::send(&self.host, self.port, request.as_bytes(), self.timeout)
            .map_err(|e| format!("cannot reach fm-serve at {}:{} — is it running? ({e})", self.host, self.port))?;
        let text = String::from_utf8_lossy(&raw);
        let (head, resp) = text.split_once("\r\n\r\n").ok_or("malformed fm-serve response")?;
        let status = head.lines().next().unwrap_or("");
        if !status.contains(" 200") {
            return Err(format!("fm-serve {cmd}: {} — {}", status.trim(), resp.trim()));
        }
        if resp.trim().is_empty() {
            return Ok(Value::Null);
        }
        serde_json::from_str(resp.trim())
            .map_err(|e| format!("fm-serve {cmd} response was not JSON: {e}"))
    }

    /// A note's full detail (has `body`, `title`).
    pub fn get(&self, id: &str) -> Result<Value, String> {
        self.call("get", json!({ "id": id }))
    }

    /// A note's discussion — `{ root, count, messages: [{ id, body, ... }] }`.
    pub fn thread(&self, note: &str) -> Result<Value, String> {
        self.call("thread", json!({ "id": note }))
    }

    /// Every first-class discussion, newest-active first — `[{ id, title, ... }]`.
    pub fn discussions(&self) -> Result<Value, String> {
        self.call("discussions", json!({}))
    }

    /// Is fm-serve answering? A cheap liveness probe (fm-serve shuts down with the app).
    pub fn alive(&self) -> bool {
        self.call("alive", Value::Null).is_ok()
    }

    /// Full-text search over the vault (RAG retrieval) — `[{ id, title, preview }]`.
    pub fn search(&self, query: &str) -> Result<Value, String> {
        self.call("search", json!({ "query": query }))
    }

    /// Post a message to a note's discussion.
    pub fn reply(&self, note: &str, body: &str) -> Result<Value, String> {
        self.call("reply", json!({ "id": note, "body": body }))
    }

    /// Create a proposal edit to `note`, attributed to the model `(name, email)`.
    pub fn create_proposal(&self, note: &str, body: &str, name: &str, email: &str) -> Result<Value, String> {
        self.call(
            "create_proposal",
            json!({ "id": note, "body": body, "authorName": name, "authorEmail": email }),
        )
    }

    /// Report the current pipeline stage for a discussion, so the UI can show a live "working…" wheel
    /// with what the agent is doing (`reading your notes`, `searching the web`, `thinking`).
    /// Best-effort: a status update must never break or slow a real turn, so failures are ignored.
    pub fn activity(&self, disc: &str, stage: &str, question: &str) {
        let _ = self.call("agent_activity", json!({ "discussion": disc, "stage": stage, "question": question }));
    }

    /// Clear a discussion's status — the reply landed, or the turn errored/timed out. Hides the wheel.
    pub fn activity_done(&self, disc: &str) {
        let _ = self.call("agent_activity", json!({ "discussion": disc, "done": true }));
    }

    /// Heartbeat: "this agent, by this name, is alive right now." fm-serve times these out, so the app
    /// can list online agents (the @-picker) and warn instead of going silent when one is off.
    /// Best-effort — a missed heartbeat only briefly shows the agent as offline.
    pub fn present(&self, name: &str) {
        let _ = self.call("agent_present", json!({ "name": name }));
    }
}
