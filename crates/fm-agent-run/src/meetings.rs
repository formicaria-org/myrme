//! The meeting pass — the assistant reading email conversations on its own and proposing the meetings
//! they fix (`decisions.md` 2026-10-09, *the assistant proposes meetings*).
//!
//! Runs from the resident watcher's loop, **one conversation per call**, so a question asked in a
//! discussion is never kept waiting behind a backlog of mail. A conversation is read again only when
//! a new message has arrived in it (`mail_last` moved). Everything it produces is a proposal: the
//! conversation note gaining the first meeting's date, time and place, and each further meeting as a
//! proposed note linked back to it. The person accepts or rejects each one; nothing here writes to
//! `main`.
//!
//! Its memory — which conversation it last read at which message, and which meeting times it has
//! already proposed — is a small file in the assistant's runtime folder, not the vault: it is this
//! machine's bookkeeping, and a meeting the person rejected must not be asked about again.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use fm_agent::meetings::{self, Meeting};
use fm_agent::zone::Here;
use fm_agent::LlmStep;
use serde_json::{json, Value};

use crate::fmserve::{Origin, VaultAccess};
use crate::Agent;

/// The fence around the assistant's meeting block in a conversation note.
const TAG: &str = "fm:meeting";
/// The tag a called-off meeting carries — the same one the calendars give a cancelled event.
const CANCELLED: &str = "cancelled";

/// Who the mailbox belongs to when formicaria does not say. Both mail calls name the same one: with a
/// shared mail prompt a different name would make the two system prompts differ.
const OWNER: &str = "the mailbox's owner";

/// A title already in the "Person — who they are" form: given by the assistant, or by the person.
fn named(title: &str) -> bool {
    title.contains(" — ")
}

/// Bumped whenever the way meetings are found changes, so every conversation is read once more under
/// the new rules. What was already proposed is kept, so nothing is asked twice. 2: the date is found
/// by Rust in the sentence (or the email before it), not taken from the model (2026-10-09). 3: the
/// question is where the meetings stand now, our box is hidden from the model, and a near-copied quote
/// stands as the email's own sentence (2026-10-15) — so a reply the old rules misread (a moved meeting
/// dropped over "16th of October") is read again and proposed. 4: on such a re-read, a call-off that is
/// still the conversation's latest word is proposed too (2026-10-15). 5: a quote shortened with "…" is
/// checked piece by piece (2026-10-16) — a move dropped over one is read again and proposed. 6: another
/// conversation's meeting no longer counts as already on the agenda (`mail_conversations`' `taken`).
/// 7: a time written on another clock ("10am Berlin time") is moved onto the person's (2026-10-23) —
/// so a meeting proposed at the clock it was written on is read again and proposed moving.
const PASS_VERSION: u64 = 7;

#[derive(Default)]
struct Memory {
    /// conversation id -> (`mail_last` when last read, meeting starts already proposed)
    seen: HashMap<String, (String, HashSet<String>)>,
    /// Conversations already given a title — asked once, accepted or not (`fm_agent::titles`).
    titled: HashSet<String>,
}

impl Memory {
    fn load(path: &Path) -> Self {
        let v: Value = std::fs::read_to_string(path)
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or(Value::Null);
        let mut m = Memory::default();
        let current = v["_version"].as_u64() == Some(PASS_VERSION);
        for (id, e) in v.as_object().into_iter().flatten() {
            if id == "_version" {
                continue;
            }
            // Under older rules: forget where reading stopped, so the conversation is read again.
            let last =
                if current { e["last"].as_str().unwrap_or("").to_string() } else { String::new() };
            let proposed = e["proposed"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|s| s.as_str().map(str::to_string))
                .collect();
            m.seen.insert(id.clone(), (last, proposed));
            if e["titled"].as_bool() == Some(true) {
                m.titled.insert(id.clone());
            }
        }
        m
    }

    fn save(&self, path: &Path) {
        let empty = (String::new(), HashSet::new());
        let ids: HashSet<&String> = self.seen.keys().chain(self.titled.iter()).collect();
        let mut v: serde_json::Map<String, Value> = ids
            .into_iter()
            .map(|id| {
                let (last, proposed) = self.seen.get(id).unwrap_or(&empty);
                let mut p: Vec<&String> = proposed.iter().collect();
                p.sort();
                let mut e = json!({ "last": last, "proposed": p });
                if self.titled.contains(id) {
                    e["titled"] = json!(true);
                }
                (id.clone(), e)
            })
            .collect();
        v.insert("_version".into(), json!(PASS_VERSION));
        let _ = std::fs::write(path, Value::Object(v).to_string());
    }
}

