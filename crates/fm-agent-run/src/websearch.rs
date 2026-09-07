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
use std::time::Duration;

const UA: &str = "Mozilla/5.0 (compatible; formicaria-study-agent)";

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
            let mut r = agent.get(&url).set("User-Agent", UA);
            if let Some(a) = accept {
                r = r.set("Accept", a);
            }
            // Fail-soft: a source that errors contributes nothing (matches search-proxy.py).
            r.call().ok()?.into_string().ok()
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

        let arxiv = get(
            format!("http://export.arxiv.org/api/query?search_query=all:{q}&max_results={n}"),
            None,
        )
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
}
