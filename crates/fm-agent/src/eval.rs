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
use crate::zone::Here;

/// One line per meeting, as cases write them: `fixed 2026-10-15T15:00`, `cancelled 2026-10-15`,
/// with ` @ <place>` when the method found one. Sorted, so order never counts. `here` is the reader's
/// own clock (a case's `here`), onto which a time written on another is moved.
pub fn meeting_lines(answer: &str, body: &str, today: Date, here: Option<&Here>) -> Vec<String> {
    let mut out: Vec<String> = meetings::parse(answer)
        .iter()
        .filter_map(|f| meetings::resolve(f, body, today, here).ok())
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

/// Word error rate of `hypothesis` against `reference` (M4): word-level edit distance over the
/// reference's length, after both are lower-cased and every character that is not a letter or a
/// digit is turned into a space — so a reader's capitals and punctuation do not count, and an
/// elision (`l'anno`) splits the same way on both sides. A number written as digits against one
/// spelled out still counts as an error; that is a real difference a person would fix.
pub fn wer(reference: &str, hypothesis: &str) -> f64 {
    let words = |s: &str| -> Vec<String> {
        s.chars()
            .flat_map(char::to_lowercase)
            .map(|c| if c.is_alphanumeric() { c } else { ' ' })
            .collect::<String>()
            .split_whitespace()
            .map(str::to_string)
            .collect()
    };
    let (r, h) = (words(reference), words(hypothesis));
    if r.is_empty() {
        return if h.is_empty() { 0.0 } else { 1.0 };
    }
    let mut prev: Vec<usize> = (0..=h.len()).collect();
    for (i, rw) in r.iter().enumerate() {
        let mut cur = vec![i + 1; h.len() + 1];
        for (j, hw) in h.iter().enumerate() {
            let sub = prev[j] + usize::from(rw != hw);
            cur[j + 1] = sub.min(prev[j + 1] + 1).min(cur[j] + 1);
        }
        prev = cur;
    }
    prev[h.len()] as f64 / r.len() as f64
}

/// How a recipe (M7) scored against the recipe it was said from. An ingredient line is *found* when
/// most of its words of three letters or more (after [`crate::textmatch::loose`], so "egg" and
/// "oil" count) are in one
/// line of the result, quantities aside; a result line is *invented* when it finds no listed
/// ingredient. `exact` asks for every ingredient found, none invented, and at least as many steps
/// as half the recipe's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecipeOutcome {
    pub exact: bool,
    pub found: usize,
    pub listed: usize,
    pub invented: usize,
    pub steps: usize,
    pub why: String,
}

pub fn recipe_outcome(
    got_ingredients: &[String],
    got_steps: usize,
    listed: &[String],
    listed_steps: usize,
) -> RecipeOutcome {
    let words = |s: &str| -> Vec<String> {
        crate::textmatch::loose(s)
            .split(|c: char| !c.is_alphanumeric())
            .filter(|w| w.chars().count() >= 3 && !w.chars().all(|c| c.is_ascii_digit()))
            .map(str::to_string)
            .collect()
    };
    let covers = |want: &str, line: &str| {
        let (w, l) = (words(want), words(line));
        !w.is_empty() && w.iter().filter(|x| l.contains(x)).count() * 2 >= w.len()
    };
    let found =
        listed.iter().filter(|want| got_ingredients.iter().any(|g| covers(want, g))).count();
    let invented = got_ingredients
        .iter()
        .filter(|g| !listed.iter().any(|want| covers(want, g) || covers(g, want)))
        .count();
    let mut why = Vec::new();
    if found < listed.len() {
        why.push(format!("found {found}/{} ingredients", listed.len()));
    }
    if invented > 0 {
        why.push(format!("{invented} invented"));
    }
    if got_steps * 2 < listed_steps {
        why.push(format!("{got_steps}/{listed_steps} steps"));
    }
    RecipeOutcome {
        exact: why.is_empty(),
        found,
        listed: listed.len(),
        invented,
        steps: got_steps,
        why: why.join("; "),
    }
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
    fn wer_ignores_case_and_punctuation_and_counts_word_edits() {
        assert_eq!(wer("ma per contrario lasceranno", "Ma, per contrario, lasceranno."), 0.0);
        assert_eq!(wer("l'anno scorso", "L'anno scorso!"), 0.0);
        assert!((wer("one two three four", "one too three") - 0.5).abs() < 1e-9);
        assert_eq!(wer("", ""), 0.0);
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
