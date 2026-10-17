//! The resident agent's **watch loop**, extracted so it runs the same everywhere: the desktop
//! `agent-serve` binary (over HTTP via [`FmServe`]) and the in-process mobile app (over
//! [`fm_app::dispatch`] via a `DispatchVault`) both call [`serve_loop`] with their own
//! [`VaultAccess`](crate::fmserve::VaultAccess) and their own model lifecycle. The loop itself knows
//! nothing about which — that is the whole point of the seam.
//!
//! [`FmServe`]: crate::fmserve::FmServe

use crate::fmserve::VaultAccess;
use crate::Agent;
use fm_agent::convo;
use serde_json::json;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::io::Write;
use std::path::Path;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// Wait for the model server to answer `/health` (up to ~40 s). Shared by the desktop binary and the
/// in-process app — both launch a local `llama-server` and talk to it over `127.0.0.1`.
pub fn wait_ready(port: u16) {
    let req =
        format!("GET /health HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n");
    for _ in 0..40 {
        if let Ok(raw) =
            fm_agent::http::send("127.0.0.1", port, req.as_bytes(), Duration::from_secs(2))
        {
            if String::from_utf8_lossy(&raw).contains("\"status\":\"ok\"") {
                return;
            }
        }
        std::thread::sleep(Duration::from_secs(1));
    }
}

/// Watch a vault's discussions and answer `@name` mentions until the model goes away.
///
/// `finished` reports whether the model process has ended (the loop then stops); `stop_model` is the
/// off-switch the loop trips when the *vault* becomes unreachable (desktop: fm-serve closed). In
/// process where the vault can't go away, `alive()` stays true and only `finished` ends the loop.
pub fn serve_loop<V: VaultAccess>(
    agent: &Agent<V>,
    name: &str,
    runtime: &Path,
    poll_secs: u64,
    finished: &dyn Fn() -> bool,
    stop_model: &dyn Fn(),
) {
    // Message ids already seen, so each is answered at most once. Seed it with every message that
    // already exists, so a fresh start (or a restart) answers only what arrives *while it watches* —
    // never the historical backlog of every discussion.
    let mut handled: HashSet<String> = HashSet::new();
    // Last message count we saw per discussion, from the discussions list. A poll skips any
    // discussion whose count is unchanged — no thread fetch, no work — so it can run often (snappy
    // pickup) while costing only one `discussions()` call plus a `thread()` for what actually moved.
    let mut last_count: HashMap<String, u64> = HashMap::new();
    if let Ok(discs) = agent.fm.discussions() {
        if let Some(arr) = discs.as_array() {
            for d in arr {
                let Some(id) = d["id"].as_str() else { continue };
                last_count.insert(id.to_string(), d["count"].as_u64().unwrap_or(0));
                let Ok(view) = agent.fm.thread(id) else { continue };
                let Some(msgs) = view["messages"].as_array() else { continue };
                for m in msgs {
                    if let Some(mid) = m["id"].as_str() {
                        handled.insert(mid.to_string());
                    }
                }
            }
        }
    }
    // Poll fast right after activity, then ease off toward POLL_MAX while idle — snappy when the user
    // is talking, near-free when they are not (the Android-battery concern). The heartbeat and the
    // full rescan are time-based, so they stay regular even as the poll backs off.
    const HEARTBEAT: Duration = Duration::from_secs(5);
    const FORCE_SCAN: Duration = Duration::from_secs(60);
    let poll_min = Duration::from_secs(poll_secs.max(1));
    let poll_max = Duration::from_secs(5);
    let mut interval = poll_min;
    let mut last_full_scan = Instant::now();
    // The meeting pass found a conversation to read last time, so more may be waiting: read the next
    // one at the next poll rather than a minute later. Five changed conversations used to take five
    // minutes to reach Collaboration (2026-10-15).
    let mut mail_backlog = false;
    // **New mail wakes the pass** (2026-10-16): the mail reader raises a counter whenever it writes
    // something, and it is asked at every poll — seconds, not the minute rescan — so a reply reaches
    // the meeting pass as soon as it reaches the vault. `None` until first asked, which counts as new.
    let mut mail_seen: Option<u64> = None;
    // Present as **Myrme**, the one name a person types on every device (2026-10-18); the model's
    // name still addresses it (`convo::addressed_here`), it is just not what the `@` list offers.
    agent.fm.present(convo::MYRME, agent.image_reader().as_ref()); // show online at once, before the first heartbeat tick
    let mut last_hb = Instant::now();
    loop {
        std::thread::sleep(interval);
        if finished() {
            break;
        }
        if !agent.fm.alive() {
            stop_model();
            break;
        }

        // Heartbeat at most every HEARTBEAT, independent of the (backing-off) poll cadence.
        if last_hb.elapsed() >= HEARTBEAT {
            agent.fm.present(convo::MYRME, agent.image_reader().as_ref());
            last_hb = Instant::now();
        }

        // Cheap count-skip most polls; a full rescan only occasionally, so a message can never be
        // *permanently* missed if a count ever fails to reflect a new one.
        let force_scan = last_full_scan.elapsed() >= FORCE_SCAN;
        if force_scan {
            last_full_scan = Instant::now();
        }

        let mut worked = false; // answered anything this poll? then snap back to fast polling
        let discs = agent.fm.discussions().ok();
        for d in discs.as_ref().and_then(|d| d.as_array()).into_iter().flatten() {
            let Some(id) = d["id"].as_str() else { continue };
            let count = d["count"].as_u64().unwrap_or(0);
            if !force_scan && last_count.get(id) == Some(&count) {
                continue; // nothing new here since we last looked — skip the thread fetch entirely
            }
            let Ok(view) = agent.fm.thread(id) else { continue };
            let Some(msgs) = view["messages"].as_array() else { continue };
            // Answer *every* pending mention in order, not just the last; coalesce identical resends.
            let mut asked_this_pass: HashSet<String> = HashSet::new();
            let mut posted = 0u64;
            for m in msgs {
                let (Some(mid), Some(body)) = (m["id"].as_str(), m["body"].as_str()) else {
                    continue;
                };
                if !handled.insert(mid.to_string()) {
                    continue;
                }
                // `@Myrme` asks the copy on the device the message was written on; `@Myrme·phone`
                // asks one by name; the model's own name still works (`convo::addressed_here`).
                let written_on = m["props"]["device"].as_str();
                let Some(intent) = convo::addressed_here(body, name, &agent.device, written_on)
                else {
                    continue;
                };
                if !asked_this_pass.insert(intent.ask.clone()) {
                    continue;
                }
                worked = true;

                let question = intent.ask.clone();
                let marks = RefCell::new(Vec::<(String, Instant)>::new());
                let on_stage = |stage: &str| {
                    marks.borrow_mut().push((stage.to_string(), Instant::now()));
                    agent.fm.activity(id, stage, &question);
                };
                // Allow the user's EXPLICIT `/propose` / `/research` here: the discussion root is always
                // a note, so a proposal to it is valid, and `handle` only acts on those commands when the
                // user actually typed them (a plain mention still just chats). This is the path the
                // frontend uses, so without it `/research` would silently degrade to a chat answer.
                let result = agent.handle(id, &intent, &on_stage);
                log_timing(runtime, id, &question, &marks.borrow(), Instant::now(), result.is_ok());
                match result {
                    Ok((reply, reply_id)) => {
                        if let Some(rid) = reply_id {
                            handled.insert(rid);
                        }
                        posted += 1;
                        println!(
                            "@{name} replied in {}: {}",
                            &id[..8.min(id.len())],
                            reply.chars().take(60).collect::<String>()
                        );
                    }
                    Err(e) => {
                        eprintln!("turn failed in {id}: {e}");
                        let notice = format!(
                            "⚠️ I couldn't finish that one — {e}. Try again, or simplify it."
                        );
                        let (who, email) = agent.author();
                        if let Ok(meta) = agent.fm.reply_as(id, &notice, &who, &email) {
                            posted += 1;
                            if let Some(rid) = meta["id"].as_str() {
                                handled.insert(rid.to_string());
                            }
                        }
                    }
                }
                agent.fm.activity_done(id);
            }
            last_count.insert(id.to_string(), count + posted);
        }
        // **The meeting pass**, on the minute rescan — or at once while a backlog remains — and only when
        // no one was just answered: one email conversation per poll, so a question in a discussion is
        // always looked at first and never waits behind mail.
        // Its failures (the model busy or gone) are logged and retried next minute; they never stop
        // the loop. `decisions.md` 2026-10-09, *the assistant proposes meetings*.
        let novelty = agent.fm.mail_novelty();
        let mail_new = novelty.is_some() && novelty != mail_seen;
        if (force_scan || mail_backlog || mail_new) && !worked {
            mail_seen = novelty;
            // The answer is capped so prompt + answer always fit the model's window together
            // (`fm_agent::meetings::TEXT_TOKENS`).
            let llm = agent
                .llm()
                .with_temperature(0.0)
                .with_max_tokens(fm_agent::meetings::ANSWER_TOKENS);
            // UTC's day: this process cannot read the machine's offset (`fm_core::events`). Within a
            // few hours of midnight a meeting "today" may be judged by the neighbouring day.
            let today = time::OffsetDateTime::now_utc().date();
            mail_backlog = false;
            match agent.meeting_pass(&llm, &runtime.join("mail-meetings.json"), today) {
                Ok(Some(said)) => {
                    println!("@{name} {said}");
                    mail_backlog = true;
                }
                Ok(None) => {}
                Err(e) => eprintln!("meeting pass: {e}"),
            }
        }
        interval = if worked { poll_min } else { (interval * 2).min(poll_max) };
    }
}