/// The block the conversation note gains: what was found, and the sentence it came from. Placed at
/// the **top** of the note, so it never sits where new messages are appended (and so cannot conflict
/// with them when the proposal is merged).
fn block(m: &Meeting, model: &str) -> String {
    let when = match (m.start, m.end) {
        (Some(s), Some(e)) => format!(
            "{} {:02}:{:02}–{:02}:{:02}",
            day(m),
            s.hour(),
            s.minute(),
            e.hour(),
            e.minute()
        ),
        (Some(s), None) => format!("{} {:02}:{:02}", day(m), s.hour(), s.minute()),
        _ => day(m),
    };
    // The time as the email wrote it, when it was moved onto the person's clock.
    let when = match &m.written {
        Some(w) => format!("{when} ({w})"),
        None => when,
    };
    let place = m.place.as_deref().map(|p| format!(" · {p}")).unwrap_or_default();
    let quote = fm_agent::adjunct::defang(&m.quote, TAG).replace('\n', " ");
    // Who said it, and when: "proposed by me, not answered yet" is read at a glance from this.
    let said = match (&m.said_by, &m.sent_at) {
        (Some(by), Some(at)) => format!(" — {}, {}", by, at.replace('T', " ")),
        (Some(by), None) => format!(" — {by}"),
        _ => String::new(),
    };
    let said = fm_agent::adjunct::defang(&said, TAG);
    let head =
        if m.cancelled { format!("Called off — {when}") } else { format!("{when}{place}") };
    format!(
        "{open}\n> [!note] Meeting found by the assistant\n> {head}\n> «{quote}»{said}\n{end}\n",
        open = fm_agent::adjunct::open_mark(TAG, model),
        end = fm_agent::adjunct::end_mark(TAG),
    )
}

fn day(m: &Meeting) -> String {
    format!("{:04}-{:02}-{:02}", m.day.year(), u8::from(m.day.month()), m.day.day())
}

/// A note's `start` in words: `2026-10-23T16:00` → `Fri 23 Oct at 16:00`, a day alone → `Fri 23 Oct`.
/// What the person reads in Collaboration; the stamp itself was the owner's "wrong words"
/// (2026-10-20).
pub fn when(stamp: &str) -> String {
    let (d, t) = stamp.split_once('T').unwrap_or((stamp, ""));
    let mut it = d.splitn(3, '-').map(|x| x.parse::<i32>().ok());
    let date = match (it.next().flatten(), it.next().flatten(), it.next().flatten()) {
        (Some(y), Some(m), Some(dd)) => time::Month::try_from(m as u8)
            .ok()
            .and_then(|m| time::Date::from_calendar_date(y, m, dd as u8).ok()),
        _ => None,
    };
    let Some(date) = date else { return stamp.replace('T', " ") };
    let day = format!(
        "{} {} {}",
        &date.weekday().to_string()[..3],
        date.day(),
        &date.month().to_string()[..3]
    );
    match t.get(..5) {
        Some(hm) => format!("{day} at {hm}"),
        None => day,
    }
}

/// Minutes since the epoch of a `start` with a time, for telling two meetings apart.
fn minutes(stamp: &str) -> Option<i64> {
    let (d, t) = stamp.split_once('T')?;
    let mut it = d.splitn(3, '-').map(|x| x.parse::<i32>().ok());
    let (y, m, dd) = (it.next()??, it.next()??, it.next()??);
    let date =
        time::Date::from_calendar_date(y, time::Month::try_from(m as u8).ok()?, dd as u8).ok()?;
    let (h, mi) = t.get(..5)?.split_once(':')?;
    Some(
        i64::from(date.to_julian_day()) * 1440
            + h.parse::<i64>().ok()? * 60
            + mi.parse::<i64>().ok()?,
    )
}

/// **Two meetings at once** (the owner, 2026-10-20: two conversations both kept a meeting on Friday
/// at 16:00, and *"I had to catch the conflict by myself"*). Another meeting the vault knows that
/// starts less than an hour from `start` — a conversation's own meeting or one linked to it, named by
/// the conversation, or any other meeting note — said as words to add to the verdict and the note.
/// The vault gives starts, not ends, so an hour is the measure. `own` is the conversation being read.
pub fn clash(start: &str, own: &str, list: &Value) -> Option<String> {
    let at = minutes(start)?;
    let near = |s: &str| minutes(s).is_some_and(|m| (m - at).abs() < 60);
    for conv in list["conversations"].as_array().into_iter().flatten() {
        let id = conv["id"].as_str().unwrap_or_default();
        if id == own {
            continue;
        }
        let title =
            conv["title"].as_str().filter(|t| !t.is_empty()).unwrap_or("another conversation");
        let theirs = conv["start"]
            .as_str()
            .into_iter()
            .chain(list["linked"][id].as_array().into_iter().flatten().filter_map(Value::as_str));
        for s in theirs {
            if near(s) {
                return Some(format!(
                    "it clashes with the meeting of \u{201c}{title}\u{201d}, {}",
                    when(s)
                ));
            }
        }
    }
    let other =
        list["taken"].as_array().into_iter().flatten().filter_map(Value::as_str).find(|s| near(s));
    other.map(|s| format!("it clashes with another meeting in your calendar, {}", when(s)))
}

