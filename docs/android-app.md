# The OpenShell Android app

`apps/android/` is the native Android host: it runs OpenShell apps on the phone with the unchanged
core, keeps them alive with a foreground service and can be published in the Google Play Store.

```
apps/android/                    Gradle project (Kotlin, no third-party libraries)
  app/src/main/java/…/
    LauncherActivity.kt          app manager: installed apps, add .osapp, stop, battery settings
    AppActivity.kt               one app: its frontend in a WebView, lifecycle events
    RuntimeService.kt            foreground service (specialUse), notification, wake lock, network events
    NativeRuntime.kt             JNI functions of libopenshell_android.so
    Apps.kt                      bundle import, app and state directories
  app/src/main/assets/launcher.html
  store/                         Play listing, policy answers, privacy policy template
crates/android/                  openshell-android: the Rust side (JNI bridge around openshell-core)
```

## How it works

1. The user adds an app bundle (`.osapp`, built with `openshell package --target bundle`) from the
   file picker or the share sheet. It is unpacked into the app's private storage
   (`files/apps/<identifier>/`); its data lives in `files/state/<identifier>/` and survives updates.
2. Opening the app starts `RuntimeService` (foreground service, type `specialUse`) which calls
   `NativeRuntime.start(…)`. The core runs with an `AndroidAdapter` that allows the APK's native
   library directory and registers bundled runtimes (`libnode.so` → `node`, `libpython.so` →
   `python`) when the APK contains them.
3. `AppActivity` shows the frontend from `http://127.0.0.1:<port>` in a WebView without any
   JavaScript bridge to Android, and forwards `onStart` / `onStop` / `onTrimMemory` as lifecycle
   events (`foreground`, `background`, `low_memory`); the service forwards network changes.
4. The notification lists the running services and has a *Stop* action. A partial wake lock keeps
   the CPU serving requests with the screen off; *Run in background…* opens the system's battery
   optimisation settings so the user can exempt OpenShell (always-on server).
5. The remote gateway (`runtime.remote`) works as on the desktop – see [remote-access.md](remote-access.md).

What runs inside the app: static frontends, the built-in database, functions with system tools or a
bundled runtime, services with bundled programs. Dev servers and toolchains do not – that is what
[Termux](platforms.md#termux-apps-on-the-phone) is for.

## Build

Requirements: JDK 17, Android SDK (API 36), Android NDK, Rust with
`rustup target add aarch64-linux-android x86_64-linux-android`, and
[`cargo-ndk`](https://github.com/bbqsrc/cargo-ndk) (`cargo install cargo-ndk`).

```bash
# 1. the Rust core as libopenshell_android.so, 16 KB page aligned (required for Android 15+ / Play)
RUSTFLAGS="-C link-arg=-Wl,-z,max-page-size=16384" \
  cargo ndk --platform 26 -t arm64-v8a -t x86_64 -o apps/android/app/src/main/jniLibs \
  build --release -p openshell-android

# 2. APK for testing and AAB for the Play Store
cd apps/android
gradle assembleRelease bundleRelease     # Gradle 8.14 (or add a wrapper: `gradle wrapper`)
adb install app/build/outputs/apk/release/app-release.apk
```

CI does the same in `.github/workflows/android-app.yml` and uploads the APK and AAB as artifacts.

| Setting | Value |
|---|---|
| Application ID | `dev.openshell.android` |
| `minSdk` / `targetSdk` / `compileSdk` | 26 / 36 / 36 |
| ABIs | `arm64-v8a` (phones), `x86_64` (emulators, Chromebooks) |
| Native libraries | extracted (`useLegacyPackaging = true`) so bundled runtimes can be executed |
| Version | `VERSION_NAME` / `VERSION_CODE` environment variables (CI: tag and run number) |

## Signing

Google Play uses **Play App Signing**: you sign the AAB with an *upload key*, Google re-signs for
distribution.

```bash
keytool -genkeypair -v -keystore upload.jks -alias upload -keyalg RSA -keysize 4096 -validity 10000
cp apps/android/keystore.properties.example apps/android/keystore.properties   # fill in, never commit
```

In CI set the secrets `ANDROID_KEYSTORE` (base64 of `upload.jks`), `ANDROID_KEYSTORE_PASSWORD`,
`ANDROID_KEY_ALIAS`, `ANDROID_KEY_PASSWORD`. Without them the release build is unsigned.

## Publishing in the Play Store – checklist

What this repository prepares and what the account owner has to do:

| Step | Prepared here | You do |
|---|---|---|
| Developer account | – | Register at play.google.com/console (one-time fee, identity verification) |
| App and package name | `dev.openshell.android` | Create the app in the console (change the ID first if you publish under your own name) |
| Signed AAB | CI workflow, `keystore.properties` | Create the upload key, add the secrets, enable Play App Signing |
| Target API level | `targetSdk 36` | Keep it current every year (Play deadlines usually end of August) |
| 16 KB page size | linker flag + CI check | – |
| Store listing | `store/listing.md` | Add icon (512 px), feature graphic (1024 × 500), screenshots |
| Privacy policy | `store/privacy-policy.md` (template) | Fill in publisher and contact, host it, enter the URL |
| Data safety form | answers in `store/policy.md` | Enter them in *App content* |
| Foreground service declaration | justification in `store/policy.md` | Submit it with a short video |
| Content rating, target audience | answers in `store/policy.md` | Complete the questionnaires |
| Testing | – | New personal accounts must run a closed test (12+ testers, 14 days) before production |
| Release | – | Upload the AAB to internal testing → closed → production |

## Status and open points

- ✅ JNI bridge (`crates/android`) with tests on the desktop; builds for `aarch64-linux-android` in CI.
- ✅ Kotlin host: launcher, app view, foreground service, notification, wake lock, network and
  lifecycle events, bundle import.
- 🚧 Not yet verified on a device in this round (the Gradle build runs in CI; no Android SDK on
  the development machine).
- ⏳ Bundled runtimes (`libnode.so`, `libpython.so`): need prebuilt Android binaries and their
  licences in the APK.
- ⏳ Approvals on the phone (agent asks for permission): the core supports it; the app has no dialog yet.
- ⏳ Pairing by scanning a QR code (`openshell://pair?...`) and using apps hosted on other devices.
- ⏳ Tauri mobile shell: the plain Kotlin host was chosen for full control over the foreground
  service and Play policies; the core is the same either way.
