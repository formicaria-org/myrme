//! **A recipe from what was said** — `/recipe` (`decisions.md` 2026-10-18, *a recipe from a
//! recording*). The owner recorded a tiramisù on the phone and asked for the assistant to make a
//! recipe of it, *"that a user can accept or modify … exactly as a collaboration"*.
//!
//! Pure: the instruction to the model, reading its answer, the check that keeps only what was said,
//! and the note's text. The runner transcribes, asks, and proposes; nothing here touches a vault.
//!
//! **Nothing is invented.** The model shapes a transcript into parts; it does not add to it. That
//! is asked for, and then checked: an ingredient none of whose words was said, or a step most of
//! whose words were not, is **dropped**, and the reply says how many. The whole transcript goes in
//! the note too, under *What was said*, so the person can see what the shaping worked from.
use serde_json::Value;
use std::collections::{BTreeMap, HashSet};

/// What the model is told. Its answer is JSON; the language of the recording is kept.
pub const INSTRUCTION: &str = "You turn a person's recipe, as they said or wrote it, into its parts. \
Use ONLY what is in the text: never add an ingredient, a quantity, a step or a time that is not there. \
Keep the language of the text (an Italian recipe stays in Italian). Keep each quantity as it was said. \
Answer with JSON only, no other words, in exactly this shape: \
{\"title\": \"…\", \"servings\": <a number, or null if not said>, \
\"ingredients\": [\"one line per ingredient, with its quantity\"], \
\"steps\": [\"one line per step, in order\"], \
\"times\": {\"<what it is, e.g. preparazione>\": \"<as said, e.g. 20 minuti>\"}, \
\"notes\": [\"anything else that was said: tips, variations\"]}";

/// The parts, as the model gave them, before the check.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Parts {
    pub title: String,
    pub servings: Option<u32>,
    pub ingredients: Vec<String>,
    pub steps: Vec<String>,
    pub times: BTreeMap<String, String>,
    pub notes: Vec<String>,
}

/// The model's answer, read: the first `{` to the last `}`, so a fence or a word around it does not
/// matter. `None` when there is no recipe in it.
pub fn read(answer: &str) -> Option<Parts> {
    let start = answer.find('{')?;
    let end = answer.rfind('}')?;
    let v: Value = serde_json::from_str(&answer[start..=end]).ok()?;
    let tidy = |s: &str| s.split_whitespace().collect::<Vec<_>>().join(" ");
    let list = |key: &str| -> Vec<String> {
        v[key]
            .as_array()
            .map(|a| {
                a.iter().filter_map(Value::as_str).map(tidy).filter(|s| !s.is_empty()).collect()
            })
            .unwrap_or_default()
    };
    let times = v["times"]
        .as_object()
        .map(|o| {
            o.iter()
                .filter_map(|(k, x)| {
                    let val = x.as_str().map(tidy).or_else(|| x.as_u64().map(|n| n.to_string()))?;
                    (!val.is_empty()).then(|| (tidy(k), val))
                })
                .collect()
        })
        .unwrap_or_default();
    let p = Parts {
        title: v["title"].as_str().map(tidy).unwrap_or_default(),
        servings: v["servings"]
            .as_u64()
            .and_then(|n| u32::try_from(n).ok())
            .filter(|n| (1..=100).contains(n)),
        ingredients: list("ingredients"),
        steps: list("steps"),
        times,
        notes: list("notes"),
    };
    (!p.ingredients.is_empty() || !p.steps.is_empty()).then_some(p)
}

/// Words, lower case, accents folded, of at least `min` letters.
fn words(text: &str, min: usize) -> Vec<String> {
    let fold = |c: char| match c {
        'à' | 'á' | 'â' => 'a',
        'è' | 'é' | 'ê' => 'e',
        'ì' | 'í' => 'i',
        'ò' | 'ó' => 'o',
        'ù' | 'ú' => 'u',
        c => c,
    };
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .map(|w| w.chars().map(fold).collect::<String>())
        .filter(|w| w.chars().count() >= min && !w.chars().all(|c| c.is_ascii_digit()))
        .collect()
}

/// Do two words name the same thing — the same, or one the start of the other at four letters
/// ("uova"/"uovo", "zucchero"/"zuccheri")? Speech and its shaping change endings, not stems.
fn same(a: &str, b: &str) -> bool {
    let stem = |w: &str| w.chars().take(4).collect::<String>();
    a == b || (a.chars().count() >= 4 && b.chars().count() >= 4 && stem(a) == stem(b))
}

