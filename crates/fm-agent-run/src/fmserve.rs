//! The **vault-access seam** the agent runs on, and its desktop implementation.
//!
//! [`VaultAccess`] is the whole surface the runner needs from a vault — read a note/thread, list
//! discussions, search, post a reply, propose an edit, and report presence/activity. [`Agent`] is
//! generic over it (`fm_agent::StudyAssistant` already did the same for the model and the web), so the
//! *same* orchestrator runs against any implementation.
//!
//! [`FmServe`] is the **desktop** impl: it POSTs `/api/<command>` to a running `fm-serve` (the exact
//! JSON surface the browser uses), so `fm-serve` stays the single vault writer — the agent never opens
//! a second `FileStore`, no double storage, safe alongside the live app. A non-browser client sends no
//! `Origin` header, which passes `fm-serve`'s CSRF guard. Hand-rolled over the shared `fm_agent::http`
//! plumbing (no framework, no TLS — localhost only).
//!
//! A mobile impl (calling `fm_app::dispatch` in-process, where there is no localhost HTTP server) is a
//! second implementor of this trait — that is the point of the seam, and the port to Android rides on
//! it with no change to [`Agent`].
//!
//! [`Agent`]: crate::Agent

use fm_agent::http;
use serde_json::{json, Value};
use std::time::Duration;

/// Everything the runner needs from a vault. One trait so the same [`Agent`](crate::Agent) runs over
/// HTTP on the desktop ([`FmServe`]) and in-process on a phone. Reads/writes return raw `Value` (the
/// same JSON the command surface speaks); presence/activity are best-effort and cannot fail a turn.
pub trait VaultAccess {
    /// A note's full detail (has `body`, `title`).
    fn get(&self, id: &str) -> Result<Value, String>;
    /// A note's discussion — `{ root, count, messages: [{ id, body, ... }] }`.
    fn thread(&self, note: &str) -> Result<Value, String>;
    /// Every first-class discussion, newest-active first — `[{ id, title, count, ... }]`.
    fn discussions(&self) -> Result<Value, String>;
    /// Is the vault reachable? A cheap liveness probe.
    fn alive(&self) -> bool;
    /// Full-text search over the vault (RAG retrieval) — `[{ id, title, preview }]`.
    fn search(&self, query: &str) -> Result<Value, String>;
    /// Post a message to a note's discussion, unattributed — used to record a *user's* message.
    fn reply(&self, note: &str, body: &str) -> Result<Value, String>;
    /// Post the agent's **own** message, attributed to its model identity `(name, email)`.
    fn reply_as(&self, note: &str, body: &str, name: &str, email: &str) -> Result<Value, String>;
    /// Create a proposal edit to `note`, attributed to the model `(name, email)`.
    fn create_proposal(&self, note: &str, body: &str, name: &str, email: &str) -> Result<Value, String>;
    /// Read a blob's raw bytes and sniffed MIME, given an asset `reference` (`asset:sha256-<hash>` /
    /// `sha256:<hash>`). This is how a modality specialist gets its input **by value** — the runner
    /// reads the bytes here and hands *only* the bytes to the specialist, never a path into the blob
    /// store, so a wedged specialist can never corrupt the vault's only copy (the transcribe invariant).
    fn blob_bytes(&self, reference: &str) -> Result<(Vec<u8>, String), String>;
    /// Report the current pipeline stage for a discussion (the live "working…" wheel). Best-effort.
    fn activity(&self, disc: &str, stage: &str, question: &str);
    /// Clear a discussion's status — the reply landed, or the turn errored/timed out. Best-effort.
    fn activity_done(&self, disc: &str);
    /// Heartbeat: "this agent is alive right now," by name. Best-effort.
    fn present(&self, name: &str);
}

/// The desktop [`VaultAccess`]: a tiny HTTP client for a running `fm-serve`.
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
}

impl VaultAccess for FmServe {
    fn get(&self, id: &str) -> Result<Value, String> {
        self.call("get", json!({ "id": id }))
    }

    fn thread(&self, note: &str) -> Result<Value, String> {
        self.call("thread", json!({ "id": note }))
    }

    fn discussions(&self) -> Result<Value, String> {
        // `thread_roots`, not `discussions`: the agent must watch **note comment threads** too, not
        // only first-class discussions — else an @-mention in a note's discussion is never seen.
        self.call("thread_roots", json!({}))
    }

    fn alive(&self) -> bool {
        self.call("alive", Value::Null).is_ok()
    }

    fn search(&self, query: &str) -> Result<Value, String> {
        self.call("search", json!({ "query": query }))
    }

    fn reply(&self, note: &str, body: &str) -> Result<Value, String> {
        self.call("reply", json!({ "id": note, "body": body }))
    }

    fn reply_as(&self, note: &str, body: &str, name: &str, email: &str) -> Result<Value, String> {
        self.call(
            "reply",
            json!({ "id": note, "body": body, "authorName": name, "authorEmail": email }),
        )
    }

    fn create_proposal(&self, note: &str, body: &str, name: &str, email: &str) -> Result<Value, String> {
        self.call(
            "create_proposal",
            json!({ "id": note, "body": body, "authorName": name, "authorEmail": email }),
        )
    }

    fn blob_bytes(&self, reference: &str) -> Result<(Vec<u8>, String), String> {
        // `GET /api/blob/<ref>` streams the bytes with the sniffed Content-Type (no Range → whole
        // file). This is binary, so we parse the response bytes directly rather than through the
        // JSON `call` path, and split head/body on the raw `\r\n\r\n` so no byte is lossily decoded.
        let request = format!(
            "GET /api/blob/{r} HTTP/1.1\r\nHost: {host}:{port}\r\nConnection: close\r\n\r\n",
            r = http::encode(reference),
            host = self.host,
            port = self.port,
        );
        let raw = http::send(&self.host, self.port, request.as_bytes(), self.timeout)
            .map_err(|e| format!("cannot fetch blob from fm-serve: {e}"))?;
        let sep = raw
            .windows(4)
            .position(|w| w == b"\r\n\r\n")
            .ok_or("malformed blob response from fm-serve")?;
        let head = String::from_utf8_lossy(&raw[..sep]);
        let status = head.lines().next().unwrap_or("");
        if !status.contains(" 200") {
            return Err(format!("fm-serve blob {reference}: {}", status.trim()));
        }
        let mime = head
            .lines()
            .find_map(|l| l.strip_prefix("Content-Type:").or_else(|| l.strip_prefix("content-type:")))
            .map(|v| v.trim().to_string())
            .unwrap_or_else(|| "application/octet-stream".to_string());
        Ok((raw[sep + 4..].to_vec(), mime))
    }

    fn activity(&self, disc: &str, stage: &str, question: &str) {
        let _ = self.call("agent_activity", json!({ "discussion": disc, "stage": stage, "question": question }));
    }

    fn activity_done(&self, disc: &str) {
        let _ = self.call("agent_activity", json!({ "discussion": disc, "done": true }));
    }

    fn present(&self, name: &str) {
        let _ = self.call("agent_present", json!({ "name": name }));
    }
}
