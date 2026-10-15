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

/// A conversation's title: who it is with and who they are to the owner (`crate::titles`). Measured on
/// `qwen3-vl-4b` over the owner's five real conversations (2026-10-15): right names, short roles; the
/// one long description is what `titles::check` refuses. `{me}` is the mailbox owner's name.
pub const TITLE_INSTRUCTION: &str = "You read an email exchange from the mailbox of {me}. Name the one \
person {me} is corresponding with, and say in a few words who that person is to {me}, using only what \
the emails say: their role, their course or project, their organisation. Prefer the plainest role the \
emails support. Answer ONLY with a JSON object, no other text, with these keys:\n\
\"name\": that person's full name, copied exactly as the emails write it;\n\
\"about\": two to six words, in English, such as \"Master student, ME5400A project\", \"Collaborator on \
the Moonshot project\" or \"Director of the Robotics Lab\". Never guess what the emails do not say.";

/// The meeting pass's question: **where do the meetings stand now**, after the last message — not
/// "which meetings does it fix". Measured on `qwen3-vl-4b` against the owner's real mail (2026-10-15):
/// the old wording reported every moved meeting at its *old* time, and missed a cancellation; this one
/// reported all five states right (two moves with their links, a proposal for the following week, a
/// confirmation, and a call-off). An "agreed" key was tried and dropped: it answered `true` for a time
/// nobody had replied to — who wrote the sentence is read from the email's heading instead.
pub const MEETINGS_INSTRUCTION: &str = "You read an email exchange, oldest message first, and say where its \
meetings stand NOW, after the LAST message. A later message overrides an earlier one: when a meeting was \
moved, report it only at its new time; when someone wrote that they cannot come, or that it will not take \
place, and no new time was set afterwards, report it as cancelled. Read the meetings only from the emails \
themselves. Answer ONLY with a JSON array, no other text. Each item is an object with these keys:\n\
\"status\": \"fixed\" when a day is set for it now, or \"cancelled\" when it was called off and no new time \
was set;\n\
\"quote\": the sentence from the email that sets the current time, or that calls it off, copied character \
for character in its original language — never translated, never reworded;\n\
\"title\": a short name for the meeting, in the language of the emails;\n\
\"date\": the words in that email that give the day, copied exactly and untranslated, or \"\";\n\
\"time\": the words in that email that give the start time, copied exactly, or \"\";\n\
\"end\": the words that give the end time, copied exactly, or \"\";\n\
\"place\": where — a room, or a meeting link copied exactly from the email — or \"\".\n\
If the exchange never fixed a meeting, answer [].";

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
/// The instruction for a packet's nutrition label (`fm_agent_run::label`).
///
/// **A transcription, like the one below, and nothing more**: the columns kept with their headings,
/// each number with its unit. Which column is per 100 g, what a serving weighs and what each figure
/// is are read out of the answer by code, never asked of the model.
///
/// **Measured on `qwen3-vl-4b`, 2026-10-13**, on four photographs from Open Food Facts: two upright
/// tables (per 100 ml with a can, per 100 g with a serving) came back exact at 768 and 1024 pixels;
/// a table photographed sideways came back as invented figures; one printed as two tables side by
/// side had its rows crossed even upright. So the person sees the picture beside the form.
pub const LABEL_INSTRUCTION: &str = "Transcribe the nutrition table printed in this image as a \
Markdown table, keeping every column and its heading exactly as printed (for example \"per 100 g\", \
\"per serving 30 g\"). Copy each number with its unit exactly as printed. Transcribe only what is \
actually there — do not correct, complete, convert, calculate or explain anything. Write [?] for \
anything unreadable rather than guessing. If there is no nutrition table in the image, reply with \
exactly: NO TABLE. Reply with the table and nothing else.";

pub const IMAGE_INSTRUCTION: &str = "Transcribe everything written in this image into Markdown, keeping \
its original structure and order. Write mathematics as LaTeX ($...$ inline, $$...$$ displayed), and \
put code or pseudocode in a fenced code block with its indentation. For a plot or diagram, \
transcribe its title, axis labels, tick values and legend, then one line naming what it is. \
Transcribe only what is actually there — do not correct, complete, summarise or explain it. Write \
[?] for anything genuinely unreadable rather than guessing. Reply with the transcription and \
nothing else.";