/// Append one turn's per-stage timing to `<runtime>/timing.jsonl` and echo a one-line summary, so it
/// is clear where the orchestration spends its time (the LLM is often *not* the slow part). Best-effort.
fn log_timing(
    runtime: &Path,
    disc: &str,
    question: &str,
    marks: &[(String, Instant)],
    ended: Instant,
    ok: bool,
) {
    if marks.is_empty() {
        return;
    }
    let dur = |i: usize, t: &Instant| {
        let next = marks.get(i + 1).map(|(_, t2)| *t2).unwrap_or(ended);
        next.saturating_duration_since(*t).as_millis() as u64
    };
    let mut stages = serde_json::Map::new();
    for (i, (name, t)) in marks.iter().enumerate() {
        stages.insert(name.clone(), json!(dur(i, t)));
    }
    let total_ms = ended.saturating_duration_since(marks[0].1).as_millis() as u64;
    let slowest = marks
        .iter()
        .enumerate()
        .map(|(i, (name, t))| (name.as_str(), dur(i, t)))
        .max_by_key(|(_, ms)| *ms)
        .map(|(name, ms)| format!("{name} {ms}ms"))
        .unwrap_or_default();
    println!("  ⏱ turn {total_ms}ms (slowest: {slowest})");

    let ts = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let line = json!({
        "ts": ts, "disc": disc, "q": question.chars().take(80).collect::<String>(),
        "total_ms": total_ms, "ok": ok, "stages": stages,
    });
    let path = runtime.join("timing.jsonl");
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
        let _ = writeln!(f, "{line}");
    }
}
