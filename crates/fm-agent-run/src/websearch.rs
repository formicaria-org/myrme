//! **In-process web search, from the sources on the list only** — every device's `/research`.
//!
//! It runs in the app's own process over `ureq`/rustls (the TLS stack the model fetcher already uses),
//! on a phone and on a computer alike. The orchestrator runs it; the model never does.
//!
//! **Reputable sources only** (2026-10-29, `docs/research/2026-10-29-reputable-sources.md`): code
//! reads the question, decides what the person is doing ([`Need`]) and reads only that need's sources,
//! all on the list ([`crate::sources::LISTED`]):
//! - **health** — MedlinePlus, the public bodies' pages from the sources pack, and Wikipedia labelled
//!   *encyclopedia, not medical guidance*;
//! - **food** — the bodies' pages, MedlinePlus and Wikipedia (the app's own food list is read by the
//!   caller, which can reach it);
//! - **papers** — Europe PMC, arXiv, the papers the Wikipedia article cites (read through Europe PMC),
//!   and Crossref for a DOI Europe PMC lacks;
//! - **code** — Stack Overflow and GitHub;
//! - **anything else** — Wikipedia, arXiv and GitHub, as before.
//!
//! Every hit is checked against the list once more before it leaves, and carries its body's name in
//! its title, so each claim says whose it is. Each source is **fail-soft**: one erroring or empty
//! never sinks the others. **No general search engine** is asked: DuckDuckGo's robots.txt disallows
//! its result pages and it has no API (2026-10-29); a person's own key is a later step.
//!
//! Honest privacy note: notes never leave the device, but the *question's words* reach these public
//! services.

use crate::apis;
use crate::sources::{listed, pack_search, paper_id, Need, PaperId};
use fm_agent::{AgentError, SearchHit, WebSearch};
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

/// **Who is asking, and where to reach the project** — never a browser's name. Wikimedia's policy
/// says a bot must not copy a browser's user agent and must give a contact, and its 2026 limits give
/// an identified client 200 requests a minute against 10 for an unidentified one. A project address,
/// never a person's (2026-10-29, `docs/research/2026-10-29-web-research.md`, D2).
pub const UA: &str = concat!(
    "formicaria-myrme/",
    env!("CARGO_PKG_VERSION"),
    " (+https://github.com/formicaria-org/myrme)"
);

/// Multi-source, text-only grounded search done in-process. See the module docs.
pub struct DirectSearch {
    max_results: usize,
    per_source: usize,
    timeout: Duration,
}

impl DirectSearch {
    pub fn new() -> Self {
        Self { max_results: 6, per_source: 4, timeout: Duration::from_secs(20) }
    }

    /// One answer from a source on the list, through the cache; `None` when the source is resting,
    /// errs or is not on the list.
    fn get(&self, agent: &ureq::Agent, url: &str, accept: Option<&str>) -> Option<String> {
        listed(url)?;
        let now = Instant::now();
        if let Some(body) = polite().fresh(url, now) {
            return Some(body);
        }
        if polite().resting(host(url), now) {
            return None;
        }
        let mut r = agent.get(url).set("User-Agent", UA);
        if let Some(a) = accept {
            r = r.set("Accept", a);
        }
        // Fail-soft: a source that errors contributes nothing.
        match r.call() {
            Ok(resp) => {
                let body = resp.into_string().ok()?;
                polite().keep(url.to_string(), body.clone(), now);
                Some(body)
            }
            Err(ureq::Error::Status(429 | 503, resp)) => {
                polite().rest(host(url), now + retry_after(resp.header("retry-after")));
                None
            }
            Err(_) => None,
        }
    }
}

impl Default for DirectSearch {
    fn default() -> Self {
        Self::new()
    }
}

impl WebSearch for DirectSearch {
    fn search(&self, query: &str) -> Result<Vec<SearchHit>, AgentError> {
        self.search_asked(query, query)
    }

