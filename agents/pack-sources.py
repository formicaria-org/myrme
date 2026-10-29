#!/usr/bin/env python3
"""Build the sources pack: `crates/fm-agent-run/src/sources.tsv`.

The topic pages of the public health bodies on Myrme's list (`sources.rs::LISTED`), one line a page:
language, URL, title. `/research` searches it on the device and opens the best pages through the
guarded fetch, so a health question reaches the WHO, the NHS or the German health ministry with no
search engine in between (`docs/research/2026-10-29-reputable-sources.md`).

Each body's own topic index or sitemap names its pages; **every page is then fetched once**, with
Myrme's User-Agent and as the app would (no redirect off the list, HTML only, robots.txt honoured), and
kept only if it loads. Its title is the page's own `<h1>`. So the pack holds only pages known to be
readable on the day it was built, and the diff of `sources.tsv` is what a reviewer reads.

Run: `pixi run pack-sources` (about an hour the first time — one request a second per host, and BLV
alone has some 1,200 pages; answers are cached in `~/.cache/myrme-pack-sources`, so a second run
takes a minute).
"""

import concurrent.futures
import hashlib
import html
import os
import re
import sys
import time
import urllib.parse
import urllib.request
import urllib.robotparser
from pathlib import Path

UA = "formicaria-myrme/pack (+https://github.com/formicaria-org/myrme)"
TOKEN = "formicaria-myrme"
OUT = Path(__file__).resolve().parent.parent / "crates/fm-agent-run/src/sources.tsv"
CACHE = Path(os.environ.get("XDG_CACHE_HOME", Path.home() / ".cache")) / "myrme-pack-sources"

# Must match `sources.rs::LISTED` for these bodies (a test there checks every line of the pack).
HOSTS = {
    "www.who.int", "www.nhs.uk", "www.efsa.europa.eu", "gesund.bund.de", "www.bfr.bund.de",
    "www.dge.de", "www.blv.admin.ch", "www.santepubliquefrance.fr", "www.anses.fr",
    "www.epicentro.iss.it",
}

# (language, how, where, which URLs to keep). `links`: the anchors of an index page; `sitemap`: the
# URLs of a sitemap (a sitemap index is followed into the children whose address holds `child`).
SOURCES = [
    ("en", "links", "https://www.who.int/news-room/fact-sheets", r"^/news-room/fact-sheets/detail/"),
    ("fr", "links", "https://www.who.int/fr/news-room/fact-sheets", r"^/fr/news-room/fact-sheets/detail/"),
    ("zh", "links", "https://www.who.int/zh/news-room/fact-sheets", r"^/zh/news-room/fact-sheets/detail/"),
    ("en", "links", "https://www.nhs.uk/conditions/", r"^/conditions/[a-z0-9-]+/$"),
    ("en", "links", "https://www.nhs.uk/conditions/vitamins-and-minerals/", r"^/conditions/vitamins-and-minerals/[a-z0-9-]+/$"),
    ("en", "links", "https://www.efsa.europa.eu/en/topics", r"^/en/topics/topic/"),
    ("de", "sitemap", "https://gesund.bund.de/sitemap.xml", r"^https://gesund\.bund\.de/[a-z0-9-]+$", "articles"),
    ("de", "links", "https://www.bfr.bund.de/themen-a-z/", r"^/(lebensmittel|verbraucherprodukte|chemikalien|risikokommunikation)[a-z0-9-]*/.+/$"),
    ("de", "sitemap", "https://www.dge.de/sitemap.xml", r"^https://www\.dge\.de/(gesunde-ernaehrung|wissenschaft/referenzwerte)/.+", "pages"),
    ("de", "sitemap", "https://www.blv.admin.ch/sitemap/de.xml", r"^https://www\.blv\.admin\.ch/de/[a-z0-9-]+$"),
    ("fr", "sitemap", "https://www.blv.admin.ch/sitemap/fr.xml", r"^https://www\.blv\.admin\.ch/fr/[a-z0-9-]+$"),
    ("it", "sitemap", "https://www.blv.admin.ch/sitemap/it.xml", r"^https://www\.blv\.admin\.ch/it/[a-z0-9-]+$"),
    ("fr", "sitemap", "https://www.santepubliquefrance.fr/sitemap.xml", r"^https://www\.santepubliquefrance\.fr/(maladies-et-traumatismes|determinants-de-sante|nutrition-et-activite-physique|[a-z-]+)/[a-z0-9-]+$"),
    ("fr", "sitemap", "https://www.anses.fr/sitemap.xml", r"^https://www\.anses\.fr/fr/(content|article)/[a-z0-9-]+$"),
    ("it", "links", "https://www.epicentro.iss.it/index/MalattieInfettive", r"^/[a-z0-9-]+/$"),
    ("it", "links", "https://www.epicentro.iss.it/index/MalattieCroniche", r"^/[a-z0-9-]+/$"),
    ("it", "links", "https://www.epicentro.iss.it/index/StiliVita", r"^/[a-z0-9-]+/$"),
    ("it", "links", "https://www.epicentro.iss.it/index/SaluteMaternoInfantile", r"^/[a-z0-9-]+/$"),
    ("it", "links", "https://www.epicentro.iss.it/index/SaluteMentale", r"^/[a-z0-9-]+/$"),
    ("it", "links", "https://www.epicentro.iss.it/index/InvecchiareSalute", r"^/[a-z0-9-]+/$"),
    ("it", "links", "https://www.epicentro.iss.it/index/Prevenzione", r"^/[a-z0-9-]+/$"),
]

