#!/usr/bin/env python3
"""Builds the generated parts of the website (website/).

The site is plain static HTML. This script only does two chores so that nothing drifts:

1. Renders the Markdown documentation (docs/*.md, ROADMAP.md, CHANGELOG.md, ...) into website/docs/*.html,
   writes the docs hub (website/docs/index.html) and the search index (website/assets/search-index.js).
2. Rewrites the shared header and footer of every page between the markers
   `<!-- chrome:header -->...<!-- /chrome:header -->` and `<!-- chrome:footer -->...<!-- /chrome:footer -->`,
   marking the current page in the navigation (`<body data-page="...">`).

Run it after changing docs or the shared header/footer:

    python scripts/build-website.py          # write
    python scripts/build-website.py --check  # exit 1 if anything would change (CI)

No dependencies besides the Python standard library.
"""
from __future__ import annotations

import html
import json
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SITE = ROOT / "website"
REPO = "https://github.com/RobinsSuperSchmiede/EDA"
BLOB = f"{REPO}/blob/main"
TREE = f"{REPO}/tree/main"


def read_version() -> str:
    m = re.search(r'^version\s*=\s*"([^"]+)"', (ROOT / "Cargo.toml").read_text(encoding="utf-8"), re.M)
    return m.group(1) if m else "0"


VERSION = read_version()

# ---------------------------------------------------------------------------------------------- documents
# (group, slug, source file, navigation label, one-line description for the hub)
DOCS = [
    ("Learn", "getting-started", "docs/getting-started.md", "Getting started", "Install OpenShell, create a first app or bring an existing project."),
    ("Learn", "tutorial", "docs/tutorial.md", "Tutorial", "Build a complete app – frontend, backend, agent functions and an installer – step by step."),
    ("Learn", "building-blocks", "docs/building-blocks.md", "Building blocks", "Frontend, backend, database, functions: use only what you need."),
    ("Learn", "languages", "docs/languages.md", "Languages", "Python, Node.js, Java, C++, Go, Rust and .NET backends."),
    ("Reference", "configuration", "docs/configuration.md", "Configuration", "Every field of openshell.json."),
    ("Reference", "cli", "docs/cli.md", "CLI", "All commands of the openshell binary."),
    ("Reference", "sdk", "docs/sdk.md", "Frontend SDK", "Runtime status, window control, notifications, databases and agent actions from the frontend."),
    ("Reference", "control-api", "docs/control-api.md", "Control API", "The local HTTP API behind the CLI, the SDK and MCP."),
    ("Ship and run", "installers", "docs/installers.md", "Installers and releases", "openshell package, app bundles, one download link per system, signing and update checks."),
    ("Ship and run", "platforms", "docs/platforms.md", "Platforms", "Desktop, server, Android and Termux: platform adapters, lifecycle, remote gateway."),
    ("Ship and run", "remote-access", "docs/remote-access.md", "Remote access", "Use an app from other devices: pairing, allowlist, TLS, exposed services."),
    ("Ship and run", "android-app", "docs/android-app.md", "Android app", "The native Android host: how it works, building the AAB, Play Store checklist."),
    ("AI agents", "agents", "docs/agents.md", "Agent Interaction Layer", "MCP profiles, tools, UI automation, capabilities, permissions and approvals."),
    ("AI agents", "ai", "docs/ai/README.md", "AI material", "Setup recipes for Claude Code, Codex, Cursor, Copilot and more."),
    ("AI agents", "ai-app-development", "docs/ai/app-development.md", "Building apps with AI", "The complete guide for coding agents that build apps on OpenShell."),
    ("Project", "architecture", "ARCHITECTURE.md", "Architecture", "Components, data flow and design decisions."),
    ("Project", "platform-milestones", "docs/platform-milestones.md", "Platform milestones", "What is done and what is next on Android and connected devices."),
    ("Project", "roadmap", "ROADMAP.md", "Roadmap", "What is done, in progress and planned."),
    ("Project", "changelog", "CHANGELOG.md", "Changelog", "What changed in every release."),
    ("Project", "security", "SECURITY.md", "Security policy", "Threat model and how to report a vulnerability."),
    ("Project", "contributing", "CONTRIBUTING.md", "Contributing", "Set up a development environment and where things live."),
]
GROUPS = ["Learn", "Reference", "Ship and run", "AI agents", "Project"]
DOC_BY_SOURCE = {src: slug for _g, slug, src, _l, _d in DOCS}

