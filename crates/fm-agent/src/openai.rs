//! An [`LlmStep`](crate::LlmStep) that talks to an **OpenAI-compatible chat endpoint** on
//! `localhost` — a local model server such as llama.cpp's `llama-server` or ollama.
//!
//! **Hand-rolled over `std::net`, no HTTP framework and no TLS.** The same minimal-dependency stance
//! as fm-serve's hand-rolled server (`deny.toml` is permissive-only; `reqwest` would drag in
//! tokio+hyper+TLS and a licence expansion), and a loopback server needs no TLS. Remote HTTPS
//! providers are deliberately out of scope here — local-first is the default; a remote seam, if it
//! ever lands, is a separate opt-in.
//!
//! It sends **one** request and reads **one** response — no streaming, no retries, bounded by a
//! timeout — because the orchestrator calls it as a single bounded step. The model is never in
//! charge; this is the narrow place where its text comes back.

use crate::{http, AgentError, LlmStep};
use std::time::Duration;

/// A single-shot client for a local OpenAI-compatible `/v1/chat/completions` endpoint.
pub struct OpenAiStep {
    host: String,
    port: u16,
    model: String,
    timeout: Duration,
    temperature: f32,
}

impl OpenAiStep {
    /// Point at a model server on `127.0.0.1:<port>` serving `model`. Conservative defaults: a
    /// low temperature (research/summary wants faithfulness, not flourish) and a generous but finite
    /// timeout so a stuck server fails rather than hangs the pipeline.
    pub fn local(port: u16, model: impl Into<String>) -> Self {
        Self {
            host: "127.0.0.1".into(),
            port,
            model: model.into(),
            timeout: Duration::from_secs(120),
            temperature: 0.2,
        }
    }

    pub fn with_host(mut self, host: impl Into<String>) -> Self {
        self.host = host.into();
        self
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    pub fn with_temperature(mut self, temperature: f32) -> Self {
        self.temperature = temperature;
        self
    }
}

impl LlmStep for OpenAiStep {
    fn complete(&self, system: &str, user: &str) -> Result<String, AgentError> {
        let body = serde_json::json!({
            "model": self.model,
            "temperature": self.temperature,
            "stream": false,
            "messages": [
                { "role": "system", "content": system },
                { "role": "user", "content": user },
            ],
        })
        .to_string();

        let request = format!(
            "POST /v1/chat/completions HTTP/1.1\r\n\
             Host: {host}:{port}\r\n\
             Content-Type: application/json\r\n\
             Content-Length: {len}\r\n\
             Connection: close\r\n\r\n{body}",
            host = self.host,
            port = self.port,
            len = body.len(),
        );
        let raw = http::send(&self.host, self.port, request.as_bytes(), self.timeout)?;
        let json = http::body(&raw)?;
        parse_completion(&json)
    }
}

/// Pull `choices[0].message.content` out of a chat-completion JSON body. Split out and pure so the
/// transport above stays small and the extraction is trivial to test.
fn parse_completion(json: &str) -> Result<String, AgentError> {
    let v: serde_json::Value = serde_json::from_str(json.trim())
        .map_err(|e| AgentError::new(format!("model server response was not JSON: {e}")))?;
    v["choices"][0]["message"]["content"]
        .as_str()
        .map(str::to_string)
        .ok_or_else(|| AgentError::new("model response had no choices[0].message.content"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read as _, Write as _};
    use std::net::TcpListener;

    #[test]
    fn it_pulls_the_message_content_out_of_a_completion_body() {
        let json = "{\"choices\":[{\"message\":{\"content\":\"the answer\"}}]}";
        assert_eq!(parse_completion(json).unwrap(), "the answer");
    }

    #[test]
    fn a_response_without_content_is_an_error() {
        assert!(parse_completion("{\"choices\":[]}").unwrap_err().to_string().contains("content"));
    }

    /// End to end over a real loopback socket: a fake server returns a canned completion, and the
    /// step sends a well-formed request and returns the content. Hermetic — no external network.
    #[test]
    fn it_talks_to_a_local_server_over_a_real_socket() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();

        let server = std::thread::spawn(move || {
            let (mut sock, _) = listener.accept().unwrap();
            // Read the request (headers + small body) — enough to assert on.
            let mut buf = [0u8; 4096];
            let n = sock.read(&mut buf).unwrap();
            let req = String::from_utf8_lossy(&buf[..n]).to_string();
            let json = "{\"choices\":[{\"message\":{\"content\":\"HELLO FROM FAKE MODEL\"}}]}";
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                json.len(),
                json
            );
            sock.write_all(resp.as_bytes()).unwrap();
            req
        });

        let step = OpenAiStep::local(port, "test-model");
        let out = step.complete("SYS", "USER").unwrap();
        assert_eq!(out, "HELLO FROM FAKE MODEL");

        let req = server.join().unwrap();
        assert!(req.starts_with("POST /v1/chat/completions HTTP/1.1"));
        assert!(req.contains("\"model\":\"test-model\""));
        assert!(req.contains("SYS") && req.contains("USER"), "messages not sent: {req}");
    }
}
