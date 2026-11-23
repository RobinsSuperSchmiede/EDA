"""Generate llms-full.txt: the documentation relevant for building OpenShell apps, in one file.

Usage (from the repository root): python scripts/gen-llms-full.py
CI fails if the committed file is outdated (python scripts/gen-llms-full.py --check).
"""
import pathlib
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
DOCS = [
    "README.md",
    "docs/ai/app-development.md",
    "docs/getting-started.md",
    "docs/building-blocks.md",
    "docs/tutorial.md",
    "docs/configuration.md",
    "docs/languages.md",
    "docs/agents.md",
    "docs/installers.md",
    "docs/platforms.md",
    "docs/cli.md",
    "docs/sdk.md",
    "docs/control-api.md",
]

HEADER = """# OpenShell – full documentation for LLMs

Generated from the repository by scripts/gen-llms-full.py. Source: https://github.com/RobinsSuperSchmiede/EDA
Contents: {files}
"""


def build() -> str:
    parts = [HEADER.format(files=", ".join(DOCS))]
    for rel in DOCS:
        text = (ROOT / rel).read_text(encoding="utf-8").strip()
        parts.append(f"\n\n{'=' * 80}\nFILE: {rel}\n{'=' * 80}\n\n{text}\n")
    return "".join(parts)


def main() -> int:
    out = ROOT / "llms-full.txt"
    content = build()
    if "--check" in sys.argv:
        current = out.read_text(encoding="utf-8") if out.exists() else ""
        if current != content:
            print("llms-full.txt is outdated – run: python scripts/gen-llms-full.py")
            return 1
        print("llms-full.txt is up to date")
        return 0
    out.write_text(content, encoding="utf-8", newline="\n")
    print(f"wrote {out.relative_to(ROOT)} ({len(content):,} characters)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
