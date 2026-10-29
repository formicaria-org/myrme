//! **Reputable sources only** (2026-10-29, `docs/research/2026-10-29-reputable-sources.md`): the list
//! of sources `/research` may read, which of them a question goes to, and the sources pack — the
//! topic indexes of public health bodies, searched on the device.
//!
//! A small model cannot judge credibility: given a misleading health page it agreed with it far more
//! often than with no page at all (web-research §4.3). So **the list is the safety mechanism**: every
//! hit, every fetch and every redirect is checked against [`LISTED`], and nothing else is opened.
//! Code chooses the sources from the question ([`Need::of`]); the model never does.

use crate::websearch::terms;
use std::sync::OnceLock;

/// A source on the list: its exact host and the name a person reads beside a claim.
pub struct Body {
    pub host: &'static str,
    pub name: &'static str,
}

/// **The list.** Hosts are matched exactly (no suffixes), after lower-casing. Adding a host is a
/// decision: `docs/context/decisions.md`, 2026-10-29, *research reads only the list*.
pub const LISTED: &[Body] = &[
    // Encyclopedia, in the languages the app is used in.
    Body { host: "en.wikipedia.org", name: "Wikipedia" },
    Body { host: "fr.wikipedia.org", name: "Wikipedia" },
    Body { host: "de.wikipedia.org", name: "Wikipedia" },
    Body { host: "it.wikipedia.org", name: "Wikipedia" },
    Body { host: "es.wikipedia.org", name: "Wikipedia" },
    Body { host: "zh.wikipedia.org", name: "Wikipedia" },
    // Public health bodies.
    Body { host: "medlineplus.gov", name: "MedlinePlus (US National Library of Medicine)" },
    Body { host: "wsearch.nlm.nih.gov", name: "MedlinePlus (US National Library of Medicine)" },
    Body { host: "www.who.int", name: "World Health Organization" },
    Body { host: "www.nhs.uk", name: "NHS (UK)" },
    Body { host: "www.cdc.gov", name: "CDC (US)" },
    Body { host: "www.efsa.europa.eu", name: "EFSA (EU)" },
    Body { host: "gesund.bund.de", name: "gesund.bund.de (German Federal Ministry of Health)" },
    Body { host: "www.bfr.bund.de", name: "BfR (Germany)" },
    Body { host: "www.dge.de", name: "DGE (Germany)" },
    Body { host: "www.blv.admin.ch", name: "BLV (Switzerland)" },
    Body { host: "www.santepubliquefrance.fr", name: "Santé publique France" },
    Body { host: "www.anses.fr", name: "ANSES (France)" },
    Body { host: "www.epicentro.iss.it", name: "ISS EpiCentro (Italy)" },
    // Papers.
    Body { host: "www.ebi.ac.uk", name: "Europe PMC" },
    Body { host: "europepmc.org", name: "Europe PMC" },
    Body { host: "export.arxiv.org", name: "arXiv" },
    Body { host: "arxiv.org", name: "arXiv" },
    Body { host: "api.crossref.org", name: "Crossref" },
    Body { host: "doi.org", name: "Crossref" },
    // Code.
    Body { host: "api.github.com", name: "GitHub" },
    Body { host: "github.com", name: "GitHub" },
    Body { host: "api.stackexchange.com", name: "Stack Overflow" },
    Body { host: "stackoverflow.com", name: "Stack Overflow" },
];

/// The host of an `https` URL, lower-cased; `None` for anything else (another scheme, a port, user
/// information, an IP address).
pub fn https_host(url: &str) -> Option<String> {
    let rest = url.strip_prefix("https://")?;
    let authority = rest.split(['/', '?', '#']).next()?;
    if authority.is_empty() || authority.contains(['@', ':', '[', ']']) {
        return None;
    }
    let host = authority.to_ascii_lowercase();
    if host.parse::<std::net::Ipv4Addr>().is_ok() || !host.contains('.') {
        return None;
    }
    Some(host)
}

/// The body a URL belongs to, if it is on the list.
pub fn listed(url: &str) -> Option<&'static Body> {
    let host = https_host(url)?;
    LISTED.iter().find(|b| b.host == host)
}

/// **What the person is doing**, which decides the sources. Chosen by code from the question.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Need {
    /// A symptom, a nutrient's role, a dose: MedlinePlus and public bodies; Wikipedia labelled as an
    /// encyclopedia; papers only when asked for.
    Health,
    /// How much of a nutrient a food has, foods rich in one: the app's food list, public bodies,
    /// Wikipedia.
    Food,
    /// A DOI, an arXiv id, a PMID, or papers asked for: Europe PMC, arXiv, Crossref.
    Papers,
    /// Programming: GitHub and Stack Overflow.
    Code,
    /// Anything else: Wikipedia, arXiv and GitHub, as before.
    General,
}

