//! Every instruction this crate gives a model, in one file.
//!
//! They were spread over five modules, so reading what the assistant is actually told meant opening
//! each one, and a wording rule applied to one prompt was easy to miss in the next. Nothing here is
//! user-editable, and nothing here is shown to a person: these are the model's instructions, not the
//! assistant's replies.

/// The fixed **house-format instruction** given to the model as the system prompt for the writing
/// step. It formats within a closed set — Markdown plus the note vocabulary the renderers already
/// understand — and is **not** user-editable, the same literal-free discipline the renderers enforce.
pub const OUTPUT_FORMAT_INSTRUCTION: &str = "\
You write the body of a study note. Output ONLY the note body itself — do NOT repeat the task or \
these instructions, do NOT add a heading like '# Task', and do NOT wrap the whole answer in a code \
fence. Use GitHub-flavored Markdown, and ONLY: headings, paragraphs, bullet and numbered lists, \
tables, fenced code blocks (for code only), block quotes, callouts (`> [!note]` / `> [!tip]` / \
`> [!warning]`), Mermaid diagrams (```mermaid fenced), and KaTeX math ($…$ inline, $$…$$ block). Do \
not invent other syntax, do not add front-matter, and do not answer from memory: use only the \
provided notes and search results, and say plainly when they do not answer the question.";

/// The system prompt for the optional query-refinement step: rough request in, one clean search
/// query out. Bounded, single-shot, no tools.
pub const QUERY_REFINE_INSTRUCTION: &str = "\
Rewrite the user's request as a single, well-formed web-search query. Fix spelling and grammar and \
keep it short. Output only the query, nothing else.";

/// The system prompt for a **conversational reply** in a discussion. Deliberately light: frame the
/// role and ask for a direct, concise answer — nothing more. The owner's call is that pre/post exist
/// for safety only and the user should see the model as it is, so this does not micro-manage format
/// or forbid the model its own knowledge; any provided notes/search are offered, not mandated.
pub const CHAT_INSTRUCTION: &str = "\
You are a study assistant in a note's discussion. Reply to the latest message directly and concisely. \
Use the conversation and any notes or search results provided; you may also draw on your own knowledge. \
Write any math as KaTeX: $x$ inline and $$x$$ on its own line — never \\( \\) or \\[ \\].";

/// The system prompt for the grounded writing step — the *quote-first* contract. Adapted from the
/// open answer-engine convention (Perplexica/Vane: cite every claim, avoid unsupported assumptions,
/// say so when nothing supports an answer), sharpened to demand a **verbatim quote** so the claim is
/// deterministically checkable. Not user-editable; the same literal-free discipline the renderers keep.
pub const GROUNDED_WRITE_INSTRUCTION: &str = "\
You write a study note using ONLY the numbered sources provided. Output a list of claims, one per \
item. For each claim: write ONE sentence on its own line, starting with '- ' and ending with the \
citation '[n]' of the source it comes from; then on the NEXT line, starting with '> ', copy a SHORT \
VERBATIM quote from that same source that supports the claim — copy the words EXACTLY, do not \
paraphrase and do not shorten across gaps. Cite every claim. Use only the numbered sources, never your \
own knowledge, and never state anything the sources do not. If the sources do not answer the question, \
reply with a single '- ' line saying so and no citation. Output only the list — no preamble, no \
headings, no Sources section.";

pub const MEETINGS_INSTRUCTION: &str = "You read an email exchange and list the meetings with people that it FIXES — \
agreed appointments, calls or visits on a day. Ignore proposals that were declined or replaced: if a \
later message moves or cancels a meeting, report only the final arrangement. Do not report deadlines or \
tasks. Answer ONLY with a JSON array, no other text. Each item is an object with these keys:\n\
\"quote\": the sentence from the email that fixes the meeting, copied character for character in its \
original language — never translated, never reworded;\n\
\"title\": a short name for the meeting, in the language of the emails;\n\
\"date\": the words in the email that give the day, copied exactly and untranslated, or \"\";\n\
\"time\": the words in the email that give the start time, copied exactly, or \"\";\n\
\"end\": the words that give the end time, copied exactly, or \"\";\n\
\"place\": where, copied from the email, or \"\".\n\
If no meeting is fixed, answer [].";

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
pub const IMAGE_INSTRUCTION: &str = "Transcribe everything written in this image into Markdown, keeping \
its original structure and order. Write mathematics as LaTeX ($...$ inline, $$...$$ displayed), and \
put code or pseudocode in a fenced code block with its indentation. For a plot or diagram, \
transcribe its title, axis labels, tick values and legend, then one line naming what it is. \
Transcribe only what is actually there — do not correct, complete, summarise or explain it. Write \
[?] for anything genuinely unreadable rather than guessing. Reply with the transcription and \
nothing else.";