/// **The check.** Keeps an ingredient when a word of it was said; a step when at least half of its
/// longer words were. Answers what was kept, and how many of each were dropped.
///
/// A line said twice is kept once — and a note that only repeats a step goes. Speech recognition
/// loops on its last words (the tiramisù recording ended on the same line three times), and the
/// model copies what it is given; the repeats are the recogniser's, not the cook's.
pub fn keep_what_was_said(mut p: Parts, said: &str) -> (Parts, usize) {
    let mut seen: HashSet<String> = HashSet::new();
    let key = |s: &str| words(s, 1).join(" ");
    for list in [&mut p.ingredients, &mut p.steps, &mut p.notes] {
        list.retain(|line| seen.insert(key(line)));
    }
    let heard: HashSet<String> = words(said, 3).into_iter().collect();
    let was_said = |w: &String| heard.iter().any(|h| same(h, w));
    let before = p.ingredients.len() + p.steps.len() + p.notes.len();
    p.ingredients.retain(|line| words(line, 3).iter().any(was_said));
    let mostly = |line: &String| {
        let ws = words(line, 4);
        ws.is_empty() || ws.iter().filter(|w| was_said(w)).count() * 2 >= ws.len()
    };
    p.steps.retain(mostly);
    p.notes.retain(mostly);
    p.times.retain(|k, v| {
        words(&format!("{k} {v}"), 3).iter().any(was_said) || v.chars().any(|c| c.is_ascii_digit())
    });
    let dropped = before - (p.ingredients.len() + p.steps.len() + p.notes.len());
    (p, dropped)
}

/// The recipe note's text — the same shape as every recipe's (`ui/src/lib/food/recipeText.ts`):
/// The recipe template's sections, as formicaria's UI names them (`ui/src/lib/templates.ts`: key,
/// heading). Part of the contract: a recipe note Myrme writes must read as one made from the
/// template (`docs/context/contract.md`, part 4).
pub const TEMPLATE_HEADINGS: [(&str, &str); 4] =
    [("ingredients", "Ingredients"), ("method", "Method"), ("times", "Times"), ("notes", "Notes")];

