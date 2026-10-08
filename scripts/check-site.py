#!/usr/bin/env python3
"""Offline validation of the Jelly GitHub Pages artifact.

Validate every page's local links and fragments, metadata, sitemap, navigation,
documentation cross-links, published tool names, and source-aligned security
claims. Does not connect to external services or start Chromium/Jelly.
"""
from __future__ import annotations

from collections import Counter
from html.parser import HTMLParser
from pathlib import Path
from urllib.parse import unquote, urlsplit
import re
import json
import html
import tomllib
import xml.etree.ElementTree as ET

ROOT = Path(__file__).resolve().parent.parent
SITE = ROOT / "site"
BASE = "https://fabioflorey.github.io/jelly/"
PREFIX = "/jelly/"
GIT = "https://github.com/FabioFlorey/jelly/"
INDEXABLE = {
    "index.html", "getting-started.html", "clients.html", "tools.html",
    "examples.html", "troubleshooting.html"
}
EXPECTED_BROWSER_TOOLS = {"browser-schema", "browser-call", "browser-events"}


class Page(HTMLParser):
    def __init__(self) -> None:
        super().__init__(convert_charrefs=True)
        self.ids: set[str] = set()
        self.refs: list[str] = []
        self.links: list[str] = []
        self.meta: list[dict[str, str | None]] = []
        self.headings: Counter[str] = Counter()
        self.titles: list[str] = []
        self.tags: Counter[str] = Counter()
        self._title = False
        self._title_text: list[str] = []
        self.skip = False
        self.nav = False
        self.tool_records: set[str] = set()

    def handle_starttag(self, tag: str, attrs: list[tuple[str, str | None]]) -> None:
        data = dict(attrs)
        self.tags[tag] += 1
        if data.get("id"):
            key = data["id"]
            assert key not in self.ids, f"Duplicate HTML ID: {key}"
            self.ids.add(key)
            if tag == "section" and "tool-record" in (data.get("class") or ""):
                self.tool_records.add(key)
        for name in ("href", "src", "poster"):
            if data.get(name):
                self.refs.append(data[name] or "")
        if tag == "a":
            if data.get("href"):
                self.links.append(data["href"] or "")
            if data.get("class") == "skip-link" and data.get("href") == "#main":
                self.skip = True
        if tag == "nav" and data.get("aria-label"):
            self.nav = True
        if tag in ("h1", "h2", "h3"):
            self.headings[tag] += 1
        if tag == "title":
            self._title = True
            self._title_text = []
        if tag == "meta":
            self.meta.append(data)
            if data.get("property") in {"og:image", "og:image:secure_url"}:
                self.refs.append(data.get("content") or "")
            if data.get("name") == "twitter:image":
                self.refs.append(data.get("content") or "")

    def handle_data(self, data: str) -> None:
        if self._title:
            self._title_text.append(data)

    def handle_endtag(self, tag: str) -> None:
        if tag == "title" and self._title:
            self.titles.append("".join(self._title_text).strip())
            self._title = False

    def metadata(self, key: str, value: str) -> list[str]:
        return [a.get("content", "") or "" for a in self.meta if a.get(key) == value]


def site_path(ref: str, page: str) -> tuple[Path | None, str]:
    uri = urlsplit(ref)
    if uri.scheme in {"mailto", "tel", "data", "javascript"}:
        return None, uri.fragment
    if uri.scheme or uri.netloc:
        if ref.startswith(GIT):
            relative_git = ref[len(GIT):]
            if relative_git.startswith(("blob/main/", "tree/main/")):
                relative = relative_git.split("/main/", 1)[1].split("#", 1)[0]
                return (ROOT / unquote(relative)).resolve(), ""
        if not ref.startswith(BASE):
            return None, uri.fragment
        relative = uri.path.removeprefix(PREFIX)
    else:
        relative = uri.path
        if relative.startswith("/"):
            assert relative.startswith(PREFIX), f"{page}: URL outside project prefix: {ref}"
            relative = relative.removeprefix(PREFIX)
    if not relative or relative in {".", "./"}:
        relative = page if uri.fragment and not uri.path else "index.html"
    if relative.endswith("/"):
        relative += "index.html"
    path = (SITE / unquote(relative)).resolve()
    assert path == SITE.resolve() or SITE.resolve() in path.parents, f"{page}: path escape: {ref}"
    return path, uri.fragment


