//! A **manual** run of the meeting pass's model step against a *real* local model, over conversation
//! notes kept outside the repository (they are someone's email). `#[ignore]`d, like the other live
//! tests: the gate ships no model.
//!
//! ```text
//! FM_MODEL_PORT=8081 FM_MODEL=qwen3-vl-4b FM_MEETING_CASES=<dir> FM_TODAY=2026-10-15 \
//!   pixi run cargo test -p fm-agent --test live_meetings -- --ignored --nocapture
//! ```
//!
//! Each `<dir>/*.md` is one conversation: a first line `# <title>`, then the note body exactly as the
//! pass reads it. Prints, per case, the model's raw answer and what [`meetings::resolve`] made of each
//! item — for a person to judge against what the emails say. It asserts nothing about the answers:
//! the point is to read them (`decisions.md` 2026-10-09: the first real run is what found the bugs).

use fm_agent::meetings;
use fm_agent::openai::OpenAiStep;
use fm_agent::LlmStep;

#[test]
#[ignore = "needs a live local model server and private cases; set FM_MODEL_PORT / FM_MEETING_CASES"]
fn the_meeting_pass_reads_real_conversations() {
    let (Some(port), Ok(dir)) = (
        std::env::var("FM_MODEL_PORT").ok().and_then(|p| p.parse::<u16>().ok()),
        std::env::var("FM_MEETING_CASES"),
    ) else {
        eprintln!("skip: set FM_MODEL_PORT and FM_MEETING_CASES");
        return;
    };
    let model = std::env::var("FM_MODEL").unwrap_or_else(|_| "qwen3-vl-4b".into());
    let today = std::env::var("FM_TODAY").ok().and_then(|d| {
        let f = time::macros::format_description!("[year]-[month]-[day]");
        time::Date::parse(&d, &f).ok()
    });
    let today = today.unwrap_or_else(|| time::OffsetDateTime::now_utc().date());
    let llm = OpenAiStep::local(port, &model)
        .with_temperature(0.0)
        .with_max_tokens(meetings::ANSWER_TOKENS);

    // A trial wording, to compare against the shipped one without rebuilding.
    let instruction = std::env::var("FM_PROMPT_FILE")
        .ok()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .unwrap_or_else(|| fm_agent::prompts::MEETINGS_INSTRUCTION.to_string());

    let mut cases: Vec<_> = std::fs::read_dir(&dir)
        .expect("FM_MEETING_CASES is a directory")
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "md"))
        .collect();
    cases.sort();
    for path in cases {
        let text = std::fs::read_to_string(&path).unwrap();
        let (title, body) = text.split_once('\n').unwrap_or(("", &text));
        let title = title.trim_start_matches('#').trim();
        let body = &meetings::without_box(body);
        // FM_ME=<owner>: the title the pass would propose instead (`fm_agent::titles`).
        if let Ok(me) = std::env::var("FM_ME") {
            let answer = llm
                .complete(&fm_agent::titles::instruction(&me), &meetings::user_prompt(title, body))
                .expect("the model answered")
                .content;
            let checked = fm_agent::titles::parse(&answer)
                .and_then(|(n, a)| fm_agent::titles::check(&n, &a, body, Some(&me)));
            eprintln!("\n===== {} =====\n{}\n  -> {:?}", path.display(), answer.trim(), checked);
            continue;
        }
        let answer = llm
            .complete(&instruction, &meetings::user_prompt(title, body))
            .expect("the model answered")
            .content;
        eprintln!("\n===== {} =====\n--- answer\n{}\n--- resolved", path.display(), answer.trim());
        // A trial wording that is not about meetings (a title, say): the raw answer is the result.
        if std::env::var("FM_RAW").is_ok() {
            continue;
        }
        for f in meetings::parse(&answer) {
            match meetings::resolve(&f, body, today, None) {
                Ok(m) => eprintln!(
                    "  {} {} place={:?} by={:?} sent={:?} «{}»",
                    if m.cancelled { "CANCELLED" } else { "fixed" },
                    m.start_stamp(),
                    m.place,
                    m.said_by,
                    m.sent_at,
                    m.quote
                ),
                Err(e) => eprintln!("  dropped {e:?}: «{}»", f.quote),
            }
        }
    }
}