    fn search_asked(&self, asked: &str, query: &str) -> Result<Vec<SearchHit>, AgentError> {
        let agent = ureq::builder().timeout(self.timeout).build();
        let get = |url: String, accept: Option<&str>| self.get(&agent, &url, accept);
        let n = self.per_source;
        let need = Need::of(asked);
        let lang = language(asked);
        // The person's own words first; the model's tidied query when they find nothing (a typo).
        let mut words = keywords(asked, lang);
        let wiki_wanted = !matches!(need, Need::Code) && paper_id(asked).is_none();
        let focus = crate::sources::focus(asked);
        let mut wiki = if wiki_wanted {
            wikipedia(&get, &words, focus.as_deref(), lang, n)
        } else {
            Wiki::default()
        };
        if wiki_wanted && wiki.hits.is_empty() && query.trim() != asked.trim() {
            let ql = language(query);
            let tidied = keywords(query, ql);
            let again = wikipedia(&get, &tidied, focus.as_deref(), ql, n);
            if !again.hits.is_empty() {
                (wiki, words) = (again, tidied);
            }
        }
        // English words for the English-only services: the person's, or the English article's title
        // that Wikipedia's interlanguage link gave — code crosses the languages, the model does not.
        let english = if lang == "en" {
            words.clone()
        } else {
            wiki.english.clone().unwrap_or_else(|| keywords(query, language(query)))
        };

        let ids: Vec<PaperId> = paper_id(asked).into_iter().collect();
        let terms = arxiv_query(&crate::sources::without_asking(&english));
        let sources: Vec<Vec<SearchHit>> = std::thread::scope(|s| {
            let wiki_hits = wiki.hits.clone();
            match need {
                Need::Health | Need::Food => {
                    let medline = s.spawn(|| {
                        get(apis::medlineplus_url(&english, false, 2), None)
                            .map(|b| apis::parse_medlineplus(&b, 2))
                            .unwrap_or_default()
                            .into_iter()
                            // Its search ranks loosely: *Uterine fibroids* came second for vitamin D.
                            .filter(|h| crate::sources::shares_topic(&h.title, &english))
                            .collect::<Vec<_>>()
                    });
                    let pages = s.spawn(|| {
                        pack_pages(&words, lang, &english, &format!("{words} {english}"))
                    });
                    let (medline, pages) = (join(medline), join(pages));
                    if need == Need::Health {
                        vec![medline, pages, wiki_hits]
                    } else {
                        vec![pages, medline, wiki_hits]
                    }
                }
                Need::Papers => {
                    let epmc = s.spawn(|| {
                        let q = if ids.is_empty() {
                            apis::europepmc_query(&crate::sources::without_asking(&english))
                        } else {
                            apis::europepmc_ids(&ids)
                        };
                        (!q.is_empty())
                            .then(|| get(apis::europepmc_url(&q, n), None))
                            .flatten()
                            .map(|b| apis::parse_europepmc(&b, n))
                            .unwrap_or_default()
                    });
                    let arxiv = s.spawn(|| {
                        let id = ids.iter().find_map(|i| match i {
                            PaperId::Arxiv(a) => Some(a.clone()),
                            _ => None,
                        });
                        let url = match id {
                            Some(a) => format!("https://export.arxiv.org/api/query?id_list={a}"),
                            None if ids.is_empty() => {
                                if terms.is_empty() {
                                    return Vec::new();
                                }
                                format!(
                                    "https://export.arxiv.org/api/query?search_query={terms}&max_results={n}"
                                )
                            }
                            None => return Vec::new(),
                        };
                        get(url, None).map(|b| parse_arxiv(&b, n)).unwrap_or_default()
                    });
                    let cited = s
                        .spawn(|| cited(&get, &wiki, &crate::sources::without_asking(&english), 2));
                    let (epmc, arxiv, cited) = (join(epmc), join(arxiv), join(cited));
                    let crossref = match ids.first() {
                        Some(PaperId::Doi(d)) if epmc.is_empty() => {
                            get(apis::crossref_url(d), None)
                                .and_then(|b| apis::parse_crossref(&b))
                                .into_iter()
                                .collect()
                        }
                        _ => Vec::new(),
                    };
                    vec![epmc, crossref, arxiv, cited]
                }
                Need::Code => {
                    let so = s.spawn(|| stackoverflow(&get, &english, 3));
                    let gh = s.spawn(|| github(&get, &english, n));
                    vec![join(so), join(gh)]
                }
                Need::General => {
                    let gh = s.spawn(|| github(&get, &english, n));
                    let arxiv = s.spawn(|| {
                        (!terms.is_empty())
                            .then(|| {
                                get(
                                    format!(
                                        "https://export.arxiv.org/api/query?search_query={terms}&max_results={n}"
                                    ),
                                    None,
                                )
                            })
                            .flatten()
                            .map(|b| parse_arxiv(&b, n))
                            .unwrap_or_default()
                    });
                    vec![wiki_hits, join(gh), join(arxiv)]
                }
            }
        });

        // Round-robin, so the top few span the sources; deduped by URL. Then the list once more, and
        // each hit named by its body. Empty is a legitimate answer: the caller grounds in the note, and
        // the quote check drops anything unsupported.
        let hits = interleave_dedup(&sources, self.max_results);
        Ok(hits.into_iter().filter_map(|h| labelled(h, need)).collect())
    }
}

fn join<T: Default>(h: std::thread::ScopedJoinHandle<'_, T>) -> T {
    h.join().unwrap_or_default()
}

/// A hit as it leaves: only from a body on the list, its title beginning with the body's name —
/// Wikipedia, for a health question, as *an encyclopedia, not medical guidance*.
pub fn labelled(hit: SearchHit, need: Need) -> Option<SearchHit> {
    let body = listed(&hit.url)?;
    let name = if body.name == "Wikipedia" && need == Need::Health {
        "Wikipedia (encyclopedia, not medical guidance)"
    } else {
        body.name
    };
    Some(SearchHit { title: format!("{name}: {}", hit.title), ..hit })
}

/// **The bodies' pages for a health or food question** (R3 + R4): the sources pack searched with the
/// person's words in their language and with the English words, the two best pages read through the
/// guarded fetch, each cut to its passage that best answers `asked`.
fn pack_pages(words: &str, lang: &str, english: &str, asked: &str) -> Vec<SearchHit> {
    let mut pages = pack_search(words, lang, 2);
    if lang != "en" {
        for p in pack_search(english, "en", 2) {
            if !pages.iter().any(|q| q.url == p.url) {
                pages.push(p);
            }
        }
    }
    pages.truncate(2);
    std::thread::scope(|s| {
        let reads: Vec<_> = pages
            .iter()
            .map(|p| s.spawn(move || crate::guarded::fetch(p.url).map(|text| (p, text))))
            .collect();
        reads
            .into_iter()
            .filter_map(|r| r.join().ok().flatten())
            .filter_map(|(p, text)| {
                let text = best_passage(&format!("{asked} {}", p.title), &passages(&text))?;
                Some(SearchHit { title: p.title.to_string(), url: p.url.to_string(), text })
            })
            .collect()
    })
}

/// **The papers a Wikipedia article cites** (R5), read through Europe PMC: the article's links to
/// PubMed and DOIs, at most twenty, asked for in one query; the abstracts that best answer `asked`.
fn cited(
    get: &(dyn Fn(String, Option<&str>) -> Option<String> + Sync),
    wiki: &Wiki,
    asked: &str,
    max: usize,
) -> Vec<SearchHit> {
    let Some((lang, title)) = wiki.first.clone() else { return Vec::new() };
    let links = get(
        format!(
            "https://{lang}.wikipedia.org/w/api.php?action=query&prop=extlinks&ellimit=500&redirects=1&titles={}&format=json",
            enc(&title)
        ),
        None,
    )
    .map(|b| apis::parse_extlinks(&b))
    .unwrap_or_default();
    let ids = apis::cited_papers(&links, 20);
    if ids.is_empty() {
        return Vec::new();
    }
    // An article cites papers on everything it covers (*Iron* cites geology); a paper is kept only
    // when its title and abstract hold most of the question's topic words.
    let papers: Vec<SearchHit> = get(apis::europepmc_url(&apis::europepmc_ids(&ids), 20), None)
        .map(|b| apis::parse_europepmc(&b, 20))
        .unwrap_or_default()
        .into_iter()
        .filter(|p| crate::sources::covers(&format!("{} {}", p.title, p.text), asked))
        .collect();
    let texts: Vec<String> = papers.iter().map(|p| format!("{} {}", p.title, p.text)).collect();
    let mut ranked: Vec<(usize, &SearchHit)> = Vec::new();
    for _ in 0..max {
        let left: Vec<String> = texts
            .iter()
            .enumerate()
            .filter(|(i, _)| !ranked.iter().any(|(j, _)| j == i))
            .map(|(_, t)| t.clone())
            .collect();
        let Some(best) = best_passage(asked, &left) else { break };
        let Some(i) = texts.iter().position(|t| *t == best) else { break };
        ranked.push((i, &papers[i]));
    }
    ranked.into_iter().map(|(_, p)| p.clone()).collect()
}

