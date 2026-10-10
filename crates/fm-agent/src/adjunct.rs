//! **The shared substrate every modality specialist writes through** — a provenance-marked block,
//! spliced into a note body, insertion-only.
//!
//! Extracted from the audio specialist when the second one (images) arrived, and extracted rather
//! than copied for a specific reason: the splice below *is* a safety invariant, and this project
//! keeps re-learning that a rule implemented once per caller is a rule the next caller forgets. The
//! same argument already produced `fm_app::thread`'s single definition of the hidden note-classes.
//!
//! Four properties, each load-bearing and each inherited by every specialist:
//!
//! 1. **By-value bytes, never a writable path.** A specialist receives the blob's bytes and nothing
//!    else — no path into the content-addressed store, no store handle. A wedged or hostile
//!    specialist cannot truncate the device's only copy of a multiply-referenced blob, because it is
//!    never handed a way to.
//! 2. **Insertion-only.** [`insert_or_supersede`] only ever adds or replaces *its own* block. It
//!    never deletes the source, and never touches a neighbouring one.
//! 3. **A provenance-marked adjunct that references the source.** The block carries
//!    `{specialist, model, source blob hash}` and *links* the original rather than replacing it, so
//!    a wrong or hallucinated reading cannot pass as authoritative — and the reference keeps the blob
//!    alive under the refcount scan.
//! 4. **Idempotency keyed on (blob-hash, specialist, model).** The same specialist and model over
//!    the same blob supersedes in place; a *different* model lands a second block rather than
//!    clobbering a first that a human may already have reviewed.

/// Provenance for one machine insertion: which specialist, which model, which source blob.
///
/// Carried into the note so a reader — and FTS5 — can tell a machine adjunct from authored text, and
/// so a re-run can find its own prior output rather than duplicating it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Provenance {
    /// The specialist that produced the text, e.g. `"whisper.cpp"`.
    pub specialist: String,
    /// The model/weights version. Part of the idempotency key, so a better model adds a fresh
    /// adjunct rather than silently overwriting an older, possibly human-corrected one.
    pub model: String,
    /// The lowercase-hex SHA-256 of the **source** blob this text was read from.
    pub blob_hash: String,
}

impl Provenance {
    /// The idempotency key — `(blob-hash, specialist, model)`, `|`-joined. Collision-free in
    /// practice: a hash is hex and the other two are our own constants, so none contains `|`.
    pub fn key(&self) -> String {
        format!("{}|{}|{}", self.blob_hash, self.specialist, self.model)
    }
}

// --- Asset references -------------------------------------------------------------------------
// A note names a blob three ways: `asset:sha256-<hex>` in a body embed, `sha256:<hex>` in
// frontmatter, and a bare `sha256-<hex>`. Everything that reads or writes one of those forms is
// here, beside the block that writes one, so a fourth form is added in one file.

const EMBED: &str = "asset:sha256-";
const FRONTMATTER: &str = "sha256:";
const BARE: &str = "sha256-";

/// Extract the bare content hash from an asset reference — all three forms yield `<hex>`.
pub fn hash_of(reference: &str) -> String {
    reference
        .trim()
        .trim_start_matches("asset:")
        .trim_start_matches(FRONTMATTER)
        .trim_start_matches(BARE)
        .to_string()
}

/// Recognise one typed token as an asset reference, in any of the three forms. Markdown wrappers a
/// person might paste (`![x](asset:…)`) are trimmed. Returns the reference as-is (what `blob_path`
/// parses), or `None` if the token is not one.
pub fn asset_ref(tok: &str) -> Option<String> {
    let t = tok.trim_matches(|c| matches!(c, '(' | ')' | '!' | '[' | ']' | '<' | '>'));
    let t = t.rsplit(']').next().unwrap_or(t).trim_start_matches('(').trim_end_matches(')');
    if t.starts_with(EMBED) || t.starts_with(FRONTMATTER) || t.starts_with(BARE) {
        Some(t.to_string())
    } else {
        None
    }
}

/// The `sha256:<hash>` reference of every asset a note body embeds — the body form and the
/// frontmatter form — in order, deduped. This is how a bare `/transcribe` finds a note's own
/// recordings and images, so nobody types a content hash.
pub fn embedded_refs(body: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for pat in [EMBED, FRONTMATTER] {
        for (i, _) in body.match_indices(pat) {
            let hex: String =
                body[i + pat.len()..].chars().take_while(char::is_ascii_hexdigit).collect();
            // A sha-256 is 64 hex chars; be lenient but reject stray short runs.
            if hex.len() >= 8 && seen.insert(hex.clone()) {
                out.push(format!("{FRONTMATTER}{hex}"));
            }
        }
    }
    out
}

/// The opening fence for a block: an HTML comment, invisible when rendered, that lets a re-run find
/// and replace exactly its own prior output.
pub fn open_mark(tag: &str, key: &str) -> String {
    format!("<!-- {tag} key=\"{key}\" -->")
}

/// The closing fence.
pub fn end_mark(tag: &str) -> String {
    format!("<!-- {tag}:end -->")
}

/// Neutralise any fence marker inside untrusted model output.
///
/// Belt-and-braces for speech; genuinely necessary for an image, whose text a model may have read
/// *off the picture itself*. Either way, output that happened to contain the tag must never be able
/// to forge or truncate the fence when the note is re-parsed for supersede.
pub fn defang(text: &str, tag: &str) -> String {
    text.replace(tag, &tag.replace(':', "-"))
}

