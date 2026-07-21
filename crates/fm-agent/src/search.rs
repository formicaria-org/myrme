//! A [`WebSearch`](crate::WebSearch) backed by a **private, self-hosted SearXNG** on localhost.
//!
//! Hand-rolled HTTP GET (via [`http`](crate::http)), and **text-only**: it returns SearXNG's result
//! *snippets* and **never fetches an arbitrary page**, so there is no binary-download and no
//! loopback-SSRF surface at all — the two guards the plan calls for hold by construction, because the
//! only URL this ever requests is the one configured SearXNG. The orchestrator runs it; the model
//! never does.
//!
//! Honest privacy note: your notes never leave the device, but the *query* reaches SearXNG's upstream
//! engines. SearXNG anonymizes the source, not the query.

use crate::{http, AgentError, SearchHit, WebSearch};
use std::time::Duration;

/// A single-shot search against a local SearXNG's JSON API (`/search?format=json`).
pub struct SearxngSearch {
    host: String,
    port: u16,
    max_results: usize,
    timeout: Duration,
}

impl SearxngSearch {
    /// A SearXNG on `127.0.0.1:<port>`. Caps results (the orchestrator trims context anyway) and
    /// times out rather than hanging the pipeline.
    pub fn local(port: u16) -> Self {
        Self { host: "127.0.0.1".into(), port, max_results: 6, timeout: Duration::from_secs(30) }
    }

    pub fn with_host(mut self, host: impl Into<String>) -> Self {
        self.host = host.into();
        self
    }

    pub fn with_max_results(mut self, max_results: usize) -> Self {
        self.max_results = max_results;
        self
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }
}

impl WebSearch for SearxngSearch {
    fn search(&self, query: &str) -> Result<Vec<SearchHit>, AgentError> {
        let request = format!(
            "GET /search?q={q}&format=json HTTP/1.1\r\n\
             Host: {host}:{port}\r\n\
             Accept: application/json\r\n\
             Connection: close\r\n\r\n",
            q = http::encode(query),
            host = self.host,
            port = self.port,
        );
        let raw = http::send(&self.host, self.port, request.as_bytes(), self.timeout)?;
        let body = http::body(&raw)?;
        parse_results(&body, self.max_results)
    }
}

/// Turn a SearXNG JSON response into hits, capped at `max`. Pure, so it is tested directly.
fn parse_results(json: &str, max: usize) -> Result<Vec<SearchHit>, AgentError> {
    let v: serde_json::Value = serde_json::from_str(json.trim())
        .map_err(|e| AgentError::new(format!("search response was not JSON: {e}")))?;
    let results = v["results"]
        .as_array()
        .ok_or_else(|| AgentError::new("search response had no results array"))?;
    Ok(results
        .iter()
        .filter_map(|r| {
            Some(SearchHit {
                title: r["title"].as_str()?.to_string(),
                url: r["url"].as_str()?.to_string(),
                // A missing snippet is fine — title + url still locate the source.
                text: r["content"].as_str().unwrap_or("").to_string(),
            })
        })
        .take(max)
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read as _, Write as _};
    use std::net::TcpListener;

    #[test]
    fn it_maps_results_to_hits_and_caps_them() {
        let json = r#"{"results":[
            {"title":"A","url":"https://a","content":"about a"},
            {"title":"B","url":"https://b","content":"about b"},
            {"title":"C","url":"https://c"}
        ]}"#;
        let hits = parse_results(json, 2).unwrap();
        assert_eq!(hits.len(), 2, "capped at max_results");
        assert_eq!(hits[0], SearchHit { title: "A".into(), url: "https://a".into(), text: "about a".into() });
        assert_eq!(hits[1].text, "about b");
    }

    #[test]
    fn a_result_missing_a_url_is_skipped_not_fatal() {
        let json = r#"{"results":[{"title":"no url"},{"title":"ok","url":"https://ok","content":"x"}]}"#;
        let hits = parse_results(json, 6).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].url, "https://ok");
    }

    /// End to end over a real loopback socket, and it must URL-encode the query in the request line.
    #[test]
    fn it_queries_a_local_searxng_over_a_real_socket() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();

        let server = std::thread::spawn(move || {
            let (mut sock, _) = listener.accept().unwrap();
            let mut buf = [0u8; 2048];
            let n = sock.read(&mut buf).unwrap();
            let req = String::from_utf8_lossy(&buf[..n]).to_string();
            let json = r#"{"results":[{"title":"CDC","url":"https://cdc.gov","content":"facts"}]}"#;
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                json.len(),
                json
            );
            sock.write_all(resp.as_bytes()).unwrap();
            req
        });

        let hits = SearxngSearch::local(port).search("mRNA vaccine").unwrap();
        assert_eq!(hits, vec![SearchHit { title: "CDC".into(), url: "https://cdc.gov".into(), text: "facts".into() }]);

        let req = server.join().unwrap();
        assert!(req.starts_with("GET /search?q=mRNA%20vaccine&format=json"), "query not encoded: {req}");
    }
}
