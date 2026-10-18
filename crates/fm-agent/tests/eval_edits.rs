//! M5's eval — note edits (`/propose`), against `methods/edits/eval/cases.json`.
//!
//! The case's note and request go through the real turn ([`StudyAssistant::turn`]) with the note
//! given exactly as the runner gives it (`fm-agent-run`'s `host_note`: the first [`HOST_NOTE_CHARS`]
//! characters, labelled as this note); the edits it answers with are applied to the whole note
//! ([`fm_agent::edits::apply`]), and the result is checked with [`fm_agent::eval::edit_outcome`].
//!
//! **In the gate:** each `oracle` must pass; every `recorded` proposal is re-scored and printed.
//! **By hand:** `pixi run eval -- edits <gguf> <name>`, or as the other evals with `FM_MODEL_PORT`.

use fm_agent::convo::Intent;
use fm_agent::eval::edit_outcome;
use fm_agent::openai::OpenAiStep;
use fm_agent::{AgentError, InputDoc, LlmResponse, LlmStep, SearchHit, StudyAssistant, WebSearch};
use serde_json::Value;

const CASES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../methods/edits/eval/cases.json");
/// What `fm-agent-run`'s `host_note` passes of the note (2026-10-18).
const HOST_NOTE_CHARS: usize = 1200;

fn load() -> Value {
    serde_json::from_str(&std::fs::read_to_string(CASES).expect("the case file")).expect("JSON")
}

struct NoWeb;
impl WebSearch for NoWeb {
    fn search(&self, _: &str) -> Result<Vec<SearchHit>, AgentError> {
        Err(AgentError::new("the eval never searches"))
    }
}

struct Says(String);
impl LlmStep for Says {
    fn complete(&self, _: &str, _: &str) -> Result<LlmResponse, AgentError> {
        Ok(LlmResponse::complete(self.0.clone()))
    }
}

fn strings(v: &Value) -> Vec<String> {
    v.as_array().into_iter().flatten().filter_map(Value::as_str).map(str::to_string).collect()
}

/// The proposal the turn makes, and whether it passes the case.
fn run<L: LlmStep>(llm: L, case: &Value) -> Result<(String, bool, String), AgentError> {
    let intent = Intent {
        ask: case["ask"].as_str().unwrap().into(),
        search: false,
        propose: true,
        research: false,
        transcribe: None,
        recipe: false,
    };
    let note = InputDoc {
        label: format!("{} (this note)", case["title"].as_str().unwrap()),
        text: case["body"].as_str().unwrap().chars().take(HOST_NOTE_CHARS).collect(),
    };
    let turn = StudyAssistant::new(llm, NoWeb).turn("", &intent, &[note], None)?;
    // As the runner does: the edits go onto the whole note, not the part the model saw.
    let applied = fm_agent::edits::apply(case["body"].as_str().unwrap(), &turn.edits.unwrap());
    let proposal = applied.body;
    let add: Vec<Vec<String>> = case["add"].as_array().into_iter().flatten().map(strings).collect();
    let (ok, why) =
        edit_outcome(&proposal, &strings(&case["keep"]), &add, &strings(&case["remove"]));
    Ok((proposal, ok, why))
}

#[test]
fn every_oracle_proposal_passes_its_case() {
    let file = load();
    let mut wrong = Vec::new();
    for case in file["cases"].as_array().expect("cases") {
        let (_, ok, why) = run(Says(case["oracle"].as_str().unwrap().into()), case).unwrap();
        if !ok {
            wrong.push(format!("{}: {why}", case["id"].as_str().unwrap()));
        }
    }
    assert!(wrong.is_empty(), "cases whose perfect proposal fails:\n{}", wrong.join("\n"));
}

#[test]
fn recorded_proposals_are_scored() {
    let file = load();
    let mut tallies: std::collections::BTreeMap<String, (u32, u32)> = Default::default();
    for case in file["cases"].as_array().expect("cases") {
        for (who, answer) in case["recorded"].as_object().into_iter().flatten() {
            let (_, ok, _) = run(Says(answer.as_str().unwrap_or_default().into()), case).unwrap();
            let t = tallies.entry(who.clone()).or_default();
            t.0 += 1;
            t.1 += u32::from(ok);
        }
    }
    for (who, (n, ok)) in &tallies {
        println!("{who:32} exact {ok}/{n}");
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
    let record = std::env::var("FM_EVAL_RECORD").is_ok();
    /// The live model, remembering its last raw answer so the recording replays through `run`.
    struct Recording(OpenAiStep, std::cell::RefCell<String>);
    impl LlmStep for &Recording {
        fn complete(&self, s: &str, u: &str) -> Result<LlmResponse, AgentError> {
            let r = self.0.complete(s, u)?;
            *self.1.borrow_mut() = r.content.clone();
            Ok(r)
        }
        fn complete_constrained(
            &self,
            s: &str,
            u: &str,
            schema: &Value,
        ) -> Result<LlmResponse, AgentError> {
            let r = self.0.complete_constrained(s, u, schema)?;
            *self.1.borrow_mut() = r.content.clone();
            Ok(r)
        }
    }
    let llm = Recording(OpenAiStep::local(port, &model).with_temperature(0.0), Default::default());

    let mut file = load();
    let (mut n, mut pass) = (0, 0);
    let t0 = std::time::Instant::now();
    for case in file["cases"].as_array_mut().expect("cases") {
        let id = case["id"].as_str().unwrap().to_string();
        n += 1;
        match run(&llm, case) {
            Ok((_, ok, why)) => {
                pass += u32::from(ok);
                println!("{id:20} {} {why}", if ok { "PASS" } else { "FAIL" });
            }
            Err(e) => println!("{id:20} ERROR {e}"),
        }
        if record {
            case["recorded"][&model] = Value::String(llm.1.borrow().clone());
        }
    }
    println!("== {model}: exact {pass}/{n} · {:.1}s total\n", t0.elapsed().as_secs_f64());
    if record {
        std::fs::write(CASES, serde_json::to_string_pretty(&file).unwrap() + "\n").unwrap();
    }
}
