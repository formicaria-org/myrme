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

use crate::{AgentError, LlmStep};
use std::io::{Read, Write};
use std::net::TcpStream;
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

        let mut stream = TcpStream::connect((self.host.as_str(), self.port)).map_err(|e| {
            AgentError::new(format!(
                "cannot reach the model server at {}:{} — is it running? ({e})",
                self.host, self.port
            ))
        })?;
        let net = |e: std::io::Error| AgentError::new(format!("model server I/O failed: {e}"));
        stream.set_read_timeout(Some(self.timeout)).map_err(net)?;
        stream.set_write_timeout(Some(self.timeout)).map_err(net)?;

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
        stream.write_all(request.as_bytes()).map_err(net)?;

        // `Connection: close` ⇒ the server closes when done, so read to EOF.
        let mut raw = Vec::new();
        stream.read_to_end(&mut raw).map_err(net)?;

        let content = parse_completion(&raw)?;
        Ok(content)
    }
}

/// Pull `choices[0].message.content` out of a raw HTTP response. Split out and pure so it can be
/// tested on captured bytes, and so the transport above stays small.
fn parse_completion(raw: &[u8]) -> Result<String, AgentError> {
    let text = String::from_utf8_lossy(raw);
    let (head, body) = text
        .split_once("\r\n\r\n")
        .ok_or_else(|| AgentError::new("malformed HTTP response from the model server"))?;

    let status = head.lines().next().unwrap_or("");
    if !status.contains(" 200") {
        return Err(AgentError::new(format!("model server returned: {}", status.trim())));
    }

    // Content-Length + `Connection: close` is what llama-server/ollama send for a non-streaming
    // reply, so `body` is the JSON directly. Handle `Transfer-Encoding: chunked` too, in case a
    // server ignores `Connection: close`.
    let json = if head.to_ascii_lowercase().contains("transfer-encoding: chunked") {
        dechunk(body)?
    } else {
        body.to_string()
    };

    let v: serde_json::Value = serde_json::from_str(json.trim())
        .map_err(|e| AgentError::new(format!("model server response was not JSON: {e}")))?;
    v["choices"][0]["message"]["content"]
        .as_str()
        .map(str::to_string)
        .ok_or_else(|| AgentError::new("model response had no choices[0].message.content"))
}

/// Minimal HTTP/1.1 chunked-body decoder: `<hex-size>\r\n<data>\r\n…0\r\n\r\n`.
fn dechunk(body: &str) -> Result<String, AgentError> {
    let mut out = String::new();
    let mut rest = body;
    loop {
        let (size_line, after) = rest
            .split_once("\r\n")
            .ok_or_else(|| AgentError::new("truncated chunked response"))?;
        let size = usize::from_str_radix(size_line.trim(), 16)
            .map_err(|_| AgentError::new("bad chunk size in response"))?;
        if size == 0 {
            break;
        }
        if after.len() < size {
            return Err(AgentError::new("truncated chunk in response"));
        }
        out.push_str(&after[..size]);
        // Skip the chunk data and its trailing CRLF.
        rest = after[size..].strip_prefix("\r\n").unwrap_or(&after[size..]);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read as _;
    use std::net::TcpListener;

    /// Parsing works on a plain Content-Length response.
    #[test]
    fn it_pulls_the_message_content_out_of_a_completion() {
        let raw = b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 60\r\n\r\n\
                    {\"choices\":[{\"message\":{\"content\":\"the answer\"}}]}";
        assert_eq!(parse_completion(raw).unwrap(), "the answer");
    }

    #[test]
    fn a_non_200_status_is_an_error_not_a_parse() {
        let raw = b"HTTP/1.1 500 Internal Server Error\r\n\r\noops";
        let err = parse_completion(raw).unwrap_err();
        assert!(format!("{err}").contains("500"), "got: {err}");
    }

    #[test]
    fn it_decodes_a_chunked_body() {
        let raw = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n\
                    2c\r\n{\"choices\":[{\"message\":{\"content\":\"hi\"}}]}\r\n0\r\n\r\n";
        assert_eq!(parse_completion(raw).unwrap(), "hi");
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
