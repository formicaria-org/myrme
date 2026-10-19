//! The mail pass's prompt layouts under study (speed study, `docs/research/2026-10-19-speed.md`
//! §2.7). Both mail calls — the meetings and the title — read the same emails; a layout decides
//! whether the second call can reuse the first one's cached prompt. Production runs `old` or `b`, as
//! the model's catalogue entry says (`shared_mail_prompt`); `first` and `c` are eval-only. Chosen by
//! `FM_LAYOUT`, else the model's own.
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

/// The layout named by `FM_LAYOUT`; unset, the one `FM_MODEL` runs in production — `b` when its
/// catalogue entry has `shared_mail_prompt = true`, else `old` — so an eval measures what ships.
pub fn chosen() -> String {
    if let Ok(l) = std::env::var("FM_LAYOUT") {
        return l.to_lowercase();
    }
    let model = std::env::var("FM_MODEL").unwrap_or_default();
    let catalogue = include_str!("../../../fm-agent-run/models.toml");
    let shared = catalogue
        .split("[[models]]")
        .find(|block| block.lines().any(|l| l.trim() == format!("name = \"{model}\"")))
        .is_some_and(|block| block.lines().any(|l| l.trim() == "shared_mail_prompt = true"));
    if shared { "b" } else { "old" }.into()
}

/// `(system, user)` for one question about one conversation. `old` and `b` are production's own
/// (`meetings::prompt`, apart and shared), so the evals measure what ships.
pub fn prompt(layout: &str, q: Question, title: &str, body: &str, me: &str) -> (String, String) {
    let pq = match q {
        Question::Meetings => meetings::Question::Meetings,
        Question::Title => meetings::Question::Title,
    };
    let meet = fm_agent::prompts::MEETINGS_INSTRUCTION.to_string();
    let emails = meetings::user_prompt(title, body);
    match layout {
        "b" => meetings::prompt(true, pq, title, body, me),
        "first" => {
            let own = meetings::prompt(false, pq, title, body, me).0;
            (
                "You read an email exchange and answer the question that follows it.".into(),
                format!("{emails}\n\n{own}"),
            )
        }
        "c" => match q {
            Question::Meetings => (meet, emails),
            Question::Title => (
                meet,
                format!(
                    "{emails}\n\nThis time, answer a different question instead. {}",
                    fm_agent::titles::instruction(me)
                ),
            ),
        },
        _ => meetings::prompt(false, pq, title, body, me),
    }
}