impl Need {
    /// The need a question states, in order: a paper's identifier; code; papers asked for; food;
    /// health (a nutrient, a health word, or a topic a health body has a page on); otherwise general.
    pub fn of(asked: &str) -> Need {
        let folded = fm_agent::textmatch::loose(asked);
        let words = words(&folded);
        if paper_id(asked).is_some() {
            return Need::Papers;
        }
        let has = |list: &[&str]| has_any(&words, &folded, list, false);
        if has(CODE_WORDS) {
            return Need::Code;
        }
        if has(PAPER_WORDS) {
            return Need::Papers;
        }
        let nutrient = !nutrients(asked).is_empty();
        if has(FOOD_WORDS) || (nutrient && has(AMOUNT_WORDS) && has(IN_WORDS)) {
            return Need::Food;
        }
        if nutrient || has_any(&words, &folded, HEALTH_WORDS, true) {
            return Need::Health;
        }
        // A question naming what a health body has a page on (*measles*, *Masern*) is about health,
        // with no health word in it: the pack's titles are the vocabulary.
        if names_a_topic(&crate::websearch::keywords(asked, crate::websearch::language(asked))) {
            return Need::Health;
        }
        Need::General
    }
}

/// A paper's identifier in the question: a DOI, a PMID, a PMCID or an arXiv id.
pub fn paper_id(text: &str) -> Option<PaperId> {
    for raw in text.split_whitespace() {
        let w = raw.trim_matches(|c: char| matches!(c, '(' | ')' | ',' | ';' | '<' | '>' | '"'));
        let w = w.trim_end_matches('.');
        let lower = w.to_ascii_lowercase();
        if let Some(at) = lower.find("10.") {
            let doi = &w[at..];
            let (prefix, suffix) = doi.split_once('/').unwrap_or((doi, ""));
            if prefix.len() >= 7
                && prefix[3..].chars().all(|c| c.is_ascii_digit())
                && !suffix.is_empty()
            {
                return Some(PaperId::Doi(doi.to_string()));
            }
        }
        if let Some(n) = lower.strip_prefix("pmc").filter(|n| digits(n, 5..=9)) {
            return Some(PaperId::Pmcid(format!("PMC{n}")));
        }
        if let Some(n) = lower.strip_prefix("pmid:").or_else(|| lower.strip_prefix("pmid")) {
            if digits(n, 5..=9) {
                return Some(PaperId::Pmid(n.to_string()));
            }
        }
        let bare = lower.strip_prefix("arxiv:").unwrap_or(&lower);
        let bare = bare.split('v').next().unwrap_or(bare);
        if let Some((a, b)) = bare.split_once('.') {
            if is_arxiv(a, b) {
                return Some(PaperId::Arxiv(bare.to_string()));
            }
        }
    }
    None
}

/// An arXiv identifier as arXiv issues them since April 2007: `YYMM.NNNN` until 2014, `YYMM.NNNNN`
/// since — so a version number or a telephone number is not one.
fn is_arxiv(yymm: &str, number: &str) -> bool {
    if !digits(yymm, 4..=4) || !digits(number, 4..=5) {
        return false;
    }
    let (yy, mm): (u32, u32) = (yymm[..2].parse().unwrap_or(0), yymm[2..].parse().unwrap_or(0));
    (1..=12).contains(&mm) && (yy > 7 || (yy == 7 && mm >= 4)) && (number.len() == 5) == (yy >= 15)
}

fn digits(s: &str, len: std::ops::RangeInclusive<usize>) -> bool {
    len.contains(&s.len()) && s.chars().all(|c| c.is_ascii_digit())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PaperId {
    Doi(String),
    Pmid(String),
    Pmcid(String),
    Arxiv(String),
}

/// A nutrient a question may name, the key the app's food list stores it under, and its names in
/// English, French, German, Italian and Chinese (lower case, accents folded).
pub struct Nutrient {
    pub key: &'static str,
    /// Its name in English, for a sentence.
    pub word: &'static str,
    /// The letter (and number) after "vitamin", for the vitamins.
    vitamin: Option<&'static str>,
    names: &'static [&'static str],
}

const fn n(
    key: &'static str,
    word: &'static str,
    vitamin: Option<&'static str>,
    names: &'static [&'static str],
) -> Nutrient {
    Nutrient { key, word, vitamin, names }
}

