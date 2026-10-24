//! Meetings found in an email exchange — `decisions.md` 2026-10-09, *the assistant proposes meetings*.
//!
//! # Who decides what
//!
//! The model reads a conversation note (`fm_core::mail`) — since 2026-10-24 a [`window`] of it: what is
//! new since its last read and what is still in play — and answers with the meetings it sees as
//! *fixed*, each **pointing at the sentence that fixes it**, copied from the email.
//! That is the one thing it is asked to get right. It does not supply the date: run against the real
//! model (qwen3-vl-4b, 2026-10-09), it translated "the 16th of October" into "16 ottobre" and offered
//! "domani" for a sentence that said no such thing. So everything after the pointing is this module:
//!
//! 1. **The quote must be in the emails** (whitespace, case and accents aside). A meeting the model
//!    cannot point at is dropped — the same rule as [`crate::grounding`].
//! 2. **Rust finds the date** in that sentence; if the sentence only says "that day", in the text
//!    before it **in the same email** (the nearest date wins). Only when neither has one are the
//!    model's own day words tried, and then only if they appear in that email. Counted from the day
//!    *that email* was sent, read off its heading, in Italian or English. Nothing found → dropped.
//! 3. **Rust finds the time** in that sentence (`12pm`, `alle 15`, `11:30am`), never elsewhere: a time
//!    from another sentence could belong to anything. None found → the meeting is for the day.
//!    **A zone written right after it** ("10am Berlin time", "15:00 CET") moves it onto the person's
//!    own clock ([`crate::zone`], 2026-10-23); with none written it stays as written.
//! 4. **The plan** ([`plan`]) decides what to propose: the conversation note takes the first meeting;
//!    each further one becomes a proposed meeting note; anything already on the agenda (an invitation
//!    made it), already linked, or already proposed is left alone.
//!
//! The person accepts or rejects every one of them. Nothing here writes.

use std::collections::{HashMap, HashSet};

use crate::textmatch::{fold, loose};
use crate::zone::{self, Here};

use serde_json::Value;
use time::{Date, Duration, Month, Time, Weekday};

/// One meeting as the model reported it: words copied from the email.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Found {
    /// `"cancelled"` when the exchange calls the meeting off with no new time; anything else is a
    /// meeting fixed now.
    pub status: String,
    pub title: String,
    pub date: String,
    pub time: String,
    pub end: String,
    pub place: String,
    pub quote: String,
}

/// A meeting this module could check and date.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Meeting {
    pub title: String,
    pub day: Date,
    pub start: Option<Time>,
    pub end: Option<Time>,
    pub place: Option<String>,
    pub quote: String,
    /// When the email the sentence is in was sent (`2026-10-15T17:46`, the form `mail_last` takes),
    /// so a meeting from a message that arrived since the last read can be told from history.
    pub sent_at: Option<String>,
    /// Who wrote that email, from its heading — shown with the quote, so the person sees at a glance
    /// whether a time is theirs and still unanswered, or the other side's.
    pub said_by: Option<String>,
    /// The sentence calls the meeting off, with no new time. `day`/`start` then say *which* meeting,
    /// when the sentence names it ("our meeting at 12 pm" tomorrow); see `dated`.
    pub cancelled: bool,
    /// Whether `day` was read from the emails. A cancellation that names no day ("I can't make it")
    /// is about the meeting the conversation stands for.
    pub dated: bool,
    /// The time as the email wrote it, when it was on another clock and was moved onto the person's:
    /// `10:00 Berlin time`. Shown beside the converted time, so the person can check the arithmetic.
    pub written: Option<String>,
}

/// The meeting a conversation note stands for now, as [`plan`] weighs it.
#[derive(Debug, Clone, Copy)]
pub struct Current<'a> {
    /// Its `start`.
    pub start: &'a str,
    /// Still ahead of us: a meeting already held is history and is never moved.
    pub upcoming: bool,
    /// Its `location`, if it has one.
    pub place: Option<&'a str>,
    /// When the conversation's last email was sent (`mail_last`). On a re-read, a call-off counts only
    /// when it is in that email — the latest word — and that email fixes no other time.
    pub latest: Option<&'a str>,
}

impl Meeting {
    /// `2026-10-16T15:00`, or `2026-10-16` with no time — the form a note's `start` takes.
    pub fn start_stamp(&self) -> String {
        stamp(self.day, self.start)
    }
    /// The note's `due`: the end time if the email gave one, else the start (a single marker).
    pub fn due_stamp(&self) -> String {
        stamp(self.day, self.end.or(self.start))
    }
}

fn stamp(day: Date, t: Option<Time>) -> String {
    let d = format!("{:04}-{:02}-{:02}", day.year(), u8::from(day.month()), day.day());
    match t {
        Some(t) => format!("{d}T{:02}:{:02}", t.hour(), t.minute()),
        None => d,
    }
}

/// Why a reported meeting was not proposed. Counted, so the pass can say what it left out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dropped {
    /// The quote is not in the emails.
    NotInEmail,
    /// No date could be read in the sentence or in the email before it.
    NoDate,
    /// It is already over.
    Past,
    /// The sentence is a question nobody has answered yet: no later email from anyone else. "Would
    /// Thursday or Friday work?" fixes nothing until someone says which (2026-10-18, eval case
    /// `proposal-not-fixed`). Once a reply exists, whether it accepted is the model's to read.
    Unanswered,
}

/// Tokens the text the model reads may take. The laptop model runs with an 8192-token window
/// (`agents/models.toml`); the instructions take ~350 (~700 shared with the title question) and the
/// answer is capped at [`ANSWER_TOKENS`], so this leaves a margin for the estimate below being low.
/// Since 2026-10-24 the model reads a [`window`], which is far shorter; this is the last resort.
pub const TEXT_TOKENS: usize = 6000;
/// The longest answer the meeting pass asks for: [`MAX_ITEMS`] items. One quoting a sentence takes
/// about 105 tokens (`weekly-long`, 2026-10-24) and one whose quote carries a meeting link more, so
/// this leaves room for four of those.
pub const ANSWER_TOKENS: u32 = 720;
/// The most meetings one answer may list. A window holds the meetings still to come and the new
/// emails, rarely more than two or three; the whole history once needed more, and the model filled
/// the old limit of 8 with one past meeting repeated (2026-10-24).
pub const MAX_ITEMS: usize = 4;
/// The new emails one [`step`] reads at most, in tokens (at least one email, however long). A
/// conversation never read before is read in steps of this size, oldest first.
pub const CHUNK_TOKENS: usize = 1500;

/// A cautious token estimate: a CJK character is about one token; other text about one per 3
/// characters. Erring high only means a long exchange is cut a little earlier.
fn tokens_of(s: &str) -> usize {
    let (mut cjk, mut other) = (0usize, 0usize);
    for c in s.chars() {
        if matches!(c as u32, 0x3000..=0x9FFF | 0xAC00..=0xD7AF | 0xF900..=0xFAFF | 0xFF00..=0xFFEF)
        {
            cjk += 1;
        } else {
            other += 1;
        }
    }
    cjk + other.div_ceil(3)
}

/// The user turn: the conversation's title and its text, cut to [`TEXT_TOKENS`]. The **tail** is kept,
/// because the final arrangement is what matters and the later messages carry it.
/// The conversation without the assistant's own box (`<!-- fm:meeting … -->` … `<!-- fm:meeting:end -->`).
///
/// **The box is our writing, not the exchange**, and it repeats the arrangement the assistant found
/// last time. Left in, a small model read its own earlier answer first and reported it again over a
/// later email that moved the meeting (all three moved meetings, a user's mail, 2026-10-15); and a
/// quote matched inside it had no email to date it from. Both the model and [`resolve`] read this.
pub fn without_box(body: &str) -> String {
    let mut out = String::with_capacity(body.len());
    let mut inside = false;
    for line in body.split_inclusive('\n') {
        let t = line.trim();
        if !inside && t.starts_with("<!-- fm:meeting") && !t.starts_with("<!-- fm:meeting:end") {
            inside = true;
            continue;
        }
        if inside {
            if t.starts_with("<!-- fm:meeting:end") {
                inside = false;
            }
            continue;
        }
        out.push_str(line);
    }
    out
}

pub fn user_prompt(title: &str, body: &str) -> String {
    let mut start = 0;
    while tokens_of(&body[start..]) > TEXT_TOKENS {
        // Drop a line at a time from the front, so the cut lands on a line boundary.
        match body[start..].find('\n') {
            Some(i) => start += i + 1,
            None => break,
        }
    }
    format!("Conversation: {title}\n\n{}", &body[start..])
}

/// The two questions the mail pass asks of one conversation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Question {
    Meetings,
    Title,
}

/// `(system, user)` for one mail-pass question about one conversation.
///
/// - `shared == false`: the question's own instruction is the system prompt and the emails are the
///   user message. The two calls differ from the first token, so the title call reads the whole
///   exchange again.
/// - `shared == true`: **one** system prompt for both calls, holding both instructions by name, then
///   the emails, then `Question: <name>`. The title call reuses the meetings call's cached emails:
///   on a ~4000-token exchange it reads 516 tokens instead of 4,041.
///
/// It is the model's choice (`shared_mail_prompt` in the catalogue), because it is measured per
/// model (speed study §2.7, 2026-10-19): qwen3-vl-4b scored 16/17 meetings and 12/12 titles shared,
/// 15/17 and 12/12 apart; lfm2.5-vl-450m at Q4_K_M fell from 5/17 and 9/12 to 3/17 and 5/12 shared,
/// naming the mailbox's owner or the instruction's example roles. `me` is the owner's name; both
/// calls must pass the same one, or their system prompts differ.
pub fn prompt(shared: bool, q: Question, title: &str, body: &str, me: &str) -> (String, String) {
    let emails = user_prompt(title, body);
    if !shared {
        let own = match q {
            Question::Meetings => crate::prompts::MEETINGS_INSTRUCTION.to_string(),
            Question::Title => crate::titles::instruction(me),
        };
        return (own, emails);
    }
    let system = format!(
        "You read an email exchange and answer one of two questions about it. The question is \
         named on the last line, after the emails.\n\nQuestion \"meetings\": {}\n\n\
         Question \"title\": {}",
        crate::prompts::MEETINGS_INSTRUCTION,
        crate::titles::instruction(me),
    );
    let name = match q {
        Question::Meetings => "meetings",
        Question::Title => "title",
    };
    (system, format!("{emails}\n\nQuestion: {name}"))
}

/// The shape [`crate::prompts::MEETINGS_INSTRUCTION`] asks for, as a JSON Schema, for
/// [`crate::LlmStep::complete_constrained`]. **Every key is required, and that is what it is for**:
/// measured on 2026-10-18 (qwen3-4b-2507, `methods/meetings/eval`), the free answer was always valid
/// JSON but left out `quote` in 6 of 14 cases, and a meeting without its pointer is dropped; held to
/// this schema it scored 13/14 against 8/14. The keys reach the server in alphabetical order
/// (`serde_json`'s map sorts them), so the grammar generates them in that order, not the
/// instruction's. [`parse`] stays tolerant, so an unconstrained answer still reads.
pub fn answer_schema() -> Value {
    let text = serde_json::json!({ "type": "string" });
    serde_json::json!({
        "type": "array",
        "maxItems": MAX_ITEMS,
        "items": {
            "type": "object",
            "properties": {
                "status": { "enum": ["fixed", "cancelled"] },
                "quote": text, "title": text, "date": text,
                "time": text, "end": text, "place": text,
            },
            "required": ["status", "quote", "title", "date", "time", "end", "place"],
            "additionalProperties": false,
        },
    })
}

/// Read the model's answer. Tolerant of what small models wrap JSON in — a ```json fence, a
/// `<think>` block, a sentence before the array — and of a single object instead of an array.
pub fn parse(answer: &str) -> Vec<Found> {
    let mut s = answer.to_string();
    if let Some(end) = s.find("</think>") {
        s = s[end + "</think>".len()..].to_string();
    }
    let candidates = [s.find('[').zip(s.rfind(']')), s.find('{').zip(s.rfind('}'))];
    for (a, b) in candidates.into_iter().flatten() {
        if b <= a {
            continue;
        }
        let Ok(v) = serde_json::from_str::<Value>(&s[a..=b]) else { continue };
        let items = match v {
            Value::Array(items) => items,
            obj @ Value::Object(_) => vec![obj],
            _ => continue,
        };
        return items
            .iter()
            .map(|i| {
                let f = |k: &str| i.get(k).and_then(Value::as_str).unwrap_or("").trim().to_string();
                Found {
                    title: f("title"),
                    date: f("date"),
                    time: f("time"),
                    end: f("end"),
                    place: f("place"),
                    quote: f("quote"),
                    status: f("status").to_lowercase(),
                }
            })
            .filter(|f| !f.quote.is_empty())
            .collect();
    }
    Vec::new()
}

