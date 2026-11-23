# OpenShell website

The landing page of OpenShell – static files, no build step, no dependencies besides Geist and Geist Mono from Google
Fonts.

| File | Purpose |
|---|---|
| `index.html` | **Production page.** The design is fixed by the data attributes on `<html>` |
| `dev.html` | **Development view**: the production page in a frame plus a customize bar |
| `assets/site.css`, `assets/site.js` | Styles and behaviour shared by both – prod and dev never drift apart |
| `assets/dev.js` | The customize bar (only loaded by `dev.html`) |

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

Any static host works – upload the `website/` folder (`dev.html` and `assets/dev.js` can be left out):

- **GitHub Pages**: a branch deployment can only serve `/` or `/docs`, so deploy with a workflow
  (`actions/upload-pages-artifact` with `path: website`). GitHub Pages for private repositories needs a paid plan.
- **Cloudflare Pages / Netlify / Vercel**: connect the repository, build command empty, output directory `website`.

Download links point to `https://github.com/RobinsSuperSchmiede/EDA/releases/latest`; they work for visitors once
the repository is public.