/// Every nutrient of the app's food list that a question is likely to name.
pub const NUTRIENTS: &[Nutrient] = &[
    n("vitamin_k1_ug", "vitamin K", Some("k"), &["phylloquinone", "phyllochinon", "fillochinone"]),
    n("vitamin_d_ng", "vitamin D", Some("d"), &["cholecalciferol", "colecalciferolo"]),
    n("vitamin_b12_ng", "vitamin B12", Some("b12"), &["cobalamin", "cobalamine", "cobalamina"]),
    n("vitamin_b6_ug", "vitamin B6", Some("b6"), &["pyridoxine", "pyridoxin", "piridossina"]),
    n(
        "vitamin_c_ug",
        "vitamin C",
        Some("c"),
        &["ascorbic", "ascorbique", "ascorbinsaure", "ascorbico"],
    ),
    n("vitamin_a_rae_ug", "vitamin A", Some("a"), &["retinol", "retinolo"]),
    n("vitamin_e_ug", "vitamin E", Some("e"), &["tocopherol", "tocoferolo"]),
    n(
        "folate_dfe_ug",
        "folate",
        Some("b9"),
        &["folate", "folic", "folique", "folat", "folsaure", "folato", "folico", "叶酸"],
    ),
    n("thiamin_ug", "thiamin", Some("b1"), &["thiamin", "thiamine", "tiamina"]),
    n("riboflavin_ug", "riboflavin", Some("b2"), &["riboflavin", "riboflavine", "riboflavina"]),
    n("niacin_ug", "niacin", Some("b3"), &["niacin", "niacine", "niacina"]),
    n("iron_ug", "iron", None, &["iron", "fer", "eisen", "ferro", "铁"]),
    n("calcium_ug", "calcium", None, &["calcium", "kalzium", "calcio", "钙"]),
    n("magnesium_ug", "magnesium", None, &["magnesium", "magnesio", "镁"]),
    n("zinc_ug", "zinc", None, &["zinc", "zink", "zinco", "锌"]),
    n("iodine_ug", "iodine", None, &["iodine", "iode", "jod", "iodio", "碘"]),
    n("potassium_ug", "potassium", None, &["potassium", "kalium", "potassio", "钾"]),
    n("sodium_ug", "sodium", None, &["sodium", "natrium", "sodio", "钠"]),
    n("selenium_ug", "selenium", None, &["selenium", "selen", "selenio", "硒"]),
    n("copper_ug", "copper", None, &["copper", "cuivre", "kupfer", "铜"]),
    n("choline_ug", "choline", None, &["choline", "cholin", "colina", "胆碱"]),
    n(
        "protein_mg",
        "protein",
        None,
        &["protein", "proteins", "proteine", "proteines", "eiweiss", "蛋白质"],
    ),
    n(
        "fiber_mg",
        "fibre",
        None,
        &["fiber", "fibre", "fibres", "ballaststoffe", "fibra", "膳食纤维"],
    ),
    n("caffeine_ug", "caffeine", None, &["caffeine", "cafeine", "koffein", "caffeina", "咖啡因"]),
    n(
        "cholesterol_ug",
        "cholesterol",
        None,
        &["cholesterol", "cholesterin", "colesterolo", "胆固醇"],
    ),
];

/// The nutrients a question names, in the order of [`NUTRIENTS`].
pub fn nutrients(asked: &str) -> Vec<&'static Nutrient> {
    let folded = fm_agent::textmatch::loose(asked);
    let words = words(&folded);
    NUTRIENTS.iter().filter(|n| names_nutrient(n, &words, &folded)).collect()
}

fn names_nutrient(n: &Nutrient, words: &[String], folded: &str) -> bool {
    if let Some(letter) = n.vitamin {
        let after_vitamin = words
            .windows(2)
            .any(|p| VITAMIN.contains(&p[0].as_str()) && is_vitamin_letter(&p[1], letter));
        if after_vitamin || folded.contains(&format!("维生素{letter}")) {
            return true;
        }
    }
    n.names.iter().any(|name| {
        if is_han(name) {
            folded.contains(name)
        } else {
            words.iter().any(|w| word_is(w, name))
        }
    })
}

/// `k` names vitamin K, and so do `k1` and `k2`; `b1` does not name `b12`.
fn is_vitamin_letter(word: &str, letter: &str) -> bool {
    word == letter
        || (letter.len() == 1
            && word.starts_with(letter)
            && word.len() == 2
            && word[1..].chars().all(|c| c.is_ascii_digit()))
}

/// The keywords without the nutrients they name and the words that only say *food* or *how much*:
/// what is left names the food (*spinach* in "how much vitamin K in spinach"), in the question's own
/// language.
pub fn without_nutrients(keywords: &str) -> String {
    let folded = fm_agent::textmatch::loose(keywords);
    let toks = words(&folded);
    let mut keep = vec![true; toks.len()];
    for (i, w) in toks.iter().enumerate() {
        if VITAMIN.contains(&w.as_str()) {
            keep[i] = false;
            if toks.get(i + 1).is_some_and(|next| {
                NUTRIENTS.iter().any(|n| n.vitamin.is_some_and(|l| is_vitamin_letter(next, l)))
            }) {
                keep[i + 1] = false;
            }
        }
        let named =
            NUTRIENTS.iter().any(|n| n.names.iter().any(|name| !is_han(name) && word_is(w, name)));
        let generic = [FOOD_WORDS, AMOUNT_WORDS, IN_WORDS, HEALTH_WORDS]
            .iter()
            .any(|l| l.contains(&w.as_str()));
        if named || generic {
            keep[i] = false;
        }
    }
    let mut out: Vec<&str> =
        toks.iter().zip(&keep).filter(|(_, k)| **k).map(|(w, _)| w.as_str()).collect();
    if is_han(&folded) {
        // Chinese has no spaces: take the nutrient's and the filler characters out of the run.
        let mut han = folded.clone();
        for n in NUTRIENTS {
            for name in n.names.iter().filter(|x| is_han(x)) {
                han = han.replace(name, " ");
            }
            if let Some(l) = n.vitamin {
                han = han.replace(&format!("维生素{l}"), " ");
            }
        }
        for w in FOOD_WORDS.iter().chain(AMOUNT_WORDS).chain(IN_WORDS).filter(|x| is_han(x)) {
            han = han.replace(w, " ");
        }
        return han.split_whitespace().collect::<Vec<_>>().join(" ");
    }
    out.retain(|w| !w.is_empty());
    out.join(" ")
}