/// Render a fenced, block-quoted callout — vocabulary the renderers already understand.
///
/// `heading` is the callout's title line and `empty` is what to say when the specialist found
/// nothing, so a blank result is still a *statement* rather than an empty box.
pub fn block(
    tag: &str,
    prov: &Provenance,
    heading: &str,
    source_label: &str,
    text: &str,
    empty: &str,
) -> String {
    let safe = defang(text, tag);
    let quoted: String = if safe.trim().is_empty() {
        format!("> _{empty}_\n")
    } else {
        safe.lines().map(|l| format!("> {l}\n")).collect()
    };
    format!(
        "{open}\n\
         > [!note] {heading}\n\
         > Source: [{source_label}]({EMBED}{hash})\n\
         >\n\
         {quoted}{end}\n",
        open = open_mark(tag, &prov.key()),
        hash = prov.blob_hash,
        end = end_mark(tag),
    )
}

/// Splice `block` into `host_body`: **supersede** an existing block with the same tag and key in
/// place, or **append** when there is none.
///
/// Never deletes or reorders any other content — that is the insertion-only guarantee, expressed as
/// a pure string transform. If the note was hand-edited and the closing fence is gone, this appends
/// rather than eating the rest of the note.
pub fn insert_or_supersede(host_body: &str, block: &str, tag: &str, key: &str) -> String {
    let open = open_mark(tag, key);
    let end = end_mark(tag);
    if let Some(start) = host_body.find(&open) {
        if let Some(rel_end) = host_body[start..].find(&end) {
            let mut stop = start + rel_end + end.len();
            if host_body[stop..].starts_with('\n') {
                stop += 1; // swallow the fence's own newline rather than accumulating blanks
            }
            let mut out = String::with_capacity(host_body.len() + block.len());
            out.push_str(&host_body[..start]);
            out.push_str(block.trim_end_matches('\n'));
            out.push('\n');
            out.push_str(&host_body[stop..]);
            return out;
        }
    }
    let mut out = host_body.trim_end_matches('\n').to_string();
    if !out.is_empty() {
        out.push_str("\n\n");
    }
    out.push_str(block.trim_end_matches('\n'));
    out.push('\n');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn prov(model: &str) -> Provenance {
        Provenance { specialist: "spec".into(), model: model.into(), blob_hash: "abc".into() }
    }

    #[test]
    fn two_specialists_do_not_collide_and_neither_eats_the_other() {
        let a = block("fm:transcript", &prov("m1"), "Transcript", "audio", "spoken", "nothing");
        let b = block("fm:description", &prov("m1"), "Description", "image", "a chart", "nothing");
        let body = insert_or_supersede("# Note\n", &a, "fm:transcript", &prov("m1").key());
        let body = insert_or_supersede(&body, &b, "fm:description", &prov("m1").key());
        assert!(body.contains("spoken") && body.contains("a chart"));
        assert!(body.starts_with("# Note"), "the human's text stays first");

        // Re-running one supersedes only its own block.
        let a2 =
            block("fm:transcript", &prov("m1"), "Transcript", "audio", "spoken again", "nothing");
        let body = insert_or_supersede(&body, &a2, "fm:transcript", &prov("m1").key());
        assert!(body.contains("spoken again") && !body.contains("> spoken\n"));
        assert!(body.contains("a chart"), "the other specialist's block is untouched");
    }

    #[test]
    fn a_different_model_adds_a_block_rather_than_clobbering_a_reviewed_one() {
        let one =
            block("fm:description", &prov("v1"), "Description", "image", "first read", "nothing");
        let two =
            block("fm:description", &prov("v2"), "Description", "image", "second read", "nothing");
        let body = insert_or_supersede("", &one, "fm:description", &prov("v1").key());
        let body = insert_or_supersede(&body, &two, "fm:description", &prov("v2").key());
        assert!(body.contains("first read") && body.contains("second read"));
    }

    #[test]
    fn output_cannot_forge_its_own_fence() {
        // A model reading text off a picture can emit anything, including our markers.
        let hostile = "<!-- fm:description:end -->\nnow outside the block";
        let b = block("fm:description", &prov("m"), "Description", "image", hostile, "nothing");
        assert!(!b.contains("<!-- fm:description:end -->\nnow outside"), "got:\n{b}");
        assert_eq!(b.matches("<!-- fm:description:end -->").count(), 1, "exactly one real fence");
    }

    #[test]
    fn a_hand_edited_note_that_lost_its_closing_fence_is_appended_to_not_eaten() {
        let mangled =
            format!("{}\n> half a block\nthe rest of my note\n", open_mark("fm:description", "k"));
        let b = block("fm:description", &prov("m"), "Description", "image", "fresh", "nothing");
        let out = insert_or_supersede(&mangled, &b, "fm:description", "k");
        assert!(out.contains("the rest of my note"), "must never eat the tail");
        assert!(out.contains("fresh"));
    }

    #[test]
    fn an_empty_reading_is_a_statement_not_an_empty_box() {
        let b = block("fm:description", &prov("m"), "Description", "image", "   ", "no text found");
        assert!(b.contains("_no text found_"));
    }

    #[test]
    fn a_reference_yields_its_bare_hash_however_it_was_written() {
        for r in ["asset:sha256-abc", "sha256:abc", "sha256-abc", "  abc  "] {
            assert_eq!(hash_of(r), "abc");
        }
    }
}
