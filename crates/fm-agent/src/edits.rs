//! M5 — a proposed note edit as a list of **anchored edits**, applied by Rust to the whole note.
//!
//! The model used to write the note's whole new body. Measured on 2026-10-18 (`methods/edits/eval`,
//! qwen3-4b-2507 and qwen3-vl-4b, 3/6 each), that fails the way a small model fails at long copying:
//! asked to fix one word it returned the one sentence it fixed, and asked to add a line to a long note
//! it returned the line — a proposal that, accepted, would have deleted the note. It was also given
//! only the note's first 1200 characters, so even a faithful copy lost the rest.
//!
//! So the model now says **what to change**, pointing at the note's own words, and Rust changes it:
//! - `replace` — `old` becomes `new`;
//! - `insert_after` — `new` goes on a new line after the line where `old` ends;
//! - `delete` — `old` is removed;
//! - `append` — `new` goes at the end of the note (`old` is empty).
//!
//! `old` must be found **exactly once** in the note as it is at that point; an edit whose `old` is
//! missing or ambiguous is dropped and counted, never guessed at. Everything not named stays byte for
//! byte, however long the note — the model only has to see the part it changes.

use serde_json::Value;

/// One edit, as the model gave it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Edit {
    pub op: String,
    pub old: String,
    pub new: String,
}

/// The note after the edits, and how many were applied and dropped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Applied {
    pub body: String,
    pub applied: usize,
    pub dropped: usize,
}

/// The shape [`crate::prompts::EDITS_INSTRUCTION`] asks for, for
/// [`crate::LlmStep::complete_constrained`].
pub fn answer_schema() -> Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "edits": {
                "type": "array",
                "maxItems": 12,
                "items": {
                    "type": "object",
                    "properties": {
                        "op": { "enum": ["replace", "insert_after", "delete", "append"] },
                        "old": { "type": "string" },
                        "new": { "type": "string" },
                    },
                    "required": ["op", "old", "new"],
                    "additionalProperties": false,
                },
            },
        },
        "required": ["edits"],
        "additionalProperties": false,
    })
}

/// Read the model's answer; `None` when it is not the object asked for (a cut-off answer, prose).
pub fn parse(answer: &str) -> Option<Vec<Edit>> {
    let t = answer.trim();
    let t = t.find('{').zip(t.rfind('}')).map(|(a, b)| &t[a..=b])?;
    let v: Value = serde_json::from_str(t).ok()?;
    let items = v.get("edits")?.as_array()?;
    Some(
        items
            .iter()
            .map(|i| {
                let f = |k: &str| i.get(k).and_then(Value::as_str).unwrap_or("").to_string();
                Edit { op: f("op"), old: f("old"), new: f("new") }
            })
            .collect(),
    )
}

/// Apply `edits` in order to `body`. See the module header for the rules.
pub fn apply(body: &str, edits: &[Edit]) -> Applied {
    let mut out = body.to_string();
    let (mut applied, mut dropped) = (0, 0);
    for e in &merge_same_anchor(edits) {
        match apply_one(&out, e) {
            Some(next) if next != out => {
                out = next;
                applied += 1;
            }
            _ => dropped += 1,
        }
    }
    Applied { body: out, applied, dropped }
}

fn apply_one(body: &str, e: &Edit) -> Option<String> {
    if e.op == "append" {
        let new = e.new.trim();
        if new.is_empty() {
            return None;
        }
        let head = body.trim_end();
        if head.is_empty() {
            return Some(format!("{new}\n"));
        }
        // A list item after a list continues it; anything else is a paragraph of its own.
        let list = |line: &str| {
            let l = line.trim_start();
            l.starts_with("- ") || l.starts_with("* ") || l.starts_with("+ ")
        };
        let joined_list =
            head.lines().last().is_some_and(list) && new.lines().next().is_some_and(list);
        let sep = if joined_list { "\n" } else { "\n\n" };
        return Some(format!("{head}{sep}{new}\n"));
    }
    let old = e.old.trim();
    if old.is_empty() {
        return None;
    }
    let (at, found) = locate(body, old)?;
    let end = at + found.len();
    match e.op.as_str() {
        "replace" => Some(format!("{}{}{}", &body[..at], e.new, &body[end..])),
        "delete" => Some(delete(body, at, end)),
        "insert_after" => {
            let new = e.new.trim_matches('\n');
            if new.is_empty() {
                return None;
            }
            let line_end = body[end..].find('\n').map_or(body.len(), |i| end + i);
            Some(format!("{}\n{}{}", &body[..line_end], new, &body[line_end..]))
        }
        _ => None,
    }
}

/// Consecutive `replace` edits with the same `old` are one replacement written in pieces: a model
/// turning a paragraph into bullets gave one edit per bullet, each naming the whole paragraph, and
/// only the first could apply (2026-10-18, eval case `to-bullets`). Their `new` texts are joined,
/// one per line, in the order given.
fn merge_same_anchor(edits: &[Edit]) -> Vec<Edit> {
    let mut out: Vec<Edit> = Vec::with_capacity(edits.len());
    for e in edits {
        match out.last_mut() {
            Some(last)
                if e.op == "replace" && last.op == "replace" && last.old.trim() == e.old.trim() =>
            {
                last.new = format!("{}\n{}", last.new.trim_end_matches('\n'), e.new);
            }
            _ => out.push(e.clone()),
        }
    }
    out
}

