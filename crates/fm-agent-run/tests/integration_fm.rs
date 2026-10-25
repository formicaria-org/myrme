//! **Myrme against a real formicaria** — `docs/context/plan.md`, Phase B step 4; the machine check of
//! the contract's parts 2–4 (`docs/context/contract.md`): the `/api` door, and the replies and
//! proposals Myrme writes.
//!
//! A real `fm-serve` — built from the formicaria release Myrme pins, by `pixi run integration` — runs
//! on a vault in a temporary folder, with its configuration, git identity and home there too, so
//! nothing on the machine is read or written. The model is played by a few lines of socket code that
//! answer every completion with one fixed reply, so no model is needed and the result does not
//! depend on one. Everything between the two is real: the runner's `FmServe` client, the HTTP
//! server, `dispatch`, the store, git.
//!
//! `#[ignore]`d: the gate has no `fm-serve`. Run it with `pixi run integration`, or set
//! `FM_SERVE_BIN` to a built `fm-serve` and pass `--ignored`.

use fm_agent_run::fm_agent::convo;
use fm_agent_run::fmserve::{FmServe, VaultAccess};
use fm_agent_run::Agent;
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

/// A `fm-serve` that is killed however the test ends.
struct Server(Child);
impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()
}

/// One POST to `fm-serve`, for the commands the runner itself never sends (making the note,
/// finding its proposal). The runner's own calls go through its real client.
fn api(port: u16, cmd: &str, args: Value) -> Value {
    let body = args.to_string();
    let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
    write!(
        s,
        "POST /api/{cmd} HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\n\
         Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
    .unwrap();
    let mut out = String::new();
    s.read_to_string(&mut out).unwrap();
    let (head, resp) = out.split_once("\r\n\r\n").unwrap();
    assert!(head.starts_with("HTTP/1.1 200"), "{cmd}: {head}\n{resp}");
    serde_json::from_str(resp.trim()).unwrap_or(Value::Null)
}

/// Stands in for the model: answers every chat completion with `answer`, and counts the calls.
fn fake_model(answer: &'static str) -> (u16, std::sync::Arc<std::sync::atomic::AtomicUsize>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let counted = calls.clone();
    std::thread::spawn(move || {
        for sock in listener.incoming() {
            let Ok(mut sock) = sock else { continue };
            // Read the headers, then exactly the body they announce.
            let mut req = Vec::new();
            let mut buf = [0u8; 4096];
            let body_start = loop {
                let n = sock.read(&mut buf).unwrap_or(0);
                if n == 0 {
                    break None;
                }
                req.extend_from_slice(&buf[..n]);
                if let Some(i) = req.windows(4).position(|w| w == b"\r\n\r\n") {
                    break Some(i + 4);
                }
            };
            let Some(start) = body_start else { continue };
            let head = String::from_utf8_lossy(&req[..start]).to_lowercase();
            let len = head
                .lines()
                .find_map(|l| l.strip_prefix("content-length:"))
                .and_then(|v| v.trim().parse::<usize>().ok())
                .unwrap_or(0);
            while req.len() < start + len {
                let n = sock.read(&mut buf).unwrap_or(0);
                if n == 0 {
                    break;
                }
                req.extend_from_slice(&buf[..n]);
            }
            counted.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let completion = json!({
                "choices": [{ "message": { "content": answer }, "finish_reason": "stop" }]
            })
            .to_string();
            let _ = write!(
                sock,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\
                 Connection: close\r\n\r\n{completion}",
                completion.len()
            );
        }
    });
    (port, calls)
}

/// Stands in for a model that **streams**: answers one chat completion as server-sent events, one
/// piece every `gap`, the way llama-server writes a reply token by token.
fn streaming_model(pieces: &'static [&'static str], gap: Duration) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        let Some(Ok(mut sock)) = listener.incoming().next() else { return };
        let mut req = Vec::new();
        let mut buf = [0u8; 4096];
        while !String::from_utf8_lossy(&req).contains("\"stream\":true") {
            let n = sock.read(&mut buf).unwrap_or(0);
            if n == 0 {
                return;
            }
            req.extend_from_slice(&buf[..n]);
        }
        let _ = write!(sock, "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\n\r\n");
        for piece in pieces {
            let event = json!({ "choices": [{ "delta": { "content": piece } }] });
            let _ = write!(sock, "data: {event}\n\n");
            let _ = sock.flush();
            std::thread::sleep(gap);
        }
        let done = json!({ "choices": [{ "delta": {}, "finish_reason": "stop" }] });
        let _ = write!(sock, "data: {done}\n\ndata: [DONE]\n\n");
    });
    port
}

fn agent(fm_port: u16, model_port: u16) -> Agent<FmServe> {
    Agent {
        fm: FmServe::local(fm_port),
        model_port,
        model: "test-model".into(),
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
        max_reply_chars: 600,
        history_budget: 4000,
        device: "laptop".into(),
    }
}

