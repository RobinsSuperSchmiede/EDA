## What does this change?

<!-- Short description. Link the issue: Fixes #123 -->

## How was it tested?

- [ ] `cargo test --workspace --exclude openshell-desktop`
- [ ] Tried it with an example / template (which one, which OS?)

## Checklist

- [ ] Docs updated (`docs/`, `README.md`, `docs/ai/app-development.md` if app developers or AI agents are affected)
- [ ] Schema regenerated if config fields changed (`cargo run -p openshell-cli -- schema > schema/openshell.schema.json`)
- [ ] `CHANGELOG.md` entry under *Unreleased*
- [ ] New agent tools declare a permission scope and are audited
