# OpenShell website

The landing page of OpenShell – a single static `index.html` (no build step, no dependencies besides Geist and Geist
Mono from Google Fonts) plus `assets/`.

The background is an ASCII field drawn on a canvas: characters on a monospace grid whose density follows drifting
value noise with contour bands, coloured from deep green over mint to white. It is strongest behind the hero, calms
down behind the content, moves with a slight parallax and renders at ~10 fps; with `prefers-reduced-motion` it stays
static. Buttons follow the "Minimal Design System" (glass buttons, landing CTA). Product mockups (app window, console, terminal, agent) and the architecture diagram are plain HTML/inline SVG.

## Preview

```bash
cd website
python -m http.server 4173
# open http://127.0.0.1:4173
```

## Publish

Any static host works – upload the `website/` folder:

- **GitHub Pages**: a branch deployment can only serve `/` or `/docs`, so deploy with a workflow
  (`actions/upload-pages-artifact` with `path: website`). GitHub Pages for private repositories needs a paid plan.
- **Cloudflare Pages / Netlify / Vercel**: connect the repository, build command empty, output directory `website`.

Download links point to `https://github.com/RobinsSuperSchmiede/EDA/releases/latest`; they work for visitors once
the repository is public.