/// Start `fm-serve` on a fresh vault in its own temporary home, and wait until it answers.
fn start(root: &std::path::Path) -> (Server, u16) {
    let bin = std::env::var("FM_SERVE_BIN").expect("FM_SERVE_BIN");
    let port = free_port();
    let vault = root.join("vault");
    std::fs::create_dir_all(&vault).unwrap();
    let child = Command::new(bin)
        .env_clear()
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .env("HOME", root)
        .env("XDG_CONFIG_HOME", root.join("config"))
        .env("FM_ADDR", format!("127.0.0.1:{port}"))
        .env("FM_VAULT", &vault)
        .env("FM_VAULTS", root.join("vaults.json"))
        .env("FM_OPEN", "0")
        .env("FM_AUTO_SHUTDOWN", "0")
        .env("GIT_AUTHOR_NAME", "Integration Test")
        .env("GIT_AUTHOR_EMAIL", "test@example.org")
        .env("GIT_COMMITTER_NAME", "Integration Test")
        .env("GIT_COMMITTER_EMAIL", "test@example.org")
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .spawn()
        .expect("fm-serve starts");
    let server = Server(child);
    let fm = FmServe::local(port);
    let t0 = Instant::now();
    while !fm.alive() {
        assert!(t0.elapsed() < Duration::from_secs(30), "fm-serve did not answer within 30 s");
        std::thread::sleep(Duration::from_millis(200));
    }
    (server, port)
}

