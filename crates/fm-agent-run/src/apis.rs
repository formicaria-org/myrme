//! **The keyless APIs on the list** (2026-10-29, R2 of
//! `docs/research/2026-10-29-reputable-sources.md`): the address to ask and the parser for each answer,
//! pure and tested on answers saved from the live services (`tests/fixtures/sources/`).
//!
//! - **MedlinePlus** (US National Library of Medicine): health topics with their summaries, in
//!   English and Spanish; 85 requests a minute per address, answers to be kept 12–24 h.
//! - **Europe PMC**: abstracts of PubMed and more, with open-access full text; also how a PMID or DOI
//!   that Wikipedia cites is read (R5).
//! - **Crossref**: what a DOI is, when Europe PMC does not have it.
//! - **Stack Overflow** (Stack Exchange API): questions and their best answers; 300 requests a day per
//!   address without a key.

use crate::guarded::page_text;
use crate::sources::PaperId;
use fm_agent::SearchHit;
use serde_json::Value;

/// The MedlinePlus health-topics search for `terms`, in English or Spanish.
pub fn medlineplus_url(terms: &str, spanish: bool, n: usize) -> String {
    let db = if spanish { "healthTopicsSpanish" } else { "healthTopics" };
    format!("https://wsearch.nlm.nih.gov/ws/query?db={db}&term={}&retmax={n}", enc(terms))
}

/// A MedlinePlus search answer → one hit a topic: its page, its title, its full summary as text.
pub fn parse_medlineplus(xml: &str, max: usize) -> Vec<SearchHit> {
    xml.split("<document ")
        .skip(1)
        .filter_map(|doc| {
            let url = attr(doc, "url")?;
            let title = text_of(&content(doc, "title")?);
            let summary = text_of(&content(doc, "FullSummary").unwrap_or_default());
            (!title.is_empty() && !summary.is_empty()).then_some(SearchHit {
                title,
                url,
                text: summary,
            })
        })
        .take(max)
        .collect()
}

fn attr(tag: &str, name: &str) -> Option<String> {
    let start = tag.find(&format!("{name}=\""))? + name.len() + 2;
    let end = tag[start..].find('"')? + start;
    Some(tag[start..end].to_string())
}

/// A `<content name="…">` element's text, still escaped HTML.
fn content(doc: &str, name: &str) -> Option<String> {
    let open = format!("<content name=\"{name}\">");
    let start = doc.find(&open)? + open.len();
    let end = doc[start..].find("</content>")? + start;
    Some(doc[start..end].to_string())
}

/// Escaped HTML (as MedlinePlus and Europe PMC send it) → plain text, one line a paragraph.
fn text_of(escaped: &str) -> String {
    let html = escaped
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&amp;", "&");
    page_text(&format!("<body>{html}</body>"))
}

/// The Europe PMC query for a question's words: every word in the title or abstract, with an abstract.
pub fn europepmc_query(words: &str) -> String {
    let words: Vec<String> = words
        .split_whitespace()
        .map(|w| w.trim_matches(|c: char| !c.is_alphanumeric() && c != '-').replace('"', ""))
        .filter(|w| !w.is_empty())
        .collect();
    // "vitamin K" is one name: as two words, *K* matches nothing and *vitamin* anything.
    let mut parts: Vec<String> = Vec::new();
    let mut i = 0;
    while i < words.len() {
        let vitamin = words[i].to_ascii_lowercase().starts_with("vitamin");
        match words.get(i + 1) {
            Some(next) if vitamin && next.len() <= 3 => {
                parts.push(format!("\"{} {}\"", words[i], next));
                i += 2;
            }
            _ => {
                parts.push(words[i].clone());
                i += 1;
            }
        }
    }
    parts.truncate(6);
    if parts.is_empty() {
        return String::new();
    }
    format!("TITLE_ABS:({}) AND HAS_ABSTRACT:y", parts.join(" AND "))
}

