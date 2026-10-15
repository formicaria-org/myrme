//! A conversation note's title — **who it is with, and who they are to the person** ("Marta Bianchi —
//! Master student, gaze estimation project") — in place of the email subject it was created with
//! ("Re: Fw: Asking about the project course"). The owner asked for it on 2026-10-15.
//!
//! The model names the person and describes them in a few words; this module checks what it can:
//! the name must be written in the emails and must not be the mailbox's owner, and the description is
//! two to six words on one line. A longer one is refused, not cut — a cut description reads as a
//! confident one. The result is a **proposal**: the person sees it, and can edit it, before it lands.

use crate::textmatch::loose;
use serde_json::Value;

/// Most words a description may have.
const ABOUT_WORDS: usize = 6;

/// The instruction, with the mailbox owner's name in it (`{me}`).
pub fn instruction(me: &str) -> String {
    crate::prompts::TITLE_INSTRUCTION.replace("{me}", me)
}

/// Read `{"name": …, "about": …}` out of the model's answer, tolerating a fence or a `<think>` block.
pub fn parse(answer: &str) -> Option<(String, String)> {
    let s = answer.rsplit("</think>").next().unwrap_or(answer);
    let (a, b) = (s.find('{')?, s.rfind('}')?);
    let v: Value = serde_json::from_str(s.get(a..=b)?).ok()?;
    let f = |k: &str| v.get(k).and_then(Value::as_str).unwrap_or("").trim().to_string();
    Some((f("name"), f("about")))
}

/// The title, when the answer passes: `Name — about`. `body` is the conversation, `me` its owner.
pub fn check(name: &str, about: &str, body: &str, me: Option<&str>) -> Option<String> {
    let n = loose(name);
    if n.split_whitespace().count() < 1 || !loose(body).contains(&n) {
        return None;
    }
    if me.is_some_and(|me| loose(me) == n) {
        return None;
    }
    let words = about.split_whitespace().count();
    if !(2..=ABOUT_WORDS).contains(&words) || about.contains('\n') {
        return None;
    }
    Some(format!("{} — {}", display_name(name), about.trim().trim_end_matches('.')))
}

/// `MARTA BIANCHI` → `Marta Bianchi`; a name already in mixed case is left as written.
fn display_name(name: &str) -> String {
    let name = name.trim();
    if name.chars().any(char::is_lowercase) {
        return name.to_string();
    }
    name.split_whitespace()
        .map(|w| {
            let mut c = w.chars();
            c.next()
                .map(|f| f.to_uppercase().chain(c.flat_map(char::to_lowercase)).collect())
                .unwrap_or_default()
        })
        .collect::<Vec<String>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    const BODY: &str = "### MARTA BIANCHI — 2026-10-15 14:19\n\nI won't be able to make it.\n\n\
                        ### Anna Ferri — 2026-10-15 15:33\n\nNo problem.\n";

    #[test]
    fn a_name_from_the_emails_and_a_few_words_make_the_title() {
        assert_eq!(
            check(
                "MARTA BIANCHI",
                "Master student, attention estimation",
                BODY,
                Some("Anna Ferri")
            )
            .as_deref(),
            Some("Marta Bianchi — Master student, attention estimation")
        );
        assert_eq!(
            check("Marta Bianchi", "Collaborator.", BODY, None),
            None,
            "one word says too little"
        );
    }

    #[test]
    fn an_invented_name_the_owner_or_a_long_description_is_refused() {
        let me = Some("Anna Ferri");
        assert_eq!(check("Maria Rossi", "Master student", BODY, me), None, "not in the emails");
        assert_eq!(check("Anna Ferri", "Professor at NUS", BODY, me), None, "the owner");
        assert_eq!(
            check(
                "Marta Bianchi",
                "Collaborator on gaze estimation and social navigation planning",
                BODY,
                me
            ),
            None,
            "refused, not cut"
        );
    }

    #[test]
    fn the_answer_is_read_through_a_fence() {
        let a = "```json\n{\"name\": \"Sara Lodi\", \"about\": \"Student, ME5400A project\"}\n```";
        assert_eq!(parse(a), Some(("Sara Lodi".into(), "Student, ME5400A project".into())));
        assert_eq!(parse("no idea"), None);
    }
}
