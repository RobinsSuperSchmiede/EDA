// OpenShell landing page – shared behaviour of index.html (prod) and dev.html.
// Reads its settings from data attributes on <html> (see site.css) and reacts when the dev bar
// changes them, so prod and dev always run the same code.
(() => {
  const root = document.documentElement;
  const setting = (name, fallback) => root.getAttribute(`data-${name}`) || fallback;
  const reducedMotion = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
  const motionOn = () => !reducedMotion && setting("motion", "on") === "on";

  // ------------------------------------------------------------------ header: hide on scroll down
  const bar = document.getElementById("top-bar");
  let lastY = window.scrollY;
  window.addEventListener(
    "scroll",
    () => {
      const y = window.scrollY;
      bar.classList.toggle("scrolled", y > 20);
      if (y > 120 && y > lastY + 4) bar.classList.add("hidden");
      else if (y < lastY - 4 || y < 120) bar.classList.remove("hidden");
      lastY = y;
    },
    { passive: true },
  );

  // ------------------------------------------------------------------ hero variant: where the app window lives
  const stage = document.getElementById("stage");
  function placeStage() {
    const slot = document.querySelector(setting("hero", "wordmark") === "stage" ? "#hero .stage-slot" : "#how .stage-slot");
    if (slot && stage.parentElement !== slot) slot.append(stage);
  }

  // ------------------------------------------------------------------ features as a spec sheet
  const FEATURES = [
    ["Runtime", ["Start order and health checks", "Restarts with backoff", "Logs from every service", "CPU and memory per service", "Builds for C++, Java, Go, Rust", "Hot reload on save"]],
    ["Desktop", ["Native Tauri 2 window", "Service menu in the title bar", "Tray and background mode", "Your own title bar", "Notifications", "Launch at login"]],
    ["Platforms", ["Windows, macOS, Linux", "Headless servers", "Android runtime and CLI", "services.platforms", "Pause in the background", "Remote gateway with pairing", "Android app|in progress", "Device mesh|planned"]],
    ["Building blocks", ["Frontend in any framework", "Backend in any language", "Built-in SQLite", "Script functions", "Console for apps without UI", "openshell add / create --with"]],
    ["Agents", ["MCP over stdio and HTTP", "App and developer profiles", "OpenAPI import", "Semantic UI automation", "MCP resources and live updates", "Screenshot fallback|planned"]],
    ["Security", ["Permission scopes per mode", "Human approvals", "Audit log", "Project-scoped files", "Per-database agent access", "Agent sessions with own tokens|planned"]],
    ["Shipping", ["NSIS, MSI, DMG, DEB, AppImage", "Your name and icon", "Per-user data", "Installed apps speak MCP", "Code signing|planned", "Auto-updates|planned"]],
  ];
  const spec = document.getElementById("feature-grid");
  for (const [title, items] of FEATURES) {
    const row = document.createElement("div");
    row.className = "spec-row reveal";
    const h = document.createElement("h3");
    h.textContent = title;
    const count = document.createElement("small");
    count.textContent = `${items.length} items`;
    h.append(count);
    const ul = document.createElement("ul");
    for (const raw of items) {
      const [text, later] = raw.split("|");
      const li = document.createElement("li");
      li.textContent = text;
      if (later) {
        const s = document.createElement("span");
        s.className = "later";
        s.textContent = later;
        li.append(s);
      }
      ul.append(li);
    }
    row.append(h, ul);
    spec.append(row);
  }

  // ------------------------------------------------------------------ block composer
  const NOTES = {
    frontend: "A desktop UI – static HTML or any framework",
    backend: "A supervised service that runs in the tray or headless",
    database: "A local database with a browser in the console",
    functions: "Tools for AI agents only – no UI, no server",
    "frontend,database": "A desktop UI that keeps its data – no backend",
    "database,functions": "An AI interface over your data – no UI, no server",
    "frontend,backend": "Your web app as a desktop app",
    "backend,database": "An API with built-in storage",
    "frontend,functions": "A UI plus tools agents can call",
    "backend,functions": "A service plus script tools for agents",
  };
  const picker = document.getElementById("picker");
  function compose() {
    const on = [...picker.querySelectorAll('[aria-pressed="true"]')].map((b) => b.dataset.block);
    const key = on.join(",");
    document.getElementById("cmd").textContent = on.length ? `openshell create my-app --with ${key}` : "openshell create my-app --with …";
    document.getElementById("cmd-note").textContent = !on.length
      ? "Pick at least one block"
      : NOTES[key] || (on.length === 4 ? "Everything: UI, API, data and agent tools" : "Any combination works – blocks connect themselves");
  }
  picker.addEventListener("click", (e) => {
    const b = e.target.closest(".block");
    if (!b) return;
    b.setAttribute("aria-pressed", b.getAttribute("aria-pressed") === "true" ? "false" : "true");
    compose();
  });
  compose();

  // ------------------------------------------------------------------ gallery tabs
  document.querySelectorAll(".tab").forEach((tab) =>
    tab.addEventListener("click", () => {
      document.querySelectorAll(".tab").forEach((t) => t.setAttribute("aria-selected", String(t === tab)));
      document.querySelectorAll(".shot").forEach((s) => s.classList.toggle("on", s.dataset.shot === tab.dataset.shot));
    }),
  );

  // ------------------------------------------------------------------ copy quick start
  document.getElementById("copy").addEventListener("click", async (e) => {
    const text = document
      .getElementById("quickstart")
      .innerText.split("\n")
      .map((l) => l.replace(/^\$\s*/, "").replace(/\s+#.*$/, ""))
      .filter((l) => l.trim() && !l.trim().startsWith("#"))
      .join("\n");
    try {
      await navigator.clipboard.writeText(text);
      e.target.textContent = "Copied";
    } catch {
      e.target.textContent = "Select and copy";
    }
    setTimeout(() => (e.target.textContent = "Copy"), 1500);
  });

  // ------------------------------------------------------------------ entrance on scroll
  const io = new IntersectionObserver(
    (entries) => entries.forEach((en) => en.isIntersecting && (en.target.classList.add("in"), io.unobserve(en.target))),
    { rootMargin: "0px 0px -8% 0px" },
  );
  document.querySelectorAll(".reveal").forEach((el) => io.observe(el));

  // ------------------------------------------------------------------ ASCII field
  // Code glyphs on a grid; their density follows slowly drifting value noise with contour
  // bands. Colours run from the page colour over the theme accent to white. The field lives in
  // the hero and the final section (or the whole page) and stays calm behind the headline.
  const canvas = document.getElementById("field");
  const ctx = canvas.getContext("2d");
  const RAMP = " .:-=/<{#";
  const PERM = new Uint8Array(512);
  (() => {
    const p = [...Array(256).keys()];
    let s = 20241122;
    for (let i = 255; i > 0; i--) {
      s = (s * 48271) % 2147483647;
      const j = s % (i + 1);
      [p[i], p[j]] = [p[j], p[i]];
    }
    for (let i = 0; i < 512; i++) PERM[i] = p[i & 255];
  })();
  const fade = (t) => t * t * (3 - 2 * t);
  function noise(x, y) {
    const xi = Math.floor(x), yi = Math.floor(y), xf = x - xi, yf = y - yi;
    const h = (a, b) => PERM[(PERM[a & 255] + b) & 255] / 255;
    const u = fade(xf), v = fade(yf);
    const a = h(xi, yi), b = h(xi + 1, yi), c = h(xi, yi + 1), d = h(xi + 1, yi + 1);
    return a + (b - a) * u + (c - a) * v + (a - b - c + d) * u * v;
  }
  const fbm = (x, y) => 0.6 * noise(x, y) + 0.28 * noise(x * 2.1, y * 2.1) + 0.12 * noise(x * 4.3, y * 4.3);

  function parseColor(str) {
    str = str.trim();
    if (str.startsWith("#")) {
      const h = str.length === 4 ? [...str.slice(1)].map((c) => c + c).join("") : str.slice(1);
      return [0, 2, 4].map((i) => parseInt(h.slice(i, i + 2), 16));
    }
    const m = str.match(/[\d.]+/g) || ["0", "0", "0"];
    return m.slice(0, 3).map(Number);
  }
  const mix = (a, b, t) => a.map((v, i) => Math.round(v + (b[i] - v) * t));
  const css = (c) => `rgb(${c[0]},${c[1]},${c[2]})`;

  let colors = [];
  let bgColor = "#000";
  function readPalette() {
    const style = getComputedStyle(root);
    const bg = parseColor(style.getPropertyValue("--bg"));
    const accent = parseColor(style.getPropertyValue("--accent"));
    const white = [246, 250, 248];
    bgColor = css(bg);
    colors = [0.16, 0.28, 0.42, 0.58, 0.78].map((t) => css(mix(bg, accent, t)));
    colors.push(css(mix(accent, white, 0.45)), css(mix(accent, white, 0.85)));
  }

  let cols = 0, rows = 0, cw = 0, ch = 0;
  let calmRects = []; // text areas (document coordinates) where the field stays quiet
  function measure() {
    const dpr = Math.min(window.devicePixelRatio || 1, 2);
    const size = window.innerWidth < 700 ? 12 : 14;
    cw = size * 0.62;
    ch = size * 1.05;
    cols = Math.ceil(window.innerWidth / cw);
    rows = Math.ceil(window.innerHeight / ch);
    canvas.width = Math.floor(window.innerWidth * dpr);
    canvas.height = Math.floor(window.innerHeight * dpr);
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.font = `${size}px "Geist Mono", ui-monospace, monospace`;
    ctx.textBaseline = "top";
    calmRects = [...document.querySelectorAll("#hero .copy-block, #download > div")].map((el) => {
      const r = el.getBoundingClientRect();
      return { left: r.left, right: r.right, top: r.top + window.scrollY, bottom: r.bottom + window.scrollY };
    });
  }

  // vertical bands (document coordinates) in which the field is drawn
  function regions() {
    const mode = setting("field", "hero");
    if (mode === "off") return [];
    if (mode === "page") return [[0, document.documentElement.scrollHeight, 0.55]];
    const out = [];
    for (const id of ["hero", "download"]) {
      const el = document.getElementById(id);
      if (!el) continue;
      const top = el.offsetTop;
      out.push([top, top + el.offsetHeight, 1]);
    }
    return out;
  }

  const buckets = [[], [], [], [], [], [], []];
  function render(time) {
    const t = motionOn() ? time / 1000 : 0;
    const calmOn = setting("calm", "on") !== "off";
    const intensity = Math.max(0.2, Math.min(1.6, parseFloat(setting("intensity", "1")) || 1));
    const scrollY = window.scrollY;
    const viewTop = scrollY;
    const viewBottom = scrollY + window.innerHeight;
    const bands = regions().filter(([a, b]) => b > viewTop && a < viewBottom);
    ctx.fillStyle = bgColor;
    ctx.fillRect(0, 0, window.innerWidth, window.innerHeight);
    if (!bands.length) return;
    buckets.forEach((b) => (b.length = 0));
    const FADE = 160;
    for (let y = 0; y < rows; y++) {
      const docY = scrollY + y * ch;
      let band = 0;
      for (const [a, b, strength] of bands) {
        if (docY >= a && docY <= b) {
          const edge = Math.min(docY - a, b - docY);
          band = Math.max(band, strength * Math.min(1, edge / FADE + (a === 0 ? 1 : 0)));
        }
      }
      if (band <= 0) continue;
      const gy = docY / ch * 0.075;
      for (let x = 0; x < cols; x++) {
        const px = x * cw;
        // quiet space behind headlines so the text never fights the field
        let calm = 1;
        for (const r of calmOn ? calmRects : []) {
          const dx = Math.max(r.left - 24 - px, 0, px - r.right - 24);
          const dy = Math.max(r.top - 24 - docY, 0, docY - r.bottom - 16);
          calm = Math.min(calm, 0.06 + 0.94 * Math.min(1, Math.hypot(dx, dy) / 120));
        }
        const n = fbm(x * 0.045 + t * 0.04, gy + t * 0.02);
        const contour = Math.abs(Math.sin(n * 18 + t * 0.6));
        let v = ((n - 0.25) * 1.3 + (1 - contour) * 0.55) * band * calm * intensity;
        if (v <= 0.08) continue;
        v = Math.min(0.999, v);
        const ci = Math.min(6, Math.floor(v * 7));
        const chr = RAMP[Math.min(RAMP.length - 1, 1 + Math.floor(v * (RAMP.length - 1)))];
        buckets[ci].push(px, y * ch, chr);
      }
    }
    for (let i = 0; i < 7; i++) {
      const b = buckets[i];
      if (!b.length) continue;
      ctx.fillStyle = colors[i];
      for (let k = 0; k < b.length; k += 3) ctx.fillText(b[k + 2], b[k], b[k + 1]);
    }
  }

  let last = 0;
  function frame(now) {
    requestAnimationFrame(frame);
    if (!motionOn() || now - last < 100) return;
    last = now;
    render(now);
  }
  const redraw = () => render(performance.now());
  window.addEventListener("resize", () => (measure(), redraw()));
  window.addEventListener("scroll", () => motionOn() || redraw(), { passive: true });

  // ------------------------------------------------------------------ apply settings (also when the dev bar changes them)
  function apply() {
    placeStage();
    readPalette();
    measure();
    redraw();
  }
  new MutationObserver(apply).observe(root, { attributes: true, attributeFilter: ["data-theme", "data-surface", "data-hero", "data-field", "data-intensity", "data-motion", "data-calm"] });
  window.OpenShellSite = { apply };
  apply();
  document.fonts.ready.then(apply);
  requestAnimationFrame(frame);
})();