/// The verdict, with the clash after it when there is one.
fn with_clash(said: String, clashes: &Option<String>) -> String {
    match clashes {
        Some(c) => format!("{said} \u{2014} but {c}"),
        None => said,
    }
}

/// A meeting box with the clash written under it, so the proposal shows it before it is accepted.
fn warned(block: String, clashes: &Option<String>) -> String {
    let Some(c) = clashes else { return block };
    // Inside the box, before its closing mark: the box is replaced whole on the next reading.
    match block.trim_end().rsplit_once('\n') {
        Some((head, end)) => format!("{head}\n> **Two meetings at once:** {c}.\n{end}\n"),
        None => block,
    }
}

/// Put the block at the top, or replace the one already there.
fn with_block(body: &str, block: &str, model: &str) -> String {
    if body.contains(&fm_agent::adjunct::open_mark(TAG, model)) {
        return fm_agent::adjunct::insert_or_supersede(body, block, TAG, model);
    }
    format!(
        "{}\n{}",
        block.trim_end(),
        if body.starts_with('\n') { body.to_string() } else { format!("\n{body}") }
    )
}

impl<V: VaultAccess> Agent<V> {
    /// The person's own clock: formicaria's setting, else this computer's zone.
    pub fn here(&self) -> Option<Here> {
        self.fm.zone().and_then(|z| Here::parse(&z)).or_else(Here::system)
    }