def main() -> None:
    assert (SITE / ".nojekyll").exists()
    assert not (ROOT / "docs/index.html").exists()
    assert (ROOT / "docs/INDEX.md").is_file()
    assert (ROOT / "docs/reference/GLOSSARY.md").is_file()
    assert not (ROOT / "docs/wiki").exists()
    actual = {p.name for p in SITE.glob("*.html") if p.name != "404.html"}
    assert actual == INDEXABLE, f"Unexpected/missing indexable pages: {INDEXABLE ^ actual}"
    pages: dict[str, Page] = {}
    titles: set[str] = set()
    descriptions: set[str] = set()
    canonicals: set[str] = set()

    for name in sorted(INDEXABLE | {"404.html"}):
        content = (SITE / name).read_text(encoding="utf-8")
        parser = Page()
        parser.feed(content)
        pages[name] = parser
        assert "\u2014" not in content, f"{name}: em dash violates Jelly writing style"
        if name != "404.html":
            assert "Jelly · Browser instrumentation for agents" in content, f"{name}: footer subtitle drift"
        assert parser.headings["h1"] == 1, f"{name}: exactly one H1 required"
        assert len(parser.titles) == 1, f"{name}: missing/duplicate HTML title"
        assert parser.nav, f"{name}: missing labeled primary navigation"
        assert parser.skip and "main" in parser.ids, f"{name}: missing keyboard skip link"
        assert parser.metadata("name", "description"), f"{name}: missing page description"
        assert 'href="./assets/css/style.css"' in content or name == "404.html"
        assert "<style>" not in content, f"{name}: inline CSS"
        if name == "404.html":
            assert "noindex" in parser.metadata("name", "robots")
            continue
        title = parser.titles[0]
        desc = parser.metadata("name", "description")[0]
        assert title and title not in titles, f"{name}: title duplicate"
        assert desc and desc not in descriptions, f"{name}: description duplicate"
        titles.add(title)
        descriptions.add(desc)
        assert "index,follow" in parser.metadata("name", "robots")
        canonical = [x for x in re.findall(r'<link rel="canonical" href="([^"]+)"', content)]
        expected = BASE + ("" if name == "index.html" else name)
        assert canonical == [expected], f"{name}: incorrect canonical"
        canonicals.add(expected)
        for ref in parser.refs:
            target, _ = site_path(ref, name)
            if target is not None:
                assert target.is_file(), f"{name}: missing asset or page for {ref}"
        for href in parser.links:
            target, fragment = site_path(href, name)
            if fragment:
                if not href.split("#",1)[0] and fragment not in parser.ids:
                    raise AssertionError(f"{name}: missing local anchor {href}")
                if target is not None and target.is_file() and target.suffix == ".html":
                    other = name if target.name == name else target.name
                    if other in pages and fragment not in pages[other].ids:
                        raise AssertionError(f"{name}: missing target anchor {href}")
        assert "Jelly" in title

    home = (SITE / "index.html").read_text(encoding="utf-8")
    assert '<p class="hero-subtitle">' in home, "Homepage subtitle missing"
    assert "Browser instrumentation for agents" in home
    # Preserve the original Jelly homepage visuals during future content edits.
    # The WebGL script expects the hero ID to match this element exactly.
    assert 'class="hero" id="hero"' in home, "Missing animated hero container"
    assert 'class="hero-liquid" id="heroLiquid"' in home, "Missing hero WebGL canvas"
    assert 'getElementById("hero")' in (SITE / "assets/js/main.js").read_text(), "Hero animation is disconnected"
    assert home.count('class="quickstart-stage"') == 1, "Missing illustrated quickstart"
    assert 'class="quickstart-chan"' in home, "Missing Jelly-chan quickstart illustration"
    assert 'src="./assets/images/jelly-chan.png"' in home, "Wrong Jelly-chan image asset"
    assert home.index('id="quickstart"') > home.index('id="integrations"'), "Quickstart illustration must appear near the page footer"
    assert '.quickstart-chan' in (SITE / "assets/css/style.css").read_text(), "Missing Jelly-chan positioning styles"
    assert home.index('id="hero"') < home.index('id="possibilities"') < home.index('id="capabilities"'), "Customer introduction should lead into capabilities"
    assert 'Put your AI to work on the real web.' in home, "Missing use-case headline"
    assert home.split('<section id="possibilities">', 1)[1].split('</section>', 1)[0].count('class="possibility"') == 6, "Expected six concrete use cases"
    capabilities = home.split('<section id="capabilities">', 1)[1].split('</section>', 1)[0]
    assert capabilities.count('class="card"') == 6, "Homepage capabilities should have six cards"
    assert 'href="./tools.html#hitl"' in capabilities, "Human-in-the-loop capability missing"
    recording = home.split('<section id="demo">', 1)[1].split('</section>', 1)[0]
    assert 'Work around the click!' in recording, "Demo chapter heading missing"
    assert 'Every step is captioned in the video.' in recording, "Recording needs a plain-language explanation"
    assert 'autoplay' in recording and 'loop' in recording and 'muted' in recording, "Demo must autoplay muted and loop"
    assert 'src="./assets/images/jelly-demo.mp4"' in recording, "Demo MP4 missing"
    assert 'poster="./assets/images/jelly-demo-poster.png"' in recording, "Demo poster missing"

    # Full cross-page anchor check after parsing all pages.
    for name, doc in pages.items():
        for ref in doc.links:
            target, fragment = site_path(ref, name)
            if target is not None:
                assert target.is_file(), f"{name}: broken link {ref}"
                if fragment and target.name in pages and target.suffix == ".html":
                    assert fragment in pages[target.name].ids, f"{name}: broken anchor {ref}"

    xml = ET.parse(SITE / "sitemap.xml")
    ns = "{http://www.sitemaps.org/schemas/sitemap/0.9}"
    locs = {el.text for el in xml.iter(ns + "loc")}
    assert locs == canonicals, f"Sitemap vs canonicals mismatch: {locs ^ canonicals}"
    assert f"Sitemap: {BASE}sitemap.xml" in (SITE / "robots.txt").read_text()
    ET.parse(SITE / "favicon.svg")
    assert (SITE / "assets/fonts/.gitkeep").is_file()
    assert (SITE / "assets/js/main.js").stat().st_size > 0
    assert (SITE / "assets/css/style.css").stat().st_size > 0
    assert re.search(r'<script\s+defer\s+src="\./assets/js/main\.js">\s*</script>',
                     (SITE / "index.html").read_text()), "Missing deferred main script"
    assert "prefers-reduced-motion" in (SITE / "assets/css/style.css").read_text()
    assert "a:not(:last-child) { display: none; }" not in (SITE / "assets/css/style.css").read_text()

    # Any JSON-like code blocks published as examples must remain valid JSON.
    checked_examples = 0
    for name in ("examples.html", "tools.html"):
        content = (SITE / name).read_text(encoding="utf-8")
        for raw in re.findall(r"<pre><code>(.*?)</code></pre>", content, re.S):
            value = html.unescape(raw).strip()
            if value.startswith(("{", "[")):
                json.loads(value)
                checked_examples += 1
    assert checked_examples >= 16, f"Lost published JSON examples: {checked_examples}"

    # Source-aligned small-surface inventory: the sole source of system names.
    spec = (ROOT / "src/execution/tools.rs").read_text()
    system_names = set(re.findall(r'^\s*name:\s*"([a-z-]+)",', spec.split('pub static TOOLS: &[ToolSpec] = &[', 1)[1], re.M))
    # Rust registry has 13 published system tool names and separately declared categories.
    assert len(system_names) == 13, f"Unexpected system tool count: {sorted(system_names)}"
    assert pages["tools.html"].tool_records == system_names | EXPECTED_BROWSER_TOOLS, (
        f"Tool page inventory drift: {pages['tools.html'].tool_records ^ (system_names | EXPECTED_BROWSER_TOOLS)}"
    )
    # Large-surface entries come from the Rust registry, not a maintained list.
    registry = (ROOT / "src/execution/registry.rs").read_text()
    primitive_body = registry.split("primitives! {", 1)[1].split("\n}", 1)[0]
    primitive_names = set(re.findall(r'^\s*"([a-z-]+)"\s*=>', primitive_body, re.M))
    large_section = (SITE / "tools.html").read_text().split('id="large-surface"', 1)[1]
    documented_large = set(re.findall(r'<tr>\s*<td>\s*<code>([a-z-]+)</code>', large_section))
    assert primitive_names == documented_large, (
        f"Large-surface primitive documentation drift: {primitive_names ^ documented_large}"
    )
    assert '<code>cdp-call</code>' in large_section, "Privileged cdp-call reference missing"
    config = tomllib.loads((ROOT / "config/jelly.toml").read_text())
    assert config["mcp"]["surface"] == "small-surface", "Update site default-surface claims"
    raw_enabled = str(config["mcp"]["raw_cdp"]).lower()
    assert (ROOT / "SECURITY.md").is_file(), "Security policy is maintained in the repository"
    assert not (SITE / "security.html").exists(), "Duplicate public security page"
    assert "raw_cdp = " + raw_enabled in (ROOT / "SECURITY.md").read_text(), "Security policy configuration drift"

    print(f"PASS: {len(INDEXABLE)} indexable pages, 404, canonical URLs, sitemap, links, anchors, metadata, "
          f"{len(system_names | EXPECTED_BROWSER_TOOLS)} small-surface and "
          f"{len(primitive_names)} large-surface browser tools, and source-aligned security statements.")


if __name__ == "__main__":
    main()
