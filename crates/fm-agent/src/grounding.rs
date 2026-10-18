//! Deterministic **grounding** for the research profile: turn a set of numbered sources plus a model's
//! *quote-first* draft into a note whose every claim is verified against its cited source — in **pure
//! Rust, with no NLI model and no second LLM call**.
//!
//! The design (owner, 2026-07-23; research-backed): a small local model is an unreliable citer, so the
//! *orchestrator*, not the model, enforces grounding. The model is made to copy a **verbatim supporting
//! quote** for each claim; we simply check that quote actually occurs in the source it cites. A claim
//! whose quote is not found — or whose citation is out of range — is **dropped and reported**, never
//! silently shipped (the same fail-loud stance the merge driver takes with conflicts). This is the
//! deterministic analogue of citation *precision* (cf. ALCE's NLI check), at zero model cost.
//!
//! The module is **source-type-agnostic**: a [`Source`] is the same whether it came from a web engine,
//! GitHub, or arXiv — that distinction lives entirely in which SearXNG engines the query named, never
//! here. It is also store- and network-agnostic (it takes already-fetched source text), so the whole
//! thing is exercised with fakes, no model and no network.
//!
//! The orchestrator owns the source registry: the model emits only body prose, `[n]` markers, and
//! quotes; the deterministic `## Sources` list is appended *here* from the verified set, so a source
//! number or URL can never be hallucinated.

/// A numbered source the grounded writer may cite. `id` is the 1-based number the model uses as `[id]`;
/// `text` is the exact text a supporting quote must be found in (a search snippet/abstract in v1, a
/// fetched page section later). `url` may be empty for a non-web input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Source {
    pub id: usize,
    pub title: String,
    pub url: String,
    pub text: String,
}

/// One parsed-and-checked claim: the sentence, the source it cited, the verbatim quote it offered, and
/// whether that quote was found in the cited source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Claim {
    pub text: String,
    pub source_id: usize,
    pub quote: String,
    pub verified: bool,
}

/// The result of grounding a draft: the assembled note `body` (verified claims + a deterministic
/// `## Sources` section), plus the split of claims for the caller to surface ("3 unsupported claims
/// were dropped") and to compute deterministic grounding metrics from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroundedNote {
    pub body: String,
    pub verified: Vec<Claim>,
    pub dropped: Vec<Claim>,
}

impl GroundedNote {
    /// Total claims the model made (verified + dropped).
    pub fn total_claims(&self) -> usize {
        self.verified.len() + self.dropped.len()
    }

    /// The deterministic grounding metric: fraction of claims whose verbatim quote was found in its
    /// cited source. `1.0` when there were no claims (nothing unsupported shipped). This is the
    /// zero-LLM, reference-free proxy for citation precision — log it, or fail CI below a threshold.
    pub fn verified_quote_rate(&self) -> f32 {
        let total = self.total_claims();
        if total == 0 {
            return 1.0;
        }
        self.verified.len() as f32 / total as f32
    }
}

/// Assemble the numbered source pack that goes into the writing prompt — `[n] Title — URL` followed by
/// the source text, each truncated to `per_source_chars` so many sources fit a tiny model's context.
/// The `[n]` numbering here is the contract the model cites against and the verifier checks against.
pub fn pack_sources(sources: &[Source], per_source_chars: usize) -> String {
    let mut p = String::new();
    for s in sources {
        let head = if s.url.trim().is_empty() {
            format!("[{}] {}", s.id, s.title.trim())
        } else {
            format!("[{}] {} — {}", s.id, s.title.trim(), s.url.trim())
        };
        p.push_str(&head);
        p.push('\n');
        let text: String = s.text.trim().chars().take(per_source_chars).collect();
        p.push_str(&text);
        p.push_str("\n\n");
    }
    p.trim_end().to_string()
}

