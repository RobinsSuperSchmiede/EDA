<div align="center">

<img src="docs/assets/logo.png" width="96" alt="OpenShell Logo" />

# OpenShell

**Jede Web-App als native Desktop-App – verwaltet von einer Runtime, die Menschen *und* KI-Agenten bedienen können.**

[English](README.md) · [Dokumentation (englisch)](docs/README.md) · [Konzept (deutsch)](docs/concept/README.md)

</div>

---

OpenShell (Projektname **EDA – Evolving Desktop App**) ist eine Open-Source-Desktop-Shell auf Basis von **Rust und
Tauri 2**. Du bringst ein normales Web-Frontend und ein beliebiges Backend mit – Python, Node.js, Java, C++, Go, Rust,
.NET – und beschreibst beides in einer `openshell.json`. OpenShell macht daraus eine native Desktop-App: Es startet und
überwacht deine Services, zeigt das Frontend im Fenster, lädt bei Codeänderungen neu und baut Installer. Eigenen
Desktop- oder Prozessverwaltungs-Code schreibst du nicht.

Das Besondere: Jede OpenShell-App ist **agent-native**. KI-Agenten (Claude, OpenAI, lokale Modelle über Ollama /
LM Studio, jeder MCP-Client) können die App über ihre eigenen Funktionen *benutzen* und über eine kontrollierte,
berechtigungsgeprüfte Schnittstelle *weiterentwickeln* – ohne sich durch die Oberfläche klicken zu müssen.

## Schnellstart

```bash
# 1. Installieren (aus dem Quellcode, benötigt Rust)
git clone https://github.com/RobinsSuperSchmiede/EDA.git openshell
cd openshell && cargo build --release      # danach target/release in den PATH aufnehmen

# 2. App erstellen und starten
openshell create meine-app                 # HTML/JS + Python, keine weiteren Installationen nötig
cd meine-app
openshell dev                              # natives Fenster, Logs im Terminal, Hot Reload

# 3. Ausliefern
openshell package                          # Windows-Setup.exe / macOS .dmg / Linux .deb + .AppImage

# 4. KI darauf loslassen
claude mcp add meine-app -- openshell mcp --app --start --project .
```

Bestehendes Projekt: `openshell init` erkennt Frontend (Vite, Next, CRA, Angular, Vue) und Backend (Python, Node, Go,
Rust, Java/Maven/Gradle/Spring Boot, C++/CMake) und schreibt die Konfiguration. `openshell doctor` prüft die Umgebung.

## Die wichtigsten Funktionen

| Bereich | Was OpenShell übernimmt |
|---|---|
| Bausteine | Frontend, Backend, eingebaute SQLite-Datenbank und KI-Funktionen (Skripte) frei kombinierbar: `openshell create --with frontend,database`, `openshell add function …` – z. B. eine Oberfläche ohne Backend oder eine reine KI-Schnittstelle ohne Oberfläche |
| Runtime | Start in Abhängigkeitsreihenfolge, Health Checks, Neustarts mit Backoff, Logs, dynamische Ports, Build-Schritte für C++/Java, CPU/RAM pro Service |
| Desktop | Fenster, Ladebildschirm, Runtime-Panel, Tray, Benachrichtigungen, Rechtsklick auf die Titelleiste zur Service-Überwachung |
| KI-Agenten | MCP mit zwei Profilen: **App** (nur die Funktionen der App – zum Benutzen) und **Full** (32 Werkzeuge – zum Entwickeln), OpenAPI-Import, UI-Automatisierung, Berechtigungen, Freigaben, Audit-Log |
| Auslieferung | `openshell package`: Installer mit Name und Icon der App; installierte Apps sind selbst MCP-Server (`meine-app.exe --mcp`) |

## Mit einem KI-Coding-Agenten eine neue App bauen

Das Repository ist für KI-Harnesses vorbereitet: [`AGENTS.md`](AGENTS.md), [`CLAUDE.md`](CLAUDE.md),
[`llms.txt`](llms.txt) und der Leitfaden [`docs/ai/app-development.md`](docs/ai/app-development.md) (auch per
`openshell guide`). Jedes neue Projekt bekommt eine eigene `AGENTS.md`. Ein Auftrag wie

> *Baue mir mit OpenShell eine Desktop-App zur Rezeptverwaltung mit React-Frontend und Python-Backend. KI-Agenten
> sollen Rezepte anlegen und suchen können.*

genügt – der Agent findet Vertrag, Templates, Prüfbefehle und Paketierung selbst.

## Status

Alpha (0.1), unter Windows 11 durchgängig getestet; macOS und Linux werden in der CI gebaut. Details, Dokumentation und
Mitarbeit: siehe die [englische README](README.md).

Lizenz: [MIT](LICENSE)
