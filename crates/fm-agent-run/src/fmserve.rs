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

/// What produced a proposal — the **input** half of a supervision record.
///
/// A proposal on its own is an answer with no question: the model's output survives in git forever,
/// while what it was asked and what it was given are recorded nowhere. That makes the pair unusable
/// as an instruction example, and it bites `/research` hardest, because the sources it cites are a
/// live web that will not exist by the time anyone trains on this.
///
/// Every field is cheap here and **impossible to reconstruct later**, which is the whole argument
/// for capturing it at the moment the proposal is made.
#[derive(Default, Clone, Debug)]
pub struct Origin {
    /// The deterministic tool that produced the output — the task label. This app has no
    /// tool-calling abstraction, so every AI output already belongs to exactly one named pipeline.
    pub tool: &'static str,
    /// What was actually asked, verbatim.
    pub query: Option<String>,
    /// What the model was given: URLs for research, `asset:` references for a transcript.
    pub sources: Vec<String>,
}

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
    /// Post a message to a note's discussion, unattributed — used to record a *user's* message.
    fn reply(&self, note: &str, body: &str) -> Result<Value, String>;
    /// Post the agent's **own** message, attributed to its model identity `(name, email)`.
    fn reply_as(&self, note: &str, body: &str, name: &str, email: &str) -> Result<Value, String>;
    /// Create a proposal edit to `note`, attributed to the model `(name, email)`.
    fn create_proposal(
        &self,
        note: &str,
        body: &str,
        name: &str,
        email: &str,
        origin: &Origin,
    ) -> Result<Value, String>;
    /// Read a blob's raw bytes and sniffed MIME, given an asset `reference` (`asset:sha256-<hash>` /
    /// `sha256:<hash>`). This is how a modality specialist gets its input **by value** — the runner
    /// reads the bytes here and hands *only* the bytes to the specialist, never a path into the blob
    /// store, so a wedged specialist can never corrupt the vault's only copy (the transcribe invariant).
    fn blob_bytes(&self, reference: &str) -> Result<(Vec<u8>, String), String>;
    /// Report the current pipeline stage for a discussion (the live "working…" wheel). Best-effort.
    fn activity(&self, disc: &str, stage: &str, question: &str);
    /// Clear a discussion's status — the reply landed, or the turn errored/timed out. Best-effort.
    fn activity_done(&self, disc: &str);
    /// Heartbeat: "this agent is alive right now," by name — and, when something here can read a
    /// picture, which model server that is, so the app can hand it a label (`crate::label`).
    /// Best-effort.
    fn present(&self, name: &str, reader: Option<&crate::ImageReader>);

    // ── the meeting pass (`decisions.md` 2026-10-09, *the assistant proposes meetings*) ──────────
    // Defaulted, so a vault without mail (the phone, the test fakes) needs nothing and the pass simply
    // finds nothing to do.

    /// Every email conversation and the meeting starts already on the agenda (`mail_conversations`).
    fn mail_conversations(&self) -> Result<Value, String> {
        Err("there is no mail on this device".into())
    }
    /// A number that moves whenever a mail read wrote something new — the cue to run the meeting
    /// pass at once. `None` where there is no mail reader to ask.
    fn mail_novelty(&self) -> Option<u64> {
        None
    }
    /// What the meeting pass concluded about one conversation, for the person to see. Best-effort.
    fn mail_heard(&self, _note: &str, _title: &str, _latest: &str, _said: &str) {}
    /// The person's own clock — formicaria's *your zone* setting (`Asia/Singapore`, `+08:00`) — onto
    /// which a meeting written on another clock is moved. `None` where it cannot be asked; the
    /// computer's own zone is used then.
    fn zone(&self) -> Option<String> {
        None
    }
    /// A proposal that also sets the allowlisted properties (`start`/`due`/`location`, added tags).
    fn propose_with(
        &self,
        _note: &str,
        _body: &str,
        _props: &Value,
        _name: &str,
        _email: &str,
        _origin: &Origin,
    ) -> Result<Value, String> {
        Err("proposals with dates are not available on this device".into())
    }
    /// The text of `note`'s proposal still waiting for an answer, if it has one — so a further change
    /// (a title) revises that proposal without dropping what it already proposes.
    fn open_proposal_body(&self, _note: &str) -> Option<String> {
        None
    }
    /// Propose a **new** note linked to `about` (`propose_note`): it exists only on its proposal branch
    /// until a person accepts it.
    fn propose_note(
        &self,
        _about: &str,
        _note: &Value,
        _name: &str,
        _email: &str,
        _origin: &Origin,
    ) -> Result<Value, String> {
        Err("proposing new notes is not available on this device".into())
    }
}

