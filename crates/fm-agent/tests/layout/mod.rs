//! The mail pass's prompt layouts under study (speed study, `docs/research/2026-10-19-speed.md`
//! §2.7). Both mail calls — the meetings and the title — read the same emails; a layout decides
//! whether the second call can reuse the first one's cached prompt. **Eval-only:** production keeps
//! `old` until a layout wins on the meetings and title evals. Chosen by `FM_LAYOUT`.
//!
//! - `old`: each question's instruction is its system prompt, the emails are the user message. No
//!   reuse: the two calls differ from the first token.
//! - `first`: one short shared system prompt; the emails, then the question's instruction.
//! - `b`: one shared system prompt holding both instructions by name; the emails, then
//!   `Question: <name>`.
//! - `c`: the meetings call exactly as `old`; the title call sends the same system prompt and
//!   emails, then its own instruction after them. Meetings accuracy is `old`'s by construction.

use fm_agent::meetings;

// Each eval asks one of the two questions, so each compiles half of this module.
#[allow(dead_code)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Question {
    Meetings,
    Title,
}

/// The layout named by `FM_LAYOUT`, `old` when unset.
pub fn chosen() -> String {
    std::env::var("FM_LAYOUT").unwrap_or_else(|_| "old".into()).to_lowercase()
}

/// `(system, user)` for one question about one conversation.
pub fn prompt(layout: &str, q: Question, title: &str, body: &str, me: &str) -> (String, String) {
    let meet = fm_agent::prompts::MEETINGS_INSTRUCTION.to_string();
    let name = fm_agent::titles::instruction(me);
    let emails = meetings::user_prompt(title, body);
    let own = if q == Question::Meetings { meet.clone() } else { name.clone() };
    match layout {
        "first" => (
            "You read an email exchange and answer the question that follows it.".into(),
            format!("{emails}\n\n{own}"),
        ),
        "b" => (
            format!(
                "You read an email exchange and answer one of two questions about it. The question \
                 is named on the last line, after the emails.\n\nQuestion \"meetings\": {meet}\n\n\
                 Question \"title\": {name}"
            ),
            format!(
                "{emails}\n\nQuestion: {}",
                if q == Question::Meetings { "meetings" } else { "title" }
            ),
        ),
        "c" => match q {
            Question::Meetings => (meet, emails),
            Question::Title => (
                meet,
                format!("{emails}\n\nThis time, answer a different question instead. {name}"),
            ),
        },
        _ => (own, emails),
    }
}
