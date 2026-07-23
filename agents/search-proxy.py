#!/usr/bin/env python3
"""A tiny local MULTI-SOURCE WEB-SEARCH PROXY for the study agent.

The Rust agent speaks plain localhost HTTP (no TLS, minimal deps); real sources are HTTPS. This proxy
bridges the gap: Python's stdlib does the HTTPS/TLS, and it answers on localhost in the **SearXNG JSON
shape** (`{"results":[{"title","url","content","engine"}]}`), so the agent's existing `SearxngSearch`
client works against it unchanged — including its `?engines=` selection. No API key, no pip installs —
stdlib only.

It fans out across the agent's DEFAULT sources so each is genuinely accessible and searchable, and
NORMALIZES their different formats into one uniform shape the model can synthesize from:
  - wikipedia   → the MediaWiki search API (clean title + snippet)
  - duckduckgo  → DuckDuckGo's keyless HTML endpoint (the general web)
  - github      → the GitHub repository search API (repo full-name + description)
  - arxiv       → the arXiv Atom API (paper title + abstract)
Results are interleaved round-robin so the top-k spans every requested source, deduped by URL. A source
that errors or returns nothing simply contributes nothing — one bad source never sinks the response.

It is an out-of-band helper (lives in agents/, never shipped in the app), and text-only: it returns
titles + snippets/abstracts, never downloads page binaries.

  python3 agents/search-proxy.py [port]                       # default 8888
  GET /search?q=<query>[&engines=wikipedia,arxiv]&format=json -> {"results":[...]}
"""
import html as H
import json
import re
import sys
import urllib.parse
import urllib.request
import xml.etree.ElementTree as ET
from concurrent.futures import ThreadPoolExecutor
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

UA = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 Chrome/120 Safari/537.36"
# Kept in sync BY HAND with `DEFAULT_ENGINES` in crates/fm-agent/src/search.rs (and the `ENGINES` map
# below). This proxy implements exactly these four; an unknown engine name is dropped. Change one place,
# change the others.
DEFAULT_ENGINES = ["wikipedia", "duckduckgo", "github", "arxiv"]
TIMEOUT = 12


def _fetch(url, data=None, headers=None):
    req = urllib.request.Request(url, data=data, headers=headers or {"User-Agent": UA})
    return urllib.request.urlopen(req, timeout=TIMEOUT).read()


def _text(s: str) -> str:
    return re.sub(r"\s+", " ", H.unescape(re.sub(r"<[^>]+>", "", s))).strip()


# --- per-source engines: each returns a list of {title,url,content,engine}, normalizing its format ---

def eng_duckduckgo(query, k):
    data = urllib.parse.urlencode({"q": query}).encode()
    html = _fetch("https://html.duckduckgo.com/html/", data=data).decode("utf-8", "replace")
    rows = re.findall(
        r'<a[^>]*class="result__a"[^>]*href="([^"]+)".*?>(.*?)</a>.*?class="result__snippet"[^>]*>(.*?)</a>',
        html,
        re.S,
    )
    out = []
    for href, title, snippet in rows:
        m = re.search(r"uddg=([^&]+)", href)
        url = urllib.parse.unquote(m.group(1)) if m else H.unescape(href)
        if "y.js" in url or "ad_domain" in url:  # drop sponsored
            continue
        out.append({"title": _text(title), "url": url, "content": _text(snippet), "engine": "duckduckgo"})
        if len(out) >= k:
            break
    return out


def eng_wikipedia(query, k):
    api = "https://en.wikipedia.org/w/api.php?" + urllib.parse.urlencode(
        {"action": "query", "list": "search", "srsearch": query, "format": "json", "srlimit": k}
    )
    d = json.loads(_fetch(api))
    out = []
    for r in d.get("query", {}).get("search", []):
        title = r["title"]
        out.append(
            {
                "title": title,
                "url": "https://en.wikipedia.org/wiki/" + urllib.parse.quote(title.replace(" ", "_")),
                "content": _text(r.get("snippet", "")),
                "engine": "wikipedia",
            }
        )
    return out


def eng_arxiv(query, k):
    api = "http://export.arxiv.org/api/query?" + urllib.parse.urlencode(
        {"search_query": "all:" + query, "max_results": k}
    )
    root = ET.fromstring(_fetch(api))
    ns = {"a": "http://www.w3.org/2005/Atom"}
    out = []
    for e in root.findall("a:entry", ns):
        title = _text(e.findtext("a:title", default="", namespaces=ns) or "")
        summary = _text(e.findtext("a:summary", default="", namespaces=ns) or "")
        url = (e.findtext("a:id", default="", namespaces=ns) or "").strip()
        if url:
            out.append({"title": title, "url": url, "content": summary, "engine": "arxiv"})
    return out


def eng_github(query, k):
    api = "https://api.github.com/search/repositories?" + urllib.parse.urlencode({"q": query, "per_page": k})
    d = json.loads(_fetch(api, headers={"User-Agent": UA, "Accept": "application/vnd.github+json"}))
    out = []
    for r in d.get("items", []):
        out.append(
            {
                "title": r.get("full_name", ""),
                "url": r.get("html_url", ""),
                "content": _text(r.get("description") or ""),
                "engine": "github",
            }
        )
    return out


ENGINES = {
    "duckduckgo": eng_duckduckgo,
    "wikipedia": eng_wikipedia,
    "arxiv": eng_arxiv,
    "github": eng_github,
}


def search(query, engines, k=8):
    """Fan out to the requested engines in parallel, interleave round-robin so the top-k spans every
    source, and dedupe by URL. A per-engine budget keeps any single source from crowding out the rest."""
    engines = [e for e in engines if e in ENGINES] or DEFAULT_ENGINES
    per = max(3, (k // len(engines)) + 1)

    def run(name):
        try:
            return name, ENGINES[name](query, per)
        except Exception:  # one source failing must never sink the others
            return name, []

    with ThreadPoolExecutor(max_workers=len(engines)) as pool:
        per_engine = dict(pool.map(run, engines))

    # Round-robin interleave, preserving the caller's engine order, so the first results span sources.
    ordered = []
    i = 0
    while any(i < len(per_engine[e]) for e in engines):
        for e in engines:
            if i < len(per_engine[e]):
                ordered.append(per_engine[e][i])
        i += 1

    seen, out = set(), []
    for r in ordered:
        if not r["url"] or r["url"] in seen:
            continue
        seen.add(r["url"])
        out.append(r)
    return out


class Handler(BaseHTTPRequestHandler):
    def log_message(self, *args):
        pass  # quiet

    def _json(self, obj):
        body = json.dumps(obj).encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def do_GET(self):
        parsed = urllib.parse.urlparse(self.path)
        if parsed.path != "/search":
            self.send_response(404)
            self.end_headers()
            return
        qs = urllib.parse.parse_qs(parsed.query)
        query = qs.get("q", [""])[0]
        engines = [e for e in (qs.get("engines", [""])[0]).split(",") if e] or DEFAULT_ENGINES
        try:
            self._json({"results": search(query, engines) if query.strip() else []})
        except Exception as e:  # never crash the loop; the agent degrades to no web
            self._json({"results": [], "error": str(e)})


if __name__ == "__main__":
    port = int(sys.argv[1]) if len(sys.argv) > 1 else 8888
    print(f"multi-source web-search proxy on 127.0.0.1:{port} (wikipedia+duckduckgo+github+arxiv, SearXNG JSON shape)")
    ThreadingHTTPServer(("127.0.0.1", port), Handler).serve_forever()
