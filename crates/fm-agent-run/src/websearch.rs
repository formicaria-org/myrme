//! **In-process web search for platforms with no local SearXNG proxy — the phone.**
//!
//! The desktop's [`fm_agent::search::SearxngSearch`] speaks plain HTTP to `agents/search-proxy.py`,
//! a localhost helper that does the real HTTPS. A phone can run neither a Python proxy nor a local
//! SearXNG, so this is the "through the shell's HTTPS, a later step" the mobile launcher deferred:
//! the same job done **in-process** via `ureq`/rustls (the TLS stack the model-fetcher already uses).
//!
//! **Same safety posture as the desktop proxy, by construction:** text-only (titles + snippets), it
//! **never fetches an arbitrary page** — only the fixed source APIs below — so there is no
//! page-download and no SSRF surface. The orchestrator runs it; the model never does.
//!
//! **Sources: Wikipedia (encyclopedia) · arXiv (papers) · GitHub (code)** — three stable, keyless
//! APIs, which is exactly the grounding a *study/research* assistant wants, and avoids the brittle
//! HTML scraping the general-web engine needs. Each source is **fail-soft**: one erroring or empty
//! never sinks the others (the same rule `search-proxy.py` holds). **DuckDuckGo (general web) is
//! deliberately omitted** — it needs fragile HTML scraping and actively blocks scrapers; adding it is
//! a later step (see `known-issues.md`).
//!
//! Honest privacy note, same as the proxy's: your notes never leave the device, but the *query*
//! reaches these public APIs.

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
}

impl Default for DirectSearch {
    fn default() -> Self {
        Self::new()
    }
}

impl WebSearch for DirectSearch {
    fn search(&self, query: &str) -> Result<Vec<SearchHit>, AgentError> {
        let agent = ureq::builder().timeout(self.timeout).build();
        let q = enc(query);
        let n = self.per_source;
        let get = |url: String, accept: Option<&str>| -> Option<String> {
            let now = Instant::now();
            if let Some(body) = polite().fresh(&url, now) {
                return Some(body);
            }
            if polite().resting(host(&url), now) {
                return None;
            }
            let mut r = agent.get(&url).set("User-Agent", UA);
            if let Some(a) = accept {
                r = r.set("Accept", a);
            }
            // Fail-soft: a source that errors contributes nothing (matches search-proxy.py).
            match r.call() {
                Ok(resp) => {
                    let body = resp.into_string().ok()?;
                    polite().keep(url, body.clone(), now);
                    Some(body)
                }
                Err(ureq::Error::Status(429 | 503, resp)) => {
                    polite().rest(host(&url), now + retry_after(resp.header("retry-after")));
                    None
                }
                Err(_) => None,
            }
        };

        let wiki = get(
            format!("https://en.wikipedia.org/w/api.php?action=query&list=search&srsearch={q}&format=json&srlimit={n}"),
            None,
        )
        .map(|b| parse_wikipedia(&b, n))
        .unwrap_or_default();

        let github = get(
            format!("https://api.github.com/search/repositories?q={q}&per_page={n}"),
            Some("application/vnd.github+json"),
        )
        .map(|b| parse_github(&b, n))
        .unwrap_or_default();

        let terms = arxiv_query(query);
        let arxiv = (!terms.is_empty())
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
            .unwrap_or_default();

        // Round-robin interleave (Wikipedia, GitHub, arXiv order), so the top-k spans sources; dedupe
        // by URL. Empty is a legitimate answer (no network / all sources quiet) — the caller grounds in
        // the note instead, and the deterministic quote-verify drops anything unsupported.
        Ok(interleave_dedup(&[wiki, github, arxiv], self.max_results))
    }
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
struct Polite {
    kept: HashMap<String, (Instant, String)>,
    resting: HashMap<String, Instant>,
}

/// How long an answer is reused: MedlinePlus asks for 12–24 h; search results change slowly.
const KEEP: Duration = Duration::from_secs(12 * 60 * 60);
/// How many answers are kept; the oldest goes first.
const KEPT_MAX: usize = 256;

impl Polite {
    fn fresh(&self, url: &str, now: Instant) -> Option<String> {
        self.kept
            .get(url)
            .filter(|(at, _)| now.saturating_duration_since(*at) < KEEP)
            .map(|(_, body)| body.clone())
    }

    fn keep(&mut self, url: String, body: String, now: Instant) {
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

    fn rest(&mut self, host: &str, until: Instant) {
        self.resting.insert(host.to_string(), until);
    }

    fn resting(&self, host: &str, now: Instant) -> bool {
        self.resting.get(host).is_some_and(|until| now < *until)
    }
}

fn polite() -> std::sync::MutexGuard<'static, Polite> {
    static POLITE: OnceLock<Mutex<Polite>> = OnceLock::new();
    // A poisoned lock still holds a usable cache; losing it is worse than a stale entry.
    POLITE.get_or_init(Default::default).lock().unwrap_or_else(|e| e.into_inner())
}

/// How long to leave a source alone after it said 429 or 503: its `Retry-After` in seconds, at most an
/// hour; a minute when it gives none, or gives a date.
fn retry_after(header: Option<&str>) -> Duration {
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
}
