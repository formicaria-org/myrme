#!/usr/bin/env python3
"""A tiny local WEB-SEARCH PROXY for the study agent.

The Rust agent speaks plain localhost HTTP (no TLS, minimal deps); real search engines are HTTPS.
This proxy bridges the gap: Python's stdlib does the HTTPS/TLS, and it answers on localhost in the
**SearXNG JSON shape** (`{"results":[{"title","url","content"}]}`), so the agent's existing
`SearxngSearch` client works against it unchanged. No API key, no pip installs — stdlib only.

It is an out-of-band helper (lives in agents/, never shipped in the app), and text-only: it returns
result titles + snippets, never downloads page binaries.

  python3 agents/search-proxy.py [port]      # default 8888
  GET /search?q=<query>&format=json  ->  {"results":[...]}

Source: DuckDuckGo's HTML endpoint (keyless). Sponsored/ad links are dropped.
"""
import html as H
import json
import re
import sys
import urllib.parse
import urllib.request
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

UA = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 Chrome/120 Safari/537.36"


def _text(s: str) -> str:
    return H.unescape(re.sub(r"<[^>]+>", "", s)).strip()


def _real_url(href: str) -> str:
    # DuckDuckGo wraps results as //duckduckgo.com/l/?uddg=<encoded-real-url>.
    m = re.search(r"uddg=([^&]+)", href)
    return urllib.parse.unquote(m.group(1)) if m else H.unescape(href)


def search(query: str, k: int = 6):
    data = urllib.parse.urlencode({"q": query}).encode()
    req = urllib.request.Request(
        "https://html.duckduckgo.com/html/", data=data, headers={"User-Agent": UA}
    )
    html = urllib.request.urlopen(req, timeout=15).read().decode("utf-8", "replace")
    # Per result: a result__a anchor (title + href), then a result__snippet anchor.
    rows = re.findall(
        r'<a[^>]*class="result__a"[^>]*href="([^"]+)".*?>(.*?)</a>.*?class="result__snippet"[^>]*>(.*?)</a>',
        html,
        re.S,
    )
    out = []
    for href, title, snippet in rows:
        url = _real_url(href)
        if "y.js" in url or "ad_domain" in url:  # drop sponsored
            continue
        out.append({"title": _text(title), "url": url, "content": _text(snippet)})
        if len(out) >= k:
            break
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
        query = urllib.parse.parse_qs(parsed.query).get("q", [""])[0]
        try:
            self._json({"results": search(query) if query.strip() else []})
        except Exception as e:  # never crash the loop; the agent degrades to no web
            self._json({"results": [], "error": str(e)})


if __name__ == "__main__":
    port = int(sys.argv[1]) if len(sys.argv) > 1 else 8888
    print(f"web-search proxy on 127.0.0.1:{port} (DuckDuckGo, SearXNG JSON shape)")
    ThreadingHTTPServer(("127.0.0.1", port), Handler).serve_forever()