# Santé publique France's sitemap holds every regional bulletin and press release; only its topic
# pages are wanted.
SKIP = re.compile(
    r"/(regions-et-territoires|docs|les-actualites|presse|surveillance-syndromique-sursaudr|a-propos|"
    r"recherche|mentions-legales|contact|newsletter|chi-siamo|archivio|globale)(/|$)"
)
# A page whose own title says it is a list, a news item or a form is not a topic page.
NOT_TOPIC = re.compile(r"^(page not found|404|seite nicht gefunden|accueil|home|startseite)\b", re.I)

MAX_PER_SOURCE = 700

_last: dict[str, float] = {}
_robots: dict[str, urllib.robotparser.RobotFileParser] = {}


def get(url: str) -> tuple[int, str, str]:
    """(status, final URL, body), cached on disk; one request a second per host."""
    key = CACHE / hashlib.sha256(url.encode()).hexdigest()
    if key.exists():
        status, final, body = key.read_text(errors="replace").split("\n", 2)
        return int(status), final, body
    host = urllib.parse.urlsplit(url).hostname or ""
    wait = _last.get(host, 0) + 1.0 - time.monotonic()
    if wait > 0:
        time.sleep(wait)
    _last[host] = time.monotonic()
    req = urllib.request.Request(url, headers={"User-Agent": UA, "Accept": "text/html,application/xml"})
    try:
        with urllib.request.urlopen(req, timeout=30) as r:
            ctype = r.headers.get("content-type", "")
            body = r.read(4_000_000).decode("utf-8", "replace")
            status, final = r.status, r.geturl()
            if not ("html" in ctype or "xml" in ctype):
                status, body = 415, ""
    except urllib.error.HTTPError as e:
        status, final, body = e.code, url, ""
    except Exception:  # noqa: BLE001 - a page that does not load is simply left out
        status, final, body = 0, url, ""
    CACHE.mkdir(parents=True, exist_ok=True)
    key.write_text(f"{status}\n{final}\n{body}")
    return status, final, body


def allowed(url: str) -> bool:
    parts = urllib.parse.urlsplit(url)
    rp = _robots.get(parts.netloc)
    if rp is None:
        rp = urllib.robotparser.RobotFileParser()
        status, _, body = get(f"https://{parts.netloc}/robots.txt")
        rp.parse(body.splitlines() if status == 200 else [])
        _robots[parts.netloc] = rp
    return rp.can_fetch(TOKEN, url)


def text(fragment: str) -> str:
    return " ".join(html.unescape(re.sub(r"<[^>]+>", " ", fragment)).split())


