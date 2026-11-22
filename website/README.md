# OpenShell website

The landing page of OpenShell – a single static `index.html` (no build step, no dependencies besides the Geist font
from Google Fonts) plus `assets/`.

The background is a dithered pixel sky drawn on a low-resolution canvas: value-noise clouds, a 4×4 Bayer dither and a
palette that moves from day over dusk to a starry night as you scroll. It renders at ~15 fps (a few ms per frame) and
stays static with `prefers-reduced-motion`.

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
