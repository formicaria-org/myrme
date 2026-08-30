//! The **audio→transcript specialist seam** and its provenance-marked, *insertion-only* output.
//!
//! This is the first modality specialist, and it establishes the shared **safety substrate** every
//! future specialist (image→LaTeX, …) rides on. Four properties, each load-bearing:
//!
//! 1. **By-value bytes, never a writable path.** [`Transcribe::transcribe`] receives the audio as
//!    `&[u8]` and *nothing else* — no path into the content-addressed blob store, no store handle.
//!    That is the invariant that matters most: a wedged or malicious transcriber **cannot truncate or
//!    corrupt the device's only copy** of a multiply-referenced blob, because it is never handed a way
//!    to. The blob layer hands out paths (`fm_app::commands::blob_path`); the *orchestrator* reads the
//!    bytes read-only and passes them here. The specialist's entire capability is "bytes in, text out".
//! 2. **Insertion-only.** A specialist *inserts* extracted text as an adjunct; it never proposes,
//!    suggests, or performs deleting the source. Blob pruning stays a separate, human-initiated,
//!    refcounted GC (`uncopy_note`) — never an agent action. [`insert_or_supersede`] only ever adds.
//! 3. **Provenance-marked adjunct that references the source.** Every insertion carries
//!    `{specialist, model version, source blob hash}` and *links* the source audio
//!    (`asset:sha256-<hash>`) rather than replacing it — so a wrong/hallucinated transcript can't pass
//!    as authoritative, and the reference keeps the blob alive under the on-demand refcount scan.
//! 4. **Idempotency keyed on (blob-hash, specialist, model-version).** Re-running the *same* specialist
//!    and model on the *same* blob **supersedes in place** (no silent duplicate, no racing branch); a
//!    *different* model version lands a *second* adjunct rather than clobbering the first.
//!
//! Like [`LlmStep`](crate::LlmStep) / [`WebSearch`](crate::WebSearch), the seam is a narrow trait, so
//! the whole flow is unit-tested with a fake — **no model, no network, no runtime**. The real leaf (a
//! loopback `whisper.cpp` server) plugs in later without changing anything here.

use crate::{http, AgentError};
use std::time::Duration;

/// A specialist that turns audio **bytes** into a plain-text transcript.
///
/// It takes the audio *by value* and returns text — that is the whole contract, and the whole reason
/// a broken specialist can't touch a blob (see the module docs). Real implementations POST the bytes
/// to a local `whisper.cpp` server over loopback; tests use a fake.
pub trait Transcribe {
    /// `audio` is the raw blob bytes; `mime` is the sniffed type (e.g. `audio/mpeg`) for the backend
    /// to pick a decoder. Returns the transcript text, or a bounded error the orchestrator stops on.
    fn transcribe(&self, audio: &[u8], mime: &str) -> Result<String, AgentError>;
}

/// Re-exported from [`crate::adjunct`], where the shared specialist substrate now lives — the
/// audio path was the first specialist and the image path is the second, so the provenance shape and
/// the insertion-only splice belong to neither of them alone.
pub use crate::adjunct::{hash_of, Provenance};

/// This specialist's fence tag. Every specialist owns one, so two never supersede each other.
const TAG: &str = "fm:transcript";

/// The transcript adjunct: a rendered callout that *references* the source audio and carries
/// provenance, fenced so a same-key re-run supersedes it in place. See [`crate::adjunct`] for the
/// four properties this inherits.
pub fn transcript_block(transcript: &str, prov: &Provenance) -> String {
    crate::adjunct::block(
        TAG,
        prov,
        &format!("Transcript — {} · {}", prov.specialist, prov.model),
        "audio",
        transcript,
        "(no speech detected)",
    )
}

/// Splice a transcript block into `host_body` — supersede this specialist's own prior block for the
/// same key, or append. Insertion-only; see [`crate::adjunct::insert_or_supersede`].
pub fn insert_or_supersede(host_body: &str, block: &str, key: &str) -> String {
    crate::adjunct::insert_or_supersede(host_body, block, TAG, key)
}