/// Stack Overflow: the questions that match, each with its best answer.
fn stackoverflow(
    get: &(dyn Fn(String, Option<&str>) -> Option<String> + Sync),
    words: &str,
    n: usize,
) -> Vec<SearchHit> {
    let Some(found) = get(apis::stackoverflow_search_url(words, n), None) else {
        return Vec::new();
    };
    let questions = apis::parse_stackoverflow_questions(&found);
    let ids: Vec<u64> = questions.iter().map(|q| q.0).collect();
    let answers = if ids.is_empty() {
        HashMap::new()
    } else {
        get(apis::stackoverflow_answers_url(&ids), None)
            .map(|b| apis::parse_stackoverflow_answers(&b))
            .unwrap_or_default()
    };
    questions
        .into_iter()
        .map(|(id, title, url, body)| {
            let text = match answers.get(&id) {
                Some(a) => format!("Question: {body}\nAnswer: {a}"),
                None => format!("Question: {body}"),
            };
            SearchHit { title, url, text }
        })
        .collect()
}

fn github(
    get: &(dyn Fn(String, Option<&str>) -> Option<String> + Sync),
    words: &str,
    n: usize,
) -> Vec<SearchHit> {
    if words.trim().is_empty() {
        return Vec::new();
    }
    get(
        format!("https://api.github.com/search/repositories?q={}&per_page={n}", enc(words)),
        Some("application/vnd.github+json"),
    )
    .map(|b| parse_github(&b, n))
    .unwrap_or_default()
}

/// What the Wikipedia step found: its hits, the first article (to read its citations) and the
/// English article's title (to search the English-only services with).
#[derive(Default, Clone)]
struct Wiki {
    hits: Vec<SearchHit>,
    first: Option<(&'static str, String)>,
    english: Option<String>,
}

/// **Wikipedia, read whole, in the person's language and in English** (2026-10-29, W1 of
/// `docs/research/2026-10-29-web-research.md`). A search snippet is ~300 characters cut around the
/// match, too little to quote an answer from; and English-only search answered a French or Chinese
/// question with nothing. So: that language's Wikipedia is searched with the question's keywords
/// (`words`, its filler words left out: given a whole question, Wikipedia's search answered "how much
/// iron a day" in German with *Requiem*), and its best article's interlanguage link gives the English
/// one — **code crosses the languages, the model does not translate**; English Wikipedia is searched
/// too; and from each article, fetched as plain text, the passage that best answers the question (BM25)
/// is the hit. An article with no passage that answers gives its opening.
///
/// **The best article, not the first** (R1, 2026-10-29): the search's first answer was often a longer
/// article that merely mentions the words; of its first five, the one whose title holds most of the
/// keywords is read.
fn wikipedia(
    get: &(dyn Fn(String, Option<&str>) -> Option<String> + Sync),
    words: &str,
    focus: Option<&str>,
    lang: &'static str,
    n: usize,
) -> Wiki {
    if words.trim().is_empty() {
        return Wiki::default();
    }
    let api =
        |l: &str, rest: String| format!("https://{l}.wikipedia.org/w/api.php?{rest}&format=json");
    let search_once = |l: &str, w: &str| -> Vec<String> {
        let titles = get(
            api(l, format!("action=query&list=search&srsearch={}&srlimit=5&srprop=", enc(w))),
            None,
        )
        .map(|b| parse_titles(&b))
        .unwrap_or_default();
        best_titles(w, titles)
    };
    // A verb in the question (*sert*, *äußert*) can steer the search to articles that merely use it.
    // When the best article shares no word with the question, the question's focus — the nutrient it
    // names, or the topic a health body has a page on — is searched instead.
    let search = |l: &str, w: &str| -> Vec<String> {
        let found = search_once(l, w);
        let shares = found.first().is_some_and(|t| crate::sources::shares_topic(t, w));
        match focus {
            Some(f) if !shares && f != w => {
                let again = search_once(l, f);
                if again.is_empty() {
                    found
                } else {
                    again
                }
            }
            _ => found,
        }
    };
    let mut articles: Vec<(&'static str, String)> = Vec::new();
    let mut english = None;
    if lang != "en" {
        if let Some(t) = search(lang, words).into_iter().next() {
            english = get(
                api(
                    lang,
                    format!("action=query&prop=langlinks&lllang=en&redirects=1&titles={}", enc(&t)),
                ),
                None,
            )
            .and_then(|b| parse_langlink(&b));
            articles.push((lang, t));
            articles.extend(english.clone().map(|e| ("en", e)));
        }
    }
    // A question in another language is searched in English by its English article's title.
    let en_words =
        if lang == "en" { words.to_string() } else { english.clone().unwrap_or_default() };
    if !en_words.is_empty() {
        for t in search("en", &en_words).into_iter().take(2) {
            if !articles.iter().any(|(l, x)| *l == "en" && *x == t) {
                articles.push(("en", t));
            }
        }
    }
    articles.truncate(3);
    let first = articles.first().cloned();
    let hits = articles
        .into_iter()
        .filter_map(|(l, title)| {
            let text = get(
                api(
                    l,
                    format!(
                        "action=query&prop=extracts&explaintext=1&exsectionformat=plain&redirects=1&titles={}",
                        enc(&title)
                    ),
                ),
                None,
            )
            .and_then(|b| parse_extract(&b))?;
            // The English article of a question asked in another language shares few of its words;
            // its own title's words are what it is about.
            let asked = format!("{words} {title}");
            let text = best_passage(&asked, &passages(&text))?;
            let url = format!("https://{l}.wikipedia.org/wiki/{}", enc(&title.replace(' ', "_")));
            Some(SearchHit { title, url, text })
        })
        .take(n)
        .collect();
    let english = if lang == "en" { first.as_ref().map(|(_, t)| t.clone()) } else { english };
    Wiki { hits, first, english }
}

/// A search's titles, the one holding most of the keywords first; the search's own order otherwise.
/// For Chinese, whose words have no spaces, a title counts by the character pairs it shares.
pub fn best_titles(words: &str, titles: Vec<String>) -> Vec<String> {
    let want: std::collections::BTreeSet<String> = terms(words).into_iter().collect();
    let mut ranked: Vec<(usize, usize, String)> = titles
        .into_iter()
        .enumerate()
        .map(|(i, t)| {
            let have: std::collections::BTreeSet<String> = terms(&t).into_iter().collect();
            let shared = want.intersection(&have).count();
            // A title naming more than was asked ("Vitamin K antagonist" for "vitamin K") ranks below
            // one naming exactly that.
            let extra = have.difference(&want).count();
            (shared, i + extra, t)
        })
        .collect();
    ranked.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    ranked.into_iter().map(|(_, _, t)| t).collect()
}

/// The language a question is written in, by script and by its commonest small words; English when
/// nothing says otherwise. Only the languages the app is used in.
pub fn language(text: &str) -> &'static str {
    if text.chars().any(|c| matches!(c as u32, 0x4E00..=0x9FFF | 0x3400..=0x4DBF)) {
        return "zh";
    }
    let words: Vec<String> = text
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_lowercase)
        .collect();
    // English's small words count too: one word shared with French (*on*) must not outvote three
    // English ones. English wins a tie.
    let en = words.iter().filter(|w| ARXIV_FILLERS.contains(&w.as_str())).count();
    let mut best = ("en", en);
    for (lang, list) in [("fr", FILLERS_FR), ("de", FILLERS_DE), ("it", FILLERS_IT)] {
        let n = words.iter().filter(|w| list.contains(&w.as_str())).count();
        if n > best.1 {
            best = (lang, n);
        }
    }
    best.0
}