# hand-written pages: key (data-page), file, navigation label
NAV = [
    ("use", "use.html", "Users"),
    ("teams", "teams.html", "Teams"),
    ("developers", "developers.html", "Developers"),
    ("agents", "agents.html", "AI agents"),
    ("docs", "docs/index.html", "Docs"),
]
# extra entries for the search index: (title, file, text)
PAGE_INDEX = [
    ("For users", "use.html", "Install an OpenShell app on Windows, macOS, Linux or Android. Your data stays on your device. Let an AI assistant help, with your approval."),
    ("For teams and companies", "teams.html", "Ship internal tools as signed desktop apps. One download link per system, update checks, audit log, permission scopes, remote access with allowlist and TLS, MIT licence."),
    ("For developers", "developers.html", "Turn a web app into a desktop app. openshell.json, app contract, languages, templates, CLI, SDK, packaging, release workflow."),
    ("For AI agents", "agents.html", "App profile and developer profile over MCP, 32 tools, permission scopes, approvals, UI automation, capabilities, remote gateway, llms.txt."),
    ("Download", "download.html", "Download OpenShell for Windows, macOS, Linux, Android and Termux. Install script, checksums, build from source."),
]


# ---------------------------------------------------------------------------------------------- markdown
class Ctx:
    def __init__(self, source: str, refs: dict[str, str]):
        self.source = source  # repo-relative path of the file being rendered
        self.refs = refs
        self.slugs: dict[str, int] = {}
        self.toc: list[tuple[int, str, str]] = []
        self.sections: list[dict] = []
        self.title = ""


def strip_md(text: str) -> str:
    text = re.sub(r"!\[([^\]]*)\]\([^)]*\)", r"\1", text)
    text = re.sub(r"\[([^\]]+)\]\([^)]*\)", r"\1", text)
    text = re.sub(r"\[([^\]]+)\]\[[^\]]*\]", r"\1", text)
    text = re.sub(r"[`*]", "", text)
    return text.strip()


def slugify(text: str, ctx: Ctx) -> str:
    s = strip_md(text).lower()
    s = re.sub(r"[^\w\- ]", "", s, flags=re.UNICODE)
    s = s.replace(" ", "-")
    n = ctx.slugs.get(s, 0)
    ctx.slugs[s] = n + 1
    return s if n == 0 else f"{s}-{n}"


def resolve_href(href: str, ctx: Ctx) -> str:
    if re.match(r"^(https?:|mailto:)", href):
        return href
    if href.startswith("#"):
        return href
    path, _, anchor = href.partition("#")
    base = Path(ctx.source).parent
    target = (base / path).as_posix()
    parts: list[str] = []
    for p in target.split("/"):
        if p == "..":
            if parts:
                parts.pop()
        elif p and p != ".":
            parts.append(p)
    target = "/".join(parts)
    frag = f"#{anchor}" if anchor else ""
    if target in DOC_BY_SOURCE:
        return f"{DOC_BY_SOURCE[target]}.html{frag}"
    full = ROOT / target
    kind = "tree" if full.is_dir() else "blob"
    return f"{REPO}/{kind}/main/{target}{frag}"


def inline(text: str, ctx: Ctx) -> str:
    codes: list[str] = []

    def stash(m: re.Match) -> str:
        codes.append(m.group(2))
        return f"\x00{len(codes) - 1}\x00"

    text = re.sub(r"(`+)(.+?)\1", stash, text)
    text = html.escape(text, quote=False)

    def link(m: re.Match) -> str:
        label, href = m.group(1), html.unescape(m.group(2))
        url = resolve_href(href, ctx)
        if re.fullmatch(r"[\w./-]+\.md", label):  # a bare file name as link text: show the page's title instead
            slug = url.split("#")[0].removesuffix(".html")
            names = {s: lbl for _g, s, _src, lbl, _d in DOCS}
            label = html.escape(names.get(slug, label), quote=False)
        ext = ' rel="noopener"' if url.startswith("http") else ""
        return f'<a href="{html.escape(url)}"{ext}>{label}</a>'

    text = re.sub(r"\[([^\]]+)\]\(([^)\s]+)(?:\s+&quot;[^)]*&quot;)?\)", link, text)

    def ref(m: re.Match) -> str:
        label, key = m.group(1), (m.group(2) or m.group(1)).lower()
        if key in ctx.refs:
            url = html.escape(ctx.refs[key])
            return f'<a href="{url}" rel="noopener">{label}</a>'
        return m.group(0)

    text = re.sub(r"\[([^\]]+)\](?:\[([^\]]*)\])?(?!\()", ref, text)
    text = re.sub(r"&lt;(https?://[^&\s]+)&gt;", r'<a href="\1" rel="noopener">\1</a>', text)
    text = re.sub(r"\*\*(.+?)\*\*", r"<strong>\1</strong>", text, flags=re.S)
    text = re.sub(r"(?<![\w*])\*(?![\s*])(.+?)(?<![\s*])\*(?![\w*])", r"<em>\1</em>", text, flags=re.S)
    text = re.sub(r" {2,}\n", "<br>\n", text)
    text = re.sub(r"\x00(\d+)\x00", lambda m: f"<code>{html.escape(codes[int(m.group(1))].strip(), quote=False)}</code>", text)
    return text


