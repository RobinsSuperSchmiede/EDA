// Customize bar of dev.html. Changes the data attributes on the framed production page
// (index.html) – site.js inside the frame reacts to them – and remembers the choice locally.
(() => {
  const THEMES = [
    ["green", "Green", "#5fd896"],
    ["sky", "Light blue", "#6cc4ff"],
    ["red", "Red", "#ff6b66"],
    ["violet", "Purple", "#b39bff"],
    ["amber", "Amber", "#ffc457"],
    ["mono", "Mono", "#e8e8eb"],
  ];
  const PAGE_KEYS = ["theme", "surface", "hero", "field", "intensity", "motion"];
  const STORE = "openshell-dev";

  const frame = document.getElementById("site");
  const viewport = document.getElementById("viewport");
  const devbar = document.getElementById("devbar");
  let prod = {};
  let state = {};
  try {
    state = JSON.parse(localStorage.getItem(STORE) || "{}");
  } catch {
    state = {};
  }

  // theme swatches
  const themes = document.getElementById("themes");
  for (const [value, label, color] of THEMES) {
    const b = document.createElement("button");
    b.type = "button";
    b.className = "swatch";
    b.dataset.value = value;
    b.style.setProperty("--c", color);
    b.innerHTML = `<i></i>${label}`;
    b.addEventListener("click", () => set("theme", value));
    themes.append(b);
  }
  document.querySelectorAll(".seg").forEach((seg) =>
    seg.addEventListener("click", (e) => {
      const b = e.target.closest("button");
      if (b) set(seg.dataset.setting, b.dataset.value);
    }),
  );
  const intensity = document.getElementById("intensity");
  intensity.addEventListener("input", () => set("intensity", Number(intensity.value).toFixed(1).replace(/\.0$/, "")));

  function set(key, value) {
    state[key] = value;
    localStorage.setItem(STORE, JSON.stringify(state));
    apply();
  }

  const current = () => ({ ...prod, size: "desktop", ...state });

  function prodLine(s) {
    const attrs = PAGE_KEYS.map((k) => `data-${k}="${s[k]}"`).join(" ");
    return `<html lang="en" ${attrs}>`;
  }

  function apply() {
    const s = current();
    const doc = frame.contentDocument;
    if (doc && doc.documentElement) {
      for (const k of PAGE_KEYS) {
        if (s[k] !== undefined) doc.documentElement.setAttribute(`data-${k}`, s[k]);
      }
    }
    viewport.dataset.size = s.size;
    themes.querySelectorAll(".swatch").forEach((b) => b.setAttribute("aria-pressed", String(b.dataset.value === s.theme)));
    document.querySelectorAll(".seg").forEach((seg) =>
      seg.querySelectorAll("button").forEach((b) => b.setAttribute("aria-pressed", String(b.dataset.value === s[seg.dataset.setting]))),
    );
    intensity.value = s.intensity;
    document.getElementById("intensity-value").textContent = Number(s.intensity).toFixed(1);
    document.getElementById("prod-line").textContent = prodLine(s);
  }

  // read the production settings every time the page loads, then apply the overrides
  frame.addEventListener("load", () => {
    const el = frame.contentDocument && frame.contentDocument.documentElement;
    if (!el) return;
    prod = Object.fromEntries(PAGE_KEYS.map((k) => [k, el.getAttribute(`data-${k}`)]));
    apply();
    frame.contentDocument.addEventListener("keydown", onKey);
  });

  document.getElementById("copy-prod").addEventListener("click", async (e) => {
    try {
      await navigator.clipboard.writeText(prodLine(current()));
      e.target.textContent = "Copied";
    } catch {
      e.target.textContent = "Select and copy";
    }
    setTimeout(() => (e.target.textContent = "Copy"), 1500);
  });
  document.getElementById("reset").addEventListener("click", () => {
    state = {};
    localStorage.removeItem(STORE);
    apply();
  });

  const toggle = document.getElementById("toggle");
  function setCollapsed(on) {
    devbar.classList.toggle("collapsed", on);
    toggle.textContent = on ? "Customize" : "Hide";
    localStorage.setItem(`${STORE}-collapsed`, on ? "1" : "");
  }
  toggle.addEventListener("click", () => setCollapsed(!devbar.classList.contains("collapsed")));
  function onKey(e) {
    const tag = (e.target.tagName || "").toLowerCase();
    if (e.key === "." && !e.ctrlKey && !e.metaKey && tag !== "input" && tag !== "textarea") {
      setCollapsed(!devbar.classList.contains("collapsed"));
    }
  }
  document.addEventListener("keydown", onKey);
  setCollapsed(localStorage.getItem(`${STORE}-collapsed`) === "1");
})();
