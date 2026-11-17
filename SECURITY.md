# Security Policy

OpenShell runs processes on the user's machine and gives AI agents controlled access to applications, so security
reports are very welcome.

## Supported versions

OpenShell is in alpha. Security fixes are made on `main` and in the latest release.

## Reporting a vulnerability

Please **do not open a public issue**. Report privately through
[GitHub security advisories](https://github.com/RobinsSuperSchmiede/EDA/security/advisories/new) with:

- affected component (runtime, control API, agent layer, UI bridge, installer, …) and version / commit,
- steps to reproduce or a proof of concept,
- impact as you understand it.

You can expect an acknowledgement within a few days. We will keep you informed and credit you in the release notes
unless you prefer otherwise.

## Scope and threat model

In scope, for example: bypassing agent permissions or approvals, escaping the project directory with file tools,
reaching the control API from a web page (CSRF, DNS rebinding, Origin bypass), token leaks, injection through
configuration that is shipped in installers.

The permission layer governs **cooperating agents**. A local process running as the same user can read
`.openshell/runtime.json` and is out of scope; OpenShell is not a sandbox against local malware.
See [docs/agents.md#security](docs/agents.md#security) and [ARCHITECTURE.md](ARCHITECTURE.md#security-model-summary).
