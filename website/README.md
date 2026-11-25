# OpenShell website

The website of OpenShell: a landing page, pages for each audience, a download page and the documentation – static
files, no dependencies besides Geist and Geist Mono from Google Fonts. The only build step is
`python scripts/build-website.py` (standard library only), which renders the Markdown docs and keeps the shared header
and footer in sync.

| File | Purpose |
|---|---|
| `index.html` | **Landing page.** The design is fixed by the data attributes on `<html>` |
| `use.html` | For **users**: install an app on every system, where data lives, assistants, safety |
| `teams.html` | For **teams and companies**: rollout, deployment, security controls, fit, maturity, licence |
| `developers.html` | For **developers**: quick start, config and app contract, languages, CLI, SDK, shipping |
| `agents.html` | For **AI agents**: profiles, client setup, tools by scope, capabilities, control |
| `download.html` | Download OpenShell itself (detects the visitor's system), install scripts, build from source |
| `docs/` | **Generated** from `docs/*.md`, `ROADMAP.md`, `CHANGELOG.md` … – do not edit by hand |
| `dev.html` | **Development view**: the landing page in a frame plus a customize bar |
| `assets/site.css`, `assets/site.js` | Styles and behaviour of the landing page and the sub-page heroes (ASCII field, header) |
| `assets/pages.css`, `assets/pages.js` | Navigation, sub pages, download cards, docs layout, search, copy buttons, tab sets |
| `assets/search-index.js` | **Generated** search index over the docs and the pages |
| `assets/dev.js` | The customize bar (only loaded by `dev.html`) |

## Pages and documentation

Hand-written pages share one header and one footer. They sit between the markers
`<!-- chrome:header -->` … `<!-- /chrome:header -->` and `<!-- chrome:footer -->` … `<!-- /chrome:footer -->`; set
`<body data-page="…">` (`use`, `teams`, `developers`, `agents`, `download`, `docs`) to mark the current page in the
navigation. Edit the templates in `scripts/build-website.py`, not the pages. Plain `<div class="codeblock"
data-lang="json">` blocks are coloured by the script (JSON, shell, TOML).

```bash
python scripts/build-website.py          # render docs, sync header and footer, rebuild the search index
python scripts/build-website.py --check  # exit 1 if generated files are out of date
```

Run it after changing `docs/`, `ROADMAP.md`, `CHANGELOG.md`, `SECURITY.md`, `CONTRIBUTING.md` or `ARCHITECTURE.md`. To
add a document to the site, add a row to `DOCS` in the script. Links between documents are rewritten to the site's
pages; links to other repository files point to GitHub. The sub pages use `data-calm="on"` so the ASCII field stays
quiet behind their headlines.

## Design settings

Every setting is a data attribute on `<html>` in `index.html`:

| Attribute | Values | Effect |
|---|---|---|
| `data-theme` | `green`, `sky`, `red`, `violet`, `amber`, `mono` | Accent, page tint and ASCII palette |
| `data-surface` | `tinted`, `ink` | Page tinted by the theme or neutral ink |
| `data-header` | `fade`, `pills`, `center`, `blur`, `dock`, `corner` | Floating header without a bar: content fades out under it. Brand and button side by side, in their own pills, in one centered pill, with a blur, as a dock at the bottom, or brand top left and button bottom right |
| `data-hero` | `wordmark`, `stage` | Big `openshell` wordmark in the hero, or the app window next to the headline (the wordmark moves to the end) |
| `data-field` | `hero`, `page`, `off` | Where the ASCII field is drawn |
| `data-calm` | `off`, `on` | `off`: the ASCII field runs behind the hero and final text; `on`: it stays quiet there |
| `data-intensity` | `0.4` – `1.4` | Strength of the ASCII field |
| `data-motion` | `on`, `off` | Animations (always off with `prefers-reduced-motion`) |

Workflow: open `dev.html`, try settings in the bar (they are remembered in the browser), then copy the
"Production line" from the bar and replace line 3 of `index.html` with it.

The background is an ASCII field drawn on a canvas: code glyphs (`. : - = / < { #`) whose density follows drifting
value noise with contour bands, coloured from the page colour over the theme accent to white. It runs behind the
headlines (or stays quiet there with `data-calm="on"`), renders at ~10 fps only while visible and is static without motion. Buttons follow the "Minimal Design
System" (glass `.btn`, landing `.cta`, segmented tabs).

## Preview

```bash
cd website
python -m http.server 4173
# prod: http://127.0.0.1:4173   dev: http://127.0.0.1:4173/dev.html
```

`dev.html` needs to be served over HTTP (not opened as a file) so the bar can reach the framed page.

## Publish

Any static host works – upload the `website/` folder (`dev.html` and `assets/dev.js` can be left out). Run
`python scripts/build-website.py` first so the generated docs are current:

- **GitHub Pages**: a branch deployment can only serve `/` or `/docs`, so deploy with a workflow
  (`actions/upload-pages-artifact` with `path: website`). GitHub Pages for private repositories needs a paid plan.
- **Cloudflare Pages / Netlify / Vercel**: connect the repository, build command empty, output directory `website`.

Download links point to `https://github.com/RobinsSuperSchmiede/EDA/releases/latest`; they work for visitors once
the repository is public.