# light syntax colouring: JSON (with // comments) and shell/TOML comments, nothing heavier
JSON_TOKEN = re.compile(r'(//.*$)|("(?:[^"\\\n]|\\.)*")(\s*:)?|(-?\b\d+(?:\.\d+)?\b)|\b(true|false|null)\b', re.M)
SHELL_COMMENT = re.compile(r"(^|\s)(#.*)$", re.M)


def highlight(code: str, lang: str) -> str:
    lang = lang.lower()
    if lang in ("json", "jsonc", "json5"):
        out, pos = [], 0
        for m in JSON_TOKEN.finditer(code):
            out.append(html.escape(code[pos:m.start()], quote=False))
            if m.group(1):
                out.append(f'<span class="tc">{html.escape(m.group(1), quote=False)}</span>')
            elif m.group(2):
                cls = "tk" if m.group(3) else "ts"
                out.append(f'<span class="{cls}">{html.escape(m.group(2), quote=False)}</span>{html.escape(m.group(3) or "", quote=False)}')
            elif m.group(4):
                out.append(f'<span class="tn">{m.group(4)}</span>')
            else:
                out.append(f'<span class="tn">{m.group(5)}</span>')
            pos = m.end()
        out.append(html.escape(code[pos:], quote=False))
        return "".join(out)
    if lang in ("bash", "sh", "shell", "powershell", "toml", "yaml", "yml", ""):
        out, pos = [], 0
        for m in SHELL_COMMENT.finditer(code):
            out.append(html.escape(code[pos:m.start(2)], quote=False))
            out.append(f'<span class="tc">{html.escape(m.group(2), quote=False)}</span>')
            pos = m.end(2)
        out.append(html.escape(code[pos:], quote=False))
        return "".join(out)
    return html.escape(code, quote=False)


LIST_RE = re.compile(r"^(\s*)([-*+]|\d+[.)])(\s+)(.*)$")
FENCE_RE = re.compile(r"^(\s*)(`{3,}|~{3,})\s*([\w+.-]*)")
HEAD_RE = re.compile(r"^(#{1,6})\s+(.+?)\s*#*\s*$")
HR_RE = re.compile(r"^\s*([-*_])(\s*\1){2,}\s*$")
TABLE_SEP_RE = re.compile(r"^\s*\|?\s*:?-{2,}:?\s*(\|\s*:?-{2,}:?\s*)*\|?\s*$")


def indent_of(line: str) -> int:
    return len(line) - len(line.lstrip(" "))


def starts_block(line: str) -> bool:
    return bool(FENCE_RE.match(line) or HEAD_RE.match(line) or HR_RE.match(line) or LIST_RE.match(line) or line.lstrip().startswith(">"))


def split_row(line: str) -> list[str]:
    line = line.strip()
    if line.startswith("|"):
        line = line[1:]
    if line.endswith("|") and not line.endswith("\\|"):
        line = line[:-1]
    cells = re.split(r"(?<!\\)\|", line)
    return [c.strip().replace("\\|", "|") for c in cells]


