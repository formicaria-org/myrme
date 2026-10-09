//! Meetings found in an email exchange — `decisions.md` 2026-10-09, *the assistant proposes meetings*.
//!
//! # Who decides what
//!
//! The model reads a conversation note (the whole exchange, `fm_core::mail`) and answers with the
//! meetings it sees as *fixed*, each **pointing at the sentence that fixes it**, copied from the email.
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
//! 4. **The plan** ([`plan`]) decides what to propose: the conversation note takes the first meeting;
//!    each further one becomes a proposed meeting note; anything already on the agenda (an invitation
//!    made it), already linked, or already proposed is left alone.
//!
//! The person accepts or rejects every one of them. Nothing here writes.

use std::collections::HashSet;

use serde_json::Value;
use time::{Date, Duration, Month, Time, Weekday};

/// One meeting as the model reported it: words copied from the email.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Found {
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
}

pub const SYSTEM: &str = "You read an email exchange and list the meetings with people that it FIXES — \
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

/// Tokens the conversation text may take. The laptop model runs with an 8192-token window
/// (`agents/models.toml`); the instructions take ~350 and the answer is capped at [`ANSWER_TOKENS`],
/// so this leaves a margin for the estimate below being low.
pub const TEXT_TOKENS: usize = 6500;
/// The longest answer the meeting pass asks for — a list of a few short JSON objects.
pub const ANSWER_TOKENS: u32 = 600;

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
                }
            })
            .filter(|f| !f.quote.is_empty())
            .collect();
    }
    Vec::new()
}