    /// Read at most one conversation with new mail and propose its meetings. Returns a one-line
    /// summary when it did something, `None` when there was nothing to read.
    ///
    /// `Err` only for a failure worth logging (the model did not answer); the conversation is then
    /// left unread and tried again at the next call.
    pub fn meeting_pass<L: LlmStep>(
        &self,
        llm: &L,
        memory: &Path,
        today: time::Date,
    ) -> Result<Option<String>, String> {
        // No mail on this device, or nothing listening: nothing to do, and not an error.
        let Ok(list) = self.fm.mail_conversations() else { return Ok(None) };
        let strings = |v: &Value| -> HashSet<String> {
            v.as_array()
                .into_iter()
                .flatten()
                .filter_map(|s| s.as_str().map(str::to_string))
                .collect()
        };
        let taken = strings(&list["taken"]);
        let me = list["me"].as_str();
        let here = self.here();
        let mut mem = Memory::load(memory);
        let today_s =
            format!("{:04}-{:02}-{:02}", today.year(), u8::from(today.month()), today.day());

        for conv in list["conversations"].as_array().into_iter().flatten() {
            let (Some(id), Some(last)) = (conv["id"].as_str(), conv["last"].as_str()) else {
                continue;
            };
            if mem.seen.get(id).is_some_and(|(l, _)| l == last) {
                continue;
            }
            let title = conv["title"].as_str().unwrap_or("");
            let note = self.fm.get(id)?;
            let body = note["body"].as_str().unwrap_or_default().to_string();
            // The exchange alone: our own box repeats last time's answer, and a small model read it
            // first and gave it again over a later email that moved the meeting.
            let emails = meetings::without_box(&body);

            // Held to the answer's schema: measured on 2026-10-18 (`methods/meetings/eval`), a free
            // answer often left out the quote and lost the meeting — 8/14 against 14/14 on
            // qwen3-4b-2507, 11/14 against 12/14 on qwen3-vl-4b, and faster.
            // The layout is the model's (`shared_mail_prompt`): shared, the title call below reuses
            // this call's cached emails.
            let (system, user) = meetings::prompt(
                self.shared_mail_prompt,
                meetings::Question::Meetings,
                title,
                &emails,
                me.unwrap_or(OWNER),
            );
            let answer = llm
                .complete_constrained(&system, &user, &meetings::answer_schema())
                .map_err(|e| format!("the model did not answer for \u{201c}{title}\u{201d}: {e}"))?
                .content;
            let found = meetings::parse(&answer);
            let mut ok = Vec::new();
            let mut dropped = 0usize;
            for f in &found {
                match meetings::resolve(f, &emails, today, here.as_ref()) {
                    Ok(m) => ok.push(m),
                    Err(_) => dropped += 1,
                }
            }

            let current = conv["start"].as_str().map(|s| meetings::Current {
                start: s,
                upcoming: s >= today_s.as_str(),
                place: conv["location"].as_str().filter(|p| !p.is_empty()),
                latest: Some(last),
            });
            let linked = strings(&list["linked"][id]);
            let (read_to, proposed) = mem.seen.get(id).cloned().unwrap_or_default();
            // Where the last read stopped: what came after it is news. Empty after a change of rules,
            // when the whole conversation is read again as history.
            let since = Some(read_to.as_str()).filter(|s| !s.is_empty());
            let plan = meetings::plan(ok, current, since, &taken, &linked, &proposed);
            let new_title = self.title_once(llm, &mut mem, id, title, &emails, me);

            let (who, email) = self.author();
            let origin = Origin {
                tool: "meetings",
                query: Some(format!("the meetings fixed in \u{201c}{title}\u{201d}")),
                sources: vec![format!("note:{id}")],
            };
            let mut now_proposed = proposed;
            let mut made = 0usize;
            // What it made of the conversation, in words, for Collaboration (`mail_heard`).
            let mut verdict: Vec<String> = Vec::new();
            if let Some(m) = &plan.conversation {
                let clashes = clash(&m.start_stamp(), id, &list);
                let new_body =
                    with_block(&body, &warned(block(m, &self.model), &clashes), &self.model);
                // A new time also lifts an earlier call-off: the meeting is on again.
                let mut props = json!({
                    "start": m.start_stamp(),
                    "due": m.due_stamp(),
                    "addTags": ["meeting"],
                    "removeTags": [CANCELLED],
                });
                if let Some(p) = &m.place {
                    props["location"] = json!(p);
                }
                if let Some(t) = &new_title {
                    props["title"] = json!(t);
                }
                self.fm.propose_with(id, &new_body, &props, &who, &email, &origin)?;
                now_proposed.insert(m.start_stamp());
                made += 1;
                verdict.push(with_clash(
                    format!("proposes the meeting on {}", when(&m.start_stamp())),
                    &clashes,
                ));
            }
            if let Some(m) = &plan.cancel {
                // Marked, never deleted — the calendars' rule: the person may have written in the note,
                // and the date stays readable as the meeting that did not happen.
                let new_body = with_block(&body, &block(m, &self.model), &self.model);
                let mut props = json!({ "addTags": [CANCELLED] });
                if let Some(t) = &new_title {
                    props["title"] = json!(t);
                }
                self.fm.propose_with(id, &new_body, &props, &who, &email, &origin)?;
                made += 1;
                verdict.push("proposes marking the meeting called off".into());
            }
            if plan.conversation.is_none() && plan.cancel.is_none() {
                if let Some(t) = &new_title {
                    self.propose_title(id, &body, t)?;
                    made += 1;
                    verdict.push(format!("proposes the title \u{201c}{t}\u{201d}"));
                }
            }
            if !plan.new.is_empty() {
                verdict.push(match plan.new.len() {
                    1 => "proposes one more meeting".to_string(),
                    n => format!("proposes {n} more meetings"),
                });
            }
            for m in &plan.new {
                let quote = fm_agent::adjunct::defang(&m.quote, TAG).replace('\n', " ");
                let clashes = clash(&m.start_stamp(), id, &list);
                let warning = clashes
                    .as_deref()
                    .map(|c| format!("\n> [!warning] Two meetings at once: {c}.\n"))
                    .unwrap_or_default();
                let body = format!(
                    "> [!note] Meeting found by the assistant\n> «{quote}»\n{warning}\nFrom the conversation [{title}](note:{id}).\n"
                );
                if let Some(c) = &clashes {
                    verdict.push(format!("the meeting on {} — {c}", when(&m.start_stamp())));
                }
                let mut note = json!({
                    "title": m.title,
                    "body": body,
                    "start": m.start_stamp(),
                    "due": m.due_stamp(),
                    "tags": ["meeting", "mail"],
                });
                if let Some(p) = &m.place {
                    note["location"] = json!(p);
                }
                self.fm.propose_note(id, &note, &who, &email, &origin)?;
                now_proposed.insert(m.start_stamp());
                made += 1;
            }
            mem.seen.insert(id.to_string(), (last.to_string(), now_proposed));
            mem.save(memory);
            // In words a person reads (the owner, 2026-10-20: *"some wrong words there"* — a meeting
            // that "stays at" a time already gone, and timestamps).
            let mut said = if verdict.is_empty() {
                match conv["start"].as_str() {
                    // Begun before the newest email: it took place, and that email changed nothing.
                    Some(s) if s.get(..16).unwrap_or(s) < last.get(..16).unwrap_or(last) => {
                        format!("no change \u{2014} the meeting on {} has taken place", when(s))
                    }
                    Some(s) => with_clash(
                        format!("no change \u{2014} the meeting is still on {}", when(s)),
                        &clash(s, id, &list),
                    ),
                    None => "no meeting mentioned".to_string(),
                }
            } else {
                verdict.join("; ")
            };
            if dropped > 0 {
                said.push_str(&match dropped {
                    1 => " (one more possible meeting was not proposed: its words are not in the emails)".to_string(),
                    n => format!(" ({n} more possible meetings were not proposed: their words are not in the emails)"),
                });
            }
            self.fm.mail_heard(id, new_title.as_deref().unwrap_or(title), last, &said);
            return Ok(Some(format!(
                "read \u{201c}{title}\u{201d}: {made} proposal(s) \u{2014} {said}"
            )));
        }
        // No new mail anywhere: give one conversation still without a proper title its title.
        for conv in list["conversations"].as_array().into_iter().flatten() {
            let Some(id) = conv["id"].as_str() else { continue };
            let title = conv["title"].as_str().unwrap_or("");
            if mem.titled.contains(id) || named(title) {
                continue;
            }
            let note = self.fm.get(id)?;
            let body = note["body"].as_str().unwrap_or_default().to_string();
            let emails = meetings::without_box(&body);
            let new_title = self.title_once(llm, &mut mem, id, title, &emails, me);
            if let Some(t) = &new_title {
                self.propose_title(id, &body, t)?;
            }
            mem.save(memory);
            return Ok(Some(match new_title {
                Some(t) => format!("proposed the title \u{201c}{t}\u{201d}"),
                None => format!("no title found for \u{201c}{title}\u{201d}"),
            }));
        }
        Ok(None)
    }

