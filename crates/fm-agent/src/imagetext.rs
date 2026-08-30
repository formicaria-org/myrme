//! The **image→text specialist seam** — handwriting, boards and photographed pages turned into a
//! digital transcript. The second modality specialist, riding the substrate the first one
//! established ([`crate::adjunct`]).
//!
//! Everything structural is inherited and nothing is re-decided: bytes by value and never a path,
//! insertion-only output, a provenance-marked block that *links* the source image rather than
//! replacing it, and idempotency keyed on `(blob-hash, specialist, model)`. What is specific to
//! images is only this: the instruction given to the model, and the fence tag that keeps this
//! specialist's block from colliding with the transcript's.
//!
//! **Explicitly invoked, never automatic.** Like `/transcribe`, this runs when a person types
//! `/transcribe` — the agent has no unsolicited path (`convo::addressed` gates every turn), and an
//! image is exactly the kind of content where a model volunteering an interpretation into someone's
//! notes would be worse than silence.
//!
//! **Transcription, not description.** The job is to get what is *written* into a form you can edit,
//! search and typeset — notes on paper, a whiteboard, a photographed page. Not "a picture of a board
//! with equations on it", but the equations.
//!
//! **What the transcript is, and is not.** A vision model reading a page is a *witness*, not an
//! oracle: it will confidently misread handwriting, drop a subscript, and turn an unfamiliar symbol
//! into a familiar one. That is why the output is an adjunct beside the image and never a
//! replacement for it, and why the block names the model. The reader can always look at the page.

use crate::adjunct::{self, Provenance};
use crate::AgentError;

/// This specialist's fence tag. Distinct from the transcript's, so the two never supersede or
/// truncate each other in a note that has both.
const TAG: &str = "fm:image-text";

/// What the model is asked to do: **transcribe**, in the target notation for each kind of content.
///
/// Math as LaTeX and code as fenced blocks because that is what makes a transcript *usable* — a
/// photographed equation rendered as prose is no more editable than the photograph was. Figures get
/// their labels transcribed plus one naming line: the labels are the part you would otherwise
/// retype, and the interpretation is the part a model gets wrong.
///
/// **`[?]` rather than a guess** is the load-bearing clause. A vision model's failure mode is fluent
/// invention, and in an equation a plausible wrong character is far worse than a visible gap: the
/// gap you notice and fix, the wrong subscript you carry for a year.
///
/// **Prose, and deliberately not a bulleted list — do not "tidy" this into one.** The first version
/// was a tidy list of rules, and on a hard input (a labelled plot) `qwen3-vl-4b` reproduced *the
/// list itself* as if it were the image's content, then looped until it hit the context window. This
/// codebase already knew the shape: the chat path uses no heading scaffolding because a small model
/// parroted that back too. A list in the prompt is a list the model can mistake for the answer.
///
/// **Measured on `qwen3-vl-4b`, 2026-08-30**, against five typeset fixtures: prose transcription and
/// fenced code come back reliably; a plot's title, axes, ticks and legend come back well. **LaTeX
/// conversion is inconsistent** — the same model that writes `$$E = \\frac{1}{2}CV^2$$` under a
/// two-sentence prompt returns `E = 1/2 C V^2` under this one. That is not a bug to prompt away
/// here: it is precisely the variance the correction corpus exists to record, and a human fixing it
/// is the signal.
pub const INSTRUCTION: &str = "Transcribe everything written in this image into Markdown, keeping \
its original structure and order. Write mathematics as LaTeX ($...$ inline, $$...$$ displayed), and \
put code or pseudocode in a fenced code block with its indentation. For a plot or diagram, \
transcribe its title, axis labels, tick values and legend, then one line naming what it is. \
Transcribe only what is actually there — do not correct, complete, summarise or explain it. Write \
[?] for anything genuinely unreadable rather than guessing. Reply with the transcription and \
nothing else.";

/// A specialist that turns image **bytes** into text.
///
/// Takes the image by value and returns text — the whole contract, and the whole reason a broken
/// specialist cannot touch a blob. Real implementations POST to a local vision-capable model over
/// loopback; tests use a fake.
pub trait ReadImage {
    /// `image` is the raw blob bytes; `mime` is the sniffed type (e.g. `image/png`) so the backend
    /// can label the payload. Returns the text, or a bounded error the orchestrator stops on.
    fn read_image(&self, image: &[u8], mime: &str) -> Result<String, AgentError>;
}

/// The description adjunct: a callout that references the source image and names what read it.
pub fn image_text_block(text: &str, prov: &Provenance) -> String {
    adjunct::block(
        TAG,
        prov,
        &format!("Transcribed from the image — {} · {}", prov.specialist, prov.model),
        "image",
        text,
        "(no writing found in this image)",
    )
}

/// Splice a description into `host_body` — supersede this specialist's own prior block for the same
/// key, or append. Insertion-only.
pub fn insert_or_supersede(host_body: &str, block: &str, key: &str) -> String {
    adjunct::insert_or_supersede(host_body, block, TAG, key)
}