/// The whole audio→transcript flow, as a **pure orchestration over the seam**: transcribe the bytes,
/// build the provenance-marked adjunct, splice it into the host note body. Returns the *new body* for
/// the caller to turn into a proposal via `create_proposal` — this function creates nothing, deletes
/// nothing, and touches no blob. The `audio` it forwards is the only thing the specialist ever sees.
pub fn transcribe_into<T: Transcribe>(
    transcriber: &T,
    host_body: &str,
    audio: &[u8],
    mime: &str,
    prov: &Provenance,
) -> Result<String, AgentError> {
    let text = transcriber.transcribe(audio, mime)?;
    let block = transcript_block(&text, prov);
    Ok(insert_or_supersede(host_body, &block, &prov.key()))
}

/// The real leaf: a [`Transcribe`] backed by a local **`whisper.cpp` server** (`whisper-server`) on
/// loopback. It POSTs the audio bytes to `/inference` as `multipart/form-data` and reads back the
/// transcript — TLS-free, localhost-only, the same hand-rolled HTTP the other seams use.
///
/// It receives the audio **by value** (per the [`Transcribe`] contract) and forwards it over the
/// socket; the server never sees a vault path, so the by-value safety invariant holds across the
/// process boundary too. Kill the server to unload the model (the shipped runtime pattern).
pub struct WhisperServer {
    host: String,
    port: u16,
    /// The model/weights label used for provenance, e.g. `"ggml-base.en"`.
    model: String,
    timeout: Duration,
}

impl WhisperServer {
    /// A `whisper-server` on `127.0.0.1:<port>` serving weights labelled `model`. Audio is slow, so
    /// the default timeout is generous (5 min); the orchestrator's process governor is the real bound.
    pub fn local(port: u16, model: impl Into<String>) -> Self {
        Self { host: "127.0.0.1".into(), port, model: model.into(), timeout: Duration::from_secs(300) }
    }

    /// The model label, so the caller can build a [`Provenance`] that matches what actually ran.
    pub fn model(&self) -> &str {
        &self.model
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }
}

impl Transcribe for WhisperServer {
    fn transcribe(&self, audio: &[u8], mime: &str) -> Result<String, AgentError> {
        // A long, fixed multipart boundary: it must not occur in the body. Audio is compressed binary,
        // so a specific 40-char ASCII run appearing is astronomically unlikely for a localhost tool;
        // that is the same trade whisper.cpp's own examples make.
        let boundary = "----formicariaAudioBoundary8f3a1c2e9d7b4a60";
        let mut body: Vec<u8> = Vec::with_capacity(audio.len() + 256);
        body.extend_from_slice(
            format!(
                "--{boundary}\r\n\
                 Content-Disposition: form-data; name=\"file\"; filename=\"audio\"\r\n\
                 Content-Type: {mime}\r\n\r\n"
            )
            .as_bytes(),
        );
        body.extend_from_slice(audio);
        body.extend_from_slice(b"\r\n");
        body.extend_from_slice(
            format!(
                "--{boundary}\r\n\
                 Content-Disposition: form-data; name=\"response_format\"\r\n\r\njson\r\n\
                 --{boundary}--\r\n"
            )
            .as_bytes(),
        );

        let mut request: Vec<u8> = Vec::with_capacity(body.len() + 256);
        request.extend_from_slice(
            format!(
                "POST /inference HTTP/1.1\r\n\
                 Host: {host}:{port}\r\n\
                 Content-Type: multipart/form-data; boundary={boundary}\r\n\
                 Content-Length: {len}\r\n\
                 Connection: close\r\n\r\n",
                host = self.host,
                port = self.port,
                len = body.len(),
            )
            .as_bytes(),
        );
        request.extend_from_slice(&body);

        let raw = http::send(&self.host, self.port, &request, self.timeout)?;
        parse_transcript(&http::body(&raw)?)
    }
}