    /// Ask once for a conversation's title (`fm_agent::titles`), and remember it was asked, whatever
    /// the answer: a title is not asked again at every new email. `None` when already asked, when the
    /// note already reads "Person — …", or when the answer does not pass the checks.
    fn title_once<L: LlmStep>(
        &self,
        llm: &L,
        mem: &mut Memory,
        id: &str,
        title: &str,
        emails: &str,
        me: Option<&str>,
    ) -> Option<String> {
        if mem.titled.contains(id) || named(title) {
            return None;
        }
        mem.titled.insert(id.to_string());
        let (system, user) = meetings::prompt(
            self.shared_mail_prompt,
            meetings::Question::Title,
            title,
            emails,
            me.unwrap_or(OWNER),
        );
        let answer = llm.complete(&system, &user).ok()?.content;
        let (name, about) = fm_agent::titles::parse(&answer)?;
        fm_agent::titles::check(&name, &about, emails, me)
    }

    /// Propose `title` alone — over the conversation's proposal still waiting, if it has one, so its
    /// date and text are kept (the props of a revision go over the stored ones).
    fn propose_title(&self, id: &str, body: &str, title: &str) -> Result<(), String> {
        let body = self.fm.open_proposal_body(id).unwrap_or_else(|| body.to_string());
        let origin = Origin {
            tool: "titles",
            query: Some(format!("a title for \u{201c}{title}\u{201d}")),
            sources: vec![format!("note:{id}")],
        };
        let (who, email) = self.author();
        self.fm.propose_with(id, &body, &json!({ "title": title }), &who, &email, &origin)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fm_agent::{AgentError, LlmResponse};
    use std::cell::RefCell;
    use time::macros::date;

    /// A model that answers one fixed string, and counts how often it was asked.
    struct Canned(String, RefCell<usize>);
    impl LlmStep for Canned {
        fn complete(&self, _s: &str, _u: &str) -> Result<LlmResponse, AgentError> {
            *self.1.borrow_mut() += 1;
            Ok(LlmResponse { content: self.0.clone(), finish_reason: None, usage: None })
        }
        /// The meeting pass must hold its answer to the meeting schema; every test here checks it.
        fn complete_constrained(
            &self,
            s: &str,
            u: &str,
            schema: &serde_json::Value,
        ) -> Result<LlmResponse, AgentError> {
            if s == fm_agent::prompts::MEETINGS_INSTRUCTION {
                assert_eq!(
                    *schema,
                    meetings::answer_schema(),
                    "the meeting pass sent another schema"
                );
            }
            self.complete(s, u)
        }
    }

    #[derive(Default)]
    struct MailVault {
        calls: RefCell<Vec<(String, Value)>>,
        /// What the pass told the person, per conversation (`mail_heard`).
        heard: RefCell<Vec<(String, String)>>,
        /// The conversation's meeting already on the agenda, if any.
        start: Option<&'static str>,
        /// Other conversations in the mailbox, with their meetings.
        others: Option<Value>,
    }
    const BODY: &str = "### Maria — 2026-10-06 09:12\n\nCi vediamo giovedì alle 15 in aula 3.\n\n\
                        ### Luca — 2026-10-07 11:00\n\nE il 28 alle 10 facciamo il punto.\n";
    impl VaultAccess for MailVault {
        fn get(&self, _id: &str) -> Result<Value, String> {
            Ok(json!({ "body": BODY, "title": "Revisione" }))
        }
        fn thread(&self, _: &str) -> Result<Value, String> {
            Ok(Value::Null)
        }
        fn discussions(&self) -> Result<Value, String> {
            Ok(json!([]))
        }
        fn alive(&self) -> bool {
            true
        }
        fn reply(&self, _: &str, _: &str) -> Result<Value, String> {
            Ok(Value::Null)
        }
        fn reply_as(&self, _: &str, _: &str, _: &str, _: &str) -> Result<Value, String> {
            Ok(Value::Null)
        }
        fn create_proposal(
            &self,
            _: &str,
            _: &str,
            _: &str,
            _: &str,
            _: &Origin,
        ) -> Result<Value, String> {
            Ok(Value::Null)
        }
        fn blob_bytes(&self, _: &str) -> Result<(Vec<u8>, String), String> {
            Err("none".into())
        }
        fn activity(&self, _: &str, _: &str, _: &str) {}
        fn activity_done(&self, _: &str) {}
        fn present(&self, _: &str, _: Option<&crate::ImageReader>) {}
        fn mail_conversations(&self) -> Result<Value, String> {
            let mut conversations = vec![
                json!({ "id": "C1", "title": "Revisione", "last": "2026-10-07T11:00", "start": self.start }),
            ];
            if let Some(Value::Array(o)) = &self.others {
                conversations.extend(o.iter().cloned());
            }
            Ok(json!({ "conversations": conversations, "taken": [], "linked": {} }))
        }
        fn mail_heard(&self, note: &str, _title: &str, _latest: &str, said: &str) {
            self.heard.borrow_mut().push((note.to_string(), said.to_string()));
        }
        fn propose_with(
            &self,
            note: &str,
            body: &str,
            props: &Value,
            _: &str,
            _: &str,
            _: &Origin,
        ) -> Result<Value, String> {
            self.calls
                .borrow_mut()
                .push((format!("update {note}"), json!({ "body": body, "props": props })));
            Ok(json!({ "id": "P1" }))
        }
        fn propose_note(
            &self,
            about: &str,
            note: &Value,
            _: &str,
            _: &str,
            _: &Origin,
        ) -> Result<Value, String> {
            self.calls.borrow_mut().push((format!("new about {about}"), note.clone()));
            Ok(json!({ "id": "P2" }))
        }
    }

    /// A fresh memory file per test, without a dev-dependency for one temp path.
    fn scratch() -> std::path::PathBuf {
        let p = std::env::temp_dir().join(format!(
            "fm-meetings-{}.json",
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        ));
        let _ = std::fs::remove_file(&p);
        p
    }

    fn agent() -> Agent<MailVault> {
        Agent {
            fm: MailVault::default(),
            model_port: 0,
            model: "qwen3-vl-4b".into(),
            searxng_port: None,
            web_direct: false,
            whisper_port: None,
            whisper_model: String::new(),
            vision: false,
            read_prompt: None,
            reads_labels: false,
            shared_mail_prompt: false,
            image_specialist: None,
            ocr: None,
            max_reply_chars: 1000,
            history_budget: 1000,
            device: "laptop".into(),
        }
    }

    const ANSWER: &str = r#"[
      {"title":"Revisione","date":"giovedì","time":"alle 15","place":"aula 3","quote":"Ci vediamo giovedì alle 15 in aula 3."},
      {"title":"Punto","date":"il 28","time":"alle 10","quote":"E il 28 alle 10 facciamo il punto."},
      {"title":"Invented","date":"venerdì","time":"alle 9","quote":"ci vediamo venerdì alle 9"}
    ]"#;