/// The header every request from this process carries: **"I am the assistant, not a person."**
///
/// `fm-serve` exits when nobody has been in touch for 90 s, so that closing the browser tab closes
/// the app. It learned "somebody is here" from any authenticated request — and this process polls
/// the vault every 1–5 s for as long as it runs, so while the assistant was on that window could
/// never close. The app kept itself alive by asking whether it was alive, and a multi-GB
/// `llama-server` stayed resident with it until the machine was rebooted.
///
/// The agent is a child of that server: it watches this port and stops itself when we stop
/// answering. So it must never be the reason the server lives. Sent on **every** request, not just
/// the liveness probe, because every request refreshed that timer.
pub const AGENT_HEADER: &str = "X-Formicaria-Agent: 1\r\n";

/// The desktop [`VaultAccess`]: a tiny HTTP client for a running `fm-serve`.
pub struct FmServe {
    host: String,
    port: u16,
    timeout: Duration,
    /// The formicaria this agent belongs to — its `/api/alive` `instance` (2026-10-18). `None`
    /// follows whatever answers, as before.
    instance: Option<String>,
}

/// **Is the formicaria answering the one this agent belongs to?** Only a *different* instance is
/// another app: no instance expected (a dev run), or none reported (an older build), is ours. An
/// assistant left behind by an app that has gone used to live on with the next one, answering in
/// the old build's name and holding the model's port (2026-10-18).
pub fn same_app(expected: Option<&str>, answering: Option<&str>) -> bool {
    match (expected, answering) {
        (Some(e), Some(a)) => e == a,
        _ => true,
    }
}

impl FmServe {
    pub fn local(port: u16) -> Self {
        Self { host: "127.0.0.1".into(), port, timeout: Duration::from_secs(60), instance: None }
    }