def render_blocks(lines: list[str], ctx: Ctx) -> list[str]:
    out: list[str] = []
    i, n = 0, len(lines)

    def add_text(block_html: str) -> None:
        if ctx.sections:
            text = re.sub(r"<[^>]+>", " ", block_html)
            text = html.unescape(re.sub(r"\s+", " ", text)).strip()
            if text:
                ctx.sections[-1]["text"] += " " + text

    while i < n:
        line = lines[i]
        if not line.strip():
            i += 1
            continue

        m = FENCE_RE.match(line)
        if m:
            ind, fence, lang = len(m.group(1)), m.group(2), m.group(3)
            i += 1
            body = []
            while i < n and not lines[i].strip().startswith(fence[0] * len(fence)):
                body.append(lines[i][ind:] if lines[i][:ind].strip() == "" else lines[i])
                i += 1
            i += 1
            code = "\n".join(body)
            label = f' data-lang="{html.escape(lang)}"' if lang else ""
            out.append(f'<div class="codeblock"{label}><pre><code>{highlight(code, lang)}</code></pre></div>')
            add_text(f"<p>{html.escape(code)}</p>")
            continue

        m = HEAD_RE.match(line)
        if m:
            level, raw = len(m.group(1)), m.group(2)
            if level == 1 and not ctx.title:
                ctx.title = strip_md(raw)
            sid = slugify(raw, ctx)
            if level in (2, 3):
                ctx.toc.append((level, strip_md(raw), sid))
                ctx.sections.append({"heading": strip_md(raw), "id": sid, "text": ""})
            out.append(f'<h{level} id="{sid}">{inline(raw, ctx)}' + (f'<a class="anchor" href="#{sid}" aria-label="Link to this section">#</a>' if level in (2, 3) else "") + f"</h{level}>")
            i += 1
            continue

        if HR_RE.match(line):
            out.append("<hr>")
            i += 1
            continue

        if line.lstrip().startswith("|") and i + 1 < n and "-" in lines[i + 1] and TABLE_SEP_RE.match(lines[i + 1]):
            head = split_row(line)
            i += 2
            rows = []
            while i < n and lines[i].strip().startswith("|"):
                rows.append(split_row(lines[i]))
                i += 1
            parts = ['<div class="table-wrap"><table><thead><tr>' + "".join(f"<th>{inline(c, ctx)}</th>" for c in head) + "</tr></thead><tbody>"]
            for r in rows:
                r = (r + [""] * len(head))[: len(head)]
                parts.append("<tr>" + "".join(f"<td>{inline(c, ctx)}</td>" for c in r) + "</tr>")
            parts.append("</tbody></table></div>")
            block = "".join(parts)
            out.append(block)
            add_text(block)
            continue

        if line.lstrip().startswith(">"):
            quote = []
            while i < n and lines[i].lstrip().startswith(">"):
                quote.append(re.sub(r"^\s*>\s?", "", lines[i]))
                i += 1
            block = "<blockquote>" + "\n".join(render_blocks(quote, ctx)) + "</blockquote>"
            out.append(block)
            continue

        m = LIST_RE.match(line)
        if m:
            base = len(m.group(1))
            ordered = m.group(2)[0].isdigit()
            start = int(re.match(r"\d+", m.group(2)).group(0)) if ordered else 1
            items: list[list[str]] = []
            while i < n:
                cur = lines[i]
                if not cur.strip():
                    j = i
                    while j < n and not lines[j].strip():
                        j += 1
                    nm = LIST_RE.match(lines[j]) if j < n else None
                    if nm and len(nm.group(1)) == base and nm.group(2)[0].isdigit() == ordered:
                        i = j
                        continue
                    break
                lm = LIST_RE.match(cur)
                if not (lm and len(lm.group(1)) == base and lm.group(2)[0].isdigit() == ordered):
                    break
                width = base + len(lm.group(2)) + min(len(lm.group(3)), 4)
                item = [lm.group(4)]
                i += 1
                while i < n:
                    nxt = lines[i]
                    if not nxt.strip():
                        j = i
                        while j < n and not lines[j].strip():
                            j += 1
                        if j < n and indent_of(lines[j]) > base:
                            item.append("")
                            i += 1
                            continue
                        break
                    if indent_of(nxt) > base:
                        item.append(nxt[min(indent_of(nxt), width):])
                        i += 1
                        continue
                    if item[-1].strip() and not starts_block(nxt):
                        item.append(nxt.strip())
                        i += 1
                        continue
                    break
                items.append(item)
            tag = "ol" if ordered else "ul"
            attr = f' start="{start}"' if ordered and start != 1 else ""
            parts = [f"<{tag}{attr}>"]
            for item in items:
                inner = render_blocks(item, ctx)
                if inner and inner[0].startswith("<p>"):
                    inner[0] = inner[0][3:].replace("</p>", "", 1)
                parts.append("<li>" + "\n".join(inner) + "</li>")
            parts.append(f"</{tag}>")
            out.append("".join(parts))
            continue

        para = [line.rstrip("\n")]
        i += 1
        while i < n and lines[i].strip() and not starts_block(lines[i]) and not (lines[i].lstrip().startswith("|") and i + 1 < n and TABLE_SEP_RE.match(lines[i + 1])):
            para.append(lines[i])
            i += 1
        block = f"<p>{inline(chr(10).join(para), ctx)}</p>"
        out.append(block)
        add_text(block)

    return out


