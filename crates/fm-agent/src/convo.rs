//! The conversational layer: what a user's discussion message asks the agent to do.
//!
//! The agent lives in a note's discussion and talks back and forth. A message may carry slash
//! commands — `/search` (do a web search) and `/propose` (produce a proposal edit to the host note)
//! — placed **anywhere**; their textual order does not matter, because the orchestrator always runs
//! **search before propose** so a proposal is built with fresh results. A message with no command is
//! a plain chat turn. This module is the pure parse; the model-driven turn builds on it.

/// What a user's discussion message asks for. The `ask` is the message with command tokens removed —
/// the natural-language content, used as the search seed and the proposal instruction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Intent {
    pub ask: String,
    /// `/search` was present — run a web search (before any proposal).
    pub search: bool,
    /// `/propose` was present — produce a proposal edit to the host note.
    pub propose: bool,
    /// `/research` was present — run the **grounded research** pipeline (search → quote-first write →
    /// substring-verify) and propose the cited note. Implies search; the whole point is web grounding.
    pub research: bool,
}

impl Intent {
    /// A plain conversation turn — no command, just talk.
    pub fn is_chat(&self) -> bool {
        !self.search && !self.propose && !self.research
    }
}

/// Parse a discussion message. `/search`, `/propose`, and `/research` are recognised only as
/// **standalone** whitespace-separated tokens (so `/searching` or `path/proposed` are ordinary words);
/// everything else joins the ask. Duplicate commands are idempotent.
pub fn parse(message: &str) -> Intent {
    let mut search = false;
    let mut propose = false;
    let mut research = false;
    let mut rest = Vec::new();
    for tok in message.split_whitespace() {
        match tok {
            "/search" => search = true,
            "/propose" => propose = true,
            "/research" => research = true,
            other => rest.push(other),
        }
    }
    Intent { ask: rest.join(" "), search, propose, research }
}

/// Which agent, if any, a message addresses — so an agent responds **only when called by name**
/// (`@name`). This lets several agents (a research model, a math model, …) share a discussion and the
/// user pick one; a message that names no known agent is ignored (the agent stays quiet). Returns the
/// matched agent name and the [`Intent`] parsed from the rest of the message (the mention removed).
/// Case-insensitive on the name.
pub fn addressed<'a>(message: &str, agents: &'a [&str]) -> Option<(&'a str, Intent)> {
    for tok in message.split_whitespace() {
        if let Some(name) = tok.strip_prefix('@') {
            if let Some(agent) = agents.iter().find(|a| a.eq_ignore_ascii_case(name)) {
                let rest: Vec<&str> = message.split_whitespace().filter(|t| *t != tok).collect();
                return Some((agent, parse(&rest.join(" "))));
            }
        }
    }
    None
}

/// Cap a chat reply at the vault's user-defined `max_reply_chars`, if set. The orchestrator both
/// *asks* the model to be brief and *enforces* it here — belt-and-suspenders, like the proposal
/// guardrails — because a small model does not reliably obey a length instruction. The result never
/// exceeds `max` characters (the ellipsis is counted), and it cuts at a word boundary, never mid-word.
pub fn cap_reply(reply: &str, max_chars: Option<usize>) -> String {
    match max_chars {
        Some(max) if reply.chars().count() > max => {
            if max == 0 {
                return String::new();
            }
            // Reserve one char for the ellipsis so the whole result stays within the cap.
            let keep: String = reply.chars().take(max - 1).collect();
            let cut = keep.rfind(char::is_whitespace).unwrap_or(keep.len());
            let body = keep[..cut].trim_end();
            format!("{body}…")
        }
        _ => reply.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_message_with_no_command_is_a_chat_turn() {
        let i = parse("what does this note say about mRNA?");
        assert!(i.is_chat());
        assert_eq!(i.ask, "what does this note say about mRNA?");
    }

    #[test]
    fn search_and_propose_are_recognised_in_any_order_and_stripped_from_the_ask() {
        let a = parse("/propose tidy this note /search mRNA vaccine facts");
        let b = parse("/search mRNA vaccine facts tidy this note /propose");
        assert!(a.search && a.propose && b.search && b.propose);
        // The ask is the content minus the command tokens (order preserved from the message).
        assert_eq!(a.ask, "tidy this note mRNA vaccine facts");
        assert!(!a.is_chat());
    }

    #[test]
    fn search_only_and_propose_only() {
        let s = parse("/search best sources on X");
        assert!(s.search && !s.propose);
        assert_eq!(s.ask, "best sources on X");

        let p = parse("/propose rewrite the intro");
        assert!(p.propose && !p.search);
        assert_eq!(p.ask, "rewrite the intro");
    }

    #[test]
    fn only_standalone_tokens_count_as_commands() {
        let i = parse("see docs/proposed.md and keep /searching later");
        assert!(i.is_chat(), "embedded look-alikes are not commands");
        assert_eq!(i.ask, "see docs/proposed.md and keep /searching later");
    }

    #[test]
    fn duplicate_commands_are_idempotent() {
        let i = parse("/search /search a query /propose /propose");
        assert!(i.search && i.propose);
        assert_eq!(i.ask, "a query");
    }

    #[test]
    fn research_is_recognised_stripped_and_is_not_a_chat() {
        let i = parse("/research how do mRNA vaccines work");
        assert!(i.research && !i.is_chat());
        assert_eq!(i.ask, "how do mRNA vaccines work");
        // A look-alike embedded in a word is not the command.
        assert!(parse("see the /researcher notes").is_chat());
    }

    #[test]
    fn an_agent_responds_only_when_addressed_by_name() {
        let agents = ["lfm2.5-230m", "math-helper"];
        // Addressed → the matched agent + the parsed rest (mention removed), case-insensitive.
        let (who, intent) = addressed("Hey @Math-Helper how much is 1 + 1? /search", &agents).unwrap();
        assert_eq!(who, "math-helper");
        assert!(intent.search && !intent.propose);
        assert_eq!(intent.ask, "Hey how much is 1 + 1?");
        // Not addressed to any known agent → ignored (the agent stays quiet).
        assert!(addressed("just chatting with people @someone-else", &agents).is_none());
        assert!(addressed("no mention at all", &agents).is_none());
    }

    #[test]
    fn cap_reply_enforces_the_length_at_a_word_boundary_within_the_cap() {
        // No cap → untouched.
        assert_eq!(cap_reply("a full reply", None), "a full reply");
        // Under the cap → untouched.
        assert_eq!(cap_reply("short", Some(20)), "short");
        // Over the cap → cut at a word boundary, result (with ellipsis) within the cap.
        let out = cap_reply("hello world and then some", Some(9));
        assert!(out.chars().count() <= 9, "over cap: {out:?}");
        assert!(out.ends_with('…') && !out.contains("wor"), "cut mid-word: {out:?}");
        assert_eq!(out, "hello…");
    }
}