/// The Europe PMC query for papers known by their identifiers.
pub fn europepmc_ids(ids: &[PaperId]) -> String {
    ids.iter()
        .filter_map(|id| match id {
            PaperId::Doi(d) => Some(format!("DOI:\"{}\"", d.replace('"', ""))),
            PaperId::Pmid(p) => Some(format!("(EXT_ID:{p} AND SRC:MED)")),
            PaperId::Pmcid(p) => Some(format!("PMCID:{p}")),
            PaperId::Arxiv(_) => None,
        })
        .collect::<Vec<_>>()
        .join(" OR ")
}

pub fn europepmc_url(query: &str, n: usize) -> String {
    format!(
        "https://www.ebi.ac.uk/europepmc/webservices/rest/search?query={}&resultType=core&pageSize={n}&format=json",
        enc(query)
    )
}

/// A Europe PMC answer → one hit a paper: its page on europepmc.org, its title with journal and
/// year, its abstract as text. A paper with no abstract is left out — there is nothing to quote.
pub fn parse_europepmc(json: &str, max: usize) -> Vec<SearchHit> {
    let Ok(v) = serde_json::from_str::<Value>(json.trim()) else { return Vec::new() };
    let Some(results) = v["resultList"]["result"].as_array() else { return Vec::new() };
    results
        .iter()
        .filter_map(|r| {
            let (id, src) = (r["id"].as_str()?, r["source"].as_str()?);
            let text = text_of(r["abstractText"].as_str()?);
            if text.is_empty() {
                return None;
            }
            let title =
                text_of(r["title"].as_str().unwrap_or_default()).trim_end_matches('.').to_string();
            let journal = r["journalInfo"]["journal"]["title"].as_str().unwrap_or_default();
            let year = r["pubYear"].as_str().unwrap_or_default();
            let about = [journal, year]
                .iter()
                .filter(|s| !s.is_empty())
                .copied()
                .collect::<Vec<_>>()
                .join(", ");
            let title = if about.is_empty() { title } else { format!("{title} ({about})") };
            Some(SearchHit {
                title,
                url: format!("https://europepmc.org/article/{src}/{id}"),
                text,
            })
        })
        .take(max)
        .collect()
}

pub fn crossref_url(doi: &str) -> String {
    format!("https://api.crossref.org/works/{}", enc(doi).replace("%2F", "/"))
}

/// A Crossref work → a hit: its DOI address, its title, and its abstract, or — where it has none —
/// what it is (journal and year), which is all there is to quote.
pub fn parse_crossref(json: &str) -> Option<SearchHit> {
    let v: Value = serde_json::from_str(json.trim()).ok()?;
    let m = &v["message"];
    let title = m["title"][0].as_str()?.to_string();
    let doi = m["DOI"].as_str()?;
    let journal = m["container-title"][0].as_str().unwrap_or_default();
    let year = m["issued"]["date-parts"][0][0].as_i64().map(|y| y.to_string()).unwrap_or_default();
    let abstract_ =
        m["abstract"].as_str().map(|a| page_text(&format!("<body>{a}</body>"))).unwrap_or_default();
    let about = [journal, year.as_str()]
        .iter()
        .filter(|s| !s.is_empty())
        .copied()
        .collect::<Vec<_>>()
        .join(", ");
    let text = if abstract_.is_empty() { format!("{title}. {about}.") } else { abstract_ };
    Some(SearchHit { title, url: format!("https://doi.org/{doi}"), text })
}

pub fn stackoverflow_search_url(words: &str, n: usize) -> String {
    format!(
        "https://api.stackexchange.com/2.3/search/advanced?order=desc&sort=relevance&q={}&site=stackoverflow&pagesize={n}&filter=withbody",
        enc(words)
    )
}

pub fn stackoverflow_answers_url(ids: &[u64]) -> String {
    let ids = ids.iter().map(u64::to_string).collect::<Vec<_>>().join(";");
    format!(
        "https://api.stackexchange.com/2.3/questions/{ids}/answers?order=desc&sort=votes&site=stackoverflow&filter=withbody&pagesize=20"
    )
}