def render_markdown(source: str) -> Ctx | tuple:
    text = (ROOT / source).read_text(encoding="utf-8")
    refs: dict[str, str] = {}
    kept = []
    for ln in text.splitlines():
        m = re.match(r"^\[([^\]]+)\]:\s+(\S+)\s*$", ln)
        if m:
            refs[m.group(1).lower()] = m.group(2)
        else:
            kept.append(ln)
    ctx = Ctx(source, refs)
    body = "\n".join(render_blocks(kept, ctx))
    return ctx, body


def first_paragraph(body: str) -> str:
    m = re.search(r"<p>(.*?)</p>", body, re.S)
    text = html.unescape(re.sub(r"<[^>]+>", "", m.group(1))) if m else ""
    text = re.sub(r"\s+", " ", text).strip()
    return text[:200].rstrip() + ("…" if len(text) > 200 else "")


# ---------------------------------------------------------------------------------------------- chrome
FONTS = (
    '<link rel="preconnect" href="https://fonts.googleapis.com" />\n'
    '    <link rel="preconnect" href="https://fonts.gstatic.com" crossorigin />\n'
    '    <link href="https://fonts.googleapis.com/css2?family=Geist:wght@400;450;500;600&family=Geist+Mono:wght@400;500&display=swap" rel="stylesheet" />'
)

HEADER = """<!-- chrome:header -->
    <div class="top-fade" aria-hidden="true"></div>
    <header class="top" id="top-bar">
      <div class="top-inner">
        <a class="brand" href="{root}index.html"><img src="{root}assets/logo.png" alt="" />OpenShell</a>
        <nav class="top-nav" id="top-nav" aria-label="Main">
{links}
        </nav>
        <div class="top-end">
          <button class="nav-toggle btn btn--sm" type="button" id="nav-toggle" aria-expanded="false" aria-controls="top-nav">Menu</button>
          <a class="cta cta--sm" href="{root}download.html"><span>Download</span><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M5 12h14M13 6l6 6-6 6" /></svg></a>
        </div>
      </div>
    </header>
    <!-- /chrome:header -->"""

FOOTER = """<!-- chrome:footer -->
    <footer class="site-footer">
      <div class="wrap">
        <div class="foot-grid">
          <div class="foot-brand">
            <a class="brand" href="{root}index.html"><img src="{root}assets/logo.png" alt="" />OpenShell</a>
            <p>Any web app, a native desktop app, agents included. Free and open source under the MIT licence. Alpha, v{version}.</p>
          </div>
          <div>
            <h4>Product</h4>
            <a href="{root}download.html">Download</a>
            <a href="{root}use.html">For users</a>
            <a href="{root}teams.html">For teams</a>
            <a href="{root}developers.html">For developers</a>
            <a href="{root}agents.html">For AI agents</a>
          </div>
          <div>
            <h4>Documentation</h4>
            <a href="{root}docs/getting-started.html">Getting started</a>
            <a href="{root}docs/configuration.html">Configuration</a>
            <a href="{root}docs/cli.html">CLI</a>
            <a href="{root}docs/platforms.html">Platforms</a>
            <a href="{root}docs/remote-access.html">Remote access</a>
            <a href="{root}docs/index.html">All docs</a>
          </div>
          <div>
            <h4>Project</h4>
            <a href="{repo}">GitHub</a>
            <a href="{root}docs/changelog.html">Changelog</a>
            <a href="{root}docs/roadmap.html">Roadmap</a>
            <a href="{root}docs/security.html">Security</a>
            <a href="{root}docs/contributing.html">Contributing</a>
            <a href="{repo}/blob/main/llms.txt">llms.txt</a>
          </div>
        </div>
        <div class="foot-bottom">
          <span>© OpenShell contributors · MIT</span>
          <a class="right" href="#top-bar">Back to top ↑</a>
        </div>
      </div>
    </footer>
    <!-- /chrome:footer -->"""


