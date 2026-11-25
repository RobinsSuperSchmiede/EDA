// OpenShell website – behaviour shared by the sub pages and the documentation:
// mobile menu, tab sets, copy buttons, download detection, docs search and the table of contents.
(() => {
  const inDocs = document.body.classList.contains("docs-page");
  const root = inDocs ? "../" : "";

  // ------------------------------------------------------------------ mobile menu
  const bar = document.getElementById("top-bar");
  const toggle = document.getElementById("nav-toggle");
  const setMenu = (open) => {
    bar.classList.toggle("open", open);
    toggle?.setAttribute("aria-expanded", String(open));
    toggle && (toggle.textContent = open ? "Close" : "Menu");
  };
  toggle?.addEventListener("click", () => setMenu(!bar.classList.contains("open")));
  document.addEventListener("click", (e) => {
    if (bar.classList.contains("open") && !e.target.closest(".top-inner")) setMenu(false);
  });
  document.addEventListener("keydown", (e) => e.key === "Escape" && setMenu(false));

  // ------------------------------------------------------------------ tab sets: <div class="tabset"> .tab[data-pane] + .pane#id
  document.querySelectorAll(".tabset").forEach((set) => {
    const tabs = [...set.querySelectorAll(".tab")];
    const panes = [...set.querySelectorAll(".pane")];
    const show = (id) => {
      tabs.forEach((t) => t.setAttribute("aria-selected", String(t.dataset.pane === id)));
      panes.forEach((p) => p.classList.toggle("on", p.id === id));
    };
    tabs.forEach((t) => t.addEventListener("click", () => show(t.dataset.pane)));
    const hash = location.hash.slice(1);
    if (hash && panes.some((p) => p.id === hash)) {
      show(hash);
      set.scrollIntoView();
    }
    set.show = show;
  });

  // ------------------------------------------------------------------ copy buttons on code blocks
  const copy = async (text, btn) => {
    try {
      await navigator.clipboard.writeText(text);
      btn.textContent = "Copied";
    } catch {
      btn.textContent = "Select and copy";
    }
    setTimeout(() => (btn.textContent = "Copy"), 1500);
  };
  document.querySelectorAll(".codeblock").forEach((block) => {
    const btn = document.createElement("button");
    btn.type = "button";
    btn.className = "btn btn--sm copy-btn";
    btn.textContent = "Copy";
    btn.addEventListener("click", () => copy(block.querySelector("pre").innerText.replace(/\n$/, ""), btn));
    block.append(btn);
  });
  document.querySelectorAll("[data-copy-target]").forEach((btn) =>
    btn.addEventListener("click", () => {
      const el = document.getElementById(btn.dataset.copyTarget);
      const text = el.innerText
        .split("\n")
        .map((l) => l.replace(/^\$\s*/, "").replace(/\s+#.*$/, ""))
        .filter((l) => l.trim() && !l.trim().startsWith("#"))
        .join("\n");
      copy(text, btn);
    }),
  );

  // ------------------------------------------------------------------ download page: offer the visitor's system first
  const cards = document.querySelectorAll("[data-os]");
  if (cards.length) {
    const ua = (navigator.userAgentData?.platform || navigator.platform || "") + " " + navigator.userAgent;
    let os = "linux";
    if (/android/i.test(ua)) os = "android";
    else if (/win/i.test(ua)) os = "windows";
    else if (/mac|iphone|ipad/i.test(ua)) os = "mac";
    const names = { windows: "Windows", mac: "macOS", linux: "Linux", android: "Android" };
    cards.forEach((c) => c.classList.toggle("rec", c.dataset.os === os));
    document.querySelectorAll("[data-detected]").forEach((el) => (el.textContent = names[os]));
    document.querySelectorAll("[data-os-only]").forEach((el) => (el.hidden = el.dataset.osOnly !== os));
    const first = document.querySelector(`[data-os="${os}"]`);
    first?.parentElement?.prepend(...document.querySelectorAll(`[data-os="${os}"]`));
    const set = document.querySelector("#install-tabs");
    set?.show?.({ windows: "i-win", mac: "i-mac", linux: "i-linux", android: "i-android" }[os]);
  }

  // ------------------------------------------------------------------ docs: mobile menu, table of contents
  const docsNav = document.getElementById("docs-nav");
  if (docsNav) {
    const btn = document.createElement("button");
    btn.type = "button";
    btn.className = "btn btn--sm docs-menu-btn";
    btn.textContent = "Browse docs";
    btn.setAttribute("aria-expanded", "false");
    btn.addEventListener("click", () => btn.setAttribute("aria-expanded", String(docsNav.classList.toggle("open"))));
    docsNav.prepend(btn);
    docsNav.querySelector('[aria-current="page"]')?.scrollIntoView({ block: "nearest" });
  }
  const tocLinks = [...document.querySelectorAll(".docs-toc a")];
  if (tocLinks.length) {
    const heads = tocLinks.map((a) => document.getElementById(a.getAttribute("href").slice(1))).filter(Boolean);
    const spy = () => {
      let cur = heads[0];
      for (const h of heads) if (h.getBoundingClientRect().top < 140) cur = h;
      tocLinks.forEach((a) => a.classList.toggle("on", cur && a.getAttribute("href") === "#" + cur.id));
    };
    window.addEventListener("scroll", spy, { passive: true });
    spy();
  }

  // ------------------------------------------------------------------ search over docs and pages (window.OS_SEARCH from search-index.js)
  const DATA = window.OS_SEARCH || [];
  const norm = (s) => s.toLowerCase();
  const esc = (s) => s.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");

  function search(query) {
    const words = norm(query).split(/[^\p{L}\p{N}_.\-/]+/u).filter(Boolean);
    if (!words.length) return [];
    const out = [];
    for (const e of DATA) {
      const title = norm(e.t), head = norm(e.h), text = norm(e.x);
      let score = 0;
      let ok = true;
      for (const w of words) {
        let s = 0;
        if (title.includes(w)) s += 6;
        if (head.includes(w)) s += 8;
        if (text.includes(w)) s += 2 + Math.min(3, text.split(w).length - 2);
        if (!s) {
          ok = false;
          break;
        }
        score += s;
      }
      if (ok) out.push({ e, score: score + (e.h ? 0 : 1) });
    }
    return out.sort((a, b) => b.score - a.score).slice(0, 8);
  }

  function snippet(text, words) {
    const low = norm(text);
    let at = -1;
    for (const w of words) {
      at = low.indexOf(w);
      if (at >= 0) break;
    }
    const from = Math.max(0, at - 50);
    return (from > 0 ? "…" : "") + text.slice(from, from + 150) + (from + 150 < text.length ? "…" : "");
  }

  function mark(el, text, words) {
    if (!words.length) return void (el.textContent = text);
    const re = new RegExp("(" + words.map(esc).join("|") + ")", "ig");
    let last = 0;
    for (const m of text.matchAll(re)) {
      el.append(text.slice(last, m.index));
      const k = document.createElement("mark");
      k.textContent = m[0];
      el.append(k);
      last = m.index + m[0].length;
    }
    el.append(text.slice(last));
  }

  function wire(input, box) {
    if (!input || !box) return;
    let sel = -1;
    const render = () => {
      const q = input.value.trim();
      box.replaceChildren();
      sel = -1;
      if (!q) return void (box.hidden = true);
      const words = norm(q).split(/[^\p{L}\p{N}_.\-/]+/u).filter(Boolean);
      const hits = search(q);
      if (!hits.length) {
        const none = document.createElement("div");
        none.className = "none";
        none.textContent = "Nothing found. Try another word, for example a command or a setting.";
        box.append(none);
      }
      for (const { e } of hits) {
        const a = document.createElement("a");
        a.href = root + e.u;
        const b = document.createElement("b");
        mark(b, e.h || e.t, words);
        const s = document.createElement("small");
        s.textContent = e.h ? e.t : "Page";
        const p = document.createElement("span");
        mark(p, snippet(e.x, words), words);
        a.append(b, s, p);
        box.append(a);
      }
      box.hidden = false;
    };
    input.addEventListener("input", render);
    input.addEventListener("focus", () => input.value.trim() && render());
    input.addEventListener("keydown", (e) => {
      const items = [...box.querySelectorAll("a")];
      if (e.key === "Escape") {
        box.hidden = true;
        input.blur();
      } else if ((e.key === "ArrowDown" || e.key === "ArrowUp") && items.length) {
        e.preventDefault();
        sel = (sel + (e.key === "ArrowDown" ? 1 : -1) + items.length) % items.length;
        items.forEach((a, i) => a.classList.toggle("sel", i === sel));
        items[sel].scrollIntoView({ block: "nearest" });
      } else if (e.key === "Enter" && items.length) {
        items[Math.max(0, sel)].click();
      }
    });
    document.addEventListener("click", (e) => {
      if (!e.target.closest(".docs-search, .hub-search")) box.hidden = true;
    });
  }
  wire(document.getElementById("doc-search"), document.getElementById("search-results"));
  wire(document.getElementById("hub-search"), document.getElementById("hub-results"));
  // "/" focuses the search, like on most documentation sites
  document.addEventListener("keydown", (e) => {
    const input = document.getElementById("doc-search") || document.getElementById("hub-search");
    if (e.key === "/" && input && !/input|textarea/i.test(document.activeElement.tagName)) {
      e.preventDefault();
      input.focus();
    }
  });
})();