/// A word is a name when it is the name, or — for a name of five letters or more — begins or ends with
/// it (German compounds: *Eisenmangel*, *Vitamin-B12-Mangel*).
fn word_is(word: &str, name: &str) -> bool {
    word == name || (name.chars().count() >= 5 && (word.starts_with(name) || word.ends_with(name)))
}

fn is_han(s: &str) -> bool {
    s.chars().any(|c| matches!(c as u32, 0x4E00..=0x9FFF | 0x3400..=0x4DBF))
}

fn words(folded: &str) -> Vec<String> {
    folded
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(String::from)
        .collect()
}

/// Whether the question holds a word of `list`: as a whole word, or with `compounds` also at the start
/// or end of a longer one (German health words: *Kopfschmerz*, *Vitaminmangel*).
fn has_any(words: &[String], folded: &str, list: &[&str], compounds: bool) -> bool {
    let joined = format!(" {} ", words.join(" "));
    list.iter().any(|item| {
        if is_han(item) {
            folded.contains(item)
        } else if item.contains(' ') {
            joined.contains(&format!(" {item} "))
        } else if compounds {
            words.iter().any(|w| word_is(w, item))
        } else {
            words.iter().any(|w| w == item)
        }
    })
}

const VITAMIN: &[&str] = &["vitamin", "vitamins", "vitamine", "vitamines", "vitamina", "vitaminen"];

/// Words that ask for code.
const CODE_WORDS: &[&str] = &[
    "code",
    "coding",
    "programming",
    "programmer",
    "compiler",
    "rust",
    "python",
    "javascript",
    "typescript",
    "java",
    "golang",
    "kotlin",
    "sql",
    "regex",
    "git",
    "bash",
    "docker",
    "stack overflow",
    "stacktrace",
    "cargo",
    "npm",
    "svelte",
    "programmieren",
    "programmation",
    "programmazione",
    "代码",
    "编程",
];

/// Words that ask for papers.
const PAPER_WORDS: &[&str] = &[
    "paper",
    "papers",
    "study",
    "studies",
    "preprint",
    "preprints",
    "arxiv",
    "doi",
    "pmid",
    "publication",
    "publications",
    "meta analysis",
    "metaanalysis",
    "systematic review",
    "randomized",
    "randomised",
    "trial",
    "trials",
    "literature",
    "etude",
    "etudes",
    "essai clinique",
    "article scientifique",
    "articles scientifiques",
    "studie",
    "studien",
    "publikation",
    "veroffentlichung",
    "metaanalyse",
    "studio",
    "studi",
    "pubblicazione",
    "articolo scientifico",
    "论文",
    "文献",
];

/// Words that say the question is about food.
const FOOD_WORDS: &[&str] = &[
    "food",
    "foods",
    "eat",
    "eating",
    "diet",
    "meal",
    "meals",
    "rich",
    "contain",
    "contains",
    "aliment",
    "aliments",
    "nourriture",
    "manger",
    "riche",
    "riches",
    "contient",
    "contiennent",
    "lebensmittel",
    "nahrungsmittel",
    "essen",
    "enthalten",
    "enthalt",
    "reich",
    "alimenti",
    "alimento",
    "cibo",
    "cibi",
    "mangiare",
    "ricco",
    "ricchi",
    "ricca",
    "contiene",
    "contengono",
    "食物",
    "食品",
    "含有",
    "富含",
];

/// Words that ask *how much*.
const AMOUNT_WORDS: &[&str] = &[
    "how much", "how many", "amount", "combien", "quantite", "wie viel", "wieviel", "menge",
    "quanto", "quanta", "quanti", "quante", "多少",
];

/// Words that put an amount *in* something.
const IN_WORDS: &[&str] =
    &["in", "dans", "im", "nel", "nei", "nella", "nelle", "negli", "中", "里"];

/// Words that say the question is about health.
const HEALTH_WORDS: &[&str] = &[
    "symptom",
    "symptoms",
    "disease",
    "deficiency",
    "dose",
    "dosage",
    "pain",
    "treatment",
    "medicine",
    "medication",
    "drug",
    "blood",
    "pressure",
    "diabetes",
    "cancer",
    "heart",
    "pregnancy",
    "pregnant",
    "vaccine",
    "infection",
    "allergy",
    "anaemia",
    "anemia",
    "health",
    "symptome",
    "symptomes",
    "maladie",
    "carence",
    "douleur",
    "traitement",
    "medicament",
    "grossesse",
    "enceinte",
    "sang",
    "sante",
    "vaccin",
    "krankheit",
    "mangel",
    "schmerz",
    "schmerzen",
    "behandlung",
    "medikament",
    "schwangerschaft",
    "schwanger",
    "blut",
    "gesundheit",
    "impfung",
    "sintomo",
    "sintomi",
    "malattia",
    "carenza",
    "dolore",
    "trattamento",
    "farmaco",
    "gravidanza",
    "incinta",
    "sangue",
    "salute",
    "vaccino",
    "症状",
    "疾病",
    "缺乏",
    "治疗",
    "药",
    "健康",
    "怀孕",
    "血",
];

/// **The food a "how much" question names**, for the app's food list: what is left of its keywords
/// once the nutrients and the words that only say *food* or *how much* are out — `None` when the
/// question is not about food, names no nutrient, or names no food.
pub fn food_asked(asked: &str) -> Option<String> {
    if Need::of(asked) != Need::Food || nutrients(asked).is_empty() {
        return None;
    }
    let lang = crate::websearch::language(asked);
    let name = without_nutrients(&crate::websearch::keywords(asked, lang));
    (!name.trim().is_empty()).then_some(name)
}