def chrome_header(root: str, page: str) -> str:
    links = []
    for key, file, label in NAV:
        cur = ' aria-current="page"' if key == page else ""
        links.append(f'          <a href="{root}{file}"{cur}>{label}</a>')
    return HEADER.format(root=root, links="\n".join(links))


def chrome_footer(root: str) -> str:
    return FOOTER.format(root=root, repo=REPO, version=VERSION)


CODEBLOCK_RE = re.compile(r'(<div class="codeblock" data-lang="([\w+-]+)"><pre><code>)(.*?)(</code></pre></div>)', re.S)


def highlight_blocks(text: str) -> str:
    """Colour plain code blocks in hand-written pages (blocks that already contain spans are left alone)."""

    def one(m: re.Match) -> str:
        body = m.group(3)
        if "<span" in body:
            return m.group(0)
        return m.group(1) + highlight(html.unescape(body), m.group(2)) + m.group(4)

    return CODEBLOCK_RE.sub(one, text)


def sync_chrome(text: str, root: str) -> str:
    m = re.search(r'<body[^>]*data-page="([\w-]+)"', text)
    page = m.group(1) if m else ""
    text = re.sub(r"<!-- chrome:header -->.*?<!-- /chrome:header -->", lambda _m: chrome_header(root, page), text, flags=re.S)
    text = re.sub(r"<!-- chrome:footer -->.*?<!-- /chrome:footer -->", lambda _m: chrome_footer(root), text, flags=re.S)
    return highlight_blocks(text)


# ---------------------------------------------------------------------------------------------- docs pages
def head(title: str, description: str, root: str) -> str:
    return f"""<!doctype html>
<html lang="en" data-theme="green" data-surface="tinted" data-header="fade" data-field="off" data-motion="on">
  <head>
    <meta charset="utf-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1" />
    <title>{html.escape(title)}</title>
    <meta name="description" content="{html.escape(description, quote=True)}" />
    <meta name="theme-color" content="#0f3624" />
    <link rel="icon" href="{root}assets/logo.png" />
    {FONTS}
    <link rel="stylesheet" href="{root}assets/site.css" />
    <link rel="stylesheet" href="{root}assets/pages.css" />
  </head>"""


def docs_sidebar(current: str) -> str:
    parts = ['<aside class="docs-nav" id="docs-nav"><div class="docs-search"><input type="search" id="doc-search" placeholder="Search the docs" autocomplete="off" aria-label="Search the docs" /><div class="search-results" id="search-results" hidden></div></div><nav aria-label="Documentation">']
    parts.append(f'<a class="docs-home{" on" if current == "index" else ""}" href="index.html">Overview</a>')
    for group in GROUPS:
        parts.append(f"<h4>{group}</h4>")
        for g, slug, _src, label, _d in DOCS:
            if g == group:
                cur = ' aria-current="page"' if slug == current else ""
                parts.append(f'<a href="{slug}.html"{cur}>{html.escape(label)}</a>')
    parts.append("</nav></aside>")
    return "".join(parts)


def build_doc_page(group: str, slug: str, source: str, label: str, desc: str) -> tuple[str, list[dict]]:
    ctx, body = render_markdown(source)
    title = ctx.title or label
    toc = "".join(f'<a class="l{lvl}" href="#{sid}">{html.escape(t)}</a>' for lvl, t, sid in ctx.toc)
    toc_html = f'<aside class="docs-toc"><h4>On this page</h4>{toc}</aside>' if ctx.toc else '<aside class="docs-toc"></aside>'
    page = f"""{head(f"{title} – OpenShell docs", desc, "../")}
  <body data-page="docs" class="page docs-page">
    {{HEADER}}

    <main class="docs">
      {docs_sidebar(slug)}
      <article class="prose" data-source="{source}">
        <div class="crumbs"><a href="index.html">Docs</a><span>/</span><span>{html.escape(group)}</span></div>
        {body}
        <div class="edit"><a href="{BLOB}/{source}" rel="noopener">View this page on GitHub</a></div>
      </article>
      {toc_html}
    </main>

    {{FOOTER}}

    <script src="../assets/search-index.js"></script>
    <script src="../assets/site.js"></script>
    <script src="../assets/pages.js"></script>
  </body>
</html>
"""
    page = page.replace("{HEADER}", chrome_header("../", "docs")).replace("{FOOTER}", chrome_footer("../"))
    entries = []
    for s in ctx.sections:
        entries.append({"t": title, "h": s["heading"], "u": f"docs/{slug}.html#{s['id']}", "x": s["text"].strip()[:700]})
    intro = first_paragraph(body)
    entries.insert(0, {"t": title, "h": "", "u": f"docs/{slug}.html", "x": intro})
    return page, entries


