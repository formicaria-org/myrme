//! Scoring a method against its golden cases — `docs/context/plan.md`, Phase B.
//!
//! **Exact match on what reaches the person, never a model judging a model.** A case states what a
//! careful reader would put on the agenda; a method's answer is run through the same Rust that
//! production runs ([`crate::meetings::parse`] then [`crate::meetings::resolve`]) and the two lists
//! must be equal. A case whose right answer is *nothing* is a case like any other: saying nothing is
//! scored, and so is inventing something (the research's "irrelevance" axis).
//!
//! Two numbers are kept apart, because they fail for different reasons (research, 2026-10-18):
//! - **well-formed** — the answer *is* the JSON asked for, with nothing around it. A tolerant parser
//!   hides a malformed answer from the person; it must not hide it from the eval.
//! - **exact** — the meetings that come out are the right ones.

use time::Date;

use crate::meetings;

/// One line per meeting, as cases write them: `fixed 2026-10-15T15:00`, `cancelled 2026-10-15`,
/// with ` @ <place>` when the method found one. Sorted, so order never counts.
pub fn meeting_lines(answer: &str, body: &str, today: Date) -> Vec<String> {
    let mut out: Vec<String> = meetings::parse(answer)
        .iter()
        .filter_map(|f| meetings::resolve(f, body, today).ok())
        .map(|m| {
            let head = if m.cancelled {
                format!("cancelled {}", m.start_stamp())
            } else {
                format!("fixed {}", m.start_stamp())
            };
            match m.place {
                Some(p) if !p.trim().is_empty() => format!("{head} @ {}", p.trim()),
                _ => head,
            }
        })
        .collect();
    out.sort();
    out
}

/// Whether the produced lines meet the expected ones. A place is compared only where the case names
/// one, and then loosely — case aside, and either naming the other ("the main hall" is the main
/// hall): the date and time are what the pass is for, and a case that does not care where must not
/// fail on a room the model also mentioned.
pub fn meetings_match(expected: &[String], got: &[String]) -> bool {
    if expected.len() != got.len() {
        return false;
    }
    let mut expected = expected.to_vec();
    expected.sort();
    expected.iter().zip(got).all(|(e, g)| {
        let (e_head, e_place) = split_place(e);
        let (g_head, g_place) = split_place(g);
        e_head == g_head && e_place.is_none_or(|p| g_place.is_some_and(|q| same_place(p, q)))
    })
}

fn same_place(a: &str, b: &str) -> bool {
    let (a, b) = (a.to_lowercase(), b.to_lowercase());
    a.contains(&b) || b.contains(&a)
}

fn split_place(line: &str) -> (&str, Option<&str>) {
    match line.split_once(" @ ") {
        Some((head, place)) => (head.trim(), Some(place.trim())),
        None => (line.trim(), None),
    }
}

/// The answer is exactly a JSON array — no fence, no prose, no `<think>` block around it.
pub fn well_formed_array(answer: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(answer.trim()).is_ok_and(|v| v.is_array())
}

/// How a research note (M2's write step, after [`crate::grounding::verify`]) scored on one case.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResearchOutcome {
    /// Answerable: at least one verified claim, every verified claim citing a relevant source, and
    /// every expected fact inside a verified *quote* — a fact the model only asserted does not count.
    /// Unanswerable (`facts` empty): no verified claim at all.
    pub exact: bool,
    /// Every claim the model made carried a quote that is really in its source; nothing was dropped.
    pub grounded: bool,
    pub verified: usize,
    pub dropped: usize,
    /// What was missing or wrong, for the printout.
    pub why: String,
}