    /// Belong to one formicaria: from now on, another one answering on the port is not ours.
    pub fn with_instance(mut self, instance: Option<String>) -> Self {
        self.instance = instance;
        self
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// The instance of the formicaria answering now — `Ok(None)` from a build that does not say.
    pub fn answering_instance(&self) -> Result<Option<String>, String> {
        Ok(self.call("alive", Value::Null)?["instance"].as_str().map(str::to_string))
    }

    /// POST one command and return its JSON answer. A non-200 carries `fm-serve`'s error body.
    fn call(&self, cmd: &str, args: Value) -> Result<Value, String> {
        let body = args.to_string();
        let raw = http::post(
            &self.host,
            self.port,
            &format!("/api/{cmd}"),
            "application/json",
            AGENT_HEADER,
            body.as_bytes(),
            self.timeout,
        )
        .map_err(|e| {
            format!("cannot reach fm-serve at {}:{} — is it running? ({e})", self.host, self.port)
        })?;
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
        self.answering_instance()
            .is_ok_and(|here| same_app(self.instance.as_deref(), here.as_deref()))
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

    fn create_proposal(
        &self,
        note: &str,
        body: &str,
        name: &str,
        email: &str,
        origin: &Origin,
    ) -> Result<Value, String> {
        self.call(
            "create_proposal",
            json!({
                "id": note,
                "body": body,
                "authorName": name,
                "authorEmail": email,
                "tool": origin.tool,
                "query": origin.query,
                "sources": origin.sources,
            }),
        )
    }

    fn mail_conversations(&self) -> Result<Value, String> {
        self.call("mail_conversations", json!({}))
    }

    fn mail_novelty(&self) -> Option<u64> {
        self.call("mail_novelty", json!({})).ok()?["novelty"].as_u64()
    }

    /// `/api/calendars` answers the setting: `zone` since formicaria 0.15 (the setting, or the
    /// computer's when unset), `offset` before it.
    fn zone(&self) -> Option<String> {
        let v = self.call("calendars", json!({})).ok()?;
        let pick =
            |k: &str| v[k].as_str().map(str::trim).filter(|s| !s.is_empty()).map(String::from);
        pick("zone").or_else(|| pick("offset"))
    }

    fn mail_heard(&self, note: &str, title: &str, latest: &str, said: &str) {
        let _ = self.call(
            "mail_heard",
            json!({ "note": note, "title": title, "latest": latest, "said": said }),
        );
    }

    fn open_proposal_body(&self, note: &str) -> Option<String> {
        let pr = self.call("proposal_for", json!({ "id": note })).ok()?;
        let pr = pr.as_str()?;
        let c = self.call("proposal_content", json!({ "id": pr })).ok()?;
        c["body"].as_str().map(str::to_string)
    }

    fn propose_with(
        &self,
        note: &str,
        body: &str,
        props: &Value,
        name: &str,
        email: &str,
        origin: &Origin,
    ) -> Result<Value, String> {
        self.call(
            "create_proposal",
            json!({
                "id": note,
                "body": body,
                "props": props,
                "authorName": name,
                "authorEmail": email,
                "tool": origin.tool,
                "query": origin.query,
                "sources": origin.sources,
            }),
        )
    }

    fn propose_note(
        &self,
        about: &str,
        note: &Value,
        name: &str,
        email: &str,
        origin: &Origin,
    ) -> Result<Value, String> {
        let mut args = note.clone();
        args["about"] = json!(about);
        args["authorName"] = json!(name);
        args["authorEmail"] = json!(email);
        args["tool"] = json!(origin.tool);
        args["query"] = json!(origin.query);
        args["sources"] = json!(origin.sources);
        self.call("propose_note", args)
    }

    fn blob_bytes(&self, reference: &str) -> Result<(Vec<u8>, String), String> {
        // `GET /api/blob/<ref>` streams the bytes with the sniffed Content-Type (no Range → whole
        // file). This is binary, so we parse the response bytes directly rather than through the
        // JSON `call` path, and split head/body on the raw `\r\n\r\n` so no byte is lossily decoded.
        let request = format!(
            "GET /api/blob/{r} HTTP/1.1\r\nHost: {host}:{port}\r\n{AGENT_HEADER}Connection: close\r\n\r\n",
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
            .find_map(|l| {
                l.strip_prefix("Content-Type:").or_else(|| l.strip_prefix("content-type:"))
            })
            .map(|v| v.trim().to_string())
            .unwrap_or_else(|| "application/octet-stream".to_string());
        Ok((raw[sep + 4..].to_vec(), mime))
    }

    fn activity(&self, disc: &str, stage: &str, question: &str) {
        let _ = self.call(
            "agent_activity",
            json!({ "discussion": disc, "stage": stage, "question": question }),
        );
    }

    fn activity_done(&self, disc: &str) {
        let _ = self.call("agent_activity", json!({ "discussion": disc, "done": true }));
    }

    fn present(&self, name: &str, reader: Option<&crate::ImageReader>) {
        let reader =
            reader.map(|r| json!({ "port": r.port, "model": r.model, "labels": r.labels }));
        let _ = self.call("agent_present", json!({ "name": name, "reader": reader }));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_different_formicaria_is_not_ours() {
        assert!(same_app(Some("a1"), Some("a1")));
        assert!(!same_app(Some("a1"), Some("b2")), "another app on the port");
        assert!(same_app(None, Some("b2")), "a dev run follows whatever answers");
        assert!(same_app(Some("a1"), None), "an older build says nothing — still ours");
    }
}