def build_hub() -> str:
    cards = []
    for group in GROUPS:
        items = []
        for g, slug, _src, label, desc in DOCS:
            if g == group:
                items.append(f'<a class="doc-card" href="{slug}.html"><h3>{html.escape(label)}</h3><p>{html.escape(desc)}</p></a>')
        cards.append(f'<section class="doc-group"><h2>{group}</h2><div class="doc-cards">{"".join(items)}</div></section>')
    roles = """
        <div class="roles">
          <a href="../use.html"><b>I use apps</b><span>Install, data, assistants</span></a>
          <a href="../teams.html"><b>I roll out tools</b><span>Rollout, security, licence</span></a>
          <a href="getting-started.html"><b>I build apps</b><span>Start with the first app</span></a>
          <a href="agents.html"><b>I connect agents</b><span>MCP, tools, permissions</span></a>
        </div>"""
    return f"""{head("OpenShell documentation", "Guides, references and project documents for OpenShell: getting started, configuration, CLI, platforms, remote access, AI agents.", "../")}
  <body data-page="docs" class="page docs-page docs-hub">
    {{HEADER}}

    <main class="hub">
      <div class="wrap">
        <div class="kicker">Documentation</div>
        <h1>Everything about OpenShell</h1>
        <p class="lead">Guides for your first app, references for every setting and command, and the project's own documents. Pick your role or search.</p>
        <div class="hub-search"><input type="search" id="hub-search" placeholder="Search guides, settings, commands" autocomplete="off" aria-label="Search the docs" /><div class="search-results" id="hub-results" hidden></div></div>
        {roles}
        {"".join(cards)}
      </div>
    </main>

    {{FOOTER}}

    <script src="../assets/search-index.js"></script>
    <script src="../assets/site.js"></script>
    <script src="../assets/pages.js"></script>
  </body>
</html>
""".replace("{HEADER}", chrome_header("../", "docs")).replace("{FOOTER}", chrome_footer("../"))


# ---------------------------------------------------------------------------------------------- main
def main() -> int:
    check = "--check" in sys.argv
    outputs: dict[Path, str] = {}
    index: list[dict] = []

    for group, slug, source, label, desc in DOCS:
        page, entries = build_doc_page(group, slug, source, label, desc)
        outputs[SITE / "docs" / f"{slug}.html"] = page
        index.extend(entries)
    outputs[SITE / "docs" / "index.html"] = build_hub()

    for title, file, text in PAGE_INDEX:
        index.append({"t": title, "h": "", "u": file, "x": text})
    outputs[SITE / "assets" / "search-index.js"] = "window.OS_SEARCH = " + json.dumps(index, ensure_ascii=False, separators=(",", ":")) + ";\n"

    # shared header and footer in the hand-written pages
    for path in sorted(SITE.glob("*.html")):
        if path.name == "dev.html":
            continue
        text = path.read_text(encoding="utf-8")
        if "<!-- chrome:header -->" in text:
            outputs[path] = sync_chrome(text, "")

    changed = []
    for path, content in outputs.items():
        old = path.read_text(encoding="utf-8") if path.exists() else None
        if old != content:
            changed.append(path)
            if not check:
                path.parent.mkdir(parents=True, exist_ok=True)
                with open(path, "w", encoding="utf-8", newline="\n") as f:
                    f.write(content)
    if check:
        for p in changed:
            print(f"out of date: {p.relative_to(ROOT)}")
        return 1 if changed else 0
    print(f"website: {len(outputs)} files checked, {len(changed)} written ({len(DOCS)} doc pages, {len(index)} search entries)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