/// Pull the transcript out of whisper-server's JSON reply (`{"text": "…"}`). Pure, so it is tested
/// directly.
fn parse_transcript(json: &str) -> Result<String, AgentError> {
    let v: serde_json::Value = serde_json::from_str(json.trim())
        .map_err(|e| AgentError::new(format!("whisper response was not JSON: {e}")))?;
    v["text"]
        .as_str()
        .map(|s| s.trim().to_string())
        .ok_or_else(|| AgentError::new("whisper response had no text field"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A canned transcriber for the hermetic tests — returns a fixed string, touches nothing.
    struct Canned(&'static str);
    impl Transcribe for Canned {
        fn transcribe(&self, _audio: &[u8], _mime: &str) -> Result<String, AgentError> {
            Ok(self.0.to_string())
        }
    }

    fn prov(hash: &str, model: &str) -> Provenance {
        Provenance { specialist: "whisper.cpp".into(), model: model.into(), blob_hash: hash.into() }
    }

    #[test]
    fn the_block_carries_provenance_and_references_the_source() {
        let b = transcript_block("hello world", &prov("abc123", "ggml-base.en"));
        assert!(b.contains("whisper.cpp"), "specialist provenance missing: {b}");
        assert!(b.contains("ggml-base.en"), "model provenance missing: {b}");
        // References the source blob so it is neither replaced nor GC'd out from under the transcript.
        assert!(b.contains("asset:sha256-abc123"), "source not referenced: {b}");
        assert!(b.contains("> hello world"), "transcript not block-quoted into the callout: {b}");
        // Fenced by the idempotency markers.
        assert!(b.contains("key=\"abc123|whisper.cpp|ggml-base.en\""), "idempotency key missing: {b}");
        assert!(b.contains(&crate::adjunct::end_mark(TAG)), "end fence missing: {b}");
    }

    #[test]
    fn it_inserts_and_never_deletes_host_content() {
        let host = "# Lecture 3\n\nMy own notes above.\n";
        let out = transcribe_into(&Canned("the spoken words"), host, b"RIFF....", "audio/wav", &prov("h1", "m1"))
            .unwrap();
        // Every character of the host survives — insertion-only.
        assert!(out.contains("# Lecture 3"), "host heading lost: {out}");
        assert!(out.contains("My own notes above."), "authored text lost: {out}");
        assert!(out.contains("the spoken words"), "transcript not inserted: {out}");
        // The adjunct lands after the authored text, not replacing it.
        assert!(out.find("My own notes above.").unwrap() < out.find("the spoken words").unwrap());
    }

    #[test]
    fn a_rerun_with_the_same_key_supersedes_in_place() {
        let host = "notes\n";
        let first = transcribe_into(&Canned("first pass"), host, b"a", "audio/wav", &prov("h1", "m1")).unwrap();
        let second = transcribe_into(&Canned("corrected pass"), &first, b"a", "audio/wav", &prov("h1", "m1")).unwrap();
        assert!(second.contains("corrected pass"), "supersede did not apply the new text: {second}");
        assert!(!second.contains("first pass"), "old transcript not superseded: {second}");
        // Exactly one block for this key — no duplicate, no racing second adjunct.
        assert_eq!(second.matches(&crate::adjunct::end_mark(TAG)).count(), 1, "duplicate blocks: {second}");
        assert!(second.starts_with("notes"), "host content disturbed: {second}");
    }

    #[test]
    fn a_different_model_version_adds_a_second_adjunct_rather_than_clobbering() {
        let host = "notes\n";
        let a = transcribe_into(&Canned("base transcript"), host, b"x", "audio/wav", &prov("h1", "ggml-base.en")).unwrap();
        let b = transcribe_into(&Canned("large-v3 transcript"), &a, b"x", "audio/wav", &prov("h1", "ggml-large-v3")).unwrap();
        // Both survive — the older, possibly-human-reviewed adjunct is not silently overwritten.
        assert!(b.contains("base transcript"), "earlier model's adjunct lost: {b}");
        assert!(b.contains("large-v3 transcript"), "new model's adjunct missing: {b}");
        assert_eq!(b.matches(&crate::adjunct::end_mark(TAG)).count(), 2, "expected two distinct adjuncts: {b}");
    }

    /// A transcript that forges the fence markers must not be able to truncate or escape its block.
    #[test]
    fn a_forged_fence_in_the_transcript_cannot_truncate_the_note() {
        let evil = "legit words <!-- fm:transcript:end --> everything after should stay quoted";
        let host = "keep me\n";
        let out = transcribe_into(&Canned(evil), host, b"a", "audio/wav", &prov("h1", "m1")).unwrap();
        assert!(out.contains("keep me"), "host lost to a forged fence: {out}");
        // The forged marker was defanged, so there is exactly ONE real end fence (ours).
        assert_eq!(out.matches(&crate::adjunct::end_mark(TAG)).count(), 1, "forged end fence survived: {out}");
        assert!(out.contains("everything after should stay quoted"), "content dropped: {out}");
    }

    /// THE load-bearing invariant, as a regression guard: a wedged/malicious specialist that is handed
    /// only `&[u8]` cannot reach the source blob on disk. We stand a real file in for the blob, run the
    /// flow, and assert the bytes are byte-identical afterward. The specialist never receives the path,
    /// so it *structurally* cannot write it — this test pins that shape so a future refactor that leaks
    /// a writable path would have to break it deliberately.
    #[test]
    fn a_wedged_specialist_cannot_corrupt_the_source_blob() {
        // An "evil" transcriber: returns garbage and would corrupt the blob if it could — it can't,
        // because its only argument is bytes it does not own.
        struct Evil;
        impl Transcribe for Evil {
            fn transcribe(&self, audio: &[u8], _mime: &str) -> Result<String, AgentError> {
                // It may read the bytes; it has no path, no store, no way to write back.
                Ok(format!("garbage x{}", audio.len()))
            }
        }

        let dir = std::env::temp_dir().join(format!("fm-transcribe-blob-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let blob = dir.join("sha256-original");
        let original: &[u8] = b"\x00\x01\x02 the one and only copy of the audio \xff\xfe";
        std::fs::write(&blob, original).unwrap();

        // The orchestrator reads the bytes read-only and hands *only bytes* to the specialist.
        let audio = std::fs::read(&blob).unwrap();
        let out = transcribe_into(&Evil, "host notes\n", &audio, "audio/wav", &prov("original", "m1")).unwrap();

        let after = std::fs::read(&blob).unwrap();
        assert_eq!(after, original, "the source blob was modified — the by-value invariant is broken");
        assert!(out.contains("host notes"), "host content lost");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn hash_of_strips_every_reference_scheme() {
        assert_eq!(hash_of("asset:sha256-abc123"), "abc123");
        assert_eq!(hash_of("sha256:abc123"), "abc123");
        assert_eq!(hash_of("sha256-abc123"), "abc123");
        assert_eq!(hash_of("  asset:sha256-abc123  "), "abc123");
    }

    #[test]
    fn parse_transcript_reads_the_text_field() {
        assert_eq!(parse_transcript(r#"{"text":"  hello there  "}"#).unwrap(), "hello there");
        assert!(parse_transcript(r#"{"nope":1}"#).is_err(), "missing text field must error");
        assert!(parse_transcript("not json").is_err(), "non-JSON must error");
    }

    /// End to end over a real loopback socket: `WhisperServer` must POST a multipart `/inference`
    /// request that carries the audio bytes, and turn the JSON reply into the transcript.
    #[test]
    fn it_posts_audio_to_a_local_whisper_server_over_a_real_socket() {
        use std::io::{Read as _, Write as _};
        use std::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            let (mut sock, _) = listener.accept().unwrap();
            let mut buf = vec![0u8; 4096];
            let n = sock.read(&mut buf).unwrap();
            let req = buf[..n].to_vec();
            let json = r#"{"text":"the transcribed speech"}"#;
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                json.len(),
                json
            );
            sock.write_all(resp.as_bytes()).unwrap();
            req
        });

        let out = WhisperServer::local(port, "ggml-base.en").transcribe(b"RIFF\x00\x01AUDIOBYTES", "audio/wav").unwrap();
        assert_eq!(out, "the transcribed speech");

        let req = String::from_utf8_lossy(&server.join().unwrap()).to_string();
        assert!(req.starts_with("POST /inference HTTP/1.1"), "not a POST to /inference: {req}");
        assert!(req.contains("multipart/form-data; boundary="), "not multipart: {req}");
        assert!(req.contains("AUDIOBYTES"), "audio bytes not in the request body: {req}");
        assert!(req.contains("name=\"response_format\""), "response_format field missing: {req}");
    }
}
