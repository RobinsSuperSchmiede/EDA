# Platform milestones: Android, universal shell, secure remote access

This plan splits the platform work (Android, one download per system, apps built on the phone,
secure remote access, Play Store) into milestones. Each milestone builds on its own, has tests
and is documented. The last section lists what cannot be finished inside this repository.

Status legend: ✅ done · 🚧 partial · ⏳ open

## M1 – Secure remote gateway and exposed services ✅

Goal: reach the phone's (or any host's) websites, APIs and services from another computer over
IP address and port, with security features that can be switched on one by one.

| Feature | Config | Default |
|---|---|---|
| Bind address and port | `runtime.remote.bind`, `runtime.remote.port` | `0.0.0.0:7420` |
| IP / CIDR allowlist | `runtime.remote.allow` | everyone who can reach the port |
| Per-device tokens (pairing, QR code) | `openshell remote pair <device>` | shared pairing token |
| Rate limit + lockout after failed logins | `runtime.remote.rate_limit` | 120 requests/min, 10 failures → 5 min block |
| TLS (own certificate or self-signed with pinned fingerprint) | `runtime.remote.tls` | off |
| Opt-in exposure per service (HTTP with token, raw TCP) | `runtime.remote.expose[]` | nothing exposed |
| Audit of remote access | audit log, channel `remote` | on |

Code: `crates/config/src/model.rs` (`RemoteConfig`, `ExposeConfig`, `RateLimitConfig`,
`TlsConfig`), `crates/core/src/remote/*`, `openshell remote …` in the CLI.

## M2 – Apps on the phone (Termux) ✅

Goal: `openshell create`, `dev`, `start`, `package --target bundle` on the phone; Python, Node,
Java, Rust, Go … from Termux packages run as services, setup and build steps.

- `TermuxAdapter` (`crates/runtime/src/platform.rs`): platform `android`, `processes: full`,
  programs from `$PREFIX/bin` and the project, shell `$PREFIX/bin/sh`, `wake_lock` capability.
  Picked automatically inside Termux; `OPENSHELL_ANDROID_MODE=app|termux` overrides.
- `openshell doctor` gives `pkg install …` hints per language on Termux.
- `openshell start/dev` take a Termux wake lock (`termux-wake-lock`) while services must stay alive.

## M3 – Universal shell: one link per system ✅ (🚧 Android link)

Goal: a developer builds once; users download one file per system, the program is unchanged.

- `openshell package --target bundle`: a portable, platform-neutral app bundle (`<app>-<version>.osapp`,
  a zip with `openshell.json` and the app files) – runs with `openshell start --bundle` on any host,
  including Termux, and is what the Android app imports.
- `openshell package --manifest <base-url>`: writes `downloads.json` (version, file, size,
  SHA-256 per platform) and a `download.html` page that picks the visitor's system.
- `openshell package --ci github`: writes `.github/workflows/openshell-release.yml` into the app –
  one tag push builds Windows, macOS (arm64 + x64), Linux installers and the bundle in parallel and
  publishes them with the manifest as a GitHub release.
- Code signing (`package.signing`: Windows certificate thumbprint / sign command, macOS signing
  identity) is passed to the bundler.
- Update check: `package.update_url` points at the `downloads.json` of the newest release; the
  installed app reports a newer version (`GET /v1/update`, `openshell update --check`).

## M4 – Native Android app and Play Store 🚧

Goal: an Android app (AAB) that can be published in the Play Store.

- `crates/android` (`openshell-android`, `cdylib` `libopenshell_android.so`): JNI bridge that starts
  the unchanged core headless with an `AndroidAdapter` for the APK's native library directory and
  forwards lifecycle events.
- `apps/android/` – Gradle project (Kotlin): `MainActivity` (WebView on the app's frontend),
  `RuntimeService` (foreground service type `specialUse`, notification with running services,
  battery-optimisation request), app bundle import, `targetSdk 36`, 16 KB page-size aligned
  native libraries, release signing from `keystore.properties` / environment, `bundleRelease` → AAB.
- `apps/android/store/` – listing texts, data-safety answers, foreground-service justification.
- CI workflow `android-app.yml` builds the Rust library for arm64/x86_64 and the AAB.

## M5 – Docs, roadmap, changelog ✅

`docs/platforms.md`, `docs/remote.md`, `docs/android-app.md`, `docs/installers.md`,
`docs/configuration.md`, `docs/cli.md`, `docs/ai/app-development.md`, `ROADMAP.md`, `CHANGELOG.md`.

## What cannot be finished inside this repository

| Item | Why | What is prepared |
|---|---|---|
| Play Store publishing | needs the Google Play developer account, an upload key / Play App Signing, the store review (foreground service `specialUse` justification, data-safety form) | AAB build, signing config, listing texts, checklist in `docs/android-app.md` |
| Signed installers (Windows / macOS) | needs certificates, Apple Developer ID and notarisation credentials | `package.signing` + CI secrets |
| Building the Android app locally here | no Android SDK/NDK on this machine | CI workflow builds it; not verified on a device in this round |
| Bundled Node.js / Python inside the APK | needs prebuilt Android runtimes (`libnode.so`, `libpython.so`) and their licences | `AndroidAdapter::with_program`, `OPENSHELL_ANDROID_PROGRAMS`; Termux covers this today |
| Device mesh beyond pairing (mDNS, `--device`, project sync) | larger design work | per-device tokens + QR pairing are the base |
| Agent sessions with own permissions, screenshot fallback, installing updates automatically | not started in this round | update check (M3), per-device tokens (M1) |
| Play policy on running user code | apps from Play may not download executable code; projects are user-provided data, runtimes must be in the APK | documented in `docs/android-app.md` |