/// The shape [`crate::prompts::GROUNDED_JSON_INSTRUCTION`] asks for. `answerable` comes first (keys
/// reach the server alphabetically), so the model commits to whether there is an answer before it
/// writes one.
pub fn answer_schema() -> serde_json::Value {
    let text = serde_json::json!({ "type": "string" });
    serde_json::json!({
        "type": "object",
        "properties": {
            "answerable": { "type": "boolean" },
            "claims": {
                "type": "array",
                "maxItems": 12,
                "items": {
                    "type": "object",
                    "properties": {
                        "claim": text,
                        "quote": text,
                        "source": { "type": "integer", "minimum": 1 },
                    },
                    "required": ["claim", "quote", "source"],
                    "additionalProperties": false,
                },
            },
        },
        "required": ["answerable", "claims"],
        "additionalProperties": false,
    })
}

/// A schema-held answer turned into the claim list [`verify`] reads (`- claim [n]` then `> quote`), so
/// the check that a quote is in its source is the one and only check. `answerable: false` is no
/// claims at all. An answer that is not the object asked for is passed through as it is, and read as
/// free text.
pub fn claims_from_json(answer: &str) -> String {
    let t = answer.trim();
    let Some(v) = t
        .find('{')
        .zip(t.rfind('}'))
        .and_then(|(a, b)| serde_json::from_str::<serde_json::Value>(&t[a..=b]).ok())
    else {
        return answer.to_string();
    };
    if v.get("answerable").and_then(serde_json::Value::as_bool) == Some(false) {
        return String::new();
    }
    let mut out = String::new();
    for c in v.get("claims").and_then(serde_json::Value::as_array).into_iter().flatten() {
        let s = |k: &str| c.get(k).and_then(serde_json::Value::as_str).unwrap_or("").trim();
        let (claim, quote) = (s("claim").replace('\n', " "), s("quote").replace('\n', " "));
        if claim.is_empty() {
            continue;
        }
        match c.get("source").and_then(serde_json::Value::as_u64) {
            Some(n) => out.push_str(&format!("- {claim} [{n}]\n")),
            None => out.push_str(&format!("- {claim}\n")),
        }
        if !quote.is_empty() {
            out.push_str(&format!("> {quote}\n"));
        }
    }
    out
}

/// Ground a model draft against the sources: parse each claim, verify its quote is a verbatim substring
/// of the cited source, drop the rest, and assemble the note body with a deterministic `## Sources`
/// section built from the *verified* set only. Fail-closed: if nothing verifies, the body says so
/// plainly rather than fabricating.
pub fn verify(model_output: &str, sources: &[Source]) -> GroundedNote {
    let claims: Vec<Claim> = parse_claims(model_output)
        .into_iter()
        // A bullet that neither cites nor quotes is not a grounded claim (the "no answer" line, or
        // stray prose) — skip it entirely, so it neither ships nor counts against the metric.
        .filter(|(_, source_id, quote)| source_id.is_some() || quote.is_some())
        .map(|(text, source_id, quote)| {
            // A claim that names no source is given the one source its quote is in — only when
            // exactly one source holds it, so the guarantee is unchanged: every claim that ships has
            // a quote found word for word in the source it is credited to. Small models put the
            // `[n]` on the quote line or leave it out (2026-10-18, `methods/research/eval`).
            let source_id = source_id.or_else(|| {
                let q = quote.as_deref()?;
                let mut holding = sources.iter().filter(|s| quote_supported(q, &s.text));
                match (holding.next(), holding.next()) {
                    (Some(only), None) => Some(only.id),
                    _ => None,
                }
            });
            let verified = match source_id.and_then(|id| sources.iter().find(|s| s.id == id)) {
                Some(src) => quote.as_deref().is_some_and(|q| quote_supported(q, &src.text)),
                None => false,
            };
            Claim {
                text,
                source_id: source_id.unwrap_or(0),
                quote: quote.unwrap_or_default(),
                verified,
            }
        })
        .collect();

    let (verified, dropped): (Vec<Claim>, Vec<Claim>) =
        claims.into_iter().partition(|c| c.verified);
    let body = assemble_body(&verified, sources);
    GroundedNote { body, verified, dropped }
}

