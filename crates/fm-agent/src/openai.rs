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
    image_instruction: Option<String>,
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
            image_instruction: None,
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

    /// The instruction sent with an image, when this model needs its own; `None` keeps the house
    /// one. A reader is only as good as the words it is given: the house instruction halves a small
    /// model's accuracy that a one-line instruction leaves intact (`agents/bench/results.md`,
    /// 2026-10-10), so the catalogue can name one per model.
    pub fn with_image_instruction(mut self, instruction: Option<String>) -> Self {
        self.image_instruction = instruction;
        self
    }
}

impl OpenAiStep {
    /// One chat-completion call with these `messages` — the single request both the text step and
    /// the image step make, so the sampling settings and the transport exist once.
    fn chat(&self, messages: serde_json::Value) -> Result<LlmResponse, AgentError> {
        let body = serde_json::json!({
            "model": self.model,
            "temperature": self.temperature,
            "seed": self.seed,
            "max_tokens": self.max_tokens,
            "stream": false,
            "messages": messages,
        })
        .to_string();
        let raw = http::post(
            &self.host,
            self.port,
            "/v1/chat/completions",
            "application/json",
            "",
            body.as_bytes(),
            self.timeout,
        )?;
        parse_completion(&http::body(&raw)?)
    }
}

impl LlmStep for OpenAiStep {
    fn complete(&self, system: &str, user: &str) -> Result<LlmResponse, AgentError> {
        self.chat(serde_json::json!([
            { "role": "system", "content": system },
            { "role": "user", "content": user },
        ]))
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
        let instruction =
            self.image_instruction.as_deref().unwrap_or(crate::prompts::IMAGE_INSTRUCTION);
        let reply = self.chat(serde_json::json!([{
            "role": "user",
            "content": [
                { "type": "text", "text": instruction },
                { "type": "image_url", "image_url": { "url": url } },
            ],
        }]))?;
        // Truncation is a hard failure here, as it is on every other path in this crate. A reading
        // cut off at the token cap is a *partial transcription presented as a whole one* — and it
        // would land in a note, fenced and attributed, looking exactly as authoritative as a
        // complete one.
        if reply.truncated() {
            return Err(AgentError::new(
                "the reading was cut off at the token limit — crop the image or raise max_tokens",
            ));
        }
        Ok(reply.content)
    }
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
        prompt_tokens: u.get("prompt_tokens").and_then(serde_json::Value::as_u64).unwrap_or(0)
            as u32,
        completion_tokens: u
            .get("completion_tokens")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0) as u32,
    });
    Ok(LlmResponse { content, finish_reason, usage })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read as _, Write as _};
    use std::net::TcpListener;

    #[test]
    fn it_pulls_content_finish_reason_and_usage_out_of_a_completion_body() {
        let json =
            "{\"choices\":[{\"finish_reason\":\"stop\",\"message\":{\"content\":\"the answer\"}}],\
                    \"usage\":{\"prompt_tokens\":11,\"completion_tokens\":3}}";
        let r = parse_completion(json).unwrap();
        assert_eq!(r.content, "the answer");
        assert_eq!(r.finish_reason.as_deref(), Some("stop"));
        assert_eq!(r.usage, Some(Usage { prompt_tokens: 11, completion_tokens: 3 }));
        assert!(!r.truncated());
    }

    #[test]
    fn a_length_finish_reason_reads_as_truncated() {
        let json =
            "{\"choices\":[{\"finish_reason\":\"length\",\"message\":{\"content\":\"cut of\"}}]}";
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