pub fn research_outcome(
    note: &crate::grounding::GroundedNote,
    facts: &[String],
    relevant: &[usize],
) -> ResearchOutcome {
    let quotes: String = note
        .verified
        .iter()
        .map(|c| crate::textmatch::loose(&c.quote))
        .collect::<Vec<_>>()
        .join(" | ");
    let mut why = Vec::new();
    let exact = if facts.is_empty() {
        if !note.verified.is_empty() {
            why.push(format!("{} claims where the sources do not answer", note.verified.len()));
        }
        note.verified.is_empty()
    } else {
        let stray: Vec<usize> =
            note.verified.iter().map(|c| c.source_id).filter(|id| !relevant.contains(id)).collect();
        let missing: Vec<&String> =
            facts.iter().filter(|f| !quotes.contains(&crate::textmatch::loose(f))).collect();
        if note.verified.is_empty() {
            why.push("no verified claim".into());
        }
        if !stray.is_empty() {
            why.push(format!("cites irrelevant {stray:?}"));
        }
        if !missing.is_empty() {
            why.push(format!("missing {missing:?}"));
        }
        !note.verified.is_empty() && stray.is_empty() && missing.is_empty()
    };
    if !note.dropped.is_empty() {
        why.push(format!("{} dropped", note.dropped.len()));
    }
    ResearchOutcome {
        exact,
        grounded: note.dropped.is_empty(),
        verified: note.verified.len(),
        dropped: note.dropped.len(),
        why: why.join("; "),
    }
}

/// Whether a proposed note body (M5) did what was asked and nothing else that a case can see: every
/// `keep` phrase still there, each `add` group represented by one of its phrases, no `remove` phrase
/// left. Phrases compare as [`crate::textmatch::loose`] text. Returns the verdict and what failed.
pub fn edit_outcome(
    proposal: &str,
    keep: &[String],
    add: &[Vec<String>],
    remove: &[String],
) -> (bool, String) {
    let body = crate::textmatch::loose(proposal);
    let has = |p: &String| body.contains(&crate::textmatch::loose(p));
    let lost: Vec<&String> = keep.iter().filter(|p| !has(p)).collect();
    let absent: Vec<&Vec<String>> = add.iter().filter(|g| !g.iter().any(has)).collect();
    let left: Vec<&String> = remove.iter().filter(|p| has(p)).collect();
    let mut why = Vec::new();
    if !lost.is_empty() {
        why.push(format!("lost {lost:?}"));
    }
    if !absent.is_empty() {
        why.push(format!("did not add {absent:?}"));
    }
    if !left.is_empty() {
        why.push(format!("still has {left:?}"));
    }
    (why.is_empty(), why.join("; "))
}

/// A tally over a case battery, printed the same way by every method's eval.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Tally {
    pub cases: u32,
    pub well_formed: u32,
    pub exact: u32,
}

impl Tally {
    pub fn add(&mut self, well_formed: bool, exact: bool) {
        self.cases += 1;
        self.well_formed += u32::from(well_formed);
        self.exact += u32::from(exact);
    }
}

impl std::fmt::Display for Tally {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "exact {}/{} · well-formed {}/{}",
            self.exact, self.cases, self.well_formed, self.cases
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn a_place_counts_only_where_the_case_names_one() {
        let got = lines(&["fixed 2026-10-15T15:00 @ Room b12"]);
        assert!(meetings_match(&lines(&["fixed 2026-10-15T15:00"]), &got));
        assert!(meetings_match(&lines(&["fixed 2026-10-15T15:00 @ room B12"]), &got));
        assert!(!meetings_match(&lines(&["fixed 2026-10-15T15:00 @ B13"]), &got));
        let hall = lines(&["fixed 2026-10-15T09:00 @ the main hall"]);
        assert!(meetings_match(&lines(&["fixed 2026-10-15T09:00 @ main hall"]), &hall));
        assert!(!meetings_match(
            &lines(&["fixed 2026-10-15T15:00 @ B12"]),
            &lines(&["fixed 2026-10-15T15:00"])
        ));
    }

    #[test]
    fn saying_nothing_is_scored_and_so_is_inventing_something() {
        assert!(meetings_match(&[], &[]));
        assert!(!meetings_match(&[], &lines(&["fixed 2026-10-16T17:00"])));
        assert!(!meetings_match(&lines(&["fixed 2026-10-16T17:00"]), &[]));
    }

    #[test]
    fn well_formed_means_the_bare_array() {
        assert!(well_formed_array(" [] "));
        assert!(well_formed_array("[{\"status\":\"fixed\"}]"));
        assert!(!well_formed_array("```json\n[]\n```"));
        assert!(!well_formed_array("<think>hm</think>[]"));
        assert!(!well_formed_array("{\"status\":\"fixed\"}"));
    }
}
