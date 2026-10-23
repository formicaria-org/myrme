//! M1's eval — meetings from mail, against `methods/meetings/eval/cases.json`.
//!
//! **In the gate:** every case's `oracle` (a hand-written perfect answer) is replayed through the
//! same `parse` → `resolve` production runs, and must give the case's `expect`. That checks two
//! things at once: that the cases mean what they say, and that the Rust half of the method still
//! reads dates, times, call-offs and the past the way the cases need. Every `recorded` answer from a
//! live run is replayed too and its score printed, so a change to the Rust shows what it does to real
//! models' answers without running one.
//!
//! **By hand** (`#[ignore]`d — the gate ships no model), against a running `llama-server`:
//!
//! ```text
//! FM_MODEL_PORT=8081 FM_MODEL=qwen3-4b-2507 [FM_EVAL_MODE=free|constrained|both] [FM_EVAL_RECORD=1] \
//!   pixi run cargo test -p fm-agent --test eval_meetings -- --ignored --nocapture
//! ```
//!
//! `FM_EVAL_RECORD=1` writes each answer back into the case file under `recorded["<model>/<mode>"]`.

mod layout;

use fm_agent::eval::{meeting_lines, meetings_match, well_formed_array, Tally};
use fm_agent::meetings;
use fm_agent::openai::OpenAiStep;
use fm_agent::zone::Here;
use fm_agent::LlmStep;
use serde_json::Value;
use time::Date;

const CASES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../methods/meetings/eval/cases.json");

fn load() -> Value {
    // `FM_EVAL_CASES` reads another case file (e.g. a private one from real mail), never written
    // back to.
    let path = std::env::var("FM_EVAL_CASES").unwrap_or_else(|_| CASES.to_string());
    serde_json::from_str(&std::fs::read_to_string(path).expect("the case file")).expect("JSON")
}

fn day(s: &str) -> Date {
    Date::parse(s, time::macros::format_description!("[year]-[month]-[day]"))
        .expect("a YYYY-MM-DD day")
}

/// The case's reader's own clock, when it names one (`"here": "Asia/Tokyo"`).
fn here(case: &Value) -> Option<Here> {
    case["here"].as_str().map(|z| Here::parse(z).expect("a zone name or offset"))
}

fn strings(v: &Value) -> Vec<String> {
    v.as_array().into_iter().flatten().filter_map(Value::as_str).map(str::to_string).collect()
}

#[test]
fn every_oracle_answer_resolves_to_what_its_case_expects() {
    let file = load();
    let mut wrong = Vec::new();
    for case in file["cases"].as_array().expect("cases") {
        let id = case["id"].as_str().unwrap();
        let body = case["body"].as_str().unwrap();
        let today = day(case["today"].as_str().unwrap());
        let expect = strings(&case["expect"]);
        let got = meeting_lines(&case["oracle"].to_string(), body, today, here(case).as_ref());
        if !meetings_match(&expect, &got) {
            wrong.push(format!("{id}: expected {expect:?}, got {got:?}"));
        }
    }
    assert!(wrong.is_empty(), "cases whose perfect answer does not resolve:\n{}", wrong.join("\n"));
}

#[test]
fn recorded_answers_are_scored() {
    let file = load();
    let mut tallies: std::collections::BTreeMap<String, Tally> = Default::default();
    for case in file["cases"].as_array().expect("cases") {
        let body = case["body"].as_str().unwrap();
        let today = day(case["today"].as_str().unwrap());
        let expect = strings(&case["expect"]);
        for (who, answer) in case["recorded"].as_object().into_iter().flatten() {
            let answer = answer.as_str().unwrap_or_default();
            let exact =
                meetings_match(&expect, &meeting_lines(answer, body, today, here(case).as_ref()));
            tallies.entry(who.clone()).or_default().add(well_formed_array(answer), exact);
        }
    }
    for (who, t) in &tallies {
        println!("{who:32} {t}");
    }
}

#[test]
#[ignore = "needs a live local model server; set FM_MODEL_PORT (and FM_MODEL)"]
fn a_live_model_against_the_cases() {
    let Some(port) = std::env::var("FM_MODEL_PORT").ok().and_then(|p| p.parse::<u16>().ok()) else {
        eprintln!("skip: set FM_MODEL_PORT");
        return;
    };
    let model = std::env::var("FM_MODEL").unwrap_or_else(|_| "qwen3-4b-2507".into());
    let modes: Vec<&str> = match std::env::var("FM_EVAL_MODE").as_deref() {
        Ok("free") => vec!["free"],
        Ok("constrained") => vec!["constrained"],
        _ => vec!["free", "constrained"],
    };
    let record = std::env::var("FM_EVAL_RECORD").is_ok() && std::env::var("FM_EVAL_CASES").is_err();
    let llm = OpenAiStep::local(port, &model)
        .with_temperature(0.0)
        .with_max_tokens(meetings::ANSWER_TOKENS);
    let schema = meetings::answer_schema();

    let mut file = load();
    for mode in &modes {
        let mut tally = Tally::default();
        let mut seconds = 0.0;
        for case in file["cases"].as_array_mut().expect("cases") {
            let id = case["id"].as_str().unwrap().to_string();
            let body = case["body"].as_str().unwrap().to_string();
            let today = day(case["today"].as_str().unwrap());
            let zone = here(case);
            let expect = strings(&case["expect"]);
            // The prompt layout under study (`FM_LAYOUT`, `tests/layout`); `old` is production's.
            let (system, prompt) = layout::prompt(
                &layout::chosen(),
                layout::Question::Meetings,
                case["title"].as_str().unwrap(),
                &body,
                "the mailbox's owner",
            );
            let t0 = std::time::Instant::now();
            let reply = if *mode == "constrained" {
                llm.complete_constrained(&system, &prompt, &schema)
            } else {
                llm.complete(&system, &prompt)
            };
            seconds += t0.elapsed().as_secs_f64();
            let answer = match reply {
                Ok(r) => r.content,
                Err(e) => format!("ERROR: {e}"),
            };
            let got = meeting_lines(&answer, &body, today, zone.as_ref());
            let exact = meetings_match(&expect, &got);
            let formed = well_formed_array(&answer);
            tally.add(formed, exact);
            println!(
                "{mode:11} {id:22} {} {}  expected {expect:?} got {got:?}",
                if exact { "PASS" } else { "FAIL" },
                if formed { "  " } else { "!f" },
            );
            if record {
                case["recorded"][format!("{model}/{mode}")] = Value::String(answer);
            }
        }
        let layout = layout::chosen();
        println!("== {model} / {mode} / layout {layout}: {tally} · {seconds:.1}s total\n");
    }
    if record {
        std::fs::write(CASES, serde_json::to_string_pretty(&file).unwrap() + "\n").unwrap();
    }
}