/// **A food from the app's list, as a source**: its name, its table, and the values of the nutrients
/// the question names, per 100 g, in the units a person reads. `None` when it has none of them — a
/// table that never measured a nutrient says nothing, never zero.
pub fn food_facts(asked: &str, food: &serde_json::Value) -> Option<(String, String)> {
    let name = food["name"].as_str()?;
    let table = match food["src"].as_str()? {
        "ciqual" => "Ciqual (France)",
        "bls" => "BLS (Germany)",
        "swiss" => "the Swiss food table",
        "crea" => "CREA (Italy)",
        "usda" => "USDA (United States)",
        other => other,
    };
    let per100 = food["per100"].as_object()?;
    let mut facts = Vec::new();
    for n in nutrients(asked) {
        let Some(v) = per100.get(n.key).and_then(serde_json::Value::as_i64) else { continue };
        let lent = food["borrowed"][n.key]
            .as_str()
            .map(|from| format!(" (borrowed from {from})"))
            .unwrap_or_default();
        facts.push(format!("{}: {} per 100 g{lent}", n.word, amount(n.key, v)));
    }
    if facts.is_empty() {
        return None;
    }
    let label = format!("The app's food list: {name}, from {table}");
    Some((label, format!("{name} ({table}). {}.", facts.join("; "))))
}

/// A stored whole number in the unit its key names, as a person reads it: `iron_ug` 2700 → `2.7 mg`.
pub fn amount(key: &str, v: i64) -> String {
    let one = |x: f64| {
        let s = format!("{x:.1}");
        s.strip_suffix(".0").map(String::from).unwrap_or(s)
    };
    match key.rsplit('_').next() {
        Some("mg") => format!("{} g", one(v as f64 / 1000.0)),
        Some("ug") if v >= 1000 => format!("{} mg", one(v as f64 / 1000.0)),
        Some("ug") => format!("{v} µg"),
        Some("ng") => format!("{} µg", one(v as f64 / 1000.0)),
        _ => v.to_string(),
    }
}

/// **What a question is about, in its own words**: the nutrients it names (`vitamine K`, `Eisenmangel`,
/// `维生素K`), else the words naming a topic a health body has a page on. `None` when it names
/// neither. Searched when the question's whole keywords found an article sharing none of them.
pub fn focus(asked: &str) -> Option<String> {
    let tokens: Vec<&str> = asked
        .split(|c: char| !(c.is_alphanumeric() || c == '-'))
        .filter(|w| !w.is_empty())
        .collect();
    let fold = |w: &str| fm_agent::textmatch::loose(w);
    let mut out: Vec<String> = Vec::new();
    let mut i = 0;
    while i < tokens.len() {
        let w = fold(tokens[i]);
        let parts: Vec<String> = words(&w);
        let next = tokens.get(i + 1).map(|t| fold(t));
        if VITAMIN.contains(&w.as_str())
            && next.as_deref().is_some_and(|x| {
                NUTRIENTS.iter().any(|n| n.vitamin.is_some_and(|l| is_vitamin_letter(x, l)))
            })
        {
            out.push(format!("{} {}", tokens[i], tokens[i + 1]));
            i += 2;
            continue;
        }
        let named = parts.iter().any(|p| {
            NUTRIENTS.iter().any(|n| n.names.iter().any(|name| !is_han(name) && word_is(p, name)))
                || (p.starts_with("vitamin") && p.len() > 7)
        });
        if named {
            out.push(tokens[i].to_string());
        } else if is_han(&w) {
            for n in NUTRIENTS {
                for name in n.names.iter().filter(|x| is_han(x)) {
                    if w.contains(name) {
                        out.push(name.to_string());
                    }
                }
                if let Some(l) = n.vitamin {
                    let han = format!("维生素{l}");
                    if w.contains(&han) {
                        out.push(han);
                    }
                }
            }
        }
        i += 1;
    }
    if out.is_empty() {
        let q = terms(asked);
        let page = pack().iter().find(|p| {
            !p.topic.is_empty() && p.topic.iter().all(|t| q.iter().any(|w| same(w, t)))
        })?;
        return Some(page.topic.join(" "));
    }
    out.dedup();
    Some(out.join(" "))
}

/// Whether a title shares a word naming something with a query (a lone letter counts: *Vitamin K*).
pub fn shares_topic(title: &str, query: &str) -> bool {
    let q: Vec<String> =
        terms(query).into_iter().filter(|t| !generic(t) && !STOP.contains(&t.as_str())).collect();
    terms(title)
        .iter()
        .filter(|t| !generic(t) && !STOP.contains(&t.as_str()))
        .any(|t| q.iter().any(|w| same(w, t)))
}

/// Whether `text` holds at least two thirds of the words of `query` that name something.
pub fn covers(text: &str, query: &str) -> bool {
    let have = terms(text);
    let want: Vec<String> = terms(query)
        .into_iter()
        .filter(|t| !generic(t) && !STOP.contains(&t.as_str()) && t.chars().count() > 1)
        .collect();
    let held = want.iter().filter(|w| have.iter().any(|h| same(w, h))).count();
    !want.is_empty() && 3 * held >= 2 * want.len()
}

