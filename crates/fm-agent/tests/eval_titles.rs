//! The mail pass's title question (`fm_agent::titles`), against
//! `methods/meetings/eval/titles.json`. Until 2026-10-19 the title had no eval; the layout study
//! needed one, because a layout that lets the title call reuse the meetings call's cached emails
//! changes the title's prompt (speed study §2.7).
//!
//! A case passes when the answer survives [`titles::check`] (the name is in the emails and is not
//! the owner; two to six words of description), names the expected person, and its description
//! holds one of the case's `about_any` words.
//!
//! **In the gate:** the cases are checked to mean what they say. **By hand** (`#[ignore]`d), against
//! a running `llama-server`, at the runner's own settings (temperature 0.2, `OpenAiStep::local`):
//!
//! ```text
//! FM_MODEL_PORT=8081 FM_MODEL=qwen3-vl-4b [FM_LAYOUT=old|first|b|c] \
//!   pixi run cargo test -p fm-agent --test eval_titles -- --ignored --nocapture
//! ```

mod layout;

use fm_agent::openai::OpenAiStep;
use fm_agent::textmatch::loose;
use fm_agent::titles;
use fm_agent::LlmStep;
use serde_json::Value;

const CASES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../methods/meetings/eval/titles.json");

fn load() -> Value {
    serde_json::from_str(&std::fs::read_to_string(CASES).expect("the case file")).expect("JSON")
}

/// Whether one answer passes its case, and why not when it does not.
fn score(answer: &str, case: &Value) -> Result<String, String> {
    let body = case["body"].as_str().unwrap();
    let me = case["me"].as_str().unwrap();
    let (name, about) = titles::parse(answer).ok_or("no {name, about} in the answer")?;
    let title = titles::check(&name, &about, body, Some(me))
        .ok_or_else(|| format!("refused by the checks: {name:?} / {about:?}"))?;
    if loose(&name) != loose(case["name"].as_str().unwrap()) {
        return Err(format!("named {name:?}"));
    }
    let about_lc = about.to_lowercase();
    let words = case["about_any"].as_array().unwrap();
    if !words.iter().filter_map(Value::as_str).any(|w| about_lc.contains(w)) {
        return Err(format!("described as {about:?}"));
    }
    Ok(title)
}

#[test]
fn every_case_has_a_reachable_answer() {
    for case in load()["cases"].as_array().expect("cases") {
        let id = case["id"].as_str().unwrap();
        let body = case["body"].as_str().unwrap();
        let (name, me) = (case["name"].as_str().unwrap(), case["me"].as_str().unwrap());
        assert!(loose(body).contains(&loose(me)), "{id}: the owner writes in the thread");
        assert_ne!(loose(name), loose(me), "{id}: the title names someone else");
        let word = case["about_any"][0].as_str().expect("at least one description word");
        let perfect = serde_json::json!({ "name": name, "about": format!("{word} in the emails") });
        assert!(score(&perfect.to_string(), case).is_ok(), "{id}: a perfect answer fails");
    }
}

#[test]
fn the_owner_or_a_wrong_role_fails() {
    let file = load();
    let case = &file["cases"][0];
    let owner = serde_json::json!({ "name": case["me"], "about": "Master student, thesis" });
    assert!(score(&owner.to_string(), case).is_err(), "the owner is never the title");
    let role = serde_json::json!({ "name": case["name"], "about": "Sales engineer, sensors" });
    assert!(score(&role.to_string(), case).is_err(), "the description must fit the emails");
}

#[test]
#[ignore = "needs a live local model server; set FM_MODEL_PORT (and FM_MODEL)"]
fn a_live_model_against_the_cases() {
    let Some(port) = std::env::var("FM_MODEL_PORT").ok().and_then(|p| p.parse::<u16>().ok()) else {
        eprintln!("skip: set FM_MODEL_PORT");
        return;
    };
    let model = std::env::var("FM_MODEL").unwrap_or_else(|_| "qwen3-vl-4b".into());
    let layout = layout::chosen();
    let llm = OpenAiStep::local(port, &model); // the runner's own client settings
    let (mut passed, mut seconds) = (0, 0.0);
    let cases = load()["cases"].as_array().expect("cases").clone();
    for case in &cases {
        let id = case["id"].as_str().unwrap();
        let (system, user) = layout::prompt(
            &layout,
            layout::Question::Title,
            case["title"].as_str().unwrap(),
            case["body"].as_str().unwrap(),
            case["me"].as_str().unwrap(),
        );
        let t0 = std::time::Instant::now();
        let answer = llm
            .complete(&system, &user)
            .map(|r| r.content)
            .unwrap_or_else(|e| format!("ERROR: {e}"));
        seconds += t0.elapsed().as_secs_f64();
        match score(&answer, case) {
            Ok(title) => {
                passed += 1;
                println!("{id:26} PASS  {title}");
            }
            Err(why) => println!("{id:26} FAIL  {why}"),
        }
    }
    println!("== {model} / layout {layout}: titles {passed}/{} · {seconds:.1}s total", cases.len());
}
