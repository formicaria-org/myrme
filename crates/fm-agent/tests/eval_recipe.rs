//! M7's eval — a recipe from what was said, against `methods/recipe/eval/cases.json`: 30 Wikibooks
//! Cookbook recipes turned into one run of speech each (`agents/eval-data.py recipes`). The real
//! method runs — the model under [`recipe::INSTRUCTION`], [`recipe::read`], then
//! [`recipe::keep_what_was_said`] against the transcript — and is scored by
//! [`fm_agent::eval::recipe_outcome`].
//!
//! **In the gate:** the recipe itself, as a perfect answer, scores exact; every `recorded` answer is
//! re-scored. **By hand:** `pixi run eval -- recipe <gguf> <name>` (free and held to the schema).

use fm_agent::eval::recipe_outcome;
use fm_agent::openai::OpenAiStep;
use fm_agent::recipe;
use fm_agent::LlmStep;
use serde_json::Value;
use std::collections::BTreeMap;

const CASES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../methods/recipe/eval/cases.json");

fn load() -> Value {
    serde_json::from_str(&std::fs::read_to_string(CASES).expect("the case file")).expect("JSON")
}

fn strings(v: &Value) -> Vec<String> {
    v.as_array().into_iter().flatten().filter_map(Value::as_str).map(str::to_string).collect()
}

/// The method's answer, through the method's own reading and check, scored.
fn score(case: &Value, answer: &str) -> fm_agent::eval::RecipeOutcome {
    let said = case["transcript"].as_str().unwrap();
    let (ing, steps) = match recipe::read(answer) {
        Some(p) => {
            let (kept, _) = recipe::keep_what_was_said(p, said);
            (kept.ingredients, kept.steps.len())
        }
        None => (Vec::new(), 0),
    };
    let listed = strings(&case["ingredients"]);
    recipe_outcome(&ing, steps, &listed, case["steps"].as_array().map_or(0, Vec::len))
}

#[test]
fn the_recipe_itself_scores_exact_and_recordings_are_scored() {
    let file = load();
    let mut tallies: BTreeMap<String, (u32, u32, usize, usize)> = BTreeMap::new();
    for case in file["cases"].as_array().expect("cases") {
        let perfect = serde_json::json!({
            "title": case["title"], "servings": null, "ingredients": case["ingredients"],
            "steps": case["steps"], "times": {}, "notes": []
        })
        .to_string();
        let out = score(case, &perfect);
        assert!(out.exact, "{}: the recipe itself does not score: {}", case["id"], out.why);
        for (who, answer) in case["recorded"].as_object().into_iter().flatten() {
            let o = score(case, answer.as_str().unwrap_or_default());
            let t = tallies.entry(who.clone()).or_default();
            t.0 += 1;
            t.1 += u32::from(o.exact);
            t.2 += o.found;
            t.3 += o.listed;
        }
    }
    for (who, (n, exact, found, listed)) in tallies {
        println!("{who:32} exact {exact}/{n} · ingredients found {found}/{listed}");
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
    let record = std::env::var("FM_EVAL_RECORD").is_ok();
    let llm = OpenAiStep::local(port, &model).with_temperature(0.0).with_max_tokens(1500);
    let schema = recipe::answer_schema();
    let mut file = load();
    for mode in &modes {
        let (mut n, mut exact, mut found, mut listed, mut invented) = (0, 0, 0, 0, 0);
        let t0 = std::time::Instant::now();
        for case in file["cases"].as_array_mut().expect("cases") {
            let said = case["transcript"].as_str().unwrap().to_string();
            let reply = if *mode == "constrained" {
                llm.complete_constrained(recipe::INSTRUCTION, &said, &schema)
            } else {
                llm.complete(recipe::INSTRUCTION, &said)
            };
            let answer = reply.map(|r| r.content).unwrap_or_else(|e| format!("ERROR {e}"));
            let o = score(case, &answer);
            n += 1;
            exact += u32::from(o.exact);
            found += o.found;
            listed += o.listed;
            invented += o.invented;
            println!(
                "{mode:11} {:8} {} {}",
                case["id"].as_str().unwrap(),
                if o.exact { "PASS" } else { "FAIL" },
                o.why
            );
            if record {
                case["recorded"][format!("{model}/{mode}")] = Value::String(answer);
            }
        }
        println!(
            "== {model} / {mode}: exact {exact}/{n} · ingredients found {found}/{listed} · invented {invented} · {:.1}s\n",
            t0.elapsed().as_secs_f64()
        );
    }
    if record {
        std::fs::write(CASES, serde_json::to_string_pretty(&file).unwrap() + "\n").unwrap();
    }
}