/// A query without the words that only ask for papers (*studies on*, *papers about*).
pub fn without_asking(words: &str) -> String {
    let mut out: Vec<&str> = Vec::new();
    let mut after_vitamin = false;
    for w in words.split_whitespace() {
        let f = fm_agent::textmatch::loose(w);
        if after_vitamin || !(PAPER_WORDS.contains(&f.as_str()) || STOP.contains(&f.as_str())) {
            out.push(w);
        }
        after_vitamin = VITAMIN.contains(&f.as_str());
    }
    out.join(" ")
}

/// A page in the sources pack: a public body's topic page, by its title.
pub struct Page {
    pub lang: &'static str,
    pub url: &'static str,
    pub title: &'static str,
    /// The title's words and its address's last part.
    terms: Vec<String>,
    /// The title's own words that name its topic (not the ones that only say *food*, *health* …).
    topic: Vec<String>,
}

/// **The sources pack**: `sources.tsv`, built by `agents/pack-sources.py` from the topic indexes of the
/// bodies on the list, reviewed in its diff like any source file. One line a page: language, URL,
/// title.
const PACK: &str = include_str!("sources.tsv");

/// The pack, read once.
pub fn pack() -> &'static [Page] {
    static PAGES: OnceLock<Vec<Page>> = OnceLock::new();
    PAGES.get_or_init(|| {
        PACK.lines()
            .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
            .filter_map(|l| {
                let mut f = l.split('\t');
                let (lang, url, title) = (f.next()?, f.next()?, f.next()?);
                // The address's last part (`/vitamin-k/`) often names the topic more plainly.
                let slug = url.trim_end_matches('/').rsplit('/').next().unwrap_or("");
                let slug = slug.trim_end_matches(".html").replace(['-', '_'], " ");
                let mut all = terms(&format!("{title} {slug}"));
                all.sort();
                all.dedup();
                let mut topic: Vec<String> =
                    terms(title).into_iter().filter(|t| names(t)).collect();
                topic.sort();
                topic.dedup();
                Some(Page { lang, url, title, terms: all, topic })
            })
            .collect()
    })
}

/// A word that only says *food*, *health*, *how much* or *vitamin*.
fn generic(t: &str) -> bool {
    [FOOD_WORDS, AMOUNT_WORDS, IN_WORDS, HEALTH_WORDS, VITAMIN].iter().any(|l| l.contains(&t))
}

/// A word of a title that names its topic: not generic, not a small word, not a lone letter.
fn names(t: &str) -> bool {
    !generic(t) && !STOP.contains(&t) && (t.chars().count() > 1 || is_han(t))
}

/// Small words of titles that name nothing: articles, prepositions, conjunctions.
const STOP: &[&str] = &[
    "the", "and", "of", "for", "with", "your", "you", "how", "what", "on", "about", "to", "from",
    "by", "a", "an", "is", "are", "which", "does", "do", "les", "des", "la", "le", "de", "du",
    "et", "en", "un", "une", "au", "aux", "der", "die", "das", "und", "im", "mit", "von", "zu",
    "bei", "fur", "il", "lo", "gli", "di", "da", "e", "per", "con", "del", "della",
];

/// Whether a query word and a page word are the same word: equal, or — both of five letters or more —
/// one beginning the other by a letter (a plural: *vitamins*, *vitamine*) or by four or more (a German
/// compound: *Eisenmangel* and *Eisen*, *Masernimpfung* and *Masern*). Two or three letters more is
/// another word (*checker* is not *check*).
fn same(q: &str, page: &str) -> bool {
    if q == page {
        return true;
    }
    let (a, b) = if q.len() <= page.len() { (q, page) } else { (page, q) };
    let (short, long) = (a.chars().count(), b.chars().count());
    short >= 5 && b.starts_with(a) && (long - short <= 1 || long - short >= 4)
}

fn holds(p: &Page, t: &str) -> bool {
    p.terms.iter().any(|w| same(t, w))
}