/// *Ingredients*, *Method*, *Times*, *Notes*, then where it came from and what was said.
///
/// `from` is the note it was made from, when that is another note; `kept` are the lines the note
/// already embeds (its recordings, its pictures), carried into *Notes* — a recipe made in its own
/// note never proposes losing what was in it.
pub fn body(p: &Parts, from: Option<(&str, &str)>, said: &str, kept: &[&str]) -> String {
    let mut out = String::from("## Ingredients\n\n");
    for i in &p.ingredients {
        out.push_str(&format!("- {i}\n"));
    }
    out.push_str("\n## Method\n\n");
    for (n, s) in p.steps.iter().enumerate() {
        out.push_str(&format!("{}. {s}\n", n + 1));
    }
    out.push_str("\n## Times\n\n");
    let times: Vec<String> = p.times.iter().map(|(k, v)| format!("{k}: {v}")).collect();
    out.push_str(&times.join(" · "));
    out.push_str("\n\n## Notes\n\n");
    for n in &p.notes {
        out.push_str(&format!("{n}\n\n"));
    }
    for k in kept {
        out.push_str(&format!("{k}\n\n"));
    }
    out.push_str("---\n\n");
    if let Some((from_title, from_id)) = from {
        out.push_str(&format!("From [{from_title}](note:{from_id}).\n\n"));
    }
    out.push_str("### What was said\n\n");
    for line in said.lines().filter(|l| !l.trim().is_empty()) {
        out.push_str(&format!("> {}\n", line.trim()));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAID: &str = "Allora per il tiramisù per sei persone servono cinquecento grammi di \
        mascarpone, quattro uova, cento grammi di zucchero, i savoiardi e il caffè. Separate i tuorli \
        dagli albumi e montate i tuorli con lo zucchero. Unite il mascarpone. Bagnate i savoiardi nel \
        caffè e fate gli strati. Mettete in frigo per almeno quattro ore.";

    #[test]
    fn the_model_s_answer_is_read_through_fences_and_words_around_it() {
        let a = "Ecco:\n```json\n{\"title\":\"Tiramisù\",\"servings\":6,\"ingredients\":[\"500 g di mascarpone\"],\"steps\":[\"Unite il mascarpone.\"],\"times\":{\"riposo\":\"4 ore\"},\"notes\":[]}\n```";
        let p = read(a).unwrap();
        assert_eq!((p.title.as_str(), p.servings), ("Tiramisù", Some(6)));
        assert!(read("I could not find a recipe.").is_none());
        assert!(read("{\"title\":\"x\",\"ingredients\":[],\"steps\":[]}").is_none());
    }

    #[test]
    fn what_was_not_said_is_dropped_and_counted() {
        let p = Parts {
            title: "Tiramisù".into(),
            servings: Some(6),
            ingredients: vec![
                "500 g di mascarpone".into(),
                "4 uova".into(),
                "100 g di zucchero".into(),
                "Savoiardi".into(),
                "Caffè".into(),
                "Cacao amaro in polvere".into(), // never said
            ],
            steps: vec![
                "Separate i tuorli dagli albumi.".into(),
                "Montate i tuorli con lo zucchero.".into(),
                "Spolverizzate con cacao e decorate con fragole.".into(), // never said
            ],
            times: [("riposo".to_string(), "4 ore".to_string())].into(),
            notes: vec![],
        };
        let (kept, dropped) = keep_what_was_said(p, SAID);
        assert_eq!(kept.ingredients.len(), 5, "{:?}", kept.ingredients);
        assert!(!kept.ingredients.iter().any(|i| i.contains("Cacao")));
        assert_eq!(kept.steps.len(), 2, "{:?}", kept.steps);
        assert_eq!(dropped, 2);
    }

    /// **One shape for every recipe note** (2026-10-18): the headings written here are the
    /// template's, read from the UI's own definition so the two cannot drift apart.
    ///
    /// Myrme's half: the note has every heading in [`TEMPLATE_HEADINGS`]. formicaria's half — that its
    /// recipe template (`ui/src/lib/templates.ts`) names the same ones — is checked against the pinned
    /// formicaria by `pixi run integration` (`tests/integration_fm.rs`), since that file is not in this
    /// repository (`docs/context/contract.md`, part 4).
    #[test]
    fn its_headings_are_the_recipe_template_s() {
        let b = body(&Parts::default(), None, "", &[]);
        for (_, heading) in TEMPLATE_HEADINGS {
            assert!(b.contains(&format!("## {heading}\n")), "the note has ## {heading}");
        }
    }

    #[test]
    fn a_line_the_recogniser_repeated_is_kept_once() {
        let step = "Separate i tuorli dagli albumi.";
        let p = Parts {
            ingredients: vec!["4 uova".into(), "4 uova".into()],
            steps: vec![step.into(), "Unite il mascarpone.".into()],
            notes: vec![step.into(), "separate i tuorli, dagli albumi".into()],
            ..Default::default()
        };
        let (kept, dropped) = keep_what_was_said(p, SAID);
        assert_eq!(kept.ingredients, vec!["4 uova".to_string()]);
        assert_eq!(kept.steps.len(), 2);
        assert!(kept.notes.is_empty(), "{:?}", kept.notes);
        assert_eq!(dropped, 0, "a repeat is not something unsaid");
    }

    #[test]
    fn the_note_has_the_shape_every_recipe_has_and_what_was_said() {
        let p = Parts {
            title: "Tiramisù".into(),
            ingredients: vec!["500 g di mascarpone".into()],
            steps: vec!["Unite il mascarpone.".into()],
            times: [("riposo".to_string(), "4 ore".to_string())].into(),
            ..Default::default()
        };
        let b = body(
            &p,
            Some(("Ricetta Tiramisu", "01M55H50X59QNAW5RVBQ2QDYD8")),
            "Unite il mascarpone.",
            &["![Ricetta](asset:sha256-ab)"],
        );
        assert!(b.starts_with(
            "## Ingredients\n\n- 500 g di mascarpone\n\n## Method\n\n1. Unite il mascarpone.\n"
        ));
        assert!(b.contains("## Times\n\nriposo: 4 ore\n"));
        assert!(b.contains("From [Ricetta Tiramisu](note:01M55H50X59QNAW5RVBQ2QDYD8)."));
        assert!(b.contains("### What was said\n\n> Unite il mascarpone.\n"));
        assert!(b.contains("![Ricetta](asset:sha256-ab)\n"), "the recording stays in the note");
    }
}