/// Where `old` is in `body`, as the text actually found there. Tried as given, then — when it is not
/// found — with a literal `\n` turned into a line break and the ends trimmed: a model escaped the
/// paragraph break it copied (eval case `latex`).
fn locate<'a>(body: &str, old: &'a str) -> Option<(usize, std::borrow::Cow<'a, str>)> {
    if let Some(at) = unique(body, old) {
        return Some((at, std::borrow::Cow::Borrowed(old)));
    }
    let unescaped = old.replace("\\n", "\n");
    let unescaped = unescaped.trim();
    if unescaped.is_empty() || unescaped == old {
        return None;
    }
    unique(body, unescaped).map(|at| (at, std::borrow::Cow::Owned(unescaped.to_string())))
}

/// The one place `needle` occurs in `hay`, or `None` when it occurs nowhere or more than once.
fn unique(hay: &str, needle: &str) -> Option<usize> {
    let first = hay.find(needle)?;
    hay[first + needle.len()..].find(needle).is_none().then_some(first)
}

/// Remove `body[at..end]`, and with it the line it leaves empty, so deleting a list item does not
/// leave a blank bullet or a hole in the list.
fn delete(body: &str, at: usize, end: usize) -> String {
    let line_start = body[..at].rfind('\n').map_or(0, |i| i + 1);
    let line_end = body[end..].find('\n').map_or(body.len(), |i| end + i);
    let rest_of_line = format!("{}{}", &body[line_start..at], &body[end..line_end]);
    let bare = rest_of_line.trim().trim_start_matches(['-', '*', '+']).trim().is_empty();
    if bare {
        let cut_end = (line_end + 1).min(body.len());
        let mut s = format!("{}{}", &body[..line_start], &body[cut_end..]);
        if line_end == body.len() && s.ends_with('\n') {
            s.pop();
        }
        s
    } else {
        format!("{}{}", &body[..at], &body[end..])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn e(op: &str, old: &str, new: &str) -> Edit {
        Edit { op: op.into(), old: old.into(), new: new.into() }
    }

    #[test]
    fn a_replace_changes_only_its_words() {
        let body = "One.\n\nWe discus it.\n\nThree.";
        let a = apply(body, &[e("replace", "We discus it", "We discuss it")]);
        assert_eq!(a.body, "One.\n\nWe discuss it.\n\nThree.");
        assert_eq!((a.applied, a.dropped), (1, 0));
    }

    #[test]
    fn an_append_reaches_the_real_end_of_a_note_the_model_saw_only_part_of() {
        let body = format!("{}\n\nThe last line.", "x".repeat(3000));
        let a = apply(&body, &[e("append", "", "Next survey: 14 November")]);
        assert!(a.body.starts_with(&"x".repeat(3000)));
        assert!(a.body.ends_with("The last line.\n\nNext survey: 14 November\n"));
    }

    #[test]
    fn a_list_item_appended_to_a_list_continues_it() {
        let a = apply("Before:\n\n- one\n- two", &[e("append", "", "- three")]);
        assert_eq!(a.body, "Before:\n\n- one\n- two\n- three\n");
    }

    #[test]
    fn insert_after_goes_on_the_next_line() {
        let a = apply("- one\n- two\n- four", &[e("insert_after", "two", "- three")]);
        assert_eq!(a.body, "- one\n- two\n- three\n- four");
    }

    #[test]
    fn deleting_a_list_item_takes_its_line() {
        let a = apply("- eggs\n- milk\n- bread", &[e("delete", "milk", "")]);
        assert_eq!(a.body, "- eggs\n- bread");
        let a = apply("- eggs\n- bread\n- milk", &[e("delete", "- milk", "")]);
        assert_eq!(a.body, "- eggs\n- bread");
    }

    #[test]
    fn a_replacement_given_in_pieces_against_one_anchor_is_made_once() {
        let para = "Wear goggles. Never pipette by mouth.";
        let a = apply(
            &format!("Rules:\n\n{para}\n\nEnd."),
            &[
                e("replace", para, "- Wear goggles."),
                e("replace", para, "- Never pipette by mouth."),
            ],
        );
        assert_eq!(a.body, "Rules:\n\n- Wear goggles.\n- Never pipette by mouth.\n\nEnd.");
        assert_eq!((a.applied, a.dropped), (1, 0));
    }

    #[test]
    fn an_anchor_with_an_escaped_line_break_is_found() {
        let body = "Kinetic energy is half m v squared.\n\nNext.";
        let a = apply(body, &[e("replace", "half m v squared.\\n\\n", "$\\frac12 mv^2$.")]);
        assert_eq!(a.applied, 1);
        assert!(a.body.contains("Kinetic energy is $\\frac12 mv^2$."));
    }

    #[test]
    fn a_missing_or_ambiguous_anchor_is_dropped_never_guessed() {
        let body = "the cat and the cat";
        let a = apply(body, &[e("replace", "the cat", "a dog"), e("delete", "a horse", "")]);
        assert_eq!(a.body, body);
        assert_eq!((a.applied, a.dropped), (0, 2));
        assert_eq!(apply(body, &[e("rewrite", "cat", "dog")]).dropped, 1, "an unknown op");
    }

    #[test]
    fn the_answer_is_read_through_whatever_surrounds_it_and_a_cut_one_is_refused() {
        let ok =
            parse("```json\n{\"edits\":[{\"op\":\"append\",\"old\":\"\",\"new\":\"x\"}]}\n```");
        assert_eq!(ok.unwrap().len(), 1);
        assert_eq!(parse("{\"edits\":[{\"op\":\"append\",\"old\":\"\",\"new\":\"x"), None);
        assert_eq!(parse("I would add a line."), None);
    }
}