/// The question without its filler words, in its own language (and English's, which a question in any
/// language may borrow); Chinese without its particles. What a search engine is given.
pub fn keywords(text: &str, lang: &str) -> String {
    if lang == "zh" {
        return text
            .chars()
            .filter(|c| !"的了吗呢是在和与什么怎么如何哪些多少".contains(*c))
            .collect();
    }
    let own = match lang {
        "fr" => FILLERS_FR,
        "de" => FILLERS_DE,
        "it" => FILLERS_IT,
        _ => &[],
    };
    text.split(|c: char| !(c.is_alphanumeric() || c == '-'))
        .filter(|w| !w.is_empty() && w.chars().any(char::is_alphanumeric))
        .filter(|w| {
            // Compared without accents on either side: *à* is the filler *a*, *für* is *fur*.
            let l = fm_agent::textmatch::loose(w);
            let is = |list: &[&str]| list.iter().any(|f| fm_agent::textmatch::loose(f) == l);
            !is(ARXIV_FILLERS) && !is(own) && !is(EXTRA_FILLERS)
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Question words English's filler list leaves in, because a paper may be about them.
const EXTRA_FILLERS: &[&str] = &["much", "many", "per", "day"];
const FILLERS_FR: &[&str] = &[
    "le",
    "la",
    "les",
    "l",
    "un",
    "une",
    "des",
    "du",
    "de",
    "d",
    "en",
    "et",
    "est",
    "sont",
    "au",
    "aux",
    "pour",
    "par",
    "sur",
    "dans",
    "avec",
    "que",
    "qui",
    "quoi",
    "quel",
    "quels",
    "quelle",
    "quelles",
    "comment",
    "combien",
    "pourquoi",
    "où",
    "ce",
    "cet",
    "cette",
    "ces",
    "il",
    "elle",
    "on",
    "faut",
    "contient",
    "contiennent",
    "jour",
];
const FILLERS_DE: &[&str] = &[
    "der",
    "die",
    "das",
    "den",
    "dem",
    "des",
    "ein",
    "eine",
    "einen",
    "einem",
    "und",
    "ist",
    "sind",
    "von",
    "zu",
    "im",
    "mit",
    "für",
    "auf",
    "wie",
    "viel",
    "viele",
    "was",
    "wer",
    "warum",
    "wann",
    "wo",
    "man",
    "braucht",
    "pro",
    "tag",
    "welche",
    "enthalten",
];
const FILLERS_IT: &[&str] = &[
    "il",
    "lo",
    "la",
    "i",
    "gli",
    "le",
    "l",
    "un",
    "una",
    "uno",
    "di",
    "del",
    "della",
    "dei",
    "degli",
    "delle",
    "da",
    "con",
    "per",
    "su",
    "che",
    "chi",
    "come",
    "quanto",
    "quanta",
    "quanti",
    "quale",
    "quali",
    "perché",
    "dove",
    "è",
    "sono",
    "serve",
    "al",
    "giorno",
    "contengono",
];

/// An article cut into passages of whole sentences, up to ~600 characters, one paragraph at a time;
/// a paragraph shorter than a heading's worth is left out.
pub fn passages(text: &str) -> Vec<String> {
    let text: String = text.chars().take(60_000).collect();
    let mut out = Vec::new();
    for para in text.lines().map(str::trim).filter(|p| p.chars().count() > 40) {
        let mut cur = String::new();
        for sentence in sentences(para) {
            if !cur.is_empty() && cur.chars().count() + sentence.chars().count() > 600 {
                out.push(std::mem::take(&mut cur));
            }
            if !cur.is_empty() {
                cur.push(' ');
            }
            cur.push_str(sentence);
        }
        if !cur.is_empty() {
            out.push(cur);
        }
    }
    out
}

fn sentences(para: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0;
    let chars: Vec<(usize, char)> = para.char_indices().collect();
    for (k, &(i, c)) in chars.iter().enumerate() {
        let end = matches!(c, '。' | '！' | '？')
            || (matches!(c, '.' | '!' | '?')
                && chars.get(k + 1).is_some_and(|(_, n)| n.is_whitespace()));
        if end {
            let j = i + c.len_utf8();
            out.push(para[start..j].trim());
            start = j;
        }
    }
    if start < para.len() {
        out.push(para[start..].trim());
    }
    out.into_iter().filter(|s| !s.is_empty()).collect()
}

/// The passage that best answers `asked` (BM25 over the article's own passages), or its first passage
/// when none shares a word with it; `None` for an article with no text.
pub fn best_passage(asked: &str, passages: &[String]) -> Option<String> {
    let first = passages.first()?.clone();
    let q: std::collections::BTreeSet<String> = terms(asked).into_iter().collect();
    let docs: Vec<Vec<String>> = passages.iter().map(|p| terms(p)).collect();
    let n = docs.len() as f64;
    let avg = docs.iter().map(Vec::len).sum::<usize>() as f64 / n;
    let (k1, b) = (1.2, 0.75);
    let score = |d: &Vec<String>| -> f64 {
        q.iter()
            .map(|t| {
                let tf = d.iter().filter(|w| *w == t).count() as f64;
                if tf == 0.0 {
                    return 0.0;
                }
                let df = docs.iter().filter(|x| x.contains(t)).count() as f64;
                let idf = (1.0 + (n - df + 0.5) / (df + 0.5)).ln();
                idf * tf * (k1 + 1.0) / (tf + k1 * (1.0 - b + b * d.len() as f64 / avg))
            })
            .sum()
    };
    let best = docs
        .iter()
        .enumerate()
        .map(|(i, d)| (score(d), i))
        .filter(|(s, _)| *s > 0.0)
        .max_by(|a, b| a.0.total_cmp(&b.0).then(b.1.cmp(&a.1)));
    Some(best.map_or(first, |(_, i)| passages[i].clone()))
}

/// Words for matching: lower case, accents folded, letters and digits; Chinese as character pairs. A
/// run mixing scripts (`维生素k作用`) is split where the script changes, so `维生素K` shares its pairs.
pub(crate) fn terms(text: &str) -> Vec<String> {
    let folded = fm_agent::textmatch::loose(text);
    let han = |c: char| matches!(c as u32, 0x4E00..=0x9FFF | 0x3400..=0x4DBF);
    let mut out = Vec::new();
    for w in folded.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()) {
        let chars: Vec<char> = w.chars().collect();
        for run in chars.chunk_by(|a, b| han(*a) == han(*b)) {
            if han(run[0]) && run.len() > 1 {
                out.extend(run.windows(2).map(|p| p.iter().collect::<String>()));
            } else {
                out.push(run.iter().collect());
            }
        }
    }
    out
}

/// The titles of a MediaWiki search answer.
pub fn parse_titles(json: &str) -> Vec<String> {
    serde_json::from_str::<serde_json::Value>(json.trim())
        .ok()
        .and_then(|v| v["query"]["search"].as_array().cloned())
        .unwrap_or_default()
        .iter()
        .filter_map(|r| r["title"].as_str().map(String::from))
        .collect()
}

/// The English title an article's interlanguage link names, if any.
pub fn parse_langlink(json: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(json.trim()).ok()?;
    v["query"]["pages"]
        .as_object()?
        .values()
        .find_map(|p| p["langlinks"][0]["*"].as_str().map(String::from))
}

/// An article's plain text from a `prop=extracts` answer; `None` when empty.
pub fn parse_extract(json: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(json.trim()).ok()?;
    v["query"]["pages"]
        .as_object()?
        .values()
        .find_map(|p| p["extract"].as_str().map(String::from))
        .filter(|t| !t.trim().is_empty())
}

/// Wikipedia MediaWiki search JSON → hits. Pure, tested.
pub fn parse_wikipedia(json: &str, max: usize) -> Vec<SearchHit> {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(json.trim()) else { return Vec::new() };
    let Some(arr) = v["query"]["search"].as_array() else { return Vec::new() };
    arr.iter()
        .filter_map(|r| {
            let title = r["title"].as_str()?.to_string();
            let url = format!("https://en.wikipedia.org/wiki/{}", enc(&title.replace(' ', "_")));
            let text = strip_html(r["snippet"].as_str().unwrap_or(""));
            Some(SearchHit { title, url, text })
        })
        .take(max)
        .collect()
}

/// GitHub repository search JSON → hits. Pure, tested.
pub fn parse_github(json: &str, max: usize) -> Vec<SearchHit> {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(json.trim()) else { return Vec::new() };
    let Some(arr) = v["items"].as_array() else { return Vec::new() };
    arr.iter()
        .filter_map(|r| {
            Some(SearchHit {
                title: r["full_name"].as_str()?.to_string(),
                url: r["html_url"].as_str()?.to_string(),
                text: r["description"].as_str().unwrap_or("").to_string(),
            })
        })
        .take(max)
        .collect()
}

/// arXiv Atom feed → hits. Hand-parsed (no XML dep): arXiv's Atom shape is stable, and the fields we
/// want — `<title>`, `<summary>`, `<id>` — are plain default-namespaced elements inside each
/// `<entry>`. Pure, tested.
pub fn parse_arxiv(xml: &str, max: usize) -> Vec<SearchHit> {
    let mut out = Vec::new();
    for entry in xml.split("<entry>").skip(1) {
        let entry = entry.split("</entry>").next().unwrap_or("");
        let url = inner(entry, "id").trim().to_string();
        if url.is_empty() {
            continue;
        }
        out.push(SearchHit {
            title: strip_html(&inner(entry, "title")),
            url,
            text: strip_html(&inner(entry, "summary")),
        });
        if out.len() >= max {
            break;
        }
    }
    out
}

/// The text between the first `<tag>` and its `</tag>` in `s`, or empty.
fn inner(s: &str, tag: &str) -> String {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let Some(a) = s.find(&open) else { return String::new() };
    let rest = &s[a + open.len()..];
    match rest.find(&close) {
        Some(b) => rest[..b].to_string(),
        None => String::new(),
    }
}

/// **arXiv's `search_query`, with every word that carries meaning required.** arXiv reads a space
/// between terms as OR, so `all:vitamin K deficiency` matched 642,502 papers and put a
/// particle-physics paper first; ANDed, the same words matched none, which was the truth (2026-10-29,
/// measured; D1 of `docs/research/2026-10-29-web-research.md`).
///
/// **Requiring a filler word is the opposite mistake**: *how does attention work in transformers*
/// matched 0 papers with every word required and 5,198 without *how*, *does* and *in* (measured the
/// same day). So [`ARXIV_FILLERS`] — English function words, arXiv being English — are left out;
/// a single letter such as the *K* of *vitamin K* is kept. Words are runs of letters and digits; empty
/// when none is left, and then arXiv is not asked.
pub fn arxiv_query(query: &str) -> String {
    query
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty() && !ARXIV_FILLERS.contains(&w.to_lowercase().as_str()))
        .map(|w| format!("all:{}", enc(w)))
        .collect::<Vec<_>>()
        .join("+AND+")
}

/// Words no paper is found by: articles, pronouns, auxiliaries, prepositions, conjunctions and the
/// question words. Kept short and closed on purpose — a content word wrongly here makes a search
/// broader, and one wrongly missing makes it find nothing, so only words that are never the subject
/// belong. **Kept in step with `agents/search-proxy.py`'s `ARXIV_FILLERS`.**
pub const ARXIV_FILLERS: &[&str] = &[
    "a", "about", "after", "all", "also", "am", "an", "and", "any", "are", "as", "at", "be",
    "been", "being", "between", "but", "by", "can", "could", "did", "do", "does", "doing",
    "during", "each", "for", "from", "had", "has", "have", "having", "he", "her", "his", "how",
    "i", "if", "in", "into", "is", "it", "its", "may", "me", "might", "my", "of", "on", "or",
    "our", "she", "should", "so", "some", "such", "than", "that", "the", "their", "them", "then",
    "there", "these", "they", "this", "those", "through", "to", "us", "was", "we", "were", "what",
    "when", "where", "which", "while", "who", "whom", "whose", "why", "will", "with", "would",
    "you", "your",
];

/// **Asked once, kept a while; a busy source is left alone** (2026-10-29, W2 of
/// `docs/research/2026-10-29-web-research.md`). Wikipedia's limit for an identified client is one
/// bucket shared by every install, and the other sources count per address, so the same question
/// asked again is answered from memory, and a source that says *too many requests* (429) or
/// *unavailable* (503) is not asked again until its `Retry-After` has passed — never retried in a loop.
/// In memory only, for the life of the process: nothing about a search is written to disk.
#[derive(Default)]
pub(crate) struct Polite {
    kept: HashMap<String, (Instant, String)>,
    resting: HashMap<String, Instant>,
}

/// How long an answer is reused: MedlinePlus asks for 12–24 h; search results change slowly.
const KEEP: Duration = Duration::from_secs(12 * 60 * 60);
/// How many answers are kept; the oldest goes first.
const KEPT_MAX: usize = 256;

impl Polite {
    pub(crate) fn fresh(&self, url: &str, now: Instant) -> Option<String> {
        self.kept
            .get(url)
            .filter(|(at, _)| now.saturating_duration_since(*at) < KEEP)
            .map(|(_, body)| body.clone())
    }

    pub(crate) fn keep(&mut self, url: String, body: String, now: Instant) {
        self.kept.retain(|_, (at, _)| now.saturating_duration_since(*at) < KEEP);
        if self.kept.len() >= KEPT_MAX {
            if let Some(oldest) =
                self.kept.iter().min_by_key(|(_, (at, _))| *at).map(|(k, _)| k.clone())
            {
                self.kept.remove(&oldest);
            }
        }
        self.kept.insert(url, (now, body));
    }

    pub(crate) fn rest(&mut self, host: &str, until: Instant) {
        self.resting.insert(host.to_string(), until);
    }

    pub(crate) fn resting(&self, host: &str, now: Instant) -> bool {
        self.resting.get(host).is_some_and(|until| now < *until)
    }
}

pub(crate) fn polite() -> std::sync::MutexGuard<'static, Polite> {
    static POLITE: OnceLock<Mutex<Polite>> = OnceLock::new();
    // A poisoned lock still holds a usable cache; losing it is worse than a stale entry.
    POLITE.get_or_init(Default::default).lock().unwrap_or_else(|e| e.into_inner())
}

