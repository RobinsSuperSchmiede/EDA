# Google Play policy answers – OpenShell

Prepared answers for the Play Console declarations. Review them against the current policy text
before submitting; the account owner is responsible for what is declared.

## Foreground service: `specialUse`

*App content → Foreground service permissions.*

- **Type:** `FOREGROUND_SERVICE_SPECIAL_USE`, subtype declared in the manifest
  (`PROPERTY_SPECIAL_USE_FGS_SUBTYPE`).
- **Why no other type fits:** the service runs a local app server – a web server on 127.0.0.1, an
  SQLite database and the app's functions – that the app's own UI and the user's paired devices
  use. It is not media playback, location, data sync of a bounded transfer (`dataSync` is limited
  to 6 hours per day since Android 15), a phone call or a connected device.
- **User-visible:** it starts only when the user opens an app in OpenShell, shows a persistent
  notification with the running services and a *Stop* action, and stops when the user stops it.
- **Impact if deferred or interrupted:** requests from the app UI or paired devices fail; data
  written by functions may be interrupted.
- **Video:** record a short screen capture: open an app, switch to the home screen, show the
  notification, use the app from a laptop, tap *Stop*.

## Battery optimisation

The app does **not** request `REQUEST_IGNORE_BATTERY_OPTIMIZATIONS`. *Run in background…* opens
the system's battery optimisation list (`ACTION_IGNORE_BATTERY_OPTIMIZATION_SETTINGS`), where the
user can decide.

## Executable code (Device and Network Abuse policy)

Play apps may not download executable code (native code, DEX) from outside Google Play. OpenShell:

- ships all native code in the APK/AAB (`libopenshell_android.so`, optional runtimes as `lib*.so`);
- runs only files from the APK's native library directory and system tools (W^X, enforced by
  `AndroidAdapter`); app bundles (.osapp) are user data – web pages, database migrations and
  scripts interpreted by runtimes shipped in the APK, comparable to a browser loading web pages;
- never downloads bundles by itself – the user picks or shares them.

If the review disagrees, the fallback is to distribute builds with bundled runtimes outside Play
(GitHub releases / F-Droid) and keep the Play build to static frontends, databases and functions.

## Data safety form

| Question | Answer |
|---|---|
| Does the app collect or share user data? | **No.** Everything stays on the device; there is no OpenShell server. |
| Data encrypted in transit? | Local traffic stays on 127.0.0.1. The optional remote gateway supports TLS. |
| Can users request deletion? | Yes – uninstalling or removing an app deletes its data (app-private storage). |
| Account required? | No. |

Apps the user installs may collect data themselves; they run under the user's control and are not
part of OpenShell's declaration.

## Target audience and content

- Target age group: 18+ (developer tool), not designed for children.
- Ads: none. In-app purchases: none.
- Content rating questionnaire: utility / developer tool, no user-generated content shared
  through OpenShell.