// ── Reading only what is new (2026-10-24) ───────────────────────────────────────────────────────
//
// `decisions.md` 2026-10-24, *the meeting pass reads what is new since its last read*. The model was
// shown the whole exchange at every new email and listed every meeting it had ever fixed: on a
// ten-email exchange with a meeting a week the answer outgrew its cap, and uncapped it repeated one
// past meeting until the schema's limit. The model now reads a [`window`]; Rust still checks every
// quote against the whole conversation ([`resolve`]), and [`plan`] is unchanged.

/// What the meeting pass remembers of one conversation between reads. The emails' own headings
/// (`### Sender — 2026-10-23 09:30`) are its version: a conversation note only gains emails at its end.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Ledger {
    /// The newest email the model has read, by its heading's stamp (`2026-10-23T09:30`).
    pub read: Option<String>,
    /// Emails kept in view though already read: each fixes a meeting still to come (`start`), or asks
    /// a question nobody has answered yet (`start` is `None`).
    pub keep: Vec<Kept>,
    /// A fingerprint of the emails up to `read`. When they change (an email rewritten, a re-import)
    /// the ledger no longer describes the conversation, and it is read again from its start.
    pub print: u64,
}

/// One email kept in view, by its heading's stamp.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Kept {
    pub at: String,
    pub start: Option<String>,
}

impl Ledger {
    /// As stored in the pass's memory file: `{"read": …, "keep": [[at, start|null]], "print": "hex"}`.
    pub fn to_json(&self) -> Value {
        serde_json::json!({
            "read": self.read,
            "keep": self.keep.iter().map(|k| serde_json::json!([k.at, k.start])).collect::<Vec<_>>(),
            "print": format!("{:016x}", self.print),
        })
    }

    /// Read back what [`Ledger::to_json`] wrote; anything else is an empty ledger.
    pub fn from_json(v: &Value) -> Self {
        let keep = v["keep"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|k| {
                let at = k[0].as_str()?.to_string();
                Some(Kept { at, start: k[1].as_str().map(str::to_string) })
            })
            .collect();
        Ledger {
            read: v["read"].as_str().map(str::to_string),
            keep,
            print: v["print"].as_str().and_then(|h| u64::from_str_radix(h, 16).ok()).unwrap_or(0),
        }
    }

    /// This ledger, if the emails it has read are still the ones in `mails`; else an empty one.
    fn checked(&self, mails: &[Mail]) -> Ledger {
        match &self.read {
            Some(r) if fingerprint(mails, r) == self.print => self.clone(),
            _ => Ledger::default(),
        }
    }
}

/// One email as written (its heading line and its text), for the window. `mails[0]` is whatever is
/// above the first heading.
struct Mail {
    at: Option<String>,
    text: String,
}

fn mails(body: &str) -> Vec<Mail> {
    let mut out = vec![Mail { at: None, text: String::new() }];
    for line in body.split_inclusive('\n') {
        if let Some(head) = line.strip_prefix("### ") {
            out.push(Mail { at: heading_at(head.trim_end()), text: String::new() });
        }
        if let Some(m) = out.last_mut() {
            m.text.push_str(line);
        }
    }
    out
}

/// FNV-1a over the emails sent up to `upto`, as [`loose`] reads them: stable across builds, so a
/// ledger written by one release is understood by the next.
fn fingerprint(mails: &[Mail], upto: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for m in mails.iter().filter(|m| m.at.as_deref().is_some_and(|a| a <= upto)) {
        for b in loose(&m.text).bytes().chain([0u8]) {
            h = (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3);
        }
    }
    h
}

/// What the model reads in one step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Window {
    /// The text: whatever is above the first email, one line for the emails left out, then the
    /// emails shown, oldest first.
    pub text: String,
    /// The newest email in it — where the ledger's `read` moves to.
    pub to: Option<String>,
    /// It reaches the conversation's newest email: what the model answers is where things stand now.
    pub done: bool,
    /// How many unread emails it holds, and their tokens.
    pub new: usize,
    pub new_tokens: usize,
    /// How many earlier emails the one line stands for.
    pub omitted: usize,
}

/// **What the model reads**: the emails not yet read (at most `budget` tokens of them, and at least
/// one), the email before the first of them (what it answers), and the earlier emails the ledger keeps
/// (a meeting still to come, a question still open). The earlier emails left out are one line, written
/// here: how many, when, and that none fixes a meeting still to come, which is what keeping the others
/// means. It names no time and quotes nothing, so a model that echoes it points at no email.
///
/// With nothing unread, the newest email stands in for the new ones, so the answer still says where
/// things stand.
pub fn window(body: &str, ledger: &Ledger, budget: usize) -> Window {
    let mails = mails(body);
    let ledger = ledger.checked(&mails);
    let n = mails.len();
    let read =
        |m: &Mail| matches!((&ledger.read, &m.at), (Some(r), Some(a)) if a.as_str() <= r.as_str());
    let mut first = (1..n).find(|&i| !read(&mails[i])).unwrap_or(n);
    if first == n && n > 1 {
        first = n - 1;
    }
    let (mut end, mut new_tokens) = (first, 0);
    while end < n {
        let t = tokens_of(&mails[end].text);
        if end > first && new_tokens + t > budget {
            break;
        }
        new_tokens += t;
        end += 1;
    }
    let kept: HashSet<&str> = ledger.keep.iter().map(|k| k.at.as_str()).collect();
    let shown = |i: usize| {
        i == 0
            || i + 1 == first
            || (first..end).contains(&i)
            || mails[i].at.as_deref().is_some_and(|a| kept.contains(a))
    };
    let left: Vec<usize> = (1..first).filter(|&i| !shown(i)).collect();
    let mut text = mails[0].text.clone();
    if let (Some(&a), Some(&b)) = (left.first(), left.last()) {
        let day =
            |i: usize| mails[i].at.as_deref().and_then(|s| s.get(..10)).unwrap_or("?").to_string();
        let line = if left.len() == 1 {
            format!(
                "(1 earlier email in this conversation, sent {}, is not shown: it fixes no meeting that is still to come.)",
                day(a)
            )
        } else {
            format!(
                "({} earlier emails in this conversation, sent {} to {}, are not shown: none of them fixes a meeting that is still to come.)",
                left.len(),
                day(a),
                day(b)
            )
        };
        if !text.is_empty() && !text.ends_with("\n\n") {
            text.push('\n');
        }
        text.push_str(&line);
        text.push_str("\n\n");
    }
    for (i, m) in mails.iter().enumerate().skip(1) {
        if shown(i) {
            text.push_str(&m.text);
        }
    }
    Window {
        text,
        to: mails.get(end.max(1) - 1).and_then(|m| m.at.clone()),
        done: end == n,
        new: end - first,
        new_tokens,
        omitted: left.len(),
    }
}

/// The ledger after the model read `w` and answered `found`. Kept: each email whose sentence fixes a
/// meeting still to come (Rust's dates, never the model's omission), or asks a question nobody has
/// answered in what has been read. An email whose meeting is over leaves the view.
pub fn advance(
    ledger: &Ledger,
    w: &Window,
    found: &[Found],
    body: &str,
    today: Date,
    here: Option<&Here>,
) -> Ledger {
    let ms = mails(body);
    let ledger = ledger.checked(&ms);
    let all = blocks(body);
    let read = w.to.clone().or_else(|| ledger.read.clone());
    let day = stamp(today, None);
    let upcoming = |s: &str| s >= day.as_str();
    let answered = |at: &str| {
        let by = all.iter().find(|b| b.at.as_deref() == Some(at)).and_then(|b| b.by.clone());
        all.iter().any(|b| {
            b.at.as_deref()
                .is_some_and(|x| x > at && read.as_deref().is_some_and(|r| x <= r) && b.by != by)
        })
    };
    let mut keep: Vec<Kept> = Vec::new();
    let mut add = |k: Kept| {
        if !keep.contains(&k) {
            keep.push(k);
        }
    };
    for k in &ledger.keep {
        let stays = match &k.start {
            Some(s) => upcoming(s),
            None => !answered(&k.at),
        };
        if stays {
            add(k.clone());
        }
    }
    for f in found {
        match resolve(f, body, today, here) {
            Ok(m) if !m.cancelled => {
                if let Some(at) = m.sent_at.clone() {
                    add(Kept { at, start: Some(m.start_stamp()) });
                }
            }
            Err(Dropped::Unanswered) => {
                if let Some(at) = source(f, &all) {
                    add(Kept { at, start: None });
                }
            }
            _ => {}
        }
    }
    let print = read.as_deref().map(|r| fingerprint(&ms, r)).unwrap_or(0);
    Ledger { read, keep, print }
}

/// The stamp of the email a quote is in — the same lookup [`resolve`] makes.
fn source(f: &Found, all: &[Block]) -> Option<String> {
    let copied = elided(&f.quote, all).unwrap_or_else(|| f.quote.clone());
    let nquote = loose(&copied);
    if nquote.is_empty() {
        return None;
    }
    match all.iter().find(|b| b.text.contains(&nquote)) {
        Some(b) => b.at.clone(),
        None => near_sentence(&nquote, all).and_then(|(b, _)| b.at.clone()),
    }
}

/// One step of reading a conversation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Step {
    /// What the model answered for this window, each item once.
    pub found: Vec<Found>,
    /// The ledger to remember.
    pub ledger: Ledger,
    /// The window reached the newest email: `found` is where things stand now, to be planned.
    pub done: bool,
    /// Emails passed over because even alone their answer was cut at [`ANSWER_TOKENS`].
    pub skipped: usize,
}

/// **Read one window and move the ledger** — one model call, or a few when an answer is cut.
///
/// A cut answer (`finish_reason: length`) is never read as "nothing": its JSON is incomplete, and
/// reading it as `[]` once lost a meeting for good. The window is read again at once with half as many
/// new emails; one email whose answer is still cut is passed over, and `skipped` says so.
///
/// **An answer that repeats an item is read again the same way.** On a long exchange the model once
/// wrote one past meeting eight times and never the new one, and whether it did turned on a few
/// tokens of the prompt (the conversation's title, the owner's name; 2026-10-24). A repeat is that
/// loop's mark; inside the cap it would otherwise read as "nothing new". With one new email left the
/// repeats are merged and the answer kept.
#[allow(clippy::too_many_arguments)]
pub fn step<L: crate::LlmStep>(
    llm: &L,
    shared: bool,
    title: &str,
    body: &str,
    me: &str,
    today: Date,
    here: Option<&Here>,
    ledger: &Ledger,
) -> Result<Step, crate::AgentError> {
    let mut budget = CHUNK_TOKENS;
    loop {
        let w = window(body, ledger, budget);
        let (system, user) = prompt(shared, Question::Meetings, title, &w.text, me);
        let reply = llm.complete_constrained(&system, &user, &answer_schema())?;
        let said = parse(&reply.content);
        let mut found: Vec<Found> = Vec::new();
        for f in &said {
            if !found.contains(f) {
                found.push(f.clone());
            }
        }
        let looped = found.len() < said.len();
        if (reply.truncated() || looped) && w.new > 1 {
            budget = w.new_tokens / 2;
            continue;
        }
        if reply.truncated() {
            let ledger = advance(ledger, &w, &[], body, today, here);
            return Ok(Step { found: Vec::new(), ledger, done: w.done, skipped: 1 });
        }
        let ledger = advance(ledger, &w, &found, body, today, here);
        return Ok(Step { found, ledger, done: w.done, skipped: 0 });
    }
}

/// Every step at once, from an empty ledger: what the pass reads over several of its calls. For the
/// eval, so it scores the reader that runs. Answers the last window's items.
pub fn read_all<L: crate::LlmStep>(
    llm: &L,
    shared: bool,
    title: &str,
    body: &str,
    me: &str,
    today: Date,
    here: Option<&Here>,
) -> Result<Vec<Found>, crate::AgentError> {
    let mut ledger = Ledger::default();
    loop {
        let s = step(llm, shared, title, body, me, today, here, &ledger)?;
        if s.done || s.ledger.read == ledger.read {
            return Ok(s.found);
        }
        ledger = s.ledger;
    }
}

/// One email in the conversation note: the day it was sent, and its text, normalised.
struct Block {
    sent: Option<Date>,
    /// The heading's day and time, `2026-10-07T09:12` — compared with `mail_last` as text.
    at: Option<String>,
    /// The heading's sender.
    by: Option<String>,
    text: String,
    /// The same email as written (case kept, whitespace folded) — what a near quote is replaced by.
    raw: String,
}