/// How long to leave a source alone after it said 429 or 503: its `Retry-After` in seconds, at most an
/// hour; a minute when it gives none, or gives a date.
pub(crate) fn retry_after(header: Option<&str>) -> Duration {
    let secs = header.and_then(|h| h.trim().parse::<u64>().ok()).unwrap_or(60);
    Duration::from_secs(secs.min(60 * 60))
}

/// The host of a URL, for resting a source as a whole.
fn host(url: &str) -> &str {
    let rest = url.split_once("://").map_or(url, |(_, r)| r);
    rest.split(['/', '?']).next().unwrap_or(rest)
}

/// Round-robin over the sources in order, dedupe by URL, cap at `max`.
fn interleave_dedup(sources: &[Vec<SearchHit>], max: usize) -> Vec<SearchHit> {
    let mut out: Vec<SearchHit> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let mut i = 0;
    loop {
        let mut any = false;
        for src in sources {
            if let Some(h) = src.get(i) {
                any = true;
                if !h.url.is_empty() && seen.insert(h.url.clone()) {
                    out.push(h.clone());
                    if out.len() >= max {
                        return out;
                    }
                }
            }
        }
        if !any {
            break;
        }
        i += 1;
    }
    out
}

/// Strip HTML tags and unescape the handful of entities the source snippets use, then collapse
/// whitespace — so a Wikipedia `<span class="searchmatch">` snippet or an arXiv summary becomes clean
/// prose the model can quote.
fn strip_html(s: &str) -> String {
    let mut plain = String::with_capacity(s.len());
    let mut in_tag = false;
    for c in s.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => plain.push(c),
            _ => {}
        }
    }
    let plain = plain
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&nbsp;", " ");
    plain.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Percent-encode a query string component (RFC 3986 unreserved stay literal).
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
    fn wikipedia_json_becomes_hits_with_clean_snippets() {
        let json = r#"{"query":{"search":[
            {"title":"Ada Lovelace","snippet":"<span class=\"searchmatch\">Ada</span> Lovelace was a mathematician &amp; writer"},
            {"title":"Analytical Engine","snippet":"a proposed mechanical computer"}
        ]}}"#;
        let hits = parse_wikipedia(json, 6);
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].title, "Ada Lovelace");
        assert_eq!(hits[0].url, "https://en.wikipedia.org/wiki/Ada_Lovelace");
        assert_eq!(
            hits[0].text, "Ada Lovelace was a mathematician & writer",
            "tags stripped, entity decoded"
        );
    }

    #[test]
    fn github_json_becomes_hits() {
        let json = r#"{"items":[
            {"full_name":"ggerganov/whisper.cpp","html_url":"https://github.com/ggerganov/whisper.cpp","description":"Port of OpenAI's Whisper"},
            {"full_name":"a/b","html_url":"https://github.com/a/b","description":null}
        ]}"#;
        let hits = parse_github(json, 6);
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].url, "https://github.com/ggerganov/whisper.cpp");
        assert_eq!(hits[1].text, "", "a null description is empty, not a crash");
    }

    #[test]
    fn arxiv_atom_becomes_hits() {
        let xml = r#"<feed xmlns="http://www.w3.org/2005/Atom">
          <entry><id>http://arxiv.org/abs/1706.03762</id><title>Attention Is All You Need</title>
            <summary>The dominant sequence transduction models...</summary></entry>
          <entry><id>http://arxiv.org/abs/2005.14165</id><title>Language Models are Few-Shot Learners</title>
            <summary>We show that scaling up...</summary></entry>
        </feed>"#;
        let hits = parse_arxiv(xml, 6);
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].url, "http://arxiv.org/abs/1706.03762");
        assert_eq!(hits[0].title, "Attention Is All You Need");
        assert!(hits[1].text.starts_with("We show that scaling"));
    }

    #[test]
    fn interleave_spans_sources_and_dedupes() {
        let hit = |u: &str| SearchHit { title: u.into(), url: u.into(), text: String::new() };
        let a = vec![hit("a1"), hit("a2")];
        let b = vec![hit("b1"), hit("dup")];
        let c = vec![hit("dup"), hit("c2")]; // "dup" already from b → dropped once
        let out = interleave_dedup(&[a, b, c], 10);
        let urls: Vec<_> = out.iter().map(|h| h.url.as_str()).collect();
        assert_eq!(urls, ["a1", "b1", "dup", "a2", "c2"], "round-robin, first-seen url wins");
    }

    #[test]
    fn malformed_input_yields_no_hits_not_a_panic() {
        assert!(parse_wikipedia("not json", 6).is_empty());
        assert!(parse_github("{}", 6).is_empty());
        assert!(parse_arxiv("<feed></feed>", 6).is_empty());
    }

    /// D1: arXiv reads a space as OR — "vitamin K deficiency" matched 642,502 papers, a physics paper
    /// first. Every word is required, each encoded; no words means arXiv is not asked.
    #[test]
    fn an_arxiv_query_requires_every_word() {
        assert_eq!(arxiv_query("vitamin K deficiency"), "all:vitamin+AND+all:K+AND+all:deficiency");
        assert_eq!(arxiv_query("self-attention, 2017"), "all:self+AND+all:attention+AND+all:2017");
        assert_eq!(
            arxiv_query("carence en vitamine K – légumes"),
            "all:carence+AND+all:en+AND+all:vitamine+AND+all:K+AND+all:l%C3%A9gumes"
        );
        assert_eq!(arxiv_query(" ?! "), "");
    }

    /// Requiring a filler word finds nothing: *how does attention work in transformers* matched 0
    /// papers ANDed whole, 5,198 without its fillers (2026-10-29). Fillers go, in any case; the
    /// content words and a lone letter stay; a question of fillers only asks arXiv nothing.
    #[test]
    fn an_arxiv_query_leaves_out_the_words_no_paper_is_found_by() {
        assert_eq!(
            arxiv_query("How does attention work in transformers?"),
            "all:attention+AND+all:work+AND+all:transformers"
        );
        assert_eq!(arxiv_query("what does vitamin K do"), "all:vitamin+AND+all:K");
        assert_eq!(arxiv_query("what is it"), "");
        for w in ARXIV_FILLERS {
            assert_eq!(*w, w.to_lowercase(), "the list is compared lower-case");
        }
    }

    /// An answer is reused for twelve hours and then asked again; the oldest goes when full.
    #[test]
    fn an_answer_is_kept_twelve_hours_and_the_oldest_goes_first() {
        let t0 = Instant::now();
        let mut p = Polite::default();
        p.keep("u".into(), "body".into(), t0);
        assert_eq!(p.fresh("u", t0 + Duration::from_secs(60)), Some("body".into()));
        assert_eq!(p.fresh("u", t0 + KEEP), None, "stale after twelve hours");
        for i in 0..KEPT_MAX {
            p.keep(format!("k{i}"), String::new(), t0 + Duration::from_secs(i as u64 + 1));
        }
        assert!(p.kept.len() <= KEPT_MAX);
        assert!(p.fresh("u", t0 + Duration::from_secs(1)).is_none(), "the oldest went first");
        assert!(p.fresh(&format!("k{}", KEPT_MAX - 1), t0 + Duration::from_secs(1)).is_some());
    }

    /// A source that said 429 or 503 rests for its Retry-After (at most an hour), others do not.
    #[test]
    fn a_busy_source_rests_for_its_retry_after() {
        assert_eq!(retry_after(Some("120")), Duration::from_secs(120));
        assert_eq!(retry_after(Some(" 7200 ")), Duration::from_secs(3600), "capped at an hour");
        assert_eq!(retry_after(Some("Wed, 21 Oct 2026 07:28:00 GMT")), Duration::from_secs(60));
        assert_eq!(retry_after(None), Duration::from_secs(60));
        let t0 = Instant::now();
        let mut p = Polite::default();
        p.rest(host("https://en.wikipedia.org/w/api.php?x=1"), t0 + Duration::from_secs(120));
        assert!(p.resting("en.wikipedia.org", t0 + Duration::from_secs(119)));
        assert!(!p.resting("en.wikipedia.org", t0 + Duration::from_secs(120)));
        assert!(!p.resting("api.github.com", t0));
        assert_eq!(host("https://export.arxiv.org/api/query?search_query=x"), "export.arxiv.org");
    }

    /// The desktop proxy filters the same words. It is Python, so the check reads its source.
    #[test]
    fn the_desktop_proxy_leaves_out_the_same_fillers() {
        let py = include_str!("../../../agents/search-proxy.py");
        let list = py
            .split("ARXIV_FILLERS = frozenset(")
            .nth(1)
            .and_then(|rest| rest.split(".split()").next())
            .expect("the proxy defines ARXIV_FILLERS");
        let mut theirs: Vec<&str> =
            list.split('"').skip(1).step_by(2).flat_map(str::split_whitespace).collect();
        let mut ours = ARXIV_FILLERS.to_vec();
        theirs.sort_unstable();
        ours.sort_unstable();
        assert_eq!(theirs, ours);
    }

    /// D2: the client says who it is and where to reach the project, and never poses as a browser
    /// (Wikimedia's User-Agent policy).
    #[test]
    fn the_user_agent_names_the_project_and_is_not_a_browser() {
        assert!(UA.starts_with("formicaria-myrme/"), "{UA}");
        assert!(UA.contains("(+https://github.com/formicaria-org/myrme)"), "{UA}");
        assert!(!UA.contains("Mozilla"), "{UA}");
    }

    #[test]
    fn a_question_s_language_and_keywords_are_read_by_code() {
        assert_eq!(language("à quoi sert la vitamine K"), "fr");
        assert_eq!(language("Wie äußert sich Eisenmangel?"), "de");
        assert_eq!(language("sintomi della carenza di vitamina D"), "it");
        assert_eq!(language("维生素K有什么作用"), "zh");
        assert_eq!(language("what does vitamin K do"), "en");
        assert_eq!(language("studies on vitamin K and bone density"), "en");
        assert_eq!(language("la vitamine D et le soleil"), "fr");
        assert_eq!(language("die Rolle von Vitamin D"), "de");
        assert_eq!(keywords("what does vitamin K do", "en"), "vitamin K");
        assert_eq!(keywords("维生素K有什么作用", "zh"), "维生素K有作用");
    }

    /// R1: of a search's titles, the one naming what was asked is read, not the first.
    #[test]
    fn the_article_naming_the_question_is_read_first() {
        let titles = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(
            best_titles(
                "vitamine K",
                titles(&["Antagoniste de la vitamine K", "Vitamine K", "Vitamine B"])
            )[0],
            "Vitamine K"
        );
        assert_eq!(
            best_titles("Eisenmangel", titles(&["Eisen", "Eisenmangel", "Anämie"]))[0],
            "Eisenmangel"
        );
        assert_eq!(
            best_titles("维生素K作用", titles(&["维生素", "维生素K", "消化作用"]))[0],
            "维生素K"
        );
        // Nothing shared: the search's own order stands.
        assert_eq!(
            best_titles("aqueduct", titles(&["Roman aqueduct", "Aqua Appia"]))[0],
            "Roman aqueduct"
        );
    }

    #[test]
    fn a_hit_leaves_only_from_the_list_and_named_by_its_body() {
        let hit =
            |url: &str| SearchHit { title: "Vitamin K".into(), url: url.into(), text: "t".into() };
        assert_eq!(
            labelled(hit("https://en.wikipedia.org/wiki/Vitamin_K"), Need::Health).unwrap().title,
            "Wikipedia (encyclopedia, not medical guidance): Vitamin K"
        );
        assert_eq!(
            labelled(hit("https://en.wikipedia.org/wiki/Vitamin_K"), Need::General).unwrap().title,
            "Wikipedia: Vitamin K"
        );
        assert_eq!(
            labelled(
                hit("https://www.nhs.uk/conditions/vitamins-and-minerals/vitamin-k/"),
                Need::Health
            )
            .unwrap()
            .title,
            "NHS (UK): Vitamin K"
        );
        assert!(labelled(hit("https://health-blog.example/vitamin-k"), Need::Health).is_none());
    }

    /// **Live**: real questions through the real sources, printed — what the research doc's numbers
    /// come from. `pixi run cargo test -p fm-agent-run --features download --lib -- --ignored
    /// --nocapture live_sources`.
    #[test]
    #[ignore = "reaches the network"]
    fn live_sources() {
        let questions = std::env::var("LIVE_QUESTIONS").unwrap_or_else(|_| {
            [
                "what does vitamin K do",
                "à quoi sert la vitamine K",
                "Wie äußert sich Eisenmangel?",
                "sintomi della carenza di vitamina D",
                "维生素K有什么作用",
                "which foods are rich in iron",
                "combien de vitamine K dans les épinards",
                "studies on vitamin K and bone density",
                "what does 10.1038/nature14539 show",
                "how do I fix a borrow checker error in a Rust closure",
                "who built the Analytical Engine",
            ]
            .join("|")
        });
        let search = DirectSearch::new();
        for q in questions.split('|') {
            let t = Instant::now();
            let hits = search.search_asked(q, q).unwrap_or_default();
            println!("\n== {q}  [{:?}, {:.1}s]", Need::of(q), t.elapsed().as_secs_f32());
            for h in hits {
                let text: String = h.text.chars().take(110).collect();
                println!("  - {} <{}>\n      {}", h.title, h.url, text.replace('\n', " / "));
            }
        }
    }
}