def candidates(source) -> list[tuple[str, str]]:
    """(URL, title from the index or empty) for one source."""
    lang, how, where, keep, *child = source
    status, _, body = get(where)
    if status != 200:
        print(f"  ! {where}: {status}", file=sys.stderr)
        return []
    out: list[tuple[str, str]] = []
    if how == "links":
        for href, inner in re.findall(r'<a\b[^>]*?href="([^"#?]+)"[^>]*>(.*?)</a>', body, re.S):
            absolute = urllib.parse.urljoin(where, href)
            parts = urllib.parse.urlsplit(absolute)
            if parts.netloc != urllib.parse.urlsplit(where).netloc:
                continue
            if re.search(keep, parts.path) and not SKIP.search(parts.path):
                out.append((f"https://{parts.netloc}{parts.path}", text(inner)))
    else:
        locs = [html.unescape(x) for x in re.findall(r"<loc>([^<]+)</loc>", body)]
        if "<sitemapindex" in body:
            kids = [l for l in locs if not child or child[0] in l]
            locs = []
            for k in kids:
                s, _, b = get(k)
                if s == 200:
                    locs += [html.unescape(x) for x in re.findall(r"<loc>([^<]+)</loc>", b)]
        for loc in locs:
            if re.search(keep, loc) and not SKIP.search(urllib.parse.urlsplit(loc).path):
                out.append((loc, ""))
    seen, uniq = set(), []
    for url, title in out:
        if url not in seen:
            seen.add(url)
            uniq.append((url, title))
    return uniq[:MAX_PER_SOURCE]


def page(url: str) -> tuple[str, str] | None:
    """(final URL, the page's own title) when it loads as the app would read it; else None."""
    if not allowed(url):
        return None
    status, final, body = get(url)
    if status != 200 or urllib.parse.urlsplit(final).netloc not in HOSTS or not final.startswith("https://"):
        return None
    h1 = re.search(r"<h1\b[^>]*>(.*?)</h1>", body, re.S)
    title = text(h1.group(1)) if h1 else ""
    if not title:
        t = re.search(r"<title\b[^>]*>(.*?)</title>", body, re.S)
        title = re.split(r"\s+[|–-]\s+", text(t.group(1)) if t else "")[0]
    if not title or NOT_TOPIC.search(title) or len(title) > 160:
        return None
    return final, title


def build_one(source) -> list[tuple[str, str, str]]:
    lang = source[0]
    rows = []
    for url, _ in candidates(source):
        got = page(url)
        if got:
            rows.append((lang, got[0], got[1].replace("\t", " ")))
    print(f"  {source[2]}: {len(rows)} pages", file=sys.stderr)
    return rows


def main() -> None:
    by_host: dict[str, list] = {}
    for s in SOURCES:
        by_host.setdefault(urllib.parse.urlsplit(s[2]).netloc, []).append(s)
    rows: dict[str, tuple[str, str, str]] = {}
    # Hosts in parallel, each host's pages one at a time.
    with concurrent.futures.ThreadPoolExecutor(max_workers=len(by_host)) as pool:
        for got in pool.map(lambda ss: [r for s in ss for r in build_one(s)], by_host.values()):
            for lang, url, title in got:
                rows.setdefault(url, (lang, url, title))
    lines = sorted(rows.values(), key=lambda r: (r[0], r[1]))
    head = [
        "# The sources pack: topic pages of the public bodies on Myrme's list, one a line —",
        "# language, URL, title. Built by agents/pack-sources.py; every page was fetched and read.",
        f"# {len(lines)} pages, built {time.strftime('%Y-%m-%d')}.",
    ]
    OUT.write_text("\n".join(head + ["\t".join(r) for r in lines]) + "\n")
    counts: dict[str, int] = {}
    for _, url, _ in lines:
        h = urllib.parse.urlsplit(url).netloc
        counts[h] = counts.get(h, 0) + 1
    print(f"wrote {OUT} — {len(lines)} pages: {counts}", file=sys.stderr)


if __name__ == "__main__":
    main()