    #[test]
    fn the_first_meeting_dates_the_conversation_and_the_second_is_a_new_note() {
        let mem = scratch();
        let a = agent();
        let llm = Canned(ANSWER.into(), RefCell::new(0));
        let said = a.meeting_pass(&llm, &mem, date!(2026 - 10 - 06)).unwrap().unwrap();
        assert!(
            said.contains("2 proposal(s)") && said.contains("one more possible meeting"),
            "{said}"
        );
        let heard = a.fm.heard.borrow();
        assert_eq!(heard.len(), 1);
        assert_eq!(heard[0].0, "C1");
        assert!(
            heard[0].1.starts_with(
                "proposes the meeting on Thu 8 Oct at 15:00; proposes one more meeting (one more possible meeting"
            ),
            "the person is told what was proposed, in words: {}",
            heard[0].1
        );

        let calls = a.fm.calls.borrow();
        let (what, update) = &calls[0];
        assert_eq!(what, "update C1");
        assert_eq!(update["props"]["start"], "2026-10-08T15:00");
        assert_eq!(update["props"]["location"], "aula 3");
        let body = update["body"].as_str().unwrap();
        assert!(body.starts_with("<!-- fm:meeting"), "the block goes on top: {body}");
        assert!(body.ends_with(BODY), "the emails are untouched below it");
        let (what, new) = &calls[1];
        assert_eq!(what, "new about C1");
        assert_eq!(new["start"], "2026-10-28T10:00");
        assert!(new["body"].as_str().unwrap().contains("note:C1"), "linked back");
        assert_eq!(calls.len(), 2, "the invented meeting was not proposed");
    }

