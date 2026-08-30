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

use crate::{http, AgentError, LlmResponse, LlmStep, Usage};
use std::time::Duration;

/// A single-shot client for a local OpenAI-compatible `/v1/chat/completions` endpoint.
pub struct OpenAiStep {
    host: String,
    port: u16,
    model: String,
    timeout: Duration,
    temperature: f32,
    seed: i64,
    max_tokens: u32,
}

impl OpenAiStep {
    /// Point at a model server on `127.0.0.1:<port>` serving `model`. Defaults tuned for a
    /// **deterministic** study assistant (research 2026-07-21): a **fixed `seed`** (llama.cpp's own
    /// default is `-1` = *random*, so a fixed seed is what actually makes a call reproducible) and a
    /// low temperature; a **`max_tokens` cap** as a sharper "never berserk" bound than the timeout
    /// alone; and a finite timeout so a stuck server fails rather than hangs the pipeline.
    pub fn local(port: u16, model: impl Into<String>) -> Self {
        Self {
            host: "127.0.0.1".into(),
            port,
            model: model.into(),
            timeout: Duration::from_secs(120),
            temperature: 0.2,
            seed: 0,
            max_tokens: 2048,
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

    /// A fixed seed reproduces the same output for the same input; `-1` asks the server for a random
    /// one (non-reproducible).
    pub fn with_seed(mut self, seed: i64) -> Self {
        self.seed = seed;
        self
    }

    pub fn with_max_tokens(mut self, max_tokens: u32) -> Self {
        self.max_tokens = max_tokens;
        self
    }
}

impl LlmStep for OpenAiStep {
    fn complete(&self, system: &str, user: &str) -> Result<LlmResponse, AgentError> {
        let body = serde_json::json!({
            "model": self.model,
            "temperature": self.temperature,
            "seed": self.seed,
            "max_tokens": self.max_tokens,
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

/// The largest image this will send. A refusal with a reason, never a silent downscale or a
/// truncation — the same stance as the ingest ceiling, and for the same reason: a picture that
/// quietly became unreadable is worse than one that was declined out loud.
///
/// Base64 inflates by 4/3, so this is roughly an 11 MB request body. Beyond that the failure is not
/// the transport but the model's context window, and the error should say something a person can act
/// on rather than surfacing a stalled request.
pub const MAX_IMAGE_BYTES: usize = 8 * 1024 * 1024;

impl crate::imagetext::ReadImage for OpenAiStep {
    /// A transcription turn: the instruction plus the image as a `data:` URL, in the OpenAI-compatible
    /// multipart-content shape that `llama-server` serves **when an mmproj (the multimodal
    /// projector) is loaded beside the weights**.
    ///
    /// Without that projector the same server answers text-only and will either ignore the image
    /// part or reject the request — so a deployment that forgets it does not silently describe
    /// nothing, it fails. That is the intended behaviour; the alternative is a note full of
    /// confident readings of an image the model never saw.
    fn read_image(&self, image: &[u8], mime: &str) -> Result<String, AgentError> {
        if image.len() > MAX_IMAGE_BYTES {
            return Err(AgentError::new(format!(
                "image is {} MB, over the {} MB limit for a single reading — crop or shrink it",
                image.len() / 1_048_576,
                MAX_IMAGE_BYTES / 1_048_576,
            )));
        }
        // A sniffed non-image would produce a `data:` URL the server cannot decode, and the answer
        // would be a fluent description of nothing.
        if !mime.starts_with("image/") {
            return Err(AgentError::new(format!("{mime} is not an image")));
        }
        let url = format!("data:{mime};base64,{}", crate::imagetext::base64(image));
        let body = serde_json::json!({
            "model": self.model,
            "temperature": self.temperature,
            "seed": self.seed,
            "max_tokens": self.max_tokens,
            "stream": false,
            "messages": [{
                "role": "user",
                "content": [
                    { "type": "text", "text": crate::imagetext::INSTRUCTION },
                    { "type": "image_url", "image_url": { "url": url } },
                ],
            }],
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
        let reply = parse_completion(&http::body(&raw)?)?;
        // Truncation is a hard failure here, as it is on every other path in this crate. A reading
        // cut off at the token cap is a *partial transcription presented as a whole one* — and it
        // would land in a note, fenced and attributed, looking exactly as authoritative as a
        // complete one.
        if reply.finish_reason.as_deref() == Some("length") {
            return Err(AgentError::new(
                "the reading was cut off at the token limit — crop the image or raise max_tokens",
            ));
        }
        Ok(reply.content)
    }
}

impl crate::preference::Embed for OpenAiStep {
    /// `POST /v1/embeddings`, the OpenAI-compatible shape `llama-server` also serves.
    ///
    /// Deliberately the *same* client as the chat step rather than a second one: the host, the port
    /// and the timeout are already settled here, and a separate embedder would be a second place to
    /// get them wrong. None of the sampling knobs apply — an embedding has no temperature, no seed
    /// and no token cap — so the body carries only the model and the input.
    fn embed(&self, text: &str) -> Result<Vec<f32>, AgentError> {
        let body = serde_json::json!({ "model": self.model, "input": text }).to_string();
        let request = format!(
            "POST /v1/embeddings HTTP/1.1\r\n\
             Host: {host}:{port}\r\n\
             Content-Type: application/json\r\n\
             Content-Length: {len}\r\n\
             Connection: close\r\n\r\n{body}",
            host = self.host,
            port = self.port,
            len = body.len(),
        );
        let raw = http::send(&self.host, self.port, request.as_bytes(), self.timeout)?;
        parse_embedding(&http::body(&raw)?)
    }
}

/// Pull the vector out of an embeddings response. Split out and pure for the same reason
/// [`parse_completion`] is: the transport stays small and the extraction is trivial to test.
///
/// An **empty** vector is an error rather than an empty answer. A zero-length embedding scores 0.0
/// against everything (see [`crate::preference::cosine`]), so it would silently rank last forever
/// instead of failing — a shape of bug this project has met before, where a wrong answer arrives
/// looking exactly like a quiet one.
fn parse_embedding(json: &str) -> Result<Vec<f32>, AgentError> {
    let v: serde_json::Value = serde_json::from_str(json.trim())
        .map_err(|e| AgentError::new(format!("model server response was not JSON: {e}")))?;
    let arr = v["data"][0]["embedding"]
        .as_array()
        .ok_or_else(|| AgentError::new("embeddings response had no data[0].embedding"))?;
    let out: Vec<f32> = arr.iter().filter_map(|n| n.as_f64().map(|f| f as f32)).collect();
    if out.is_empty() || out.len() != arr.len() {
        return Err(AgentError::new("embeddings response held a non-numeric or empty vector"));
    }
    Ok(out)
}

/// Pull the content, `finish_reason`, and `usage` out of a chat-completion JSON body. Split out and
/// pure so the transport above stays small and the extraction is trivial to test. The finish reason
/// matters: `"length"` tells the orchestrator the answer was truncated at the token cap.
fn parse_completion(json: &str) -> Result<LlmResponse, AgentError> {
    let v: serde_json::Value = serde_json::from_str(json.trim())
        .map_err(|e| AgentError::new(format!("model server response was not JSON: {e}")))?;
    let choice = &v["choices"][0];
    let content = choice["message"]["content"]
        .as_str()
        .ok_or_else(|| AgentError::new("model response had no choices[0].message.content"))?
        .to_string();
    let finish_reason = choice["finish_reason"].as_str().map(str::to_string);
    let usage = v["usage"].as_object().map(|u| Usage {
        prompt_tokens: u.get("prompt_tokens").and_then(serde_json::Value::as_u64).unwrap_or(0) as u32,
        completion_tokens: u
            .get("completion_tokens")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0) as u32,
    });
    Ok(LlmResponse { content, finish_reason, usage })
}

#[cfg(test)]
mod tests {
    #[test]
    fn an_embedding_is_parsed_and_a_degenerate_one_is_an_error_not_an_empty_answer() {
        let ok = super::parse_embedding(r#"{"data":[{"embedding":[0.5,-0.25]}]}"#).unwrap();
        assert_eq!(ok, vec![0.5, -0.25]);
        // Empty, non-numeric, and missing all fail loudly. A zero-length vector would otherwise
        // score 0.0 against everything and rank last forever instead of failing.
        assert!(super::parse_embedding(r#"{"data":[{"embedding":[]}]}"#).is_err());
        assert!(super::parse_embedding(r#"{"data":[{"embedding":["x"]}]}"#).is_err());
        assert!(super::parse_embedding(r#"{"data":[]}"#).is_err());
        assert!(super::parse_embedding("not json").is_err());
    }

    use super::*;
    use std::io::{Read as _, Write as _};
    use std::net::TcpListener;

    #[test]
    fn it_pulls_content_finish_reason_and_usage_out_of_a_completion_body() {
        let json = "{\"choices\":[{\"finish_reason\":\"stop\",\"message\":{\"content\":\"the answer\"}}],\
                    \"usage\":{\"prompt_tokens\":11,\"completion_tokens\":3}}";
        let r = parse_completion(json).unwrap();
        assert_eq!(r.content, "the answer");
        assert_eq!(r.finish_reason.as_deref(), Some("stop"));
        assert_eq!(r.usage, Some(Usage { prompt_tokens: 11, completion_tokens: 3 }));
        assert!(!r.truncated());
    }

    #[test]
    fn a_length_finish_reason_reads_as_truncated() {
        let json = "{\"choices\":[{\"finish_reason\":\"length\",\"message\":{\"content\":\"cut of\"}}]}";
        assert!(parse_completion(json).unwrap().truncated());
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
        assert_eq!(out.content, "HELLO FROM FAKE MODEL");

        let req = server.join().unwrap();
        assert!(req.starts_with("POST /v1/chat/completions HTTP/1.1"));
        assert!(req.contains("\"model\":\"test-model\""));
        assert!(req.contains("SYS") && req.contains("USER"), "messages not sent: {req}");
        // The determinism + bound knobs are actually sent.
        assert!(req.contains("\"seed\":0"), "seed not sent: {req}");
        assert!(req.contains("\"max_tokens\":2048"), "max_tokens not sent: {req}");
    }
}