/// Build the note body from the verified claims plus a deterministic `## Sources` list (only the
/// sources a verified claim actually cited, in ascending order). No verified claim ⇒ a single honest
/// "not supported" line, never a fabricated answer.
fn assemble_body(verified: &[Claim], sources: &[Source]) -> String {
    if verified.is_empty() {
        return "- The sources found did not support an answer to this question.".to_string();
    }
    let mut out = String::new();
    for c in verified {
        // Every statement carries the link it came from (owner: web-search facts must each be
        // traceable). The `[n]` citation is an inline Markdown link to the exact source URL — escaped
        // brackets so it renders as a clickable "[n]". A source with no URL (a user's own note) keeps a
        // plain `[n]`, still resolvable via the Sources list.
        let src = sources.iter().find(|s| s.id == c.source_id);
        let cite = match src {
            Some(s) if !s.url.trim().is_empty() => {
                format!("[\\[{}\\]]({})", c.source_id, s.url.trim())
            }
            _ => format!("[{}]", c.source_id),
        };
        out.push_str(&format!("- {} {}\n", c.text.trim(), cite));
    }
    // Which sources were actually cited by a surviving claim — unique, ascending.
    let mut used: Vec<usize> = verified.iter().map(|c| c.source_id).collect();
    used.sort_unstable();
    used.dedup();
    out.push_str("\n## Sources\n");
    for id in used {
        if let Some(s) = sources.iter().find(|s| s.id == id) {
            if s.url.trim().is_empty() {
                out.push_str(&format!("- [{}] {}\n", id, s.title.trim()));
            } else {
                out.push_str(&format!("- [{}] {} — {}\n", id, s.title.trim(), s.url.trim()));
            }
        }
    }
    out.trim_end().to_string()
}

/// Parse the quote-first draft into `(claim_text, source_id, quote)` triples. Tolerant of a small
/// model's wobble: a claim is any `- ` bullet (its trailing `[n]` is the citation, stripped from the
/// text); the following `> ` line is its quote (surrounding quote marks stripped). A bullet with no
/// following quote line, or no `[n]`, yields `None` in that slot and will fail verification.
fn parse_claims(output: &str) -> Vec<(String, Option<usize>, Option<String>)> {
    let mut claims: Vec<(String, Option<usize>, Option<String>)> = Vec::new();
    for raw in output.lines() {
        let line = raw.trim();
        if let Some(rest) = line.strip_prefix("- ").or_else(|| line.strip_prefix("-\t")) {
            let (text, id) = split_citation(rest);
            claims.push((text, id, None));
        } else if let Some(rest) = line.strip_prefix("> ") {
            // Attach the quote to the most recent claim that doesn't have one yet. A `[n]` at the end
            // of the quote line is the claim's citation put on the wrong line: it is taken off the
            // quote (it is not in the source) and kept as the citation if the claim had none.
            let (rest, cited) = split_citation(rest);
            if let Some(last) = claims.last_mut() {
                if last.2.is_none() {
                    last.2 = Some(strip_quote_marks(&rest));
                    if last.1.is_none() {
                        last.1 = cited;
                    }
                }
            }
        }
    }
    claims
}

/// Split a claim line into its text and the trailing `[n]` citation, if present. Only a bracketed run
/// of ASCII digits at (or near) the end counts; `[1]` → `Some(1)`, and the text is the line without it.
fn split_citation(line: &str) -> (String, Option<usize>) {
    // Find the last '[' … ']' whose interior is all digits.
    if let Some(open) = line.rfind('[') {
        if let Some(close_rel) = line[open..].find(']') {
            let inner = &line[open + 1..open + close_rel];
            if !inner.is_empty() && inner.chars().all(|c| c.is_ascii_digit()) {
                if let Ok(id) = inner.parse::<usize>() {
                    let text = format!("{}{}", &line[..open], &line[open + close_rel + 1..]);
                    return (text.trim().to_string(), Some(id));
                }
            }
        }
    }
    (line.trim().to_string(), None)
}

/// Strip a single pair of surrounding quote marks (straight or curly) a model tends to wrap a quote in.
fn strip_quote_marks(s: &str) -> String {
    let t = s.trim();
    let bytes: Vec<char> = t.chars().collect();
    if bytes.len() >= 2 {
        let first = bytes[0];
        let last = bytes[bytes.len() - 1];
        let is_open = matches!(first, '"' | '“' | '\'' | '‘');
        let is_close = matches!(last, '"' | '”' | '\'' | '’');
        if is_open && is_close {
            return bytes[1..bytes.len() - 1].iter().collect::<String>().trim().to_string();
        }
    }
    t.to_string()
}

