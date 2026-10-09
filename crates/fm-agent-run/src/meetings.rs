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
use fm_agent::LlmStep;
use serde_json::{json, Value};

use crate::fmserve::{Origin, VaultAccess};
use crate::Agent;

/// The fence around the assistant's meeting block in a conversation note.
const TAG: &str = "fm:meeting";

/// Bumped whenever the way meetings are found changes, so every conversation is read once more under
/// the new rules. What was already proposed is kept, so nothing is asked twice. 2: the date is found
/// by Rust in the sentence (or the email before it), not taken from the model (2026-10-09).
const PASS_VERSION: u64 = 2;

#[derive(Default)]
struct Memory {
    /// conversation id -> (`mail_last` when last read, meeting starts already proposed)
    seen: HashMap<String, (String, HashSet<String>)>,
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
        }
        m
    }

    fn save(&self, path: &Path) {
        let mut v: serde_json::Map<String, Value> = self
            .seen
            .iter()
            .map(|(id, (last, proposed))| {
                let mut p: Vec<&String> = proposed.iter().collect();
                p.sort();
                (id.clone(), json!({ "last": last, "proposed": p }))
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
    let place = m.place.as_deref().map(|p| format!(" · {p}")).unwrap_or_default();
    let quote = fm_agent::adjunct::defang(&m.quote, TAG).replace('\n', " ");
    format!(
        "{open}\n> [!note] Meeting found by the assistant\n> {when}{place}\n> «{quote}»\n{end}\n",
        open = fm_agent::adjunct::open_mark(TAG, model),
        end = fm_agent::adjunct::end_mark(TAG),
    )
}

fn day(m: &Meeting) -> String {
    format!("{:04}-{:02}-{:02}", m.day.year(), u8::from(m.day.month()), m.day.day())
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

            let answer = llm
                .complete(meetings::SYSTEM, &meetings::user_prompt(title, &body))
                .map_err(|e| format!("the model did not answer for \u{201c}{title}\u{201d}: {e}"))?
                .content;
            let found = meetings::parse(&answer);
            let mut ok = Vec::new();
            let mut dropped = 0usize;
            for f in &found {
                match meetings::resolve(f, &body, today) {
                    Ok(m) => ok.push(m),
                    Err(_) => dropped += 1,
                }
            }

            let start = conv["start"].as_str();
            let current = start.map(|s| (s, s >= today_s.as_str()));
            let linked = strings(&list["linked"][id]);
            let proposed = mem.seen.get(id).map(|(_, p)| p.clone()).unwrap_or_default();
            let plan = meetings::plan(ok, current, &taken, &linked, &proposed);

            let email = format!("{}@fm-agents.local", self.model);
            let origin = Origin {
                tool: "meetings",
                query: Some(format!("the meetings fixed in \u{201c}{title}\u{201d}")),
                sources: vec![format!("note:{id}")],
            };
            let mut now_proposed = proposed;
            let mut made = 0usize;
            if let Some(m) = &plan.conversation {
                let new_body = with_block(&body, &block(m, &self.model), &self.model);
                let mut props = json!({
                    "start": m.start_stamp(),
                    "due": m.due_stamp(),
                    "addTags": ["meeting"],
                });
                if let Some(p) = &m.place {
                    props["location"] = json!(p);
                }
                self.fm.propose_with(id, &new_body, &props, &self.model, &email, &origin)?;
                now_proposed.insert(m.start_stamp());
                made += 1;
            }
            for m in &plan.new {
                let quote = fm_agent::adjunct::defang(&m.quote, TAG).replace('\n', " ");
                let body = format!(
                    "> [!note] Meeting found by the assistant\n> «{quote}»\n\nFrom the conversation [{title}](note:{id}).\n"
                );
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
                self.fm.propose_note(id, &note, &self.model, &email, &origin)?;
                now_proposed.insert(m.start_stamp());
                made += 1;
            }
            mem.seen.insert(id.to_string(), (last.to_string(), now_proposed));
            mem.save(memory);
            let skipped = if dropped > 0 {
                format!(", {dropped} left out (not checkable)")
            } else {
                String::new()
            };
            return Ok(Some(format!(
                "read \u{201c}{title}\u{201d}: {made} meeting proposal(s){skipped}"
            )));
        }
        Ok(None)
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
    }

    #[derive(Default)]
    struct MailVault {
        calls: RefCell<Vec<(String, Value)>>,
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
        fn search(&self, _: &str) -> Result<Value, String> {
            Ok(json!([]))
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
        fn present(&self, _: &str) {}
        fn mail_conversations(&self) -> Result<Value, String> {
            Ok(json!({
                "conversations": [{ "id": "C1", "title": "Revisione", "last": "2026-10-07T11:00", "start": null }],
                "taken": [], "linked": {}
            }))
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
            max_reply_chars: 1000,
            retrieve: 0,
            history_budget: 1000,
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
        assert!(said.contains("2 meeting proposal(s)") && said.contains("1 left out"), "{said}");

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
        assert_eq!(*llm.1.borrow(), 1, "read again under the new rules");
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
        assert_eq!(*llm.1.borrow(), 1, "the model is asked once per new message, not every minute");
    }
}