fn temp_root(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("myrme-it-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    root
}

/// `/propose` through the real door: the note is made in formicaria, the edit lands on the whole
/// note (longer than the part the model is shown), the reply is posted in the note's discussion, and
/// formicaria hands the proposal back with exactly that body.
#[test]
#[ignore = "needs a built fm-serve; run `pixi run integration`"]
fn a_proposed_edit_goes_through_formicaria_and_comes_back_whole() {
    let root = temp_root("propose");
    let (_server, port) = start(&root);

    let tail = "The last paragraph, far past what the model is shown.";
    let text = format!("Survey notes\n\n{}\n\n{tail}", "A sentence of field notes. ".repeat(80));
    let made = api(port, "capture", json!({ "body": text, "vault": "" }));
    let id = made["id"].as_str().expect("capture answers with the note's id").to_string();

    let fm = FmServe::local(port);
    let before = fm.get(&id).unwrap()["body"].as_str().unwrap().to_string();
    assert!(before.contains(tail) && before.chars().count() > 1200);

    let (model_port, calls) =
        fake_model(r#"{"edits":[{"op":"append","old":"","new":"Next survey: 14 November"}]}"#);
    let intent = convo::parse("add a final line: Next survey: 14 November /propose");
    let (reply, reply_id) = agent(port, model_port).handle(&id, &intent, &|_| {}).unwrap();

    assert_eq!(
        calls.load(std::sync::atomic::Ordering::SeqCst),
        2,
        "the kind of change, then the edits"
    );
    assert_eq!(reply, fm_agent_run::fm_agent::PROPOSAL_ACK);
    assert!(reply_id.is_some(), "the reply was posted and formicaria gave it an id");

    // The reply is in the note's discussion, under Myrme's name for this device.
    let thread = fm.thread(&id).unwrap().to_string();
    assert!(thread.contains(fm_agent_run::fm_agent::PROPOSAL_ACK), "reply not in thread: {thread}");

    // formicaria hands the proposal back: everything the note had, then the new line.
    let proposed =
        fm.open_proposal_body(&id).expect("formicaria has an open proposal for the note");
    assert!(
        proposed.trim_end().ends_with("Next survey: 14 November"),
        "the edit is at the end: …{}",
        &proposed[proposed.len().saturating_sub(120)..]
    );
    assert!(proposed.starts_with(before.trim_end()), "the note's own text is kept, whole");

    // The note itself is unchanged until a person accepts.
    assert_eq!(fm.get(&id).unwrap()["body"].as_str().unwrap(), before);
    let _ = std::fs::remove_dir_all(&root);
}

/// **The reply as it is written** (contract 1.9): while a slow model streams a chat reply, formicaria's
/// activity poll shows the reply so far, and the reply itself is then posted whole. A formicaria
/// before drafts answers the poll without a `draft` field; it is still asked, and ignores it.
#[test]
#[ignore = "needs a built fm-serve; run `pixi run integration`"]
fn a_chat_reply_shows_in_formicaria_while_it_is_written() {
    let root = temp_root("draft");
    let (_server, port) = start(&root);
    let made = api(port, "capture", json!({ "body": "Field notes on the survey.", "vault": "" }));
    let id = made["id"].as_str().unwrap().to_string();

    const PIECES: &[&str] = &["The notes ", "describe ", "one survey, ", "done in ", "one day."];
    let model_port = streaming_model(PIECES, Duration::from_millis(500));
    let turn = {
        let id = id.clone();
        std::thread::spawn(move || {
            let intent = convo::parse("@Myrme what are these notes about?");
            agent(port, model_port).handle(&id, &intent, &|_| {}).unwrap()
        })
    };
    let mut drafts = Vec::new();
    let mut knows_drafts = false;
    while !turn.is_finished() {
        let a = api(port, "agent_activity_poll", json!({ "discussion": id }));
        if let Some(d) = a.get("draft").and_then(Value::as_str) {
            knows_drafts = true;
            if !d.is_empty() && drafts.last().map(String::as_str) != Some(d) {
                drafts.push(d.to_string());
            }
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    let (reply, _) = turn.join().unwrap();
    assert_eq!(reply, PIECES.concat());
    assert!(FmServe::local(port).thread(&id).unwrap().to_string().contains(&reply));
    if knows_drafts {
        assert!(!drafts.is_empty(), "no reply so far was ever shown");
        assert!(drafts.iter().all(|d| reply.starts_with(d.as_str())), "{drafts:?}");
        assert!(
            drafts.first().unwrap().len() < reply.len(),
            "it showed before the end: {drafts:?}"
        );
    } else {
        eprintln!(
            "this formicaria predates drafts: the poll has no `draft`; the reply still lands"
        );
    }
    let _ = std::fs::remove_dir_all(&root);
}

/// An edit that points at words the note does not have: nothing is proposed, and the discussion
/// says so — through the real door, so formicaria really holds no proposal afterwards.
#[test]
#[ignore = "needs a built fm-serve; run `pixi run integration`"]
fn an_edit_that_cannot_be_placed_proposes_nothing_in_formicaria() {
    let root = temp_root("unplaced");
    let (_server, port) = start(&root);
    let made = api(port, "capture", json!({ "body": "Groceries\n\n- eggs\n- bread", "vault": "" }));
    let id = made["id"].as_str().unwrap().to_string();

    let (model_port, _) =
        fake_model(r#"{"edits":[{"op":"delete","old":"no such words","new":""}]}"#);
    let intent = convo::parse("remove the milk /propose");
    let (reply, _) = agent(port, model_port).handle(&id, &intent, &|_| {}).unwrap();

    assert!(reply.starts_with("I couldn't find where in the note"), "{reply}");
    let fm = FmServe::local(port);
    assert_eq!(fm.open_proposal_body(&id), None, "no proposal may exist");
    let _ = std::fs::remove_dir_all(&root);
}

/// **formicaria's clock through the real door** (contract 1.8): a device that chose its zone in
/// Settings → Clock answers it at `/api/clock`, and Myrme reads that clock — zone, today, instants —
/// never its own. A formicaria from before 0.19 has no such route: then nothing is asserted but that
/// the assistant does not invent one.
#[test]
#[ignore = "needs a built fm-serve; run `pixi run integration`"]
fn the_assistant_reads_formicaria_s_clock() {
    let root = temp_root("clock");
    let dir = root.join("config").join("formicaria");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("clock.json"), r#"{"zone": "Asia/Kathmandu"}"#).unwrap();
    let (_server, port) = start(&root);
    let a = agent(port, 0);
    let Some(answer) = a.fm.clock() else {
        assert!(a.clock().is_none_or(|c| !c.instants), "an older formicaria takes no instants");
        eprintln!("this formicaria has no /api/clock (before 0.19): the fallback holds");
        let _ = std::fs::remove_dir_all(&root);
        return;
    };
    assert_eq!(answer["zone"], "Asia/Kathmandu", "{answer}");
    let c = a.clock().expect("a clock");
    assert_eq!(c.here.label(), "Asia/Kathmandu");
    assert!(c.instants, "formicaria 0.19 takes a start as a moment");
    assert_eq!(answer["today"].as_str(), Some(c.today.to_string().as_str()));
    let _ = std::fs::remove_dir_all(&root);
}

/// formicaria's half of the recipe-heading contract: its recipe template names every heading a
/// recipe note from Myrme has (`fm_agent::recipe::TEMPLATE_HEADINGS`; Myrme's half is a unit test
/// there). Reads the pinned checkout `pixi run integration` builds from (`FM_SRC`).
#[test]
#[ignore = "needs the pinned formicaria checkout; run `pixi run integration`"]
fn formicaria_s_recipe_template_has_the_headings_myrme_writes() {
    let src = std::env::var("FM_SRC").expect("FM_SRC");
    let ui = std::fs::read_to_string(format!("{src}/ui/src/lib/templates.ts")).unwrap();
    for (key, heading) in fm_agent_run::fm_agent::recipe::TEMPLATE_HEADINGS {
        assert!(ui.contains(&format!("{key}: '{heading}'")), "the template names {heading}");
    }
}