/// Split the note into its emails at their `### Name — YYYY-MM-DD HH:MM` headings. Anything above the
/// first heading (the person's own writing, an assistant box) is a block with no sending day.
fn blocks(body: &str) -> Vec<Block> {
    let mut out =
        vec![Block { sent: None, at: None, by: None, text: String::new(), raw: String::new() }];
    let mut raw = String::new();
    for line in body.lines() {
        if let Some(head) = line.strip_prefix("### ") {
            if let Some(b) = out.last_mut() {
                b.text = loose(&raw);
                b.raw = fold(&raw);
            }
            raw.clear();
            out.push(Block {
                sent: heading_day(head),
                at: heading_at(head),
                by: head.rsplit_once(" — ").map(|(n, _)| n.trim().to_string()),
                text: String::new(),
                raw: String::new(),
            });
            continue;
        }
        raw.push_str(line);
        raw.push('\n');
    }
    if let Some(b) = out.last_mut() {
        b.text = loose(&raw);
        b.raw = fold(&raw);
    }
    out
}

/// The sentences of an email as written, cut after `.`, `!` or `?` followed by a space.
fn sentences(raw: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0;
    let b = raw.as_bytes();
    for (i, c) in raw.char_indices() {
        if matches!(c, '.' | '!' | '?') && b.get(i + 1).is_none_or(|n| *n == b' ') {
            out.push(raw[start..=i].trim());
            start = i + 1;
        }
    }
    out.push(raw[start..].trim());
    out.retain(|s| !s.is_empty());
    out
}

/// The email sentence a near quote stands for: the model copied a sentence *almost* exactly ("16th
/// **of** October" for the "16th October" the email says — a user's mail, 2026-10-15). At least
/// five words, and at least 80% of the words in common with the closer of the two; the best match
/// wins, a later email on a tie. Returns the email and the sentence as the email writes it — the
/// date is then read from **that**, never from the model's version.
fn near_sentence<'a>(quote: &str, all: &'a [Block]) -> Option<(&'a Block, String)> {
    let words = |s: &str| -> HashSet<String> {
        s.split_whitespace()
            .map(|w| w.trim_matches(|c: char| !c.is_alphanumeric()).to_string())
            .filter(|w| !w.is_empty())
            .collect()
    };
    let q = words(quote);
    if q.len() < 5 {
        return None;
    }
    let mut best: Option<(f64, &Block, String)> = None;
    for b in all.iter().filter(|b| b.at.is_some()) {
        // Runs of one to three sentences: a model's quote often takes the next sentence along too.
        let ss = sentences(&b.raw);
        for i in 0..ss.len() {
            for j in i..ss.len().min(i + 3) {
                let run = ss[i..=j].join(" ");
                let w = words(&loose(&run));
                let score = q.intersection(&w).count() as f64 / q.len().max(w.len()) as f64;
                if score >= 0.8 && best.as_ref().is_none_or(|(s, _, _)| score >= *s) {
                    best = Some((score, b, run));
                }
            }
        }
    }
    best.map(|(_, b, s)| (b, s))
}

/// `Maria Rossi — 2026-10-07 09:12` → 7 October 2026.
fn heading_day(head: &str) -> Option<Date> {
    let (_, when) = head.rsplit_once(" — ").or_else(|| head.rsplit_once(" - "))?;
    let d = when.trim().get(..10)?;
    let mut it = d.split('-');
    let (y, m, dd) =
        (it.next()?.parse().ok()?, it.next()?.parse::<u8>().ok()?, it.next()?.parse().ok()?);
    Date::from_calendar_date(y, Month::try_from(m).ok()?, dd).ok()
}

/// `Maria Rossi — 2026-10-07 09:12` → `2026-10-07T09:12`, the form `mail_last` takes.
fn heading_at(head: &str) -> Option<String> {
    let (_, when) = head.rsplit_once(" — ").or_else(|| head.rsplit_once(" - "))?;
    let w = when.trim();
    let (d, t) = (w.get(..10)?, w.get(11..16)?);
    (heading_day(head).is_some() && t.as_bytes().get(2) == Some(&b':')).then(|| format!("{d}T{t}"))
}

/// **A quote the model shortened with "…"** (2026-10-16, a real email): it copied the opening of a
/// long email, "...", and the sentence that fixes the meeting — every piece word for word, the whole
/// in no email, so the meeting was dropped. Each piece must still be in **one** email exactly as
/// copied; the piece that carries a figure (a day, an hour) then stands as the quote, else the last.
/// `None` when the quote is not elided, or a piece is not in the emails — the strict check decides.
fn elided(quote: &str, all: &[Block]) -> Option<String> {
    let pieces: Vec<&str> = quote
        .split('…')
        .flat_map(|p| p.split("..."))
        .map(str::trim)
        .filter(|p| !loose(p).is_empty())
        .collect();
    if pieces.len() < 2 {
        return None;
    }
    all.iter().find(|b| pieces.iter().all(|p| b.text.contains(&loose(p))))?;
    let figure = pieces.iter().rev().find(|p| p.chars().any(|c| c.is_ascii_digit()));
    Some(figure.unwrap_or(pieces.last()?).to_string())
}

/// Check one reported meeting against the emails and date it. `body` is the conversation note.
///
/// `here` is the person's own clock. A time the email puts on another one ("10am Berlin time") is
/// moved onto it, and its day with it; with `here` unknown, or no zone written, the time stays as
/// written.
pub fn resolve(
    f: &Found,
    body: &str,
    today: Date,
    here: Option<&Here>,
) -> Result<Meeting, Dropped> {
    // The first (oldest) email with the sentence: a later reply quoting it does not make it new.
    let all = blocks(body);
    let copied = elided(&f.quote, &all).unwrap_or_else(|| f.quote.clone());
    let nquote = loose(&copied);
    if nquote.is_empty() {
        return Err(Dropped::NotInEmail);
    }
    // Exactly as copied, or else the email sentence it nearly copies — which then stands as the quote.
    let (exact, quote) = match all.iter().find(|b| b.text.contains(&nquote)) {
        Some(b) => (b, copied.clone()),
        None => near_sentence(&nquote, &all).ok_or(Dropped::NotInEmail)?,
    };
    let nquote = loose(&quote);
    let at = exact.text.find(&nquote).ok_or(Dropped::NotInEmail)?;
    // A quote that runs over several sentences is read in the one the model's own words for *this*
    // meeting point at: a small model reported "The kick-off is on Monday at 10am. The review will be
    // on Friday at 2pm." for both meetings, and the first date and time then went to both (2026-10-18,
    // eval case `two-meetings`). The quote shown to the person is unchanged.
    let (nquote, at) = narrow_to_sentence(&nquote, at, &[&f.time, &f.date]);
    if nquote.trim_end().ends_with('?') {
        let here = all.iter().position(|b| std::ptr::eq(b, exact)).unwrap_or(0);
        let answered = all[here + 1..].iter().any(|b| b.sent.is_some() && b.by != exact.by);
        if !answered {
            return Err(Dropped::Unanswered);
        }
    }
    let (sent, sent_at, said_by, msg) =
        (exact.sent, exact.at.clone(), exact.by.clone(), exact.text.clone());
    let anchor = sent.unwrap_or(today);
    let model_words = |w: &str, within: &str| {
        let n = loose(w);
        (!n.is_empty() && within.contains(&n)).then_some(n)
    };
    let cancelled = f.status == "cancelled";
    let day = find_day(&nquote, anchor, false)
        .or_else(|| find_day(&msg[..at], anchor, true))
        .or_else(|| model_words(&f.date, &msg).and_then(|n| read_day(&n, anchor)));
    // A meeting needs its day; a cancellation may name none ("I can't make it").
    let (mut day, dated) = match day {
        Some(d) => (d, true),
        None if cancelled => (anchor, false),
        None => return Err(Dropped::NoDate),
    };
    // A time only from the sentence itself; none there → the meeting is for the day, which is true.
    let mut start =
        find_time(&nquote).or_else(|| model_words(&f.time, &nquote).and_then(|n| read_time(&n)));
    let mut end = start
        .and(model_words(&f.end, &nquote).and_then(|n| read_time(&n)))
        .filter(|e| Some(*e) > start);
    // Written on another clock: moved onto the person's, on the meeting's own day (2026-10-23).
    let mut written = None;
    if let (Some(s), Some(z), Some(here)) = (start, zone_in(&nquote), here) {
        if let Some((d, t)) = z.convert(day, s, here) {
            if (d, t) != (day, s) {
                written = Some(format!("{:02}:{:02} {}", s.hour(), s.minute(), z.label));
            }
            // An end that lands on another day than the start is dropped: a note's `due` shares
            // its start's day here, and a marker is truer than a wrong end.
            end = end.and_then(|e| z.convert(day, e, here)).filter(|(ed, _)| *ed == d).map(|p| p.1);
            (day, start) = (d, Some(t));
        }
    }
    if dated && day < today {
        return Err(Dropped::Past);
    }
    // A meeting that had begun before the newest email was sent is history too, though its day is
    // today: a follow-up at 17:31 does not fix again the 17:00 call it follows (a user's mail,
    // 2026-10-19). A call-off is left to the day rule: it still says what happened to the meeting.
    let newest = all.iter().filter_map(|b| b.at.as_deref()).max();
    if dated
        && !cancelled
        && start.is_some()
        && newest.is_some_and(|n| stamp(day, start).as_str() < n)
    {
        return Err(Dropped::Past);
    }
    // The place may be in any email of the exchange — a link sent once, a confirmation that says "via
    // the link you shared" — but only as copied, character for character.
    let place = (!cancelled)
        .then(|| all.iter().find_map(|b| model_words(&f.place, &b.text)))
        .flatten()
        .map(|_| f.place.trim().to_string());
    Ok(Meeting {
        title: if f.title.is_empty() { quote.chars().take(60).collect() } else { f.title.clone() },
        day,
        start,
        end,
        place,
        quote,
        sent_at,
        said_by,
        cancelled,
        dated,
        written,
    })
}

/// The one sentence of a several-sentence `quote` (already [`loose`]) that contains the first of
/// `words` found in exactly one of them, with its offset added to `at`. Anything less certain — one
/// sentence, words in none or in several — leaves the quote whole, which is how it was read before.
fn narrow_to_sentence(quote: &str, at: usize, words: &[&str]) -> (String, usize) {
    let mut pieces = Vec::new();
    let mut start = 0;
    let bytes = quote.as_bytes();
    for (i, c) in quote.char_indices() {
        if matches!(c, '.' | '!' | '?') && bytes.get(i + 1) == Some(&b' ') {
            pieces.push((start, &quote[start..=i]));
            start = i + 2;
        }
    }
    if start < quote.len() {
        pieces.push((start, &quote[start..]));
    }
    if pieces.len() > 1 {
        for w in words {
            let n = loose(w);
            if n.is_empty() {
                continue;
            }
            let mut hits = pieces.iter().filter(|(_, p)| p.contains(&n));
            if let (Some(&(off, p)), None) = (hits.next(), hits.next()) {
                return (p.to_string(), at + off);
            }
        }
    }
    (quote.to_string(), at)
}

/// The words of `text` with the punctuation that hugs them taken off, keeping what a date or time is
/// written with (`16/10`, `11:30am`, `l'11`).
fn tokens(text: &str) -> Vec<String> {
    text.split_whitespace()
        .map(|w| {
            w.trim_matches(|c: char| ",;:()\"«»?!".contains(c)).trim_end_matches('.').to_string()
        })
        .filter(|w| !w.is_empty())
        .collect()
}

const DATE_GLUE: [&str; 17] = [
    "of",
    "the",
    "il",
    "del",
    "di",
    "next",
    "prossimo",
    "prossima",
    "this",
    "questo",
    "questa",
    "on",
    "lo",
    "l",
    "week",
    "settimana",
    "della",
];
const RELATIVE: [&str; 7] =
    ["oggi", "today", "domani", "tomorrow", "dopodomani", "stasera", "tonight"];

fn ordinal_or_number(w: &str) -> Option<u32> {
    let digits = w.trim_end_matches(|c: char| c.is_ascii_alphabetic());
    let suffix = &w[digits.len()..];
    if digits.is_empty() || !digits.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    if !matches!(suffix, "" | "st" | "nd" | "rd" | "th" | "o" | "°") {
        return None;
    }
    digits.parse().ok()
}

/// Can this word be part of a written date?
fn dateish(w: &str) -> bool {
    month_of(w).is_some()
        || weekday_of(w).is_some()
        || ordinal_or_number(w).is_some_and(|n| (1..=31).contains(&n) || (2000..=2100).contains(&n))
        || DATE_GLUE.contains(&w)
}