    /// **A reply that only confirms the meeting changes nothing — and says so.** A collaborator's "sounds good,
    /// 16:00 next Friday" (2026-10-16) was read within a minute and rightly proposed nothing, and the
    /// owner, seeing nothing in Collaboration, took it for the mail never having been read.
    #[test]
    fn a_start_is_said_in_words() {
        assert_eq!(when("2026-10-23T16:00"), "Fri 23 Oct at 16:00");
        assert_eq!(when("2026-10-23"), "Fri 23 Oct");
        assert_eq!(when("not a date"), "not a date");
    }

    #[test]
    fn a_meeting_within_the_hour_of_another_is_a_clash() {
        let list = json!({
            "conversations": [
                {"id": "A", "title": "Li Shuhan", "start": "2026-10-23T16:00"},
                {"id": "B", "title": "Siyu Hou", "start": "2026-10-23T16:00"},
                {"id": "C", "title": "Chen Tingyu", "start": "2026-10-30T17:00"}
            ],
            "linked": {"C": ["2026-10-24T10:00"]},
            "taken": ["2026-10-27T09:30"]
        });
        // The owner's case: two conversations, one Friday at 16:00.
        let c = clash("2026-10-23T16:00", "B", &list).unwrap();
        assert!(c.contains("Li Shuhan") && c.contains("Fri 23 Oct at 16:00"), "{c}");
        assert!(clash("2026-10-23T16:45", "B", &list).is_some(), "under an hour apart");
        assert!(clash("2026-10-23T17:00", "B", &list).is_none(), "an hour apart is not a clash");
        assert!(clash("2026-10-23T16:00", "Z", &list).is_some());
        assert!(clash("2026-10-30T17:00", "C", &list).is_none(), "its own meeting is not a clash");
        assert!(clash("2026-10-24T10:30", "A", &list).unwrap().contains("Chen Tingyu"), "linked");
        assert!(clash("2026-10-27T09:00", "A", &list).unwrap().contains("calendar"), "any meeting");
        assert!(clash("2026-10-23", "A", &list).is_none(), "a day with no time");
    }

    #[test]
    fn a_meeting_kept_that_clashes_says_so_and_one_already_held_is_said_as_held() {
        let mem = scratch();
        let llm = Canned("[]".into(), RefCell::new(0));
        let a = Agent {
            fm: MailVault {
                start: Some("2026-10-23T16:00"),
                others: Some(
                    json!([{"id": "C2", "title": "Siyu Hou", "start": "2026-10-23T16:00"}]),
                ),
                ..Default::default()
            },
            ..agent()
        };
        a.meeting_pass(&llm, &mem, date!(2026 - 10 - 16)).unwrap();
        let said = a.fm.heard.borrow()[0].1.clone();
        assert!(
            said.contains("is still on Fri 23 Oct at 16:00 \u{2014} but it clashes with"),
            "{said}"
        );
        assert!(said.contains("Siyu Hou"), "{said}");

        let mem = scratch();
        let a = Agent {
            fm: MailVault { start: Some("2026-10-01T09:00"), ..Default::default() },
            ..agent()
        };
        a.meeting_pass(&llm, &mem, date!(2026 - 10 - 16)).unwrap();
        let said = a.fm.heard.borrow()[0].1.clone();
        assert_eq!(said, "no change \u{2014} the meeting on Thu 1 Oct at 09:00 has taken place");
    }