/// Is `quote` a verbatim substring of `source_text`? Both sides are **normalized first** — collapse all
/// whitespace to single spaces, fold the unicode punctuation extracted text mangles (curly quotes →
/// straight, non-breaking/thin spaces → space, soft hyphen removed) — so a faithful quote is not
/// rejected merely because the source's markdown reflowed it. An empty quote is never supported.
fn quote_supported(quote: &str, source_text: &str) -> bool {
    // The closing punctuation is not part of the words: a quote that ends "… public library." where
    // the source goes on ", where readers …" is still word for word.
    let quote = quote.trim().trim_end_matches(['.', ',', ';', ':', '!', '?']);
    let q = crate::textmatch::fold(quote);
    if q.is_empty() {
        return false;
    }
    crate::textmatch::fold(source_text).contains(&q)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A schema-held answer becomes the claim list `verify` reads; "not answerable" is no claims,
    /// so a cited "the sources do not say" cannot ship; free text passes through untouched.
    #[test]
    fn a_held_answer_becomes_claims_and_not_answerable_becomes_none() {
        let srcs = vec![src(1, "A", "", "Quillite melts at 812 degrees Celsius.")];
        let held = r#"{"answerable":true,"claims":[{"claim":"It melts at 812 C.","quote":"melts at 812 degrees Celsius","source":1}]}"#;
        let g = verify(&claims_from_json(held), &srcs);
        assert_eq!((g.verified.len(), g.verified[0].source_id), (1, 1));
        let none = r#"{"answerable":false,"claims":[{"claim":"The sources do not say.","quote":"Quillite melts","source":1}]}"#;
        assert_eq!(verify(&claims_from_json(none), &srcs).total_claims(), 0);
        assert_eq!(claims_from_json("- free text [1]\n> q"), "- free text [1]\n> q");
    }

    /// The three ways a small model misplaced a correct answer (2026-10-18, research eval), each
    /// still verified, and an invented quote still dropped whatever is done with its citation.
    #[test]
    fn a_misplaced_citation_or_a_closing_full_stop_does_not_drop_a_true_quote() {
        let srcs = vec![
            src(1, "A", "", "The archive was founded in 1873 by the parish council."),
            src(2, "B", "", "It is kept in the basement of the library, where readers come."),
        ];
        // The citation on the quote line, none on the claim.
        let g = verify("- Founded in 1873.\n> The archive was founded in 1873 [1]", &srcs);
        assert_eq!((g.verified.len(), g.verified[0].source_id), (1, 1));
        // No citation at all: the one source holding the quote is credited.
        let g = verify("- It is in the library.\n> kept in the basement of the library", &srcs);
        assert_eq!((g.verified.len(), g.verified[0].source_id), (1, 2));
        // A full stop where the source has a comma.
        let g =
            verify("- In the library [2].\n> It is kept in the basement of the library.", &srcs);
        assert_eq!(g.verified.len(), 1);
        // Invented words are dropped however they are cited.
        let g = verify("- Founded in 1874.\n> The archive was founded in 1874 [1]", &srcs);
        assert_eq!((g.verified.len(), g.dropped.len()), (0, 1));
    }

    fn src(id: usize, title: &str, url: &str, text: &str) -> Source {
        Source { id, title: title.into(), url: url.into(), text: text.into() }
    }

    #[test]
    fn pack_sources_numbers_and_truncates_each_source() {
        let sources = vec![
            src(1, "CDC", "https://cdc.gov", "mRNA vaccines teach cells to make a spike protein."),
            src(
                2,
                "NIH",
                "https://nih.gov",
                "The mRNA never enters the nucleus where DNA is kept.",
            ),
        ];
        let pack = pack_sources(&sources, 20);
        assert!(pack.contains("[1] CDC — https://cdc.gov"));
        assert!(pack.contains("[2] NIH — https://nih.gov"));
        // Truncated to 20 chars of text.
        assert!(pack.contains("mRNA vaccines teach "));
        assert!(!pack.contains("spike protein"));
    }

    #[test]
    fn a_claim_with_a_verbatim_quote_is_verified() {
        let sources = vec![src(
            1,
            "CDC",
            "https://cdc.gov",
            "mRNA vaccines teach our cells how to make a protein.",
        )];
        let draft = "- mRNA vaccines make cells produce a protein [1]\n> \"teach our cells how to make a protein\"";
        let g = verify(draft, &sources);
        assert_eq!(g.verified.len(), 1);
        assert!(g.dropped.is_empty());
        assert!(g.body.contains("mRNA vaccines make cells produce a protein"));
        // Every statement carries an inline clickable link to the exact source it came from.
        assert!(
            g.body.contains("[\\[1\\]](https://cdc.gov)"),
            "the statement links inline to its source: {}",
            g.body
        );
        assert!(g.body.contains("## Sources"));
        assert!(g.body.contains("- [1] CDC — https://cdc.gov"));
        assert_eq!(g.verified_quote_rate(), 1.0);
    }

    #[test]
    fn a_claim_whose_quote_is_not_in_its_source_is_dropped() {
        // THE load-bearing assertion: the model invents a supporting quote → the claim is dropped, not
        // shipped. This is the deterministic grounding gate.
        let sources =
            vec![src(1, "CDC", "https://cdc.gov", "mRNA vaccines teach cells to make a protein.")];
        let draft = "- mRNA vaccines alter your DNA permanently [1]\n> \"the vaccine rewrites your genome\"";
        let g = verify(draft, &sources);
        assert!(g.verified.is_empty(), "a fabricated quote must not verify");
        assert_eq!(g.dropped.len(), 1);
        assert!(
            !g.body.contains("alter your DNA"),
            "the unsupported claim must not appear in the note"
        );
        // Fail-closed: nothing verified → an honest not-supported line, never a fabricated answer.
        assert!(g.body.contains("did not support an answer"));
        assert_eq!(g.verified_quote_rate(), 0.0);
    }

    #[test]
    fn a_citation_to_a_source_that_does_not_exist_is_dropped() {
        let sources = vec![src(1, "CDC", "https://cdc.gov", "some real text here")];
        let draft = "- a claim citing nothing real [7]\n> \"some real text here\"";
        let g = verify(draft, &sources);
        assert!(
            g.verified.is_empty(),
            "an out-of-range citation cannot verify even if the quote exists elsewhere"
        );
        assert_eq!(g.dropped.len(), 1);
        assert_eq!(g.dropped[0].source_id, 7);
    }

    #[test]
    fn a_quote_is_matched_despite_reflowed_whitespace_and_curly_quotes() {
        // Extraction mangles whitespace and punctuation; a faithful quote must still verify: the
        // source has reflowed whitespace and a curly apostrophe, the model's quote is flat + straight.
        let sources = vec![src(
            1,
            "X",
            "",
            "The result\n  was  significant across\ttrials, the model’s best.",
        )];
        let draft = "- The result was significant across trials [1]\n> \"was significant across trials, the model's best\"";
        let g = verify(draft, &sources);
        assert_eq!(
            g.verified.len(),
            1,
            "normalization must let a reflowed/curly-quote match through"
        );
    }

    #[test]
    fn mixed_claims_keep_the_verified_and_drop_the_rest() {
        let sources = vec![
            src(1, "A", "https://a", "the sky appears blue due to Rayleigh scattering"),
            src(2, "B", "https://b", "water boils at 100 degrees celsius at sea level"),
        ];
        let draft = "\
- The sky is blue because of Rayleigh scattering [1]\n\
> \"blue due to Rayleigh scattering\"\n\
- Water boils at 50 degrees [2]\n\
> \"water boils at 50 degrees\"\n\
- Water boils at 100C at sea level [2]\n\
> \"water boils at 100 degrees celsius at sea level\"";
        let g = verify(draft, &sources);
        assert_eq!(g.verified.len(), 2, "the two supported claims survive");
        assert_eq!(g.dropped.len(), 1, "the '50 degrees' claim (bad quote) is dropped");
        // Sources section lists only sources a verified claim cited, ascending & deduped.
        assert!(g.body.contains("- [1] A — https://a"));
        assert!(g.body.contains("- [2] B — https://b"));
        assert!((g.verified_quote_rate() - 2.0 / 3.0).abs() < 1e-6);
    }

    #[test]
    fn every_web_statement_carries_a_clickable_link_citation_to_its_source() {
        // Owner: in web-search mode every fact must have the link it came from. The citation marker is
        // fine as long as it IS a link — `[n]` rendered as a Markdown link to the exact source URL.
        let sources = vec![
            src(1, "A", "https://a.example/x", "the alpha fact is stated here plainly"),
            src(2, "B", "https://b.example/y", "the beta fact appears in this source"),
        ];
        let draft = "\
- Alpha holds [1]\n> \"the alpha fact is stated here plainly\"\n\
- Beta holds [2]\n> \"the beta fact appears in this source\"";
        let g = verify(draft, &sources);
        assert_eq!(g.verified.len(), 2);
        // Each statement line ends in a clickable citation link, not a bare marker.
        assert!(g.body.contains("Alpha holds [\\[1\\]](https://a.example/x)"), "body: {}", g.body);
        assert!(g.body.contains("Beta holds [\\[2\\]](https://b.example/y)"), "body: {}", g.body);
        // A source with no URL (a user's own note) keeps a plain marker, still in the Sources list.
        let with_note = vec![src(1, "my note", "", "a fact from my own note text")];
        let g2 = verify("- Local fact [1]\n> \"a fact from my own note text\"", &with_note);
        assert!(
            g2.body.contains("Local fact [1]"),
            "no-URL source keeps a plain marker: {}",
            g2.body
        );
    }

    #[test]
    fn a_claim_without_a_quote_line_cannot_verify() {
        let sources = vec![src(1, "A", "", "real supporting text")];
        let draft = "- a bare claim with a citation but no quote [1]";
        let g = verify(draft, &sources);
        assert!(g.verified.is_empty());
        assert_eq!(g.dropped.len(), 1);
    }

    #[test]
    fn an_empty_or_no_answer_draft_yields_the_honest_not_supported_body() {
        let sources = vec![src(1, "A", "", "unrelated text")];
        let g = verify("- I could not find anything relevant in the sources", &sources);
        assert!(g.verified.is_empty());
        assert_eq!(g.body, "- The sources found did not support an answer to this question.");
        assert_eq!(g.verified_quote_rate(), 1.0, "no claims ⇒ nothing unsupported shipped");
    }

    #[test]
    fn split_citation_only_treats_a_trailing_digit_bracket_as_a_citation() {
        assert_eq!(super::split_citation("a claim [3]"), ("a claim".to_string(), Some(3)));
        assert_eq!(
            super::split_citation("see [RFC] for detail"),
            ("see [RFC] for detail".to_string(), None)
        );
        assert_eq!(
            super::split_citation("no citation here"),
            ("no citation here".to_string(), None)
        );
    }

    // --- Realistic ground-truth scenarios: well-known knowledge, the way a user expects it to behave.
    // The `text` fields are authentic reference-style phrasings (as a wikipedia/web SearXNG snippet
    // returns); the drafts mimic a small model's quote-first output, including a plausible hallucination
    // it must not be allowed to ship.

    #[test]
    fn realistic_photosynthesis_query_keeps_the_grounded_claims_and_drops_the_invented_one() {
        // A user asks: "What is photosynthesis and where does it happen?"
        let sources = vec![
            src(1, "Photosynthesis - Wikipedia", "https://en.wikipedia.org/wiki/Photosynthesis",
                "Photosynthesis is a system of biological processes by which photosynthetic organisms, \
                 such as most plants, algae, and cyanobacteria, convert light energy, typically from \
                 sunlight, into the chemical energy necessary to fuel their metabolism."),
            src(2, "Chloroplast - Wikipedia", "https://en.wikipedia.org/wiki/Chloroplast",
                "In plant cells, photosynthesis takes place in organelles called chloroplasts, which \
                 contain the green pigment chlorophyll."),
        ];
        // The model gets the two facts right (verbatim quotes) but also invents a wrong one from memory.
        let draft = "\
- Photosynthesis converts light energy into the chemical energy that fuels an organism's metabolism [1]\n\
> \"convert light energy, typically from sunlight, into the chemical energy necessary to fuel their metabolism\"\n\
- In plant cells the process happens in the chloroplasts [2]\n\
> \"photosynthesis takes place in organelles called chloroplasts\"\n\
- Photosynthesis occurs mainly at night when it is cooler [1]\n\
> \"photosynthesis occurs mainly at night\"";
        let g = verify(draft, &sources);

        assert_eq!(g.verified.len(), 2, "both grounded facts survive");
        assert_eq!(g.dropped.len(), 1, "the invented 'at night' claim is dropped");
        // The note a user gets: the two true, cited facts, and a real Sources list — no fabrication.
        assert!(g.body.contains("converts light energy into the chemical energy"));
        assert!(g.body.contains("happens in the chloroplasts"));
        // Each statement links inline to the source it came from.
        assert!(
            g.body.contains("[\\[1\\]](https://en.wikipedia.org/wiki/Photosynthesis)"),
            "body: {}",
            g.body
        );
        assert!(
            g.body.contains("[\\[2\\]](https://en.wikipedia.org/wiki/Chloroplast)"),
            "body: {}",
            g.body
        );
        assert!(!g.body.contains("at night"), "the unsupported claim must never reach the note");
        assert!(g.body.contains(
            "- [1] Photosynthesis - Wikipedia — https://en.wikipedia.org/wiki/Photosynthesis"
        ));
        assert!(g
            .body
            .contains("- [2] Chloroplast - Wikipedia — https://en.wikipedia.org/wiki/Chloroplast"));
        assert!((g.verified_quote_rate() - 2.0 / 3.0).abs() < 1e-6);
    }

    #[test]
    fn realistic_a_popular_myth_the_model_believes_is_rejected_because_no_source_supports_it() {
        // The value of grounding: the model's parametric memory holds the "visible from space" myth, but
        // the sources say the opposite. A user must get the sourced truth, not the model's confident myth.
        let sources = vec![
            src(1, "Great Wall of China - Wikipedia", "https://en.wikipedia.org/wiki/Great_Wall_of_China",
                "The Great Wall of China is a series of fortifications that were built across the \
                 historical northern borders of ancient Chinese states and Imperial China as protection \
                 against various nomadic groups."),
            src(2, "Great Wall visibility", "https://en.wikipedia.org/wiki/Great_Wall_of_China",
                "Contrary to a popular myth, the Great Wall of China cannot be seen from space with the \
                 naked eye, and is barely visible from low Earth orbit."),
        ];
        let draft = "\
- The Great Wall of China is a series of fortifications built along ancient China's northern borders [1]\n\
> \"a series of fortifications that were built across the historical northern borders of ancient Chinese states\"\n\
- The Great Wall of China is visible from space with the naked eye [2]\n\
> \"the Great Wall of China can be seen from space with the naked eye\"\n\
- In fact the Great Wall cannot be seen from space with the naked eye [2]\n\
> \"the Great Wall of China cannot be seen from space with the naked eye\"";
        let g = verify(draft, &sources);

        assert_eq!(
            g.verified.len(),
            2,
            "the fortifications fact and the corrected visibility fact survive"
        );
        assert_eq!(
            g.dropped.len(),
            1,
            "the 'visible from space' myth is dropped — no source supports it"
        );
        assert!(g.body.contains("cannot be seen from space"), "the sourced truth reaches the note");
        assert!(
            !g.body.contains("is visible from space with the naked eye"),
            "the myth the model believes must not be shipped as a claim"
        );
    }

    #[test]
    fn realistic_when_the_sources_do_not_answer_the_note_says_so_rather_than_guessing() {
        // A user asks something the snippets don't cover. The model should decline; if it tries to
        // answer from memory with an unfindable quote, grounding drops it and the note stays honest.
        let sources = vec![src(
            1,
            "Mercury (planet) - Wikipedia",
            "https://en.wikipedia.org/wiki/Mercury_(planet)",
            "Mercury is the first planet from the Sun and the smallest in the Solar System.",
        )];
        // Question was "how many moons does Mercury have?" — the snippet doesn't say (it has none, but
        // that fact isn't in the source), and the model guesses with a quote that isn't there.
        let draft = "- Mercury has two small moons [1]\n> \"Mercury has two moons\"";
        let g = verify(draft, &sources);
        assert!(g.verified.is_empty());
        assert_eq!(g.body, "- The sources found did not support an answer to this question.");
    }
}