/// Does a date start here? Strict, because emails are full of numbers: a month or a full weekday name,
/// an ordinal (`16th`), `il`/`the` + a day, a slashed date (`16/10`), an ISO date, or a relative word.
/// `RM 01-02` (a room), `2999-00001` (a project) and `15.30` (a time) are not dates.
fn starts_date(words: &[String], k: usize) -> bool {
    let w = words[k].as_str();
    let prev = k.checked_sub(1).map(|p| words[p].as_str());
    let full_weekday = weekday_of(w).is_some() && w.len() >= 5;
    let ordinal = ordinal_or_number(w).is_some_and(|n| (1..=31).contains(&n))
        && !w.chars().all(|c| c.is_ascii_digit());
    let articled = w.chars().all(|c| c.is_ascii_digit())
        && ordinal_or_number(w).is_some_and(|n| (1..=31).contains(&n))
        && matches!(prev, Some("il" | "the" | "l"));
    let slashed = w.contains('/')
        && w.split('/').all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()));
    let iso = w.len() == 10
        && w.as_bytes().get(4) == Some(&b'-')
        && w.split('-').all(|p| p.chars().all(|c| c.is_ascii_digit()));
    (month_of(w).is_some() && w.len() >= 3)
        || full_weekday
        || ordinal
        || articled
        || slashed
        || iso
        || RELATIVE.contains(&w)
}

/// The dates written in `text`, read from `anchor`; the first, or with `last` the nearest the end.
fn find_day(text: &str, anchor: Date, last: bool) -> Option<Date> {
    let words = tokens(text);
    let mut found = Vec::new();
    let mut k = 0;
    while k < words.len() {
        if !starts_date(&words, k) {
            k += 1;
            continue;
        }
        // Grow the window over neighbouring date words: "by the 16th of October", "Monday 28th of Sep".
        let (mut lo, mut hi) = (k, k + 1);
        while lo > 0 && dateish(&words[lo - 1]) {
            lo -= 1;
        }
        while hi < words.len() && dateish(&words[hi]) {
            hi += 1;
        }
        if let Some(d) = read_day(&words[lo..hi].join(" "), anchor) {
            found.push(d);
        }
        k = hi;
    }
    if last {
        found.last().copied()
    } else {
        found.first().copied()
    }
}

/// The first time written in `text`: `12pm`, `11:30am`, `15:30`, `10.30`, `alle 15`, `at 10`, `ore 9`.
/// A bare number counts only after `at`/`alle`/`ore`/`h`/`dalle`/`verso`, or before `am`/`pm`.
fn find_time(text: &str) -> Option<Time> {
    times_in(&tokens(text)).into_iter().next().map(|(t, _)| t)
}

/// The clock the sentence's times are on: a zone written right after one of them, the first found.
fn zone_in(text: &str) -> Option<zone::Zone> {
    let words = tokens(text);
    times_in(&words).into_iter().find_map(|(_, next)| zone::after_time(&words[next..]))
}

/// Every time in `words`, in order, each with the index of the word after it.
fn times_in(words: &[String]) -> Vec<(Time, usize)> {
    let mut out = Vec::new();
    for (k, w) in words.iter().enumerate() {
        let core = w.trim_end_matches(|c: char| c.is_ascii_alphabetic() || c == '.');
        let suffix = &w[core.len()..];
        let ampm = matches!(suffix, "am" | "pm" | "a.m" | "p.m" | "a.m." | "p.m.");
        let (h, m) = match core.split_once([':', '.', 'h']) {
            Some((h, m)) if m.len() == 2 => (h, Some(m)),
            Some(_) => continue,
            None => (core, None),
        };
        if h.is_empty() || h.len() > 2 || !h.chars().all(|c| c.is_ascii_digit()) {
            continue;
        }
        if m.is_some_and(|m| !m.chars().all(|c| c.is_ascii_digit())) {
            continue;
        }
        let next = words.get(k + 1).map(String::as_str);
        let next_ampm = matches!(next, Some("am" | "pm" | "a.m" | "p.m"));
        let prev = k.checked_sub(1).map(|p| words[p].as_str());
        let led = matches!(prev, Some("at" | "alle" | "ore" | "h" | "dalle" | "verso" | "around"));
        if !(ampm || next_ampm || m.is_some() || led) {
            continue;
        }
        let phrase = match (led, next_ampm) {
            (_, true) => format!("{w} {}", next.unwrap_or_default()),
            _ => w.clone(),
        };
        // `read_time` reads a bare 1–7 as the afternoon only when nothing says otherwise.
        if let Some(t) = read_time(&phrase) {
            out.push((t, if next_ampm { k + 2 } else { k + 1 }));
        }
    }
    out
}

const MONTHS: [(&str, u8); 24] = [
    ("gennaio", 1),
    ("febbraio", 2),
    ("marzo", 3),
    ("aprile", 4),
    ("maggio", 5),
    ("giugno", 6),
    ("luglio", 7),
    ("agosto", 8),
    ("settembre", 9),
    ("ottobre", 10),
    ("novembre", 11),
    ("dicembre", 12),
    ("january", 1),
    ("february", 2),
    ("march", 3),
    ("april", 4),
    ("may", 5),
    ("june", 6),
    ("july", 7),
    ("august", 8),
    ("september", 9),
    ("october", 10),
    ("november", 11),
    ("december", 12),
];

fn month_of(word: &str) -> Option<u8> {
    let w = word.trim_end_matches('.');
    MONTHS.iter().find_map(|(name, n)| {
        let full = *name == w;
        // Three-letter forms: "ott", "oct", "sett", "sep"/"sept". "mar" is a month only where a day
        // number sits beside it — `read_day` asks that before treating it as Tuesday.
        let short = w.len() >= 3 && name.starts_with(w) && w != "ma";
        (full || short).then_some(*n)
    })
}

fn weekday_of(word: &str) -> Option<Weekday> {
    let w = word.trim_end_matches('.');
    let table: [(&str, Weekday); 14] = [
        ("lunedi", Weekday::Monday),
        ("martedi", Weekday::Tuesday),
        ("mercoledi", Weekday::Wednesday),
        ("giovedi", Weekday::Thursday),
        ("venerdi", Weekday::Friday),
        ("sabato", Weekday::Saturday),
        ("domenica", Weekday::Sunday),
        ("monday", Weekday::Monday),
        ("tuesday", Weekday::Tuesday),
        ("wednesday", Weekday::Wednesday),
        ("thursday", Weekday::Thursday),
        ("friday", Weekday::Friday),
        ("saturday", Weekday::Saturday),
        ("sunday", Weekday::Sunday),
    ];
    table
        .iter()
        .find_map(|(name, d)| (*name == w || (w.len() >= 3 && name.starts_with(w))).then_some(*d))
}

/// The next `w` strictly after `from` — "giovedì" said on a Thursday means next week's.
fn next_weekday(from: Date, w: Weekday) -> Date {
    let mut d = from + Duration::days(1);
    while d.weekday() != w {
        d += Duration::days(1);
    }
    d
}

/// A day phrase (already [`loose`]ed) → a date, counting from `anchor` (the day the message was sent).
/// `None` for anything not read exactly. European order for numeric dates (`16/10` is 16 October).
pub fn read_day(phrase: &str, anchor: Date) -> Option<Date> {
    let words: Vec<String> = phrase
        .split(|c: char| c.is_whitespace() || c == ',')
        .map(|w| {
            w.trim_matches(|c: char| c == '.' || c == '\'' || c == '"' || c == '(' || c == ')')
        })
        .filter(|w| !w.is_empty())
        .map(|w| {
            // "16th" → "16", "l'11" → "11"
            let w = w.rsplit('\'').next().unwrap_or(w);
            let digits = w.trim_end_matches(|c: char| c.is_ascii_alphabetic());
            if !digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit()) {
                digits.to_string()
            } else {
                w.to_string()
            }
        })
        .collect();

    // Relative words first.
    let has = |w: &str| words.iter().any(|x| x == w);
    if has("dopodomani") || phrase.contains("day after tomorrow") {
        return Some(anchor + Duration::days(2));
    }
    if has("domani") || has("tomorrow") {
        return Some(anchor + Duration::days(1));
    }
    if has("oggi") || has("today") || phrase.contains("stasera") || phrase.contains("tonight") {
        return Some(anchor);
    }

    // Numeric forms: 2026-10-16, 16/10, 16/10/2026, 16.10, 16-10-26.
    for w in &words {
        let parts: Vec<&str> = w.split(['/', '.', '-']).collect();
        if parts.len() >= 2
            && parts.iter().all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()))
        {
            let n: Vec<i32> = parts.iter().filter_map(|p| p.parse().ok()).collect();
            let (y, m, d) = if parts[0].len() == 4 {
                (Some(n[0]), n[1], *n.get(2)?)
            } else {
                let y = n.get(2).map(|y| if *y < 100 { 2000 + y } else { *y });
                (y, n[1], n[0])
            };
            return dated(d, m, y, anchor);
        }
    }

    let number = words.iter().find_map(|w| w.parse::<i32>().ok().filter(|n| (1..=31).contains(n)));
    let year =
        words.iter().find_map(|w| w.parse::<i32>().ok().filter(|n| (2000..=2100).contains(n)));
    let month =
        words.iter().find_map(|w| if w == "mar" && number.is_none() { None } else { month_of(w) });
    let weekday =
        words.iter().find_map(|w| if month.is_some() && w == "mar" { None } else { weekday_of(w) });

    match (number, month, weekday) {
        // "16 ottobre", "October 16", "giovedì 16 ottobre 2026".
        (Some(d), Some(m), wd) => {
            let date = dated(d, i32::from(m), year, anchor)?;
            // A stated weekday that disagrees with the date is a mistake in the email; refuse rather
            // than pick one of the two.
            wd.is_none_or(|w| w == date.weekday()).then_some(date)
        }
        // "giovedì 16": the next 16th that is a Thursday, within a year.
        (Some(d), None, Some(w)) => (0..13).find_map(|k| {
            let c = add_months(anchor, k)?;
            let date = Date::from_calendar_date(c.year(), c.month(), u8::try_from(d).ok()?).ok()?;
            (date >= anchor && date.weekday() == w).then_some(date)
        }),
        // "il 16": the next 16th on or after the day it was written.
        (Some(d), None, None)
            if words.iter().any(|w| w == "il" || w == "the" || w == "on") || words.len() == 1 =>
        {
            (0..13).find_map(|k| {
                let c = add_months(anchor, k)?;
                Date::from_calendar_date(c.year(), c.month(), u8::try_from(d).ok()?)
                    .ok()
                    .filter(|x| *x >= anchor)
            })
        }
        // "next week on friday", "venerdì della settimana prossima": that weekday of the FOLLOWING
        // week — written on Thursday 15 Oct, Friday 23 Oct, not the next day (a user's mail,
        // 2026-10-15).
        (None, None, Some(w))
            if (has("week") && has("next"))
                || (has("settimana") && (has("prossima") || has("prossimo"))) =>
        {
            let monday =
                anchor + Duration::days(7 - i64::from(anchor.weekday().number_days_from_monday()));
            Some(monday + Duration::days(i64::from(w.number_days_from_monday())))
        }
        // "giovedì", "lunedì prossimo", "next Monday".
        (None, None, Some(w)) => Some(next_weekday(anchor, w)),
        _ => None,
    }
}

/// Day `d` of month `m`, in `year` or — with no year — the first such date on or after `anchor`.
fn dated(d: i32, m: i32, year: Option<i32>, anchor: Date) -> Option<Date> {
    let month = Month::try_from(u8::try_from(m).ok()?).ok()?;
    let day = u8::try_from(d).ok()?;
    match year {
        Some(y) => Date::from_calendar_date(y, month, day).ok(),
        None => {
            let this = Date::from_calendar_date(anchor.year(), month, day).ok();
            match this {
                Some(t) if t >= anchor => Some(t),
                _ => Date::from_calendar_date(anchor.year() + 1, month, day).ok(),
            }
        }
    }
}

fn add_months(d: Date, k: i32) -> Option<Date> {
    let m0 = i32::from(u8::from(d.month())) - 1 + k;
    Date::from_calendar_date(
        d.year() + m0.div_euclid(12),
        Month::try_from((m0.rem_euclid(12) + 1) as u8).ok()?,
        1,
    )
    .ok()
}

