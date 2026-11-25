#!/usr/bin/env python3
"""End-to-end check of the project templates – one real app per language.

For every template: `openshell create`, `openshell dev --headless`, wait until every service is
healthy, load the frontend, call one of the app's functions as an agent, `openshell stop`.
CI runs this on Windows, macOS and Linux (`.github/workflows/ci.yml`, job `templates`), so the
language support table on the website and in `openshell languages` is backed by real runs.

    python scripts/e2e-templates.py                       # every template whose toolchain is installed
    python scripts/e2e-templates.py --only vanilla-python,react-node
    python scripts/e2e-templates.py --strict              # a missing toolchain is a failure (CI)
    python scripts/e2e-templates.py --openshell target/release/openshell

Standard library only.
"""
from __future__ import annotations

import argparse
import json
import os
import shutil
import subprocess
import sys
import tempfile
import time
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent

# template -> programs it needs on the machine (one of each tuple)
TEMPLATES: dict[str, list[tuple[str, ...]]] = {
    "vanilla-python": [("python", "python3")],
    "react-python": [("npm",), ("python", "python3")],
    "vue-python": [("npm",), ("python", "python3")],
    "react-node": [("npm",), ("node",)],
    "svelte-go": [("npm",), ("go",)],
    "vanilla-java": [("javac",), ("java",)],
    "vanilla-cpp": [("cmake",)],
}
BAD_STATES = {"failed", "crashed"}


def default_binary() -> str:
    exe = "openshell.exe" if os.name == "nt" else "openshell"
    for profile in ("debug", "release"):
        p = ROOT / "target" / profile / exe
        if p.exists():
            return str(p)
    return "openshell"


def run(cmd: list[str], cwd: Path, timeout: float = 60) -> subprocess.CompletedProcess:
    return subprocess.run(cmd, cwd=cwd, capture_output=True, text=True, timeout=timeout, encoding="utf-8", errors="replace")


def status(openshell: str, app: Path) -> dict | None:
    r = run([openshell, "status", "--json"], app, timeout=30)
    if r.returncode != 0:
        return None
    try:
        return json.loads(r.stdout)
    except json.JSONDecodeError:
        return None


def check_template(openshell: str, template: str, work: Path, timeout: float) -> tuple[bool, str]:
    app = work / template
    r = run([openshell, "create", str(app), "-t", template], work)
    if r.returncode != 0:
        return False, f"create failed: {r.stderr.strip() or r.stdout.strip()}"
    log = open(work / f"{template}.log", "w", encoding="utf-8")
    proc = subprocess.Popen([openshell, "dev", "--headless"], cwd=app, stdout=log, stderr=subprocess.STDOUT, stdin=subprocess.DEVNULL)
    try:
        deadline = time.time() + timeout
        last = "not running yet"
        while time.time() < deadline:
            if proc.poll() is not None:
                return False, f"`openshell dev` exited with {proc.returncode}"
            s = status(openshell, app)
            if s:
                services = s.get("services", [])
                bad = [f"{x['name']} {x['state']}: {x.get('last_error') or ''}".strip() for x in services if x.get("state") in BAD_STATES]
                if bad:
                    return False, "; ".join(bad)
                last = ", ".join(f"{x['name']}={x['state']}" for x in services)
                if services and all(x.get("state") == "running" for x in services):
                    break
            time.sleep(1)
        else:
            return False, f"timeout after {timeout:.0f}s ({last})"

        # the frontend answers
        url = s.get("frontend_url")
        if url:
            with urllib.request.urlopen(url, timeout=20) as resp:
                if resp.status != 200:
                    return False, f"frontend {url} answered {resp.status}"

        # an agent can call one of the app's read functions
        tools = run([openshell, "tools", "--json"], app)
        if tools.returncode != 0:
            return False, f"tools: {tools.stderr.strip()}"
        caps = [t["name"] for t in json.loads(tools.stdout)["tools"] if t["name"].startswith("cap_") and t["permission"] == "read"]
        if caps:
            call = run([openshell, "call", caps[0]], app)
            if call.returncode != 0 or '"ok": false' in call.stdout:
                return False, f"{caps[0]} failed: {(call.stdout + call.stderr).strip()[:300]}"
        return True, f"{len(s.get('services', []))} service(s) healthy" + (f", {caps[0]} ok" if caps else "")
    finally:
        run([openshell, "stop"], app, timeout=60)
        try:
            proc.wait(timeout=30)
        except subprocess.TimeoutExpired:
            proc.kill()
        log.close()


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--openshell", default=default_binary())
    ap.add_argument("--only", default="", help="comma-separated templates")
    ap.add_argument("--strict", action="store_true", help="treat a missing toolchain as a failure")
    ap.add_argument("--timeout", type=float, default=600, help="seconds per template (npm install takes a while)")
    ap.add_argument("--keep", action="store_true", help="keep the generated apps and logs")
    args = ap.parse_args()

    names = [n for n in args.only.split(",") if n] or list(TEMPLATES)
    work = Path(tempfile.mkdtemp(prefix="openshell-e2e-"))
    results: list[tuple[str, str, str]] = []
    for t in names:
        if t not in TEMPLATES:
            results.append((t, "FAIL", "unknown template"))
            continue
        missing = [alts[0] for alts in TEMPLATES[t] if not any(shutil.which(a) for a in alts)]
        if missing:
            results.append((t, "FAIL" if args.strict else "skip", "missing: " + ", ".join(missing)))
            continue
        print(f"== {t}", flush=True)
        started = time.time()
        try:
            ok, msg = check_template(args.openshell, t, work, args.timeout)
        except Exception as e:  # noqa: BLE001 – report and continue with the next template
            ok, msg = False, f"{type(e).__name__}: {e}"
        results.append((t, "ok" if ok else "FAIL", f"{msg} ({time.time() - started:.0f}s)"))
        if not ok:
            log = work / f"{t}.log"
            if log.exists():
                print(log.read_text(encoding="utf-8", errors="replace")[-4000:])

    print()
    for t, state, msg in results:
        print(f"{state:<5} {t:<16} {msg}")
    if not args.keep:
        shutil.rmtree(work, ignore_errors=True)
    else:
        print(f"\nkept in {work}")
    return 1 if any(s == "FAIL" for _, s, _ in results) else 0


if __name__ == "__main__":
    sys.exit(main())