/// Lowercase, accents off, every run of whitespace one space — the form quotes are compared in.
fn norm(s: &str) -> String {
    let folded: String = s
        .chars()
        .flat_map(char::to_lowercase)
        .map(|c| match c {
            'à' | 'á' | 'â' => 'a',
            'è' | 'é' | 'ê' => 'e',
            'ì' | 'í' | 'î' => 'i',
            'ò' | 'ó' | 'ô' => 'o',
            'ù' | 'ú' | 'û' => 'u',
            '’' | '‘' => '\'',
            '“' | '”' | '«' | '»' => '"',
            c => c,
        })
        .collect();
    folded.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// One email in the conversation note: the day it was sent, and its text, normalised.
struct Block {
    sent: Option<Date>,
    text: String,
}

/// Split the note into its emails at their `### Name — YYYY-MM-DD HH:MM` headings. Anything above the
/// first heading (the person's own writing, an assistant box) is a block with no sending day.
fn blocks(body: &str) -> Vec<Block> {
    let mut out = vec![Block { sent: None, text: String::new() }];
    let mut raw = String::new();
    for line in body.lines() {
        if let Some(head) = line.strip_prefix("### ") {
            if let Some(b) = out.last_mut() {
                b.text = norm(&raw);
            }
            raw.clear();
            out.push(Block { sent: heading_day(head), text: String::new() });
            continue;
        }
        raw.push_str(line);
        raw.push('\n');
    }
    if let Some(b) = out.last_mut() {
        b.text = norm(&raw);
    }
    out
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

/// Check one reported meeting against the emails and date it. `body` is the conversation note.
pub fn resolve(f: &Found, body: &str, today: Date) -> Result<Meeting, Dropped> {
    let nquote = norm(&f.quote);
    if nquote.is_empty() {
        return Err(Dropped::NotInEmail);
    }
    let (sent, msg, at) = blocks(body)
        .into_iter()
        .find_map(|b| b.text.find(&nquote).map(|i| (b.sent, b.text.clone(), i)))
        .ok_or(Dropped::NotInEmail)?;
    let anchor = sent.unwrap_or(today);
    let model_words = |w: &str, within: &str| {
        let n = norm(w);
        (!n.is_empty() && within.contains(&n)).then_some(n)
    };
    let day = find_day(&nquote, anchor, false)
        .or_else(|| find_day(&msg[..at], anchor, true))
        .or_else(|| model_words(&f.date, &msg).and_then(|n| read_day(&n, anchor)))
        .ok_or(Dropped::NoDate)?;
    if day < today {
        return Err(Dropped::Past);
    }
    // A time only from the sentence itself; none there → the meeting is for the day, which is true.
    let start =
        find_time(&nquote).or_else(|| model_words(&f.time, &nquote).and_then(|n| read_time(&n)));
    let end = start
        .and(model_words(&f.end, &nquote).and_then(|n| read_time(&n)))
        .filter(|e| Some(*e) > start);
    let place = model_words(&f.place, &msg).map(|_| f.place.trim().to_string());
    Ok(Meeting {
        title: if f.title.is_empty() {
            f.quote.chars().take(60).collect()
        } else {
            f.title.clone()
        },
        day,
        start,
        end,
        place,
        quote: f.quote.clone(),
    })
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

const DATE_GLUE: [&str; 14] = [
    "of", "the", "il", "del", "di", "next", "prossimo", "prossima", "this", "questo", "questa",
    "on", "lo", "l",
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
    let words = tokens(text);
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
            return Some(t);
        }
    }
    None
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

/// A day phrase (already [`norm`]ed) → a date, counting from `anchor` (the day the message was sent).
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

/// A time phrase (already [`norm`]ed) → a wall-clock time. `15`, `15:30`, `15.30`, `15h30`, `alle 3`,
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
    mut meetings: Vec<Meeting>,
    current: Option<(&str, bool)>,
    taken: &HashSet<String>,
    linked: &HashSet<String>,
    proposed: &HashSet<String>,
) -> Plan {
    meetings.sort_by_key(|m| (m.day, m.start));
    meetings.dedup_by_key(|m| m.start_stamp());
    let found: HashSet<String> = meetings.iter().map(Meeting::start_stamp).collect();
    // May the conversation note take a meeting? When undated, or when its upcoming time is gone.
    let mut open = match current {
        None => true,
        Some((c, upcoming)) => upcoming && !found.contains(c),
    };
    let mut out = Plan::default();
    for m in meetings {
        let s = m.start_stamp();
        let known = current.is_some_and(|(c, _)| c == s)
            || linked.contains(&s)
            || proposed.contains(&s)
            || taken.contains(&s);
        if known {
            continue;
        }
        if open {
            out.conversation = Some(m);
            open = false;
        } else {
            out.new.push(m);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::{date, time};

    fn d(p: &str, anchor: Date) -> Option<Date> {
        read_day(&norm(p), anchor)
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
        let t = |p: &str| read_time(&norm(p));
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
        let m = resolve(&f, BODY, date!(2026 - 10 - 06)).unwrap();
        assert_eq!(m.start_stamp(), "2026-10-08T15:00");
        // The second meeting is counted from Luca's message (the 7th), not Maria's.
        let f = found("E il 28 alle 10 facciamo il punto con Anna.", "il 28", "alle 10");
        assert_eq!(
            resolve(&f, BODY, date!(2026 - 10 - 06)).unwrap().start_stamp(),
            "2026-10-28T10:00"
        );
    }

    #[test]
    fn an_invented_quote_or_a_past_meeting_is_dropped() {
        let f = found("ci vediamo venerdì alle 9", "venerdì", "alle 9");
        assert_eq!(resolve(&f, BODY, date!(2026 - 10 - 06)), Err(Dropped::NotInEmail));
        let f = found("ci vediamo giovedì alle 15 nel mio ufficio.", "giovedì", "alle 15");
        assert_eq!(resolve(&f, BODY, date!(2026 - 10 - 20)), Err(Dropped::Past));
    }

    /// **The case that failed on the owner's real mail (2026-10-09).** The date is in the sentence
    /// before the quote, the room number looks like a date, and the model's own day words were a
    /// translation ("16 ottobre") of what the email said.
    #[test]
    fn that_day_is_read_from_the_email_before_the_quote_and_a_room_is_not_a_date() {
        let body = "### Ana Ruiz — 2026-10-02 17:37\n\nWhen can we reschedule?\n\n\
                    ### Owner Name — 2026-10-02 17:51\nTo: ANA RUIZ\n\n\
                    Sorry for the delay. I should be back in Singapore by the 16th of October. We can have\n\
                    a chat directly that day at 12pm in RM 01-02.\n";
        let f = Found {
            title: "Meeting with Ana".into(),
            date: "16 ottobre".into(),
            time: "12pm".into(),
            place: "RM 01-02".into(),
            quote: "We can have a chat directly that day at 12pm in RM 01-02.".into(),
            ..Default::default()
        };
        let m = resolve(&f, body, date!(2026 - 10 - 09)).unwrap();
        assert_eq!(m.start_stamp(), "2026-10-16T12:00");
        assert_eq!(m.place.as_deref(), Some("RM 01-02"));
    }

    #[test]
    fn the_sentences_own_date_wins_over_whatever_the_model_said() {
        let f = found("E il 28 alle 10 facciamo il punto con Anna.", "domani", "alle 10");
        assert_eq!(
            resolve(&f, BODY, date!(2026 - 10 - 06)).unwrap().start_stamp(),
            "2026-10-28T10:00"
        );
    }

    #[test]
    fn dates_and_times_are_found_in_running_text() {
        let a = date!(2026 - 09 - 18);
        assert_eq!(
            find_day(&norm("P.s. 2nd of October 2026 at 11:30am Singapore time."), a, false),
            Some(date!(2026 - 10 - 02))
        );
        assert_eq!(
            find_time(&norm("2nd of October 2026 at 11:30am Singapore time")),
            Some(time!(11:30))
        );
        assert_eq!(
            find_day(&norm("We can try next week Monday 28th of Sep at Singapore 5pm:"), a, false),
            Some(date!(2026 - 09 - 28))
        );
        assert_eq!(find_time(&norm("Monday 28th of Sep at Singapore 5pm:")), Some(time!(17:00)));
        assert_eq!(find_day(&norm("room RM 01-02, project 2999-00001, 15.30"), a, false), None);
        assert_eq!(find_time(&norm("project AB1234X 2999-00001 for 3 students")), None);
        assert_eq!(
            find_day(&norm("ci vediamo il 16 alle 15"), a, false),
            Some(date!(2026 - 10 - 16))
        );
    }

    #[test]
    fn the_time_comes_from_the_sentence_and_none_there_means_the_day() {
        // The model said 16; the sentence says 15. The sentence wins.
        let f = found("ci vediamo giovedì alle 15 nel mio ufficio.", "giovedì", "alle 16");
        assert_eq!(
            resolve(&f, BODY, date!(2026 - 10 - 06)).unwrap().start_stamp(),
            "2026-10-08T15:00"
        );
        // No time in the sentence: a meeting for the day, not a guessed hour.
        let body = "### Maria — 2026-10-06 09:12\n\nPassa da me giovedì.\n";
        let f = found("Passa da me giovedì.", "giovedì", "alle 16");
        let m = resolve(&f, body, date!(2026 - 10 - 06)).unwrap();
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
        }
    }

    #[test]
    fn the_conversation_takes_the_first_meeting_and_each_further_one_is_new() {
        let none = HashSet::new();
        let p = plan(
            vec![meeting(date!(2026 - 10 - 28), 10), meeting(date!(2026 - 10 - 08), 15)],
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
        assert_eq!(plan(ms.clone(), None, &taken, &none, &asked), Plan::default());
        // The conversation already stands for the first one: only the second is new.
        let p = plan(ms, Some(("2026-10-08T15:00", true)), &none, &none, &none);
        assert!(p.conversation.is_none());
        assert_eq!(p.new.len(), 1);
    }

    #[test]
    fn a_later_email_that_moves_the_meeting_proposes_moving_the_note() {
        let none = HashSet::new();
        let p = plan(
            vec![meeting(date!(2026 - 10 - 09), 15)],
            Some(("2026-10-08T15:00", true)),
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
            Some(("2026-10-01T15:00", false)),
            &none,
            &none,
            &none,
        );
        assert!(p.conversation.is_none(), "the note keeps the meeting that took place");
        assert_eq!(p.new.len(), 1, "the next one is its own note");
    }
}
