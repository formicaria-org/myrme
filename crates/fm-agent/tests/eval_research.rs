//! M2's eval — the research write step, against `methods/research/eval/cases.json`.
//!
//! It runs the real pipeline, [`StudyAssistant::research`], with the case's sources handed in as the
//! request's own inputs (so no web search and no query refinement), then scores the note Rust built
//! from the model's answer ([`fm_agent::eval::research_outcome`]). Two numbers: **exact** (the right
//! facts, from the right sources, or nothing when the sources do not answer) and **grounded** (no
//! claim was dropped for a quote that is not in its source).
//!
//! **In the gate:** each case's `oracle` must score exact and grounded; every `recorded` answer is
//! re-scored and printed. **By hand**, against a running `llama-server` (or `pixi run eval -- research
//! <gguf> <name>`):
//!
//! ```text
//! FM_MODEL_PORT=8091 FM_MODEL=qwen3-4b-2507 [FM_EVAL_RECORD=1] \
//!   pixi run cargo test -p fm-agent --test eval_research -- --ignored --nocapture
//! ```

use fm_agent::eval::{research_outcome, ResearchOutcome};
use fm_agent::openai::OpenAiStep;
use fm_agent::{
    AgentError, InputDoc, LlmResponse, LlmStep, ResearchRequest, SearchHit, StudyAssistant,
    WebSearch,
};
use serde_json::Value;

const CASES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../methods/research/eval/cases.json");

fn load() -> Value {
    serde_json::from_str(&std::fs::read_to_string(CASES).expect("the case file")).expect("JSON")
}

/// The web is never asked: the sources arrive as inputs.
struct NoWeb;
impl WebSearch for NoWeb {
    fn search(&self, _: &str) -> Result<Vec<SearchHit>, AgentError> {
        Err(AgentError::new("the eval never searches"))
    }
}

/// A model that gives one fixed answer — the oracle, or a recording.
struct Says(String);
impl LlmStep for Says {
    fn complete(&self, _: &str, _: &str) -> Result<LlmResponse, AgentError> {
        Ok(LlmResponse::complete(self.0.clone()))
    }
}

fn run<L: LlmStep>(llm: L, case: &Value) -> Result<ResearchOutcome, AgentError> {
    let inputs = case["sources"]
        .as_array()
        .expect("sources")
        .iter()
        .map(|s| InputDoc {
            label: s["label"].as_str().unwrap().into(),
            text: s["text"].as_str().unwrap().into(),
        })
        .collect();
    let req = ResearchRequest {
        host_note: "eval".into(),
        ask: case["ask"].as_str().unwrap().into(),
        inputs,
        search: None,
    };
    let research = StudyAssistant::new(llm, NoWeb).research(&req)?;
    let facts: Vec<String> = case["facts"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_string)
        .collect();
    let relevant: Vec<usize> = case["relevant"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_u64)
        .map(|n| n as usize)
        .collect();
    Ok(research_outcome(&research.grounded, &facts, &relevant))
}

#[test]
fn every_oracle_answer_scores_exact_and_grounded() {
    let file = load();
    let mut wrong = Vec::new();
    for case in file["cases"].as_array().expect("cases") {
        let id = case["id"].as_str().unwrap();
        let out = run(Says(case["oracle"].as_str().unwrap().into()), case).unwrap();
        if !(out.exact && out.grounded) {
            wrong.push(format!("{id}: {}", out.why));
        }
    }
    assert!(wrong.is_empty(), "cases whose perfect answer does not score:\n{}", wrong.join("\n"));
}

#[test]
fn recorded_answers_are_scored() {
    let file = load();
    let mut tallies: std::collections::BTreeMap<String, (u32, u32, u32)> = Default::default();
    for case in file["cases"].as_array().expect("cases") {
        for (who, answer) in case["recorded"].as_object().into_iter().flatten() {
            let out = run(Says(answer.as_str().unwrap_or_default().into()), case).unwrap();
            let t = tallies.entry(who.clone()).or_default();
            t.0 += 1;
            t.1 += u32::from(out.exact);
            t.2 += u32::from(out.grounded);
        }
    }
    for (who, (n, exact, grounded)) in &tallies {
        println!("{who:32} exact {exact}/{n} · grounded {grounded}/{n}");
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

    /// The live model, remembering its last answer so it can be recorded.
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
    let (mut n, mut exact, mut grounded) = (0, 0, 0);
    let t0 = std::time::Instant::now();
    for case in file["cases"].as_array_mut().expect("cases") {
        let id = case["id"].as_str().unwrap().to_string();
        let out = run(&llm, case);
        n += 1;
        match out {
            Ok(o) => {
                exact += u32::from(o.exact);
                grounded += u32::from(o.grounded);
                println!(
                    "{id:22} {} {}  verified {} dropped {}  {}",
                    if o.exact { "PASS" } else { "FAIL" },
                    if o.grounded { "  " } else { "!g" },
                    o.verified,
                    o.dropped,
                    o.why
                );
            }
            Err(e) => println!("{id:22} ERROR {e}"),
        }
        if record {
            case["recorded"][&model] = Value::String(llm.1.borrow().clone());
        }
    }
    println!(
        "== {model}: exact {exact}/{n} · grounded {grounded}/{n} · {:.1}s total\n",
        t0.elapsed().as_secs_f64()
    );
    if record {
        std::fs::write(CASES, serde_json::to_string_pretty(&file).unwrap() + "\n").unwrap();
    }
}