    #[test]
    fn a_reply_that_changes_nothing_is_still_reported() {
        let mem = scratch();
        let a = Agent {
            fm: MailVault { start: Some("2026-10-23T16:00"), ..Default::default() },
            ..agent()
        };
        let llm = Canned("[]".into(), RefCell::new(0));
        a.meeting_pass(&llm, &mem, date!(2026 - 10 - 16)).unwrap();
        assert!(a.fm.calls.borrow().is_empty(), "nothing proposed");
        assert_eq!(
            *a.fm.heard.borrow(),
            vec![(
                "C1".to_string(),
                "no change \u{2014} the meeting is still on Fri 23 Oct at 16:00".to_string()
            )]
        );
    }

    #[test]
    fn a_change_of_rules_reads_each_conversation_again_but_never_re_asks() {
        let mem = scratch();
        std::fs::write(
            &mem,
            r#"{"C1":{"last":"2026-10-07T11:00","proposed":["2026-10-08T15:00"]}}"#,
        )
        .unwrap();
        let a = agent();
        let llm = Canned(ANSWER.into(), RefCell::new(0));
        a.meeting_pass(&llm, &mem, date!(2026 - 10 - 06)).unwrap();
        assert_eq!(
            *llm.1.borrow(),
            2,
            "read again under the new rules (its meetings, and a title)"
        );
        let calls = a.fm.calls.borrow();
        assert!(
            calls.iter().all(|(_, v)| v["props"]["start"] != "2026-10-08T15:00"),
            "the 8th was already asked"
        );
    }

    #[test]
    fn a_conversation_with_no_new_mail_is_not_read_again() {
        let mem = scratch();
        let a = agent();
        let llm = Canned(ANSWER.into(), RefCell::new(0));
        a.meeting_pass(&llm, &mem, date!(2026 - 10 - 06)).unwrap();
        assert_eq!(a.meeting_pass(&llm, &mem, date!(2026 - 10 - 06)).unwrap(), None);
        assert_eq!(
            *llm.1.borrow(),
            2,
            "the meetings once per new message and the title once, not every minute"
        );
    }

    /// A model answering the title question one way and the meetings question another.
    struct ByAsk(RefCell<usize>);
    impl LlmStep for ByAsk {
        fn complete(&self, system: &str, _u: &str) -> Result<LlmResponse, AgentError> {
            *self.0.borrow_mut() += 1;
            let content = if system.contains("from the mailbox of") {
                r#"{"name": "Maria", "about": "Master student, robotics project"}"#
            } else {
                "[]"
            };
            Ok(LlmResponse { content: content.into(), finish_reason: None, usage: None })
        }
    }

    /// Records every (system, user) pair; answers the title question with a title, else `[]`.
    struct Recording(RefCell<Vec<(String, String)>>);
    impl LlmStep for Recording {
        fn complete(&self, system: &str, user: &str) -> Result<LlmResponse, AgentError> {
            self.0.borrow_mut().push((system.into(), user.into()));
            let content = if user.ends_with("Question: title") {
                r#"{"name": "Maria", "about": "Master student, robotics project"}"#
            } else {
                "[]"
            };
            Ok(LlmResponse { content: content.into(), finish_reason: None, usage: None })
        }
    }

    #[test]
    fn a_shared_mail_prompt_sends_both_questions_the_same_system_prompt() {
        let mem = scratch();
        let mut a = agent();
        a.shared_mail_prompt = true;
        let llm = Recording(RefCell::new(Vec::new()));
        a.meeting_pass(&llm, &mem, date!(2026 - 10 - 06)).unwrap();
        let calls = llm.0.borrow();
        assert_eq!(calls.len(), 2, "the meetings, then the title");
        assert_eq!(calls[0].0, calls[1].0, "one system prompt: the emails are a cached prefix");
        assert!(
            calls[0].1.ends_with("Question: meetings") && calls[1].1.ends_with("Question: title")
        );
        let proposed = a.fm.calls.borrow();
        assert_eq!(
            proposed[0].1["props"],
            json!({ "title": "Maria — Master student, robotics project" })
        );
    }

    #[test]
    fn a_conversation_is_proposed_a_title_once() {
        let mem = scratch();
        let a = agent();
        let llm = ByAsk(RefCell::new(0));
        a.meeting_pass(&llm, &mem, date!(2026 - 10 - 06)).unwrap();
        {
            let calls = a.fm.calls.borrow();
            assert_eq!(calls.len(), 1, "no meeting, so the title alone: {calls:?}");
            assert_eq!(
                calls[0].1["props"],
                json!({ "title": "Maria — Master student, robotics project" })
            );
            assert_eq!(calls[0].1["body"], BODY, "the text is left as it is");
        }
        // Asked once: nothing more, with or without new mail.
        assert_eq!(a.meeting_pass(&llm, &mem, date!(2026 - 10 - 06)).unwrap(), None);
        assert_eq!(a.fm.calls.borrow().len(), 1);
    }
}