/// A time phrase (already [`loose`]ed) → a wall-clock time. `15`, `15:30`, `15.30`, `15h30`, `alle 3`,
/// `3pm`, `3:30 pm`, `mezzogiorno`, `noon`.
///
/// **A bare hour from 1 to 7 is read as afternoon** (`alle 3` → 15:00): nobody fixes a meeting at three
/// in the morning, and it is how people write. `8`–`12` stay as written. `am`/`pm` always win.
pub fn read_time(phrase: &str) -> Option<Time> {
    if phrase.contains("mezzogiorno") || phrase.contains("noon") {
        return Time::from_hms(12, 0, 0).ok();
    }
    let pm = phrase.contains("pm")
        || phrase.contains("p.m")
        || phrase.contains("del pomeriggio")
        || phrase.contains("di sera");
    let am = phrase.contains("am") && !phrase.contains("pm")
        || phrase.contains("a.m")
        || phrase.contains("del mattino");
    let digits: String = phrase.chars().skip_while(|c| !c.is_ascii_digit()).collect();
    let mut nums = digits.split(|c: char| !c.is_ascii_digit()).filter(|p| !p.is_empty());
    let h: u8 = nums.next()?.parse().ok()?;
    let sep_follows =
        digits.trim_start_matches(|c: char| c.is_ascii_digit()).starts_with([':', '.', 'h']);
    let m: u8 = if sep_follows { nums.next().and_then(|n| n.parse().ok()).unwrap_or(0) } else { 0 };
    let h = match (h, pm, am) {
        (1..=11, true, _) => h + 12,
        (12, _, true) => 0,
        (1..=7, false, false) => h + 12,
        _ => h,
    };
    Time::from_hms(h, m, 0).ok()
}

/// What to propose for one conversation.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Plan {
    /// The meeting the conversation note itself should stand for, when it does not already.
    pub conversation: Option<Meeting>,
    /// Further meetings, each to become its own proposed meeting note.
    pub new: Vec<Meeting>,
    /// A new email calls off the meeting the conversation stands for, and fixes no other time: the
    /// note is proposed marked cancelled — kept, with its date, never deleted.
    pub cancel: Option<Meeting>,
}

/// A meeting link — `https://…` — rather than a room or a word like "online".
fn is_link(place: &str) -> bool {
    place.starts_with("https://") || place.starts_with("http://")
}