/// The pages of the pack that answer `query`, best first, at most `max`: a page must hold the query's
/// rarest topic word (a word the pack knows that does not only say *food*, *health* or *how much*);
/// the others raise its rank. **Pages in `lang` come first**, then English ones, then the rest.
pub fn pack_search(query: &str, lang: &str, max: usize) -> Vec<&'static Page> {
    let pages = pack();
    // Small words out; a lone letter stays (*vitamin A*, *vitamin E*).
    let mut q: Vec<String> = terms(query)
        .into_iter()
        .filter(|t| t.chars().count() == 1 || !STOP.contains(&t.as_str()))
        .collect();
    q.sort();
    q.dedup();
    let q_all = q.clone();
    let df = |t: &str| pages.iter().filter(|p| holds(p, t)).count();
    let known: Vec<(String, usize)> = q
        .into_iter()
        .map(|t| {
            let d = df(&t);
            (t, d)
        })
        .filter(|(_, d)| *d > 0)
        .collect();
    // The rarest word that names something must be on the page. Where the only such word is a lone
    // letter (*vitamine D*), every word must be: *Hépatite D* is not about vitamin D.
    let required: Vec<&String> = match known
        .iter()
        .filter(|(t, _)| !generic(t) && t.chars().count() > 1)
        .min_by_key(|(t, d)| (*d, t.len()))
    {
        Some((t, _)) => vec![t],
        None if known.iter().any(|(t, _)| !generic(t)) => known.iter().map(|(t, _)| t).collect(),
        None => return Vec::new(),
    };
    let n = pages.len() as f64;
    let idf = |d: usize| (1.0 + (n - d as f64 + 0.5) / (d as f64 + 0.5)).ln();
    let mut scored: Vec<(usize, f64, &Page)> = pages
        .iter()
        .filter(|p| required.iter().all(|t| holds(p, t)))
        // A Chinese question's character pairs overlap by chance (维生素 and 抗生素 share 生素): a
        // page must hold at least half of them.
        .filter(|p| {
            let han: Vec<&String> = q_all.iter().filter(|t| is_han(t)).collect();
            han.is_empty() || 2 * han.iter().filter(|t| holds(p, t)).count() >= han.len()
        })
        .map(|p| {
            let s: f64 =
                known.iter().filter(|(t, _)| holds(p, t)).map(|(_, d)| idf(*d)).sum::<f64>()
                    / (p.terms.len() as f64).sqrt();
            let order = if p.lang == lang {
                0
            } else if p.lang == "en" {
                1
            } else {
                2
            };
            (order, s, p)
        })
        .collect();
    scored.sort_by(|a, b| a.0.cmp(&b.0).then(b.1.total_cmp(&a.1)));
    scored.into_iter().take(max).map(|(_, _, p)| p).collect()
}