/// The whole image→text flow as a **pure orchestration over the seam**: read the bytes, build the
/// provenance-marked adjunct, splice it in. Creates nothing, deletes nothing, touches no blob — the
/// `image` it forwards is the only thing the specialist ever sees. Returns the new body for the
/// caller to turn into a proposal.
pub fn read_into<D: ReadImage>(
    reader: &D,
    host_body: &str,
    image: &[u8],
    mime: &str,
    prov: &Provenance,
) -> Result<String, AgentError> {
    let text = reader.read_image(image, mime)?;
    let block = image_text_block(&text, prov);
    Ok(insert_or_supersede(host_body, &block, &prov.key()))
}

/// Base64, standard alphabet with padding — the encoding a `data:` URL needs.
///
/// Hand-rolled for the same reason the HTTP client is: this crate carries `thiserror`, `serde_json`
/// and `libc`, and a dependency added for twenty lines is a dependency the licence gate and the
/// release archive both have to answer for.
pub fn base64(bytes: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for c in bytes.chunks(3) {
        let b = [c[0], *c.get(1).unwrap_or(&0), *c.get(2).unwrap_or(&0)];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        out.push(A[(n >> 18 & 63) as usize] as char);
        out.push(A[(n >> 12 & 63) as usize] as char);
        out.push(if c.len() > 1 { A[(n >> 6 & 63) as usize] as char } else { '=' });
        out.push(if c.len() > 2 { A[(n & 63) as usize] as char } else { '=' });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fake(&'static str);
    impl ReadImage for Fake {
        fn read_image(&self, _: &[u8], _: &str) -> Result<String, AgentError> {
            Ok(self.0.into())
        }
    }
    struct Broken;
    impl ReadImage for Broken {
        fn read_image(&self, _: &[u8], _: &str) -> Result<String, AgentError> {
            Err(AgentError::new("no vision model loaded"))
        }
    }

    fn prov() -> Provenance {
        Provenance {
            specialist: "qwen3-vl".into(),
            model: "Q4_K_M".into(),
            blob_hash: "deadbeef".into(),
        }
    }

    #[test]
    fn a_transcript_lands_beside_the_image_and_links_back_to_it() {
        let out = read_into(&Fake("Figure 2: cell voltage"), "# Note\n", b"\x89PNG", "image/png", &prov())
            .unwrap();
        assert!(out.starts_with("# Note"), "the human's text stays first");
        assert!(out.contains("> Figure 2: cell voltage"));
        // The source is *linked*, never replaced: a misread must not pass as authoritative, and the
        // reference is also what keeps the blob alive under the refcount scan.
        assert!(out.contains("asset:sha256-deadbeef"));
        assert!(out.contains("qwen3-vl"), "and the block names what read it");
    }

    #[test]
    fn re_transcribing_the_same_image_with_the_same_model_supersedes_rather_than_duplicating() {
        let first = read_into(&Fake("first read"), "# N\n", b"x", "image/png", &prov()).unwrap();
        let again = read_into(&Fake("second read"), &first, b"x", "image/png", &prov()).unwrap();
        assert!(again.contains("second read") && !again.contains("first read"));
        assert_eq!(again.matches("Transcribed from the image").count(), 1);
    }

    #[test]
    fn a_transcript_and_a_description_coexist_in_one_note() {
        // The two specialists own different fence tags precisely so this holds.
        let body = crate::transcribe::insert_or_supersede(
            "# N\n",
            &crate::transcribe::transcript_block("spoken words", &prov()),
            &prov().key(),
        );
        let both = read_into(&Fake("printed words"), &body, b"x", "image/png", &prov()).unwrap();
        assert!(both.contains("spoken words"), "the transcript survives");
        assert!(both.contains("printed words"));
    }

    #[test]
    fn a_specialist_that_fails_changes_nothing() {
        // The orchestrator stops; the note is not half-written. This is why the flow returns a new
        // body rather than mutating one.
        assert!(read_into(&Broken, "# N\n", b"x", "image/png", &prov()).is_err());
    }

    #[test]
    fn an_image_with_no_writing_says_so_instead_of_leaving_an_empty_box() {
        let out = read_into(&Fake("   "), "# N\n", b"x", "image/png", &prov()).unwrap();
        assert!(out.contains("_(no writing found in this image)_"), "got:\n{out}");
    }

    #[test]
    fn base64_matches_the_reference_vectors() {
        // RFC 4648 §10 — including the padding cases, which are where a hand-rolled encoder goes
        // wrong and where a `data:` URL silently becomes undecodable.
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foob"), "Zm9vYg==");
        assert_eq!(base64(b"fooba"), "Zm9vYmE=");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
        // Binary, not just ASCII: a PNG header round-trips through the same path an image takes.
        assert_eq!(base64(&[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]), "iVBORw0KGgo=");
    }
}