/// Decide what to propose.
///
/// - `current`: the conversation note's own `start`, and whether it is still ahead of us.
/// - `taken`: starts already on the agenda (invitations, accepted meetings).
/// - `linked`: starts of meeting notes already linked to this conversation.
/// - `proposed`: starts this pass proposed before — accepted, rejected or waiting. A rejection is an
///   answer, and is not asked again.
///
/// The conversation note takes the first meeting. **If the exchange no longer fixes the time the note
/// stands for** (a later email moved it) and that time is still ahead, the note is proposed *moving*
/// to the first meeting the exchange now fixes. A time already past is history and is never moved.
/// Every further meeting becomes a proposed meeting note.
pub fn plan(
    meetings: Vec<Meeting>,
    current: Option<Current<'_>>,
    since: Option<&str>,
    taken: &HashSet<String>,
    linked: &HashSet<String>,
    proposed: &HashSet<String>,
) -> Plan {
    let is_new =
        |m: &Meeting| since.is_some_and(|since| m.sent_at.as_deref().is_some_and(|a| a > since));
    let (cancels, mut meetings): (Vec<Meeting>, Vec<Meeting>) =
        meetings.into_iter().partition(|m| m.cancelled);
    // **Restated without its time, it is the same meeting** (2026-10-24): "see you on the 30th, same
    // link", on a day that already holds one of this conversation's meetings, takes that meeting's
    // time. Read as the whole day, it looked like a move and was proposed as one. Another
    // conversation's meeting (`taken`) is not this one.
    for m in meetings.iter_mut().filter(|m| m.start.is_none()) {
        let day = stamp(m.day, None);
        let ours = current.map(|c| c.start).into_iter().chain(linked.iter().map(String::as_str));
        let at = ours
            .chain(proposed.iter().map(String::as_str))
            .filter(|s| s.len() >= 16 && s.starts_with(&day))
            .min()
            .and_then(|s| Some((s.get(11..13)?.parse().ok()?, s.get(14..16)?.parse().ok()?)))
            .and_then(|(h, mi)| Time::from_hms(h, mi, 0).ok());
        if at.is_some() {
            m.start = at;
        }
    }
    // What the emails that arrived since the last read fix, by start — taken before the dedup below,
    // which keeps whichever copy sorts first and so may keep the older sentence. On a first read
    // (`since` unknown) nothing counts as new, and the rules are exactly those of the history.
    // The last email fixes a time of its own ("can't make 12 — Friday?"): then it is a move, never a
    // call-off.
    let latest_fixes = current
        .and_then(|c| c.latest)
        .is_some_and(|l| meetings.iter().any(|m| m.sent_at.as_deref() == Some(l)));
    let mut fresh: HashMap<String, Meeting> = HashMap::new();
    for m in meetings.iter().filter(|m| is_new(m)) {
        fresh.insert(m.start_stamp(), m.clone());
    }
    meetings.sort_by_key(|m| (m.day, m.start));
    meetings.dedup_by_key(|m| m.start_stamp());
    let found: HashSet<String> = meetings.iter().map(Meeting::start_stamp).collect();
    let asked = |s: &str| linked.contains(s) || proposed.contains(s) || taken.contains(s);
    let mut out = Plan::default();
    // May the conversation note take a meeting? When undated, or when its upcoming time is gone.
    let mut open = match current {
        None => true,
        Some(c) => c.upcoming && !found.contains(c.start),
    };
    if let Some(c) = current.filter(|c| c.upcoming) {
        if let Some(again) = fresh.get(c.start) {
            // A new email names the same time again. If it gives a place the note does not have (a
            // meeting link sent later), the note is proposed that place; the time stands. A word like
            // "online" never replaces a link the note already has.
            let place = again
                .place
                .as_deref()
                .filter(|p| Some(*p) != c.place)
                .filter(|p| is_link(p) || !c.place.is_some_and(is_link));
            if place.is_some() {
                out.conversation = Some(again.clone());
            }
        } else if !open {
            // A new email fixes another time and does not repeat this one: the meeting moved. The
            // old sentence is still in the history above it, which is why `found` alone cannot tell.
            open = fresh.values().any(|m| !asked(&m.start_stamp()));
        }
    }
    let known = |s: &str| current.is_some_and(|c| c.start == s) || asked(s);
    // When a new email fixes a meeting not yet on the agenda, that is the one the note takes.
    let prefer_fresh = fresh.keys().any(|s| !known(s));
    for m in meetings {
        let s = m.start_stamp();
        if known(&s) {
            continue;
        }
        if open && (!prefer_fresh || fresh.contains_key(&s)) {
            // Taken from the fresh copy, so the note quotes the email that moved it.
            out.conversation = Some(fresh.remove(&s).unwrap_or(m));
            open = false;
        } else {
            out.new.push(m);
        }
    }
    // Called off, with no new time fixed for it: by an email that arrived since the last read — or, on
    // a re-read with no "since" (a change of rules), by the conversation's LAST email, so a call-off
    // that is still the latest word is not missed, while an old one followed by more mail never
    // counts. The sentence must name this meeting's time, or name none ("I can't make it").
    if let Some(c) = current.filter(|c| c.upcoming) {
        let latest_word = |m: &Meeting| {
            since.is_none()
                && !latest_fixes
                && c.latest.is_some_and(|l| m.sent_at.as_deref() == Some(l))
        };
        if out.conversation.is_none() && fresh.is_empty() {
            out.cancel = cancels
                .into_iter()
                .filter(|m| is_new(m) || latest_word(m))
                .find(|m| !m.dated || m.start_stamp() == c.start || m.day.to_string() == c.start);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_shared_mail_prompt_is_one_system_prompt_for_both_questions() {
        let body = "### Lena Okafor — 2026-10-13 09:10\n\nCan we meet on Thursday at 3pm?\n";
        let (ms, mu) = prompt(true, Question::Meetings, "Thesis", body, "Tom Reyes");
        let (ts, tu) = prompt(true, Question::Title, "Thesis", body, "Tom Reyes");
        assert_eq!(ms, ts, "the title call reuses the meetings call's cached prompt");
        assert!(ms.contains(crate::prompts::MEETINGS_INSTRUCTION) && ms.contains("Tom Reyes"));
        assert!(mu.ends_with("Question: meetings") && tu.ends_with("Question: title"));
        let shared_prefix = mu.trim_end_matches("Question: meetings");
        assert!(tu.starts_with(shared_prefix), "the emails are the same prefix");
        // Apart: each question's own instruction is the system prompt, as before 2026-10-19.
        let (ms, mu) = prompt(false, Question::Meetings, "Thesis", body, "Tom Reyes");
        let (ts, tu) = prompt(false, Question::Title, "Thesis", body, "Tom Reyes");
        assert_eq!(ms, crate::prompts::MEETINGS_INSTRUCTION);
        assert_eq!(ts, crate::titles::instruction("Tom Reyes"));
        assert_eq!(
            (mu.as_str(), tu.as_str()),
            (user_prompt("Thesis", body).as_str(), user_prompt("Thesis", body).as_str())
        );
    }
    use time::macros::{date, time};

    fn d(p: &str, anchor: Date) -> Option<Date> {
        read_day(&loose(p), anchor)
    }

    #[test]
    fn italian_and_english_days_are_read_from_the_day_the_email_was_sent() {
        let sent = date!(2026 - 10 - 06); // Tuesday
        assert_eq!(sent.weekday(), Weekday::Tuesday);
        assert_eq!(d("giovedì", sent), Some(date!(2026 - 10 - 08)));
        assert_eq!(d("Giovedì prossimo", sent), Some(date!(2026 - 10 - 08)));
        assert_eq!(d("next Monday", sent), Some(date!(2026 - 10 - 12)));
        assert_eq!(
            d("martedì", sent),
            Some(date!(2026 - 10 - 13)),
            "said on a Tuesday: next week's"
        );
        assert_eq!(d("domani", sent), Some(date!(2026 - 10 - 07)));
        assert_eq!(d("dopodomani", sent), Some(date!(2026 - 10 - 08)));
        assert_eq!(d("16 ottobre", sent), Some(date!(2026 - 10 - 16)));
        let thu = date!(2026 - 10 - 15);
        assert_eq!(d("next week on friday", thu), Some(date!(2026 - 10 - 23)), "not tomorrow");
        assert_eq!(d("venerdì della settimana prossima", thu), Some(date!(2026 - 10 - 23)));
        assert_eq!(d("la prossima settimana lunedì", thu), Some(date!(2026 - 10 - 19)));
        assert_eq!(d("next friday", thu), Some(date!(2026 - 10 - 16)), "as before: the next one");
        assert_eq!(d("venerdì 16 ottobre", sent), Some(date!(2026 - 10 - 16)));
        assert_eq!(d("October 16th", sent), Some(date!(2026 - 10 - 16)));
        assert_eq!(d("16 ott", sent), Some(date!(2026 - 10 - 16)));
        assert_eq!(d("16/10", sent), Some(date!(2026 - 10 - 16)));
        assert_eq!(d("2/3/2027", sent), Some(date!(2027 - 03 - 02)), "European order");
        assert_eq!(d("2026-11-04", sent), Some(date!(2026 - 11 - 04)));
        assert_eq!(d("il 3", sent), Some(date!(2026 - 11 - 03)), "the next 3rd");
        assert_eq!(d("5 gennaio", sent), Some(date!(2027 - 01 - 05)), "already past this year");
        assert_eq!(d("mar 10 marzo", sent), Some(date!(2027 - 03 - 10)));
    }

    #[test]
    fn words_that_are_not_read_exactly_are_refused() {
        let sent = date!(2026 - 10 - 06);
        assert_eq!(d("la settimana prossima", sent), None);
        assert_eq!(d("presto", sent), None);
        assert_eq!(
            d("giovedì 16 ottobre", sent),
            None,
            "16 October 2026 is a Friday: the email contradicts itself"
        );
    }

    #[test]
    fn times_in_both_languages() {
        let t = |p: &str| read_time(&loose(p));
        assert_eq!(t("alle 15"), Some(time!(15:00)));
        assert_eq!(t("ore 15:30"), Some(time!(15:30)));
        assert_eq!(t("15.30"), Some(time!(15:30)));
        assert_eq!(t("alle 3"), Some(time!(15:00)), "a bare 3 is the afternoon");
        assert_eq!(t("alle 10"), Some(time!(10:00)));
        assert_eq!(t("3pm"), Some(time!(15:00)));
        assert_eq!(t("9:30 am"), Some(time!(09:30)));
        assert_eq!(t("mezzogiorno"), Some(time!(12:00)));
        assert_eq!(t("nel pomeriggio"), None);
    }

    const BODY: &str = "### Maria Rossi — 2026-10-06 09:12\nTo: Bal\n\nCiao, ci vediamo giovedì alle 15 nel mio ufficio.\n\n\
                        ### Luca — 2026-10-07 11:00\n\nPerfetto. E il 28 alle 10 facciamo il punto con Anna.\n";

    fn found(quote: &str, date: &str, time: &str) -> Found {
        Found {
            title: "Riunione".into(),
            date: date.into(),
            time: time.into(),
            quote: quote.into(),
            ..Default::default()
        }
    }

    #[test]
    fn a_quoted_meeting_is_dated_from_its_own_message() {
        let f = found("ci vediamo giovedì alle 15 nel mio ufficio.", "giovedì", "alle 15");
        let m = resolve(&f, BODY, date!(2026 - 10 - 06), None).unwrap();
        assert_eq!(m.start_stamp(), "2026-10-08T15:00");
        // The second meeting is counted from Luca's message (the 7th), not Maria's.
        let f = found("E il 28 alle 10 facciamo il punto con Anna.", "il 28", "alle 10");
        assert_eq!(
            resolve(&f, BODY, date!(2026 - 10 - 06), None).unwrap().start_stamp(),
            "2026-10-28T10:00"
        );
    }

    /// 2026-10-16, a real email: the model quoted the opening of a long email, "...", and the sentence
    /// that fixes the meeting. Each piece is in the email as written; the whole is not — so the meeting
    /// was dropped and a move never proposed. The dated piece stands; an invented piece still fails.
    #[test]
    fn a_quote_shortened_with_dots_stands_on_its_dated_piece() {
        let body = "### Anna — 2026-10-16 17:34\n\nDear Kai,\n\nas discussed f2f, I suggest the following \
                    for the next steps.\n\nThanks and see you on the 30th of Friday at 16:30 Tokyo \
                    time online.\n";
        let f = found(
            "as discussed f2f, I suggest the following ... Thanks and see you on the 30th of Friday at 16:30 Tokyo time online.",
            "30th of October",
            "16:30",
        );
        let m = resolve(&f, body, date!(2026 - 10 - 16), None).unwrap();
        assert_eq!(m.start_stamp(), "2026-10-30T16:30");
        assert_eq!(m.quote, "Thanks and see you on the 30th of Friday at 16:30 Tokyo time online.");
        // The same, with "…".
        let f = found("as discussed f2f … see you on the 30th of Friday at 16:30", "", "");
        assert_eq!(
            resolve(&f, body, date!(2026 - 10 - 16), None).unwrap().start_stamp(),
            "2026-10-30T16:30"
        );
        // A piece that is in no email sinks the whole quote, as before.
        let f = found("as discussed f2f ... see you on the 2nd at 9 in the library", "", "");
        assert_eq!(resolve(&f, body, date!(2026 - 10 - 16), None), Err(Dropped::NotInEmail));
    }

    /// Paolo's reply, 2026-10-15: the model wrote "16th **of** October" where the email says "16th
    /// October". The email's own sentence stands; an invented one is still refused.
    #[test]
    fn a_near_copy_stands_as_the_emails_own_sentence_and_an_invention_does_not() {
        let body = "### Anna — 2026-10-15 17:40\n\nI have to move it online tomorrow at 16:30.\n\n\
                    ### Paolo Conti — 2026-10-15 20:51\n\nThank you. Tomorrow (16th October) at 16:30 \
                    Tokyo time works well for me. I'll join via the Zoom link.\n";
        let f = found(
            "Tomorrow (16th of October) at 16:30 Tokyo time works well for me. I'll join via the Zoom link.",
            "16th of October",
            "16:30",
        );
        let m = resolve(&f, body, date!(2026 - 10 - 15), None).unwrap();
        assert_eq!(m.start_stamp(), "2026-10-16T16:30");
        assert_eq!(
            m.quote,
            "Tomorrow (16th October) at 16:30 Tokyo time works well for me. I'll join via the Zoom link."
        );
        assert_eq!(m.said_by.as_deref(), Some("Paolo Conti"));

        let f = found("Tomorrow at 9 we meet in the library to finish it.", "tomorrow", "9");
        assert_eq!(resolve(&f, body, date!(2026 - 10 - 15), None), Err(Dropped::NotInEmail));
    }

    #[test]
    fn a_meeting_carries_when_its_email_was_sent() {
        let f = found("ci vediamo giovedì alle 15 nel mio ufficio.", "giovedì", "alle 15");
        let m = resolve(&f, BODY, date!(2026 - 10 - 06), None).unwrap();
        assert_eq!(m.sent_at.as_deref(), Some("2026-10-06T09:12"));
        let f = found("E il 28 alle 10 facciamo il punto con Anna.", "il 28", "alle 10");
        let m = resolve(&f, BODY, date!(2026 - 10 - 06), None).unwrap();
        assert_eq!(m.sent_at.as_deref(), Some("2026-10-07T11:00"), "Luca's message, the newer one");
    }

    #[test]
    fn an_invented_quote_or_a_past_meeting_is_dropped() {
        let f = found("ci vediamo venerdì alle 9", "venerdì", "alle 9");
        assert_eq!(resolve(&f, BODY, date!(2026 - 10 - 06), None), Err(Dropped::NotInEmail));
        let f = found("ci vediamo giovedì alle 15 nel mio ufficio.", "giovedì", "alle 15");
        assert_eq!(resolve(&f, BODY, date!(2026 - 10 - 20), None), Err(Dropped::Past));
    }

    /// **A user's mail (2026-10-19):** a reply after the day's call names that call's time again;
    /// larger models reported it beside the next meeting, and the day rule let it through.
    #[test]
    fn a_meeting_already_begun_when_the_newest_email_was_sent_is_past() {
        let body = "### Ana Ruiz — 2026-10-16 09:00\n\nSee you today at 5pm. Next we meet on Friday \
                    23 October at 4pm.\n\n### Owner Name — 2026-10-16 17:31\n\nThanks for the call.\n";
        let today = date!(2026 - 10 - 16);
        let held = found("See you today at 5pm.", "today", "5pm");
        assert_eq!(resolve(&held, body, today, None), Err(Dropped::Past));
        let next = found("Next we meet on Friday 23 October at 4pm.", "Friday 23 October", "4pm");
        assert_eq!(resolve(&next, body, today, None).unwrap().start_stamp(), "2026-10-23T16:00");
        // Before the reply, the same call was still ahead.
        let before = body.split("### Owner").next().unwrap();
        assert_eq!(resolve(&held, before, today, None).unwrap().start_stamp(), "2026-10-16T17:00");
    }

    /// **The case that failed on a user's real mail (2026-10-09).** The date is in the sentence
    /// before the quote, the room number looks like a date, and the model's own day words were a
    /// translation ("16 ottobre") of what the email said.
    #[test]
    fn that_day_is_read_from_the_email_before_the_quote_and_a_room_is_not_a_date() {
        let body = "### Ana Ruiz — 2026-10-02 17:37\n\nWhen can we reschedule?\n\n\
                    ### Owner Name — 2026-10-02 17:51\nTo: ANA RUIZ\n\n\
                    Sorry for the delay. I should be back at the office by the 16th of October. We can have\n\
                    a chat directly that day at 12pm in RM 01-02.\n";
        let f = Found {
            title: "Meeting with Ana".into(),
            date: "16 ottobre".into(),
            time: "12pm".into(),
            place: "RM 01-02".into(),
            quote: "We can have a chat directly that day at 12pm in RM 01-02.".into(),
            ..Default::default()
        };
        let m = resolve(&f, body, date!(2026 - 10 - 09), None).unwrap();
        assert_eq!(m.start_stamp(), "2026-10-16T12:00");
        assert_eq!(m.place.as_deref(), Some("RM 01-02"));
    }

    #[test]
    fn the_sentences_own_date_wins_over_whatever_the_model_said() {
        let f = found("E il 28 alle 10 facciamo il punto con Anna.", "domani", "alle 10");
        assert_eq!(
            resolve(&f, BODY, date!(2026 - 10 - 06), None).unwrap().start_stamp(),
            "2026-10-28T10:00"
        );
    }

    #[test]
    fn dates_and_times_are_found_in_running_text() {
        let a = date!(2026 - 09 - 18);
        assert_eq!(
            find_day(&loose("P.s. 2nd of October 2026 at 11:30am Tokyo time."), a, false),
            Some(date!(2026 - 10 - 02))
        );
        assert_eq!(
            find_time(&loose("2nd of October 2026 at 11:30am Tokyo time")),
            Some(time!(11:30))
        );
        assert_eq!(
            find_day(&loose("We can try next week Monday 28th of Sep at Tokyo 5pm:"), a, false),
            Some(date!(2026 - 09 - 28))
        );
        assert_eq!(find_time(&loose("Monday 28th of Sep at Tokyo 5pm:")), Some(time!(17:00)));
        assert_eq!(find_day(&loose("room RM 01-02, project 2999-00001, 15.30"), a, false), None);
        assert_eq!(find_time(&loose("project AB1234X 2999-00001 for 3 students")), None);
        assert_eq!(
            find_day(&loose("ci vediamo il 16 alle 15"), a, false),
            Some(date!(2026 - 10 - 16))
        );
    }

    #[test]
    fn an_unanswered_question_fixes_nothing_and_an_answered_one_may() {
        let ask = "### Kim — 2026-10-13 09:00\n\nCan we meet on Thursday at 3pm?\n";
        let today = Date::from_calendar_date(2026, Month::October, 13).unwrap();
        let found = Found {
            status: "fixed".into(),
            quote: "Can we meet on Thursday at 3pm?".into(),
            ..Found::default()
        };
        assert_eq!(resolve(&found, ask, today, None), Err(Dropped::Unanswered));
        // Kim writing again is not an answer.
        let again = format!("{ask}\n### Kim — 2026-10-13 12:00\n\nAny news?\n");
        assert_eq!(resolve(&found, &again, today, None), Err(Dropped::Unanswered));
        let answered = format!("{ask}\n### Lee — 2026-10-13 10:00\n\nSure, see you then!\n");
        let m = resolve(&found, &answered, today, None).unwrap();
        assert_eq!(m.start_stamp(), "2026-10-15T15:00");
    }

    #[test]
    fn a_quote_of_two_sentences_is_read_in_the_one_the_models_words_point_at() {
        let body =
            "### Eva — 2026-10-14 11:11\n\nThe kick-off is on Monday at 10am. The review will \
                    be on Friday at 2pm.\n";
        let both = "The kick-off is on Monday at 10am. The review will be on Friday at 2pm.";
        let today = Date::from_calendar_date(2026, Month::October, 14).unwrap();
        let found = |date: &str, time: &str| Found {
            status: "fixed".into(),
            date: date.into(),
            time: time.into(),
            quote: both.into(),
            ..Found::default()
        };
        let review = resolve(&found("Friday", "2pm"), body, today, None).unwrap();
        assert_eq!(review.start_stamp(), "2026-10-16T14:00");
        let kickoff = resolve(&found("Monday", "10am"), body, today, None).unwrap();
        assert_eq!(kickoff.start_stamp(), "2026-10-19T10:00");
        assert_eq!(review.quote, both, "the quote shown is the one the model gave");
        // Words that point at no single sentence leave the quote whole, read as before.
        let vague = resolve(&found("", ""), body, today, None).unwrap();
        assert_eq!(vague.start_stamp(), "2026-10-19T10:00");
    }

    #[test]
    fn the_time_comes_from_the_sentence_and_none_there_means_the_day() {
        // The model said 16; the sentence says 15. The sentence wins.
        let f = found("ci vediamo giovedì alle 15 nel mio ufficio.", "giovedì", "alle 16");
        assert_eq!(
            resolve(&f, BODY, date!(2026 - 10 - 06), None).unwrap().start_stamp(),
            "2026-10-08T15:00"
        );
        // No time in the sentence: a meeting for the day, not a guessed hour.
        let body = "### Maria — 2026-10-06 09:12\n\nPassa da me giovedì.\n";
        let f = found("Passa da me giovedì.", "giovedì", "alle 16");
        let m = resolve(&f, body, date!(2026 - 10 - 06), None).unwrap();
        assert_eq!(m.start, None);
        assert_eq!(m.start_stamp(), "2026-10-08");
    }

    #[test]
    fn a_long_exchange_is_cut_from_the_front_to_fit_the_window() {
        let line =
            "### Li — 2026-10-02 11:52\n回复：好的，我们周四下午三点在RM 01-02见面。谢谢老师！\n";
        let body = line.repeat(400);
        let p = user_prompt("t", &body);
        assert!(tokens_of(&p) <= TEXT_TOKENS + 10, "{} tokens", tokens_of(&p));
        assert!(p.ends_with(line), "the latest messages are kept");
        assert!(p.contains("\n### Li"), "cut on a line boundary");
        assert_eq!(user_prompt("t", "short"), "Conversation: t\n\nshort");
    }

    #[test]
    fn the_answer_is_read_through_fences_and_thinking() {
        let a = "<think>hmm</think>Here you go:\n```json\n[{\"title\":\"R\",\"date\":\"giovedì\",\"time\":\"alle 15\",\"quote\":\"q\"}]\n```";
        assert_eq!(parse(a).len(), 1);
        assert_eq!(parse("[]"), vec![]);
        assert_eq!(parse("no meetings"), vec![]);
        assert_eq!(parse("{\"title\":\"R\",\"date\":\"d\",\"quote\":\"q\"}").len(), 1);
    }

    fn meeting(stamp_day: Date, h: u8) -> Meeting {
        Meeting {
            title: "m".into(),
            day: stamp_day,
            start: Time::from_hms(h, 0, 0).ok(),
            end: None,
            place: None,
            quote: "q".into(),
            sent_at: None,
            said_by: None,
            cancelled: false,
            dated: true,
            written: None,
        }
    }

    fn cur<'a>(start: &'a str, upcoming: bool, place: Option<&'a str>) -> Option<Current<'a>> {
        Some(Current { start, upcoming, place, latest: None })
    }

    /// A meeting from an email sent at `at`.
    fn said(day: Date, h: u8, at: &str, place: Option<&str>) -> Meeting {
        Meeting { sent_at: Some(at.into()), place: place.map(Into::into), ..meeting(day, h) }
    }

    /// A user's case: 16 Oct 14:00 fixed on 2 Oct, still in the history, and a reply today.
    const READ: Option<&str> = Some("2026-10-02T17:52");
    const OLD: &str = "2026-10-02T17:52";
    const NEW: &str = "2026-10-15T17:46";

    #[test]
    fn a_new_email_that_moves_the_meeting_moves_the_note_though_the_old_time_is_still_quoted() {
        let none = HashSet::new();
        let asked: HashSet<String> = ["2026-10-16T14:00".to_string()].into();
        let p = plan(
            vec![
                said(date!(2026 - 10 - 16), 14, OLD, Some("B2-14")),
                said(date!(2026 - 10 - 16), 17, NEW, None),
            ],
            cur("2026-10-16T14:00", true, Some("B2-14")),
            READ,
            &none,
            &none,
            &asked,
        );
        assert_eq!(
            p.conversation.unwrap().start_stamp(),
            "2026-10-16T17:00",
            "moved, not a 2nd note"
        );
        assert!(p.new.is_empty());
    }

    #[test]
    fn a_new_email_with_the_same_time_and_a_link_proposes_the_link_as_the_place() {
        let none = HashSet::new();
        let asked: HashSet<String> = ["2026-10-16T14:00".to_string()].into();
        let link = "https://zoom.us/j/1234567890";
        let p = plan(
            vec![
                said(date!(2026 - 10 - 16), 14, OLD, Some("B2-14")),
                said(date!(2026 - 10 - 16), 14, NEW, Some(link)),
            ],
            cur("2026-10-16T14:00", true, Some("B2-14")),
            READ,
            &none,
            &none,
            &asked,
        );
        let m = p.conversation.expect("the note is proposed its new place");
        assert_eq!(m.start_stamp(), "2026-10-16T14:00", "the time stands");
        assert_eq!(m.place.as_deref(), Some(link));
        assert!(p.new.is_empty());

        // The same place again, or none at all, proposes nothing.
        for place in [Some("B2-14"), None] {
            let p = plan(
                vec![said(date!(2026 - 10 - 16), 14, NEW, place)],
                cur("2026-10-16T14:00", true, Some("B2-14")),
                READ,
                &none,
                &none,
                &asked,
            );
            assert_eq!(p, Plan::default(), "{place:?}");
        }
    }

    fn cancelling(day: Date, h: Option<u8>, at: &str) -> Meeting {
        let mut m = said(day, h.unwrap_or(0), at, None);
        m.cancelled = true;
        if h.is_none() {
            m.start = None;
            m.dated = false;
        }
        m
    }

    /// Greta, 15 Oct 14:19: "won't be able to make our meeting at 12 pm" tomorrow, no new time yet.
    #[test]
    fn a_new_email_calling_the_meeting_off_proposes_marking_it_cancelled() {
        let none = HashSet::new();
        let asked: HashSet<String> = ["2026-10-16T12:00".to_string()].into();
        let cur = || cur("2026-10-16T12:00", true, Some("B2-14"));
        let p = plan(
            vec![cancelling(date!(2026 - 10 - 16), Some(12), "2026-10-15T14:19")],
            cur(),
            Some("2026-10-02T17:51"),
            &none,
            &none,
            &asked,
        );
        assert!(p.conversation.is_none() && p.new.is_empty());
        assert!(p.cancel.is_some(), "the 12:00 is called off");

        // "I can't make it" names no time: it is about the meeting the note stands for.
        let p = plan(
            vec![cancelling(date!(2026 - 10 - 15), None, "2026-10-15T14:19")],
            cur(),
            Some("2026-10-02T17:51"),
            &none,
            &none,
            &asked,
        );
        assert!(p.cancel.is_some());

        // Calling off ANOTHER meeting, or an old email doing so, changes nothing here.
        for (m, why) in [
            (cancelling(date!(2026 - 10 - 20), Some(9), "2026-10-15T14:19"), "another meeting"),
            (cancelling(date!(2026 - 10 - 16), Some(12), "2026-10-01T09:00"), "history"),
        ] {
            let p = plan(vec![m], cur(), Some("2026-10-02T17:51"), &none, &none, &asked);
            assert!(p.cancel.is_none(), "{why}");
        }
    }

    /// A re-read (no "since"): a call-off still counts when it is the conversation's last word, never
    /// when more mail followed it, nor when that last email also fixes a time.
    #[test]
    fn on_a_re_read_a_call_off_counts_only_as_the_latest_word() {
        let none = HashSet::new();
        let asked: HashSet<String> = ["2026-10-16T12:00".to_string()].into();
        let at = |latest: &'static str| {
            Some(Current {
                start: "2026-10-16T12:00",
                upcoming: true,
                place: None,
                latest: Some(latest),
            })
        };
        let off = || cancelling(date!(2026 - 10 - 16), Some(12), "2026-10-15T14:19");
        let p = plan(vec![off()], at("2026-10-15T14:19"), None, &none, &none, &asked);
        assert!(p.cancel.is_some(), "the last word");
        let p = plan(vec![off()], at("2026-10-15T15:33"), None, &none, &none, &asked);
        assert!(p.cancel.is_none(), "more mail came after it");
        let p = plan(
            vec![off(), said(date!(2026 - 10 - 23), 16, "2026-10-15T14:19", None)],
            at("2026-10-15T14:19"),
            None,
            &none,
            &none,
            &asked,
        );
        assert!(p.cancel.is_none(), "the same email fixes another time: a move");
    }

    /// Greta's whole exchange: she cancels, then a new time is proposed — that is a move, not a cancel.
    #[test]
    fn a_cancellation_followed_by_a_new_time_is_a_move() {
        let none = HashSet::new();
        let asked: HashSet<String> = ["2026-10-16T12:00".to_string()].into();
        let p = plan(
            vec![
                cancelling(date!(2026 - 10 - 16), Some(12), "2026-10-15T14:19"),
                said(date!(2026 - 10 - 23), 16, "2026-10-15T15:33", Some("probably online")),
            ],
            cur("2026-10-16T12:00", true, Some("B2-14")),
            Some("2026-10-02T17:51"),
            &none,
            &none,
            &asked,
        );
        assert_eq!(p.conversation.unwrap().start_stamp(), "2026-10-23T16:00");
        assert!(p.cancel.is_none());
    }

    /// Elena: "Meeting tomorrow at 5 p.m. online is fine" must not replace the Zoom link with "online".
    #[test]
    fn a_vaguer_place_never_replaces_a_link() {
        let none = HashSet::new();
        let asked: HashSet<String> = ["2026-10-16T17:00".to_string()].into();
        let link = "https://zoom.us/j/1234567890";
        let p = plan(
            vec![said(date!(2026 - 10 - 16), 17, "2026-10-15T17:49", Some("online"))],
            cur("2026-10-16T17:00", true, Some(link)),
            Some("2026-10-15T17:44"),
            &none,
            &none,
            &asked,
        );
        assert_eq!(p, Plan::default());
    }

    #[test]
    fn a_new_email_that_repeats_the_time_and_adds_another_meeting_moves_nothing() {
        let none = HashSet::new();
        let p = plan(
            vec![
                said(date!(2026 - 10 - 16), 14, NEW, None),
                said(date!(2026 - 10 - 30), 14, NEW, None),
            ],
            cur("2026-10-16T14:00", true, None),
            READ,
            &none,
            &none,
            &none,
        );
        assert!(p.conversation.is_none(), "the 16th still stands");
        assert_eq!(p.new.len(), 1, "the 30th is a further meeting");
    }

    #[test]
    fn history_alone_never_moves_the_note() {
        // Everything was read before: an older sentence with another time is not a move.
        let none = HashSet::new();
        let p = plan(
            vec![
                said(date!(2026 - 10 - 16), 14, OLD, None),
                said(date!(2026 - 10 - 20), 9, OLD, None),
            ],
            cur("2026-10-16T14:00", true, None),
            Some(NEW),
            &none,
            &none,
            &none,
        );
        assert!(p.conversation.is_none());
        assert_eq!(p.new.len(), 1);
    }

    #[test]
    fn the_conversation_takes_the_first_meeting_and_each_further_one_is_new() {
        let none = HashSet::new();
        let p = plan(
            vec![meeting(date!(2026 - 10 - 28), 10), meeting(date!(2026 - 10 - 08), 15)],
            None,
            None,
            &none,
            &none,
            &none,
        );
        assert_eq!(p.conversation.unwrap().start_stamp(), "2026-10-08T15:00");
        assert_eq!(p.new.len(), 1);
        assert_eq!(p.new[0].start_stamp(), "2026-10-28T10:00");
    }

    #[test]
    fn nothing_already_on_the_agenda_or_already_asked_is_proposed_again() {
        let ms = vec![meeting(date!(2026 - 10 - 08), 15), meeting(date!(2026 - 10 - 28), 10)];
        let taken: HashSet<String> = ["2026-10-08T15:00".to_string()].into();
        let asked: HashSet<String> = ["2026-10-28T10:00".to_string()].into();
        let none = HashSet::new();
        assert_eq!(plan(ms.clone(), None, None, &taken, &none, &asked), Plan::default());
        // The conversation already stands for the first one: only the second is new.
        let p = plan(ms, cur("2026-10-08T15:00", true, None), None, &none, &none, &none);
        assert!(p.conversation.is_none());
        assert_eq!(p.new.len(), 1);
    }

    #[test]
    fn a_later_email_that_moves_the_meeting_proposes_moving_the_note() {
        let none = HashSet::new();
        let p = plan(
            vec![meeting(date!(2026 - 10 - 09), 15)],
            cur("2026-10-08T15:00", true, None),
            None,
            &none,
            &none,
            &none,
        );
        assert_eq!(p.conversation.unwrap().start_stamp(), "2026-10-09T15:00");
        assert!(p.new.is_empty());
    }

    #[test]
    fn a_meeting_already_held_is_history_and_is_never_moved() {
        let none = HashSet::new();
        let p = plan(
            vec![meeting(date!(2026 - 10 - 28), 10)],
            cur("2026-10-01T15:00", false, None),
            None,
            &none,
            &none,
            &none,
        );
        assert!(p.conversation.is_none(), "the note keeps the meeting that took place");
        assert_eq!(p.new.len(), 1, "the next one is its own note");
    }

    // ── reading only what is new (2026-10-24) ──────────────────────────────────────────────────

    /// The shape that lost a meeting: weekly meetings, one moved, a long agenda email, then the next
    /// meeting proposed and accepted with "OK, I'm free at that time". Every name is invented.
    fn weekly(with_reply: bool) -> String {
        let agenda = "Here is the plan for the next weeks, as discussed. ".to_string()
            + &"We will go through the related work, the data, the method and the evaluation. "
                .repeat(60);
        let mut b = String::from(
            "### Tom Reyes — 2026-09-30 10:00\n\nDear Ana, let us meet on Thursday 2 October at 2:30pm in room B12.\n\n\
             ### Ana Ruiz — 2026-09-30 18:00\n\nThursday at 2:30pm in B12 works for me.\n\n\
             ### Tom Reyes — 2026-10-03 09:00\n\nThanks for today. Next meeting: Friday 10 October at 4pm, same room.\n\n\
             ### Ana Ruiz — 2026-10-03 12:00\n\nNoted, see you on Friday.\n\n\
             ### Tom Reyes — 2026-10-10 17:00\n\nGood progress. We meet again on Friday 17 October at 4pm.\n\n\
             ### Ana Ruiz — 2026-10-16 20:00\n\nCould we start at 4:30pm instead? I have a lab until 4.\n\n\
             ### Tom Reyes — 2026-10-17 08:00\n\nYes, Friday 17 October at 4:30pm is fine.\n\n",
        );
        b.push_str(&format!("### Tom Reyes — 2026-10-23 09:28\n\n{agenda}\n\n"));
        b.push_str(
            "### Tom Reyes — 2026-10-23 09:30\n\nI suggest the next meeting on Monday 2nd of Nov 16:30, let me know if it works for you.\n\n",
        );
        if with_reply {
            b.push_str("### Ana Ruiz — 2026-10-24 00:33\n\nOK, I'm free at that time.\n");
        }
        b
    }

    /// Every sentence of [`weekly`] that fixes a meeting, as a model that lists all it sees would
    /// report it (the history too — `resolve` drops what is over).
    const WEEKLY_SAID: [(&str, &str, &str); 5] = [
        ("Dear Ana, let us meet on Thursday 2 October at 2:30pm in room B12.", "Thursday 2 October", "2:30pm"),
        ("Thanks for today. Next meeting: Friday 10 October at 4pm, same room.", "Friday 10 October", "4pm"),
        ("Good progress. We meet again on Friday 17 October at 4pm.", "Friday 17 October", "4pm"),
        ("Yes, Friday 17 October at 4:30pm is fine.", "Friday 17 October", "4:30pm"),
        ("I suggest the next meeting on Monday 2nd of Nov 16:30, let me know if it works for you.", "Monday 2nd of Nov", "16:30"),
    ];

    /// A model that reports every meeting sentence it is shown, and whose answer is cut at the token
    /// cap when it is shown more than `cut_over` emails — what a long history did to the real one.
    /// It keeps every prompt it was given.
    struct Lister {
        cut_over: usize,
        prompts: std::cell::RefCell<Vec<String>>,
    }
    impl crate::LlmStep for Lister {
        fn complete(&self, _s: &str, u: &str) -> Result<crate::LlmResponse, crate::AgentError> {
            self.prompts.borrow_mut().push(u.to_string());
            let items: Vec<Value> = WEEKLY_SAID
                .iter()
                .filter(|(q, _, _)| u.contains(q))
                .map(|(q, date, time)| {
                    serde_json::json!({"status": "fixed", "quote": q, "title": "Meeting", "date": date,
                                       "time": time, "end": "", "place": ""})
                })
                .collect();
            let answer = Value::Array(items).to_string();
            if u.matches("\n### ").count() + usize::from(u.contains("\n\n### ")) > self.cut_over {
                let cut: String = answer.chars().take(answer.len() / 2).collect();
                return Ok(crate::LlmResponse {
                    content: cut,
                    finish_reason: Some("length".into()),
                    usage: None,
                });
            }
            Ok(crate::LlmResponse::complete(answer))
        }
    }

    fn lister(cut_over: usize) -> Lister {
        Lister { cut_over, prompts: Default::default() }
    }

    fn emails_in(prompt: &str) -> usize {
        prompt.lines().filter(|l| l.starts_with("### ")).count()
    }

    fn starts(found: &[Found], body: &str, today: Date) -> Vec<String> {
        found
            .iter()
            .filter_map(|f| resolve(f, body, today, None).ok())
            .map(|m| m.start_stamp())
            .collect()
    }

    #[test]
    fn a_long_exchange_is_read_in_steps_and_a_cut_answer_is_read_again_smaller() {
        let body = weekly(true);
        let llm = lister(4);
        let found =
            read_all(&llm, false, "Project", &body, "Tom Reyes", date!(2026 - 10 - 24), None)
                .unwrap();
        assert_eq!(starts(&found, &body, date!(2026 - 10 - 24)), ["2026-11-02T16:30"]);
        let prompts = llm.prompts.borrow();
        assert!(prompts.len() > 2, "read in steps: {}", prompts.len());
        // Every answer that was kept came from at most four emails; the cut ones were read again.
        let read: Vec<usize> = prompts.iter().map(|p| emails_in(p)).filter(|&n| n <= 4).collect();
        assert!(!read.is_empty());
        // The last window is the proposal and its reply, with the history as one line.
        let last = prompts.last().unwrap();
        assert!(last.contains("I suggest the next meeting") && last.contains("OK, I'm free"));
        assert!(
            !last.contains("Friday 10 October"),
            "a meeting that took place is not shown again"
        );
        assert!(last.contains("earlier emails in this conversation"), "{last}");
    }

    #[test]
    fn a_new_email_is_read_with_only_what_is_still_in_play() {
        let today = date!(2026 - 10 - 23);
        let before = weekly(false);
        let llm = lister(99);
        let mut ledger = Ledger::default();
        loop {
            let s =
                step(&llm, false, "Project", &before, "Tom Reyes", today, None, &ledger).unwrap();
            ledger = s.ledger;
            if s.done {
                break;
            }
        }
        assert_eq!(ledger.read.as_deref(), Some("2026-10-23T09:30"));
        // The question is not answered yet, but the sentence ends with "." — it fixes a time, and that
        // email is kept for the reply that will come.
        assert!(ledger.keep.iter().any(|k| k.at == "2026-10-23T09:30"), "{:?}", ledger.keep);

        let after = weekly(true);
        let w = window(&after, &ledger, CHUNK_TOKENS);
        assert!(w.done && w.new == 1);
        assert!(w.text.contains("I suggest the next meeting") && w.text.contains("OK, I'm free"));
        assert!(!w.text.contains("Friday 10 October") && !w.text.contains("We will go through"));
        assert_eq!(emails_in(&w.text), 2, "{}", w.text);
        assert_eq!(w.omitted, 8);
        assert!(w
            .text
            .contains("8 earlier emails in this conversation, sent 2026-09-30 to 2026-10-23"));
        // The line is Rust's: quoting it points at no email.
        let echo = Found {
            quote: w.text.lines().find(|l| l.starts_with("(8 earlier")).unwrap().into(),
            ..Found::default()
        };
        assert_eq!(resolve(&echo, &after, today, None), Err(Dropped::NotInEmail));
    }

    #[test]
    fn an_email_already_read_that_changed_means_reading_again_from_the_start() {
        let llm = lister(99);
        let today = date!(2026 - 10 - 23);
        let before = weekly(false);
        let found = step(&llm, false, "P", &before, "T", today, None, &Ledger::default()).unwrap();
        let ledger = found.ledger;
        assert!(ledger.read.is_some());
        let edited = before.replace("works for me", "works for me, thanks");
        let w = window(&edited, &ledger, CHUNK_TOKENS);
        assert_eq!(w.omitted, 0);
        assert!(w.text.contains("Dear Ana"), "read from the first email again");
        // Unchanged, the same ledger reads on from where it stopped; the first meeting is over.
        assert!(!window(&before, &ledger, CHUNK_TOKENS).text.contains("Dear Ana"));
    }

    #[test]
    fn a_question_stays_in_view_until_someone_else_answers() {
        let today = date!(2026 - 10 - 20);
        let asked = "### Tom Reyes — 2026-10-20 09:00\n\nWould Monday 26 October at 3pm work?\n\n";
        let f = Found {
            status: "fixed".into(),
            quote: "Would Monday 26 October at 3pm work?".into(),
            date: "Monday 26 October".into(),
            time: "3pm".into(),
            ..Found::default()
        };
        let w = window(asked, &Ledger::default(), CHUNK_TOKENS);
        let l = advance(&Ledger::default(), &w, std::slice::from_ref(&f), asked, today, None);
        assert_eq!(l.keep, [Kept { at: "2026-10-20T09:00".into(), start: None }]);
        // Tom writes again: still unanswered, still in view.
        let more = format!("{asked}### Tom Reyes — 2026-10-20 10:00\n\nAlso, bring the draft.\n\n");
        let w = window(&more, &l, CHUNK_TOKENS);
        assert!(w.text.contains("Would Monday 26 October"));
        let l = advance(&l, &w, &[], &more, today, None);
        assert_eq!(l.keep.len(), 1);
        // Ana answers: the reply is read with the question; after that read it leaves the view.
        let reply = format!("{more}### Ana Ruiz — 2026-10-21 08:00\n\nYes, that works.\n\n");
        let w = window(&reply, &l, CHUNK_TOKENS);
        assert!(w.text.contains("Would Monday 26 October") && w.text.contains("Yes, that works."));
        let l = advance(&l, &w, &[], &reply, today, None);
        assert!(l.keep.is_empty(), "{:?}", l.keep);
    }

    /// A model that writes one item over and over when shown many emails — the loop's mark.
    struct Repeater(std::cell::RefCell<Vec<usize>>);
    impl crate::LlmStep for Repeater {
        fn complete(&self, _s: &str, u: &str) -> Result<crate::LlmResponse, crate::AgentError> {
            let n = emails_in(u);
            self.0.borrow_mut().push(n);
            let (q, date, time) = if u.contains(WEEKLY_SAID[4].0) && n <= 3 {
                WEEKLY_SAID[4]
            } else {
                WEEKLY_SAID[2]
            };
            let item = serde_json::json!({"status": "fixed", "quote": q, "title": "M", "date": date,
                                          "time": time, "end": "", "place": ""});
            let times = if n > 3 { 4 } else { 1 };
            Ok(crate::LlmResponse::complete(Value::Array(vec![item; times]).to_string()))
        }
    }

    #[test]
    fn an_answer_that_repeats_an_item_is_read_again_with_fewer_emails() {
        let body = weekly(true);
        let llm = Repeater(Default::default());
        let found = read_all(&llm, false, "P", &body, "T", date!(2026 - 10 - 24), None).unwrap();
        assert_eq!(starts(&found, &body, date!(2026 - 10 - 24)), ["2026-11-02T16:30"]);
        assert!(llm.0.borrow().iter().any(|&n| n > 3), "a window of many emails looped first");
    }

    #[test]
    fn a_meeting_restated_without_its_time_is_the_meeting_already_on_that_day() {
        let said = |q: &str| Meeting {
            title: "Call".into(),
            day: date!(2026 - 10 - 30),
            start: None,
            end: None,
            place: None,
            quote: q.into(),
            sent_at: Some("2026-10-22T16:10".into()),
            said_by: None,
            cancelled: false,
            dated: true,
            written: None,
        };
        let current = Current {
            start: "2026-10-30T16:30",
            upcoming: true,
            place: None,
            latest: Some("2026-10-22T16:10"),
        };
        let none = HashSet::new();
        let p = plan(
            vec![said("See you on the 30th, same link.")],
            Some(current),
            None,
            &none,
            &none,
            &none,
        );
        assert_eq!(p, Plan::default(), "the 16:30 meeting, said again: nothing to propose");
        // Another conversation's meeting that day is not this one: the day stays a day of its own.
        let taken: HashSet<String> = ["2026-10-30T09:00".to_string()].into();
        let p = plan(vec![said("See you on the 30th.")], None, None, &taken, &none, &none);
        assert_eq!(p.conversation.map(|m| m.start_stamp()).as_deref(), Some("2026-10-30"));
    }

    #[test]
    fn a_ledger_survives_the_memory_file() {
        let l = Ledger {
            read: Some("2026-10-23T09:30".into()),
            keep: vec![
                Kept { at: "2026-10-23T09:30".into(), start: Some("2026-11-02T16:30".into()) },
                Kept { at: "2026-10-20T09:00".into(), start: None },
            ],
            print: 0xdead_beef_0123_4567,
        };
        assert_eq!(Ledger::from_json(&l.to_json()), l);
        assert_eq!(Ledger::from_json(&Value::Null), Ledger::default());
    }

    #[test]
    fn one_email_whose_answer_is_still_cut_is_passed_over_and_said() {
        let body = "### Tom Reyes — 2026-10-23 09:30\n\nI suggest the next meeting on Monday 2nd of Nov 16:30, let me know if it works for you.\n";
        let s = step(
            &lister(0),
            false,
            "P",
            body,
            "T",
            date!(2026 - 10 - 23),
            None,
            &Ledger::default(),
        )
        .unwrap();
        assert!(s.done && s.found.is_empty());
        assert_eq!(s.skipped, 1);
        assert_eq!(
            s.ledger.read.as_deref(),
            Some("2026-10-23T09:30"),
            "not read again at every check"
        );
    }
}
