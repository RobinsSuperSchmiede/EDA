/*
 * OpenShell UI bridge.
 *
 * Injected into application pages by the OpenShell desktop shell (or the
 * built-in static server). It connects the page to the shell's control API
 * over a WebSocket so that agents can inspect and operate the UI through a
 * semantic interface (DOM / accessibility information instead of pixels).
 *
 * Expects `window.__OPENSHELL__ = { api, bridge, token, host, mode }` to be set
 * before this script runs. Framework-agnostic; no dependencies.
 */
(function () {
  "use strict";
  if (window.__OPENSHELL_BRIDGE__) return;
  var cfg = window.__OPENSHELL__;
  if (!cfg || !cfg.bridge) return;
  if (window.top !== window) return; // only the top-level document
  if (location.protocol === "tauri:" || location.hostname === "tauri.localhost") return; // shell's own pages
  window.__OPENSHELL_BRIDGE__ = true;

  var REF_ATTR = "data-os-ref";
  var nextRef = 1;
  var ws = null;
  var retry = 500;
  var actions = (window.__OPENSHELL_ACTIONS__ = window.__OPENSHELL_ACTIONS__ || {});

  // ---------------------------------------------------------------- transport
  function send(obj) {
    if (ws && ws.readyState === 1) ws.send(JSON.stringify(obj));
  }

  function actionList() {
    return Object.keys(actions).map(function (name) {
      var a = actions[name];
      return {
        name: name,
        description: a.description || "",
        permission: a.permission || "actions",
        input_schema: a.input_schema || a.inputSchema || null,
      };
    });
  }

  function connect() {
    try {
      ws = new WebSocket(cfg.bridge);
    } catch (e) {
      return setTimeout(connect, retry);
    }
    ws.onopen = function () {
      retry = 500;
      send({ type: "hello", url: location.href, title: document.title, host: cfg.host || "browser", actions: actionList() });
    };
    ws.onmessage = function (ev) {
      var msg;
      try { msg = JSON.parse(ev.data); } catch (e) { return; }
      if (msg && msg.method) handle(msg);
    };
    ws.onclose = function () {
      ws = null;
      retry = Math.min(retry * 2, 5000);
      setTimeout(connect, retry);
    };
    ws.onerror = function () { /* onclose follows */ };
  }

  function handle(msg) {
    var fn = METHODS[msg.method];
    var p = fn
      ? Promise.resolve().then(function () { return fn(msg.params || {}); })
      : Promise.reject(new Error("unknown method " + msg.method));
    p.then(
      function (result) { send({ type: "result", id: msg.id, ok: true, result: result === undefined ? null : result }); },
      function (err) { send({ type: "result", id: msg.id, ok: false, error: String((err && err.message) || err) }); }
    );
  }

  // notify the shell about SPA navigation and title changes
  var lastUrl = location.href, lastTitle = document.title;
  setInterval(function () {
    if (location.href !== lastUrl || document.title !== lastTitle) {
      lastUrl = location.href; lastTitle = document.title;
      send({ type: "page", url: lastUrl, title: lastTitle });
    }
  }, 500);
  window.addEventListener("openshell:actions-changed", function () {
    send({ type: "actions", actions: actionList() });
  });

  // ------------------------------------------------------------- DOM helpers
  var INTERACTIVE =
    "a[href],button,input,select,textarea,summary,details,[contenteditable=''],[contenteditable='true']," +
    "[role=button],[role=link],[role=checkbox],[role=radio],[role=switch],[role=tab],[role=menuitem]," +
    "[role=option],[role=textbox],[role=combobox],[role=slider],[role=treeitem],[onclick],[tabindex]";

  function isVisible(el) {
    if (!el.isConnected) return false;
    var style = getComputedStyle(el);
    if (style.visibility === "hidden" || style.display === "none" || style.opacity === "0") return false;
    var r = el.getBoundingClientRect();
    return r.width > 0 && r.height > 0;
  }

  function clean(s, max) {
    s = (s || "").replace(/\s+/g, " ").trim();
    return max && s.length > max ? s.slice(0, max) + "…" : s;
  }

  function labelFor(el) {
    if (el.id) {
      var l = document.querySelector("label[for='" + CSS.escape(el.id) + "']");
      if (l) return clean(l.innerText);
    }
    var parent = el.closest("label");
    if (parent) return clean(parent.innerText);
    return "";
  }

  function accessibleName(el) {
    var aria = el.getAttribute("aria-label");
    if (aria) return clean(aria);
    var by = el.getAttribute("aria-labelledby");
    if (by) {
      var txt = by.split(/\s+/).map(function (id) {
        var n = document.getElementById(id);
        return n ? n.innerText : "";
      }).join(" ");
      if (clean(txt)) return clean(txt);
    }
    var tag = el.tagName.toLowerCase();
    if (tag === "input" || tag === "select" || tag === "textarea") {
      var lbl = labelFor(el);
      if (lbl) return lbl;
      if (el.placeholder) return clean(el.placeholder);
      if (tag === "input" && (el.type === "submit" || el.type === "button")) return clean(el.value);
    }
    if (tag === "img") return clean(el.alt);
    var text = clean(el.innerText || el.textContent, 120);
    if (text) return text;
    return clean(el.getAttribute("title") || el.getAttribute("name") || "");
  }

  function roleOf(el) {
    var role = el.getAttribute("role");
    if (role) return role;
    var tag = el.tagName.toLowerCase();
    if (tag === "a") return "link";
    if (tag === "button" || tag === "summary") return "button";
    if (tag === "select") return "combobox";
    if (tag === "textarea") return "textbox";
    if (tag === "input") {
      var t = (el.type || "text").toLowerCase();
      if (t === "checkbox" || t === "radio") return t;
      if (t === "submit" || t === "button" || t === "reset") return "button";
      if (t === "range") return "slider";
      return "textbox";
    }
    if (el.isContentEditable) return "textbox";
    return "generic";
  }

  function refOf(el) {
    var ref = el.getAttribute(REF_ATTR);
    if (!ref) {
      ref = "e" + nextRef++;
      el.setAttribute(REF_ATTR, ref);
    }
    return ref;
  }

  function describe(el) {
    var d = { ref: refOf(el), role: roleOf(el), name: accessibleName(el) };
    var tag = el.tagName.toLowerCase();
    if (tag === "input" || tag === "textarea" || tag === "select") {
      if (el.type === "checkbox" || el.type === "radio") d.checked = !!el.checked;
      else if (el.type !== "password") d.value = clean(String(el.value || ""), 200);
      if (el.type && tag === "input") d.type = el.type;
      if (tag === "select") {
        d.options = Array.prototype.slice.call(el.options, 0, 50).map(function (o) { return clean(o.text); });
      }
    }
    if (tag === "a" && el.getAttribute("href")) d.href = el.getAttribute("href");
    if (el.disabled || el.getAttribute("aria-disabled") === "true") d.disabled = true;
    if (el.getAttribute("aria-expanded")) d.expanded = el.getAttribute("aria-expanded") === "true";
    if (el.getAttribute("aria-selected") === "true") d.selected = true;
    if (document.activeElement === el) d.focused = true;
    return d;
  }

  function findTarget(p) {
    var el = null;
    if (p.ref) {
      el = document.querySelector("[" + REF_ATTR + "='" + CSS.escape(p.ref) + "']");
      if (!el) throw new Error("no element with ref " + p.ref + " (take a new ui_snapshot)");
      return el;
    }
    if (p.selector) {
      el = document.querySelector(p.selector);
      if (!el) throw new Error("no element matches selector " + p.selector);
      return el;
    }
    var needle = clean(p.label || p.text).toLowerCase();
    if (needle) {
      var candidates = Array.prototype.filter.call(document.querySelectorAll(INTERACTIVE), isVisible);
      var exact = null, partial = null;
      for (var i = 0; i < candidates.length; i++) {
        var name = accessibleName(candidates[i]).toLowerCase();
        if (name === needle) { exact = candidates[i]; break; }
        if (!partial && name.indexOf(needle) !== -1) partial = candidates[i];
      }
      el = exact || partial;
      if (!el) throw new Error("no interactive element named \"" + (p.label || p.text) + "\"");
      return el;
    }
    return null;
  }

  function nativeSetValue(el, value) {
    var proto = el.tagName === "TEXTAREA" ? HTMLTextAreaElement.prototype
      : el.tagName === "SELECT" ? HTMLSelectElement.prototype : HTMLInputElement.prototype;
    var desc = Object.getOwnPropertyDescriptor(proto, "value");
    if (desc && desc.set) desc.set.call(el, value); else el.value = value;
  }

  function fire(el, type) {
    el.dispatchEvent(new Event(type, { bubbles: true }));
  }

  function sleep(ms) { return new Promise(function (r) { setTimeout(r, ms); }); }

  function textOf(root, max) {
    return clean((root || document.body).innerText, max || 3000);
  }

  // ----------------------------------------------------------------- methods
  var METHODS = {
    snapshot: function (p) {
      var root = p.selector ? document.querySelector(p.selector) : document.body;
      if (!root) throw new Error("no element matches selector " + p.selector);
      var elements = [];
      var nodes = root.querySelectorAll(INTERACTIVE);
      for (var i = 0; i < nodes.length && elements.length < 400; i++) {
        var el = nodes[i];
        if (el.tagName === "INPUT" && el.type === "hidden") continue;
        if (el.closest("[aria-hidden='true']")) continue;
        if (!isVisible(el)) continue;
        elements.push(describe(el));
      }
      var headings = Array.prototype.slice.call(root.querySelectorAll("h1,h2,h3"), 0, 40)
        .filter(isVisible)
        .map(function (h) { return { level: Number(h.tagName[1]), text: clean(h.innerText, 120) }; });
      var dialogs = Array.prototype.filter.call(root.querySelectorAll("dialog[open],[role=dialog],[role=alertdialog]"), isVisible)
        .map(function (d) { return clean(d.innerText, 300); });
      return {
        url: location.href,
        title: document.title,
        headings: headings,
        dialogs: dialogs,
        elements: elements,
        text: textOf(root, p.max_text || 3000),
        actions: actionList(),
      };
    },

    click: function (p) {
      var el = findTarget(p);
      if (!el) throw new Error("specify ref, selector or text");
      if (el.disabled) throw new Error("element is disabled");
      el.scrollIntoView({ block: "center", inline: "center" });
      if (typeof el.focus === "function") el.focus({ preventScroll: true });
      var opts = { bubbles: true, cancelable: true, view: window };
      el.dispatchEvent(new PointerEvent("pointerdown", opts));
      el.dispatchEvent(new MouseEvent("mousedown", opts));
      el.dispatchEvent(new PointerEvent("pointerup", opts));
      el.dispatchEvent(new MouseEvent("mouseup", opts));
      el.click();
      return sleep(50).then(function () { return { clicked: describe(el), url: location.href }; });
    },

    fill: function (p) {
      var el = findTarget(p);
      if (!el) throw new Error("specify ref, selector, label or text");
      el.scrollIntoView({ block: "center" });
      if (typeof el.focus === "function") el.focus({ preventScroll: true });
      var v = p.value;
      if (el.type === "checkbox" || el.type === "radio") {
        var want = v === true || v === "true" || v === "on" || v === 1;
        if (el.checked !== want) el.click();
      } else if (el.tagName === "SELECT") {
        var opt = Array.prototype.find.call(el.options, function (o) {
          return o.value === String(v) || clean(o.text).toLowerCase() === String(v).toLowerCase();
        });
        if (!opt) throw new Error("option \"" + v + "\" not found");
        nativeSetValue(el, opt.value);
        fire(el, "input"); fire(el, "change");
      } else if (el.isContentEditable) {
        el.textContent = String(v);
        fire(el, "input");
      } else {
        nativeSetValue(el, v === null || v === undefined ? "" : String(v));
        fire(el, "input"); fire(el, "change");
      }
      if (p.submit) {
        var form = el.form || el.closest("form");
        if (form) {
          if (form.requestSubmit) form.requestSubmit(); else form.submit();
        } else {
          METHODS.press({ key: "Enter", ref: refOf(el) });
        }
      }
      return sleep(50).then(function () { return { filled: describe(el) }; });
    },

    press: function (p) {
      var el = findTarget(p) || document.activeElement || document.body;
      var opts = { key: p.key, code: p.key, bubbles: true, cancelable: true };
      var down = el.dispatchEvent(new KeyboardEvent("keydown", opts));
      el.dispatchEvent(new KeyboardEvent("keypress", opts));
      el.dispatchEvent(new KeyboardEvent("keyup", opts));
      if (down && p.key === "Enter" && el.form && el.tagName === "INPUT") {
        if (el.form.requestSubmit) el.form.requestSubmit(); else el.form.submit();
      }
      return { pressed: p.key };
    },

    read_text: function (p) {
      var el = findTarget(p);
      return { text: textOf(el, p.max || 20000) };
    },

    wait_for: function (p) {
      var timeout = p.timeout_ms || 5000;
      var start = Date.now();
      function present() {
        if (p.selector) {
          var el = document.querySelector(p.selector);
          return !!(el && isVisible(el));
        }
        if (p.text) return textOf(document.body, 1e7).toLowerCase().indexOf(String(p.text).toLowerCase()) !== -1;
        throw new Error("specify selector or text");
      }
      return new Promise(function (resolve, reject) {
        (function poll() {
          var ok;
          try { ok = present() !== !!p.gone; } catch (e) { return reject(e); }
          if (ok) return resolve({ ok: true, waited_ms: Date.now() - start });
          if (Date.now() - start > timeout) return reject(new Error("timed out after " + timeout + "ms"));
          setTimeout(poll, 100);
        })();
      });
    },

    navigate: function (p) {
      var target = p.url || p.path;
      if (!target) throw new Error("specify url or path");
      var url = new URL(target, location.href).href;
      setTimeout(function () {
        // SPA routers listen to history changes; fall back to a full navigation for other origins
        if (new URL(url).origin === location.origin && p.path && window.history && !p.full) {
          history.pushState({}, "", url);
          window.dispatchEvent(new PopStateEvent("popstate", { state: {} }));
        } else {
          location.href = url;
        }
      }, 10);
      return { navigating_to: url };
    },

    reload: function () {
      setTimeout(function () { location.reload(); }, 10);
      return { reloading: true };
    },

    eval: function (p) {
      var result = (0, eval)(p.script);
      return Promise.resolve(result).then(function (r) {
        try { return JSON.parse(JSON.stringify(r === undefined ? null : r)); } catch (e) { return String(r); }
      });
    },

    invoke_action: function (p) {
      var a = actions[p.name];
      if (!a || typeof a.handler !== "function") throw new Error("action " + p.name + " is not registered");
      return Promise.resolve(a.handler(p.input || {})).then(function (r) {
        return r === undefined ? null : r;
      });
    },
  };

  connect();
})();