/// **Whether a question names a topic a health body has a page on**: every topic word of some page's
/// title is in the question (*measles* names *Measles*; *Masern* names *Masern*). A page that only
/// shares a word with it (*essential* and *Essential tremor*) does not count.
pub fn names_a_topic(query: &str) -> bool {
    let q = terms(query);
    pack()
        .iter()
        .any(|p| !p.topic.is_empty() && p.topic.iter().all(|t| q.iter().any(|w| same(w, t))))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_listed_https_hosts_pass() {
        assert_eq!(
            listed("https://www.nhs.uk/conditions/vitamins-and-minerals/vitamin-k/").unwrap().name,
            "NHS (UK)"
        );
        assert_eq!(
            listed("https://WWW.WHO.INT/news-room").unwrap().name,
            "World Health Organization"
        );
        for bad in [
            "http://www.nhs.uk/conditions/",
            "https://www.nhs.uk.evil.example/",
            "https://evil.example/www.nhs.uk",
            "https://nhs.uk/",
            "https://www.nhs.uk:8443/",
            "https://user@www.nhs.uk/",
            "https://93.184.216.34/",
            "https://[::1]/",
            "https://localhost/",
            "ftp://www.who.int/",
        ] {
            assert!(listed(bad).is_none(), "{bad}");
        }
    }

    #[test]
    fn a_question_goes_to_the_sources_its_need_names() {
        let cases = [
            ("what does vitamin K do", Need::Health),
            ("à quoi sert la vitamine K", Need::Health),
            ("Wie äußert sich Eisenmangel?", Need::Health),
            ("sintomi della carenza di ferro", Need::Health),
            ("维生素K有什么作用", Need::Health),
            ("how much vitamin D a day", Need::Health),
            ("which foods are rich in vitamin K", Need::Food),
            ("how much vitamin K in spinach", Need::Food),
            ("combien de vitamine K dans les épinards", Need::Food),
            ("Wie viel Vitamin K ist in Spinat?", Need::Food),
            ("quanta vitamina K negli spinaci", Need::Food),
            ("菠菜中有多少维生素K", Need::Food),
            ("Lebensmittel mit viel Eisen", Need::Food),
            ("studies on vitamin K and bone density", Need::Papers),
            ("what does 10.1038/nature14539 show", Need::Papers),
            ("summarise arXiv:2401.01234", Need::Papers),
            ("PMID 31234567 findings", Need::Papers),
            ("how do I fix a borrow checker error in a Rust closure", Need::Code),
            ("who built the Analytical Engine", Need::General),
            ("history of the Roman aqueducts", Need::General),
            ("which amino acids are essential", Need::General),
            ("what is vitamin A for", Need::Health),
            ("a chain reaction in kidney function tests", Need::Health),
            ("Kopfschmerzen und Schwindel", Need::Health),
            ("Kopfschmerz nach dem Sport", Need::Health),
        ];
        for (q, want) in cases {
            assert_eq!(Need::of(q), want, "{q}");
        }
    }

    #[test]
    fn a_paper_is_known_by_its_identifier() {
        assert_eq!(
            paper_id("see 10.1038/nature14539."),
            Some(PaperId::Doi("10.1038/nature14539".into()))
        );
        assert_eq!(paper_id("PMC1234567"), Some(PaperId::Pmcid("PMC1234567".into())));
        assert_eq!(paper_id("PMID:31234567"), Some(PaperId::Pmid("31234567".into())));
        assert_eq!(paper_id("arXiv:2401.01234v2"), Some(PaperId::Arxiv("2401.01234".into())));
        assert_eq!(paper_id("version 10.2 of 2024.5"), None);
        assert_eq!(paper_id("call 0612.34567"), None);
        assert_eq!(paper_id("arXiv 0704.0001"), Some(PaperId::Arxiv("0704.0001".into())));
        assert_eq!(paper_id("2401.0123"), None);
        assert_eq!(paper_id("1313.12345"), None);
    }

    #[test]
    fn nutrients_are_named_in_five_languages() {
        let keys = |q: &str| nutrients(q).iter().map(|n| n.key).collect::<Vec<_>>();
        assert_eq!(keys("vitamin K"), ["vitamin_k1_ug"]);
        assert_eq!(keys("Vitamin-K-Mangel"), ["vitamin_k1_ug"]);
        assert_eq!(keys("vitamine B12 et fer"), ["vitamin_b12_ng", "iron_ug"]);
        assert_eq!(keys("vitamina b1"), ["thiamin_ug"]);
        assert_eq!(keys("Eisenmangel"), ["iron_ug"]);
        assert_eq!(keys("维生素D和钙"), ["vitamin_d_ng", "calcium_ug"]);
        assert!(keys("a vitamin shop").is_empty());
        assert!(keys("the fermentation of tea").is_empty());
    }

    #[test]
    fn what_is_left_of_an_amount_question_names_the_food() {
        assert_eq!(without_nutrients("vitamin K spinach"), "spinach");
        assert_eq!(without_nutrients("vitamine K épinards"), "epinards");
        assert_eq!(without_nutrients("Vitamin K Spinat"), "spinat");
        assert_eq!(without_nutrients("vitamina K spinaci"), "spinaci");
        assert_eq!(without_nutrients("菠菜中维生素K"), "菠菜");
        assert_eq!(without_nutrients("foods rich iron"), "");
    }

    #[test]
    fn a_food_from_the_app_s_list_reads_as_a_source() {
        assert_eq!(food_asked("how much vitamin K in spinach").as_deref(), Some("spinach"));
        assert_eq!(food_asked("combien de fer dans les lentilles").as_deref(), Some("lentilles"));
        assert_eq!(food_asked("what does vitamin K do"), None);
        assert_eq!(food_asked("foods rich in vitamin K"), None);
        let food = serde_json::json!({
            "name": "Spinach, raw", "src": "ciqual",
            "per100": {"vitamin_k1_ug": 483, "iron_ug": 2700, "kcal": 28},
            "borrowed": {}
        });
        assert_eq!(
            food_facts("how much vitamin K and iron in spinach", &food),
            Some((
                "The app's food list: Spinach, raw, from Ciqual (France)".to_string(),
                "Spinach, raw (Ciqual (France)). vitamin K: 483 µg per 100 g; iron: 2.7 mg per 100 g.".to_string()
            ))
        );
        let egg = serde_json::json!({
            "name": "Egg, whole, raw", "src": "crea", "per100": {"choline_ug": 335000, "vitamin_b12_ng": 890},
            "borrowed": {"choline_ug": "USDA: Egg, whole, raw"}
        });
        assert_eq!(
            food_facts("how much choline and vitamin B12 in eggs", &egg).unwrap().1,
            "Egg, whole, raw (CREA (Italy)). vitamin B12: 0.9 µg per 100 g; choline: 335 mg per 100 g (borrowed from USDA: Egg, whole, raw)."
        );
        // A nutrient the table never measured says nothing; none at all is no source.
        assert_eq!(food_facts("how much iodine in spinach", &food), None);
        assert_eq!(amount("protein_mg", 12500), "12.5 g");
        assert_eq!(amount("vitamin_d_ng", 2000), "2 µg");
    }

    #[test]
    fn a_question_s_focus_is_the_nutrient_or_topic_it_names() {
        assert_eq!(focus("à quoi sert la vitamine K").as_deref(), Some("vitamine K"));
        assert_eq!(focus("Wie äußert sich Eisenmangel?").as_deref(), Some("Eisenmangel"));
        assert_eq!(focus("维生素K有什么作用").as_deref(), Some("维生素k"));
        assert_eq!(focus("is measles dangerous for children").as_deref(), Some("measles"));
        assert_eq!(focus("who built the Analytical Engine"), None);
        assert!(shares_topic("Vitamin K", "vitamin k"));
        assert!(!shares_topic("Bleeding Disorders", "vitamin k"));
        assert!(!shares_topic("Feijoa sellowiana", "sert vitamine K"));
        assert_eq!(
            without_asking("studies on vitamin K and bone density"),
            "vitamin K bone density"
        );
        assert_eq!(without_asking("papers about vitamin A"), "vitamin A");
        assert!(covers(
            "Effects of iron supplementation in female soccer players and athletes",
            "iron supplements athletes"
        ));
        assert!(!covers(
            "Planktonic marine iron oxidizers drive iron mineralization",
            "iron supplements athletes"
        ));
    }

    #[test]
    fn every_page_in_the_pack_is_on_the_list() {
        assert!(pack().len() > 500, "{}", pack().len());
        for p in pack() {
            assert!(listed(p.url).is_some(), "{} is not on the list", p.url);
            assert!(["en", "fr", "de", "it", "zh"].contains(&p.lang), "{}", p.lang);
            assert!(!p.title.trim().is_empty(), "{}", p.url);
        }
    }
}