/// The questions of a Stack Exchange search: (id, title, link, body as text).
pub fn parse_stackoverflow_questions(json: &str) -> Vec<(u64, String, String, String)> {
    let Ok(v) = serde_json::from_str::<Value>(json.trim()) else { return Vec::new() };
    v["items"]
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(|q| {
                    Some((
                        q["question_id"].as_u64()?,
                        text_of(q["title"].as_str()?),
                        q["link"].as_str()?.to_string(),
                        page_text(&format!(
                            "<body>{}</body>",
                            q["body"].as_str().unwrap_or_default()
                        )),
                    ))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Each question's best answer (accepted, else most voted), as text, from a Stack Exchange answers
/// answer.
pub fn parse_stackoverflow_answers(json: &str) -> std::collections::HashMap<u64, String> {
    let mut best: std::collections::HashMap<u64, (bool, i64, String)> = Default::default();
    let Ok(v) = serde_json::from_str::<Value>(json.trim()) else { return Default::default() };
    for a in v["items"].as_array().into_iter().flatten() {
        let (Some(q), Some(body)) = (a["question_id"].as_u64(), a["body"].as_str()) else {
            continue;
        };
        let rank = (a["is_accepted"].as_bool().unwrap_or(false), a["score"].as_i64().unwrap_or(0));
        if best.get(&q).is_none_or(|(acc, score, _)| rank > (*acc, *score)) {
            best.insert(q, (rank.0, rank.1, page_text(&format!("<body>{body}</body>"))));
        }
    }
    best.into_iter().map(|(q, (_, _, text))| (q, text)).collect()
}

/// The external links of a Wikipedia article (`prop=extlinks`).
pub fn parse_extlinks(json: &str) -> Vec<String> {
    let Ok(v) = serde_json::from_str::<Value>(json.trim()) else { return Vec::new() };
    v["query"]["pages"]
        .as_object()
        .into_iter()
        .flat_map(|pages| pages.values())
        .flat_map(|p| p["extlinks"].as_array().cloned().unwrap_or_default())
        .filter_map(|l| l["*"].as_str().or_else(|| l["url"].as_str()).map(String::from))
        .collect()
}

/// **The papers an article cites** (R5): its links to PubMed, PubMed Central and DOIs, as identifiers,
/// in the article's order, without repeats. A link only *names* a paper; it is read through Europe PMC,
/// an API on the list, never opened.
pub fn cited_papers(links: &[String], max: usize) -> Vec<PaperId> {
    let mut out: Vec<PaperId> = Vec::new();
    for l in links {
        let lower = l.to_ascii_lowercase();
        let after = |marker: &str| lower.find(marker).map(|i| &l[i + marker.len()..]);
        let num = |s: &str| s.chars().take_while(char::is_ascii_digit).collect::<String>();
        let id = if let Some(rest) =
            after("pubmed.ncbi.nlm.nih.gov/").or_else(|| after("ncbi.nlm.nih.gov/pubmed/"))
        {
            Some(num(rest)).filter(|n| (5..=9).contains(&n.len())).map(PaperId::Pmid)
        } else if let Some(rest) =
            after("/pmc/articles/pmc").or_else(|| after("pmc.ncbi.nlm.nih.gov/articles/pmc"))
        {
            Some(num(rest))
                .filter(|n| (5..=9).contains(&n.len()))
                .map(|n| PaperId::Pmcid(format!("PMC{n}")))
        } else if let Some(rest) = after("doi.org/") {
            let doi = urlish_decode(rest.split(['?', '#']).next().unwrap_or(rest));
            doi.starts_with("10.").then_some(PaperId::Doi(doi))
        } else {
            None
        };
        if let Some(id) = id {
            if !out.contains(&id) {
                out.push(id);
            }
        }
        if out.len() >= max {
            break;
        }
    }
    out
}

fn urlish_decode(s: &str) -> String {
    let mut out = Vec::new();
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Ok(x) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(x);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Percent-encode a query component (RFC 3986 unreserved stay literal).
fn enc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_medlineplus_answer_becomes_a_topic_with_its_summary() {
        let hits = parse_medlineplus(include_str!("../tests/fixtures/sources/medlineplus.xml"), 3);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].title, "Vitamin K");
        assert_eq!(hits[0].url, "https://medlineplus.gov/vitamink.html");
        assert!(
            hits[0].text.starts_with("Vitamins are substances that your body needs"),
            "{}",
            hits[0].text
        );
        assert!(
            hits[0].text.contains("Vitamin K helps your body by making proteins"),
            "{}",
            hits[0].text
        );
        assert!(!hits[0].text.contains('<'), "{}", hits[0].text);
    }

    #[test]
    fn a_europepmc_answer_becomes_papers_with_their_abstracts() {
        let hits = parse_europepmc(include_str!("../tests/fixtures/sources/europepmc.json"), 3);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].url, "https://europepmc.org/article/MED/42858896");
        assert!(hits[0].title.ends_with("(The Journal of nutrition, 2026)"), "{}", hits[0].title);
        assert!(
            hits[0].text.starts_with("Background\nMaternal vitamin D status"),
            "{}",
            hits[0].text
        );
    }

    #[test]
    fn a_europepmc_query_holds_every_word_and_asks_for_an_abstract() {
        assert_eq!(
            europepmc_query("vitamin K bone"),
            "TITLE_ABS:(\"vitamin K\" AND bone) AND HAS_ABSTRACT:y"
        );
        assert_eq!(europepmc_query("  "), "");
        assert_eq!(
            europepmc_ids(&[
                PaperId::Pmid("31234567".into()),
                PaperId::Doi("10.1/x".into()),
                PaperId::Arxiv("2401.01234".into())
            ]),
            "(EXT_ID:31234567 AND SRC:MED) OR DOI:\"10.1/x\""
        );
    }

    #[test]
    fn a_crossref_work_without_an_abstract_says_what_it_is() {
        let hit = parse_crossref(include_str!("../tests/fixtures/sources/crossref.json")).unwrap();
        assert_eq!(hit.title, "Deep learning");
        assert_eq!(hit.url, "https://doi.org/10.1038/nature14539");
        assert_eq!(hit.text, "Deep learning. Nature, 2015.");
    }

    #[test]
    fn a_stackoverflow_question_is_read_with_its_body() {
        let qs = parse_stackoverflow_questions(include_str!(
            "../tests/fixtures/sources/stackoverflow.json"
        ));
        assert_eq!(qs.len(), 2);
        assert_eq!(
            qs[0].1,
            "Borrow checker complains for closure inside loop if type not provided explicitly"
        );
        assert!(qs[0].2.starts_with("https://stackoverflow.com/questions/63856055/"));
        assert!(
            qs[0].3.starts_with("I have this code, for which borrow checker shows error:"),
            "{}",
            qs[0].3
        );
        let answers = parse_stackoverflow_answers(
            r#"{"items":[{"question_id":1,"body":"<p>low</p>","score":9,"is_accepted":false},
                         {"question_id":1,"body":"<p>accepted</p>","score":2,"is_accepted":true},
                         {"question_id":2,"body":"<p>only</p>","score":0}]}"#,
        );
        assert_eq!(answers[&1], "accepted");
        assert_eq!(answers[&2], "only");
    }

    #[test]
    fn the_papers_an_article_cites_are_named_by_their_identifiers() {
        let links: Vec<String> = [
            "https://pubmed.ncbi.nlm.nih.gov/12345678",
            "https://www.ncbi.nlm.nih.gov/pubmed/23456789?dopt=Abstract",
            "https://www.ncbi.nlm.nih.gov/pmc/articles/PMC7654321/",
            "https://doi.org/10.1093%2Fajcn%2F71.5.1201",
            "https://doi.org/10.1093/ajcn/71.5.1201",
            "https://icd.who.int/browse10/2019/en#/D68.4",
            "https://pubmed.ncbi.nlm.nih.gov/12345678",
        ]
        .map(String::from)
        .to_vec();
        assert_eq!(
            cited_papers(&links, 10),
            vec![
                PaperId::Pmid("12345678".into()),
                PaperId::Pmid("23456789".into()),
                PaperId::Pmcid("PMC7654321".into()),
                PaperId::Doi("10.1093/ajcn/71.5.1201".into()),
            ]
        );
        assert_eq!(cited_papers(&links, 2).len(), 2);
        assert_eq!(
            parse_extlinks(
                r#"{"query":{"pages":{"1":{"extlinks":[{"*":"https://doi.org/10.1/a"}]}}}}"#
            ),
            vec!["https://doi.org/10.1/a"]
        );
    }
}
