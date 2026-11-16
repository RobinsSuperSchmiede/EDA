# Project: Agent-Native Desktop Runtime

## 1. Deine Aufgabe

Du bist der leitende Softwarearchitekt und Entwickler dieses Open-Source-Projekts.

Deine Aufgabe ist es, eine universelle, modulare und erweiterbare Desktop-Shell zu entwickeln, mit der Entwickler beliebige Webanwendungen in native Desktop-Anwendungen verwandeln können.

Das Besondere: Die Shell soll nicht nur für Menschen optimiert sein, sondern auch eine standardisierte Infrastruktur bieten, über die AI-Agents Anwendungen bedienen, verwalten, automatisieren und weiterentwickeln können.

Das Projekt soll Open Source sein und langfristig von einer Community verwendet und erweitert werden können.

Arbeite eigenständig, triff sinnvolle technische Entscheidungen und beginne nach der Architekturplanung unmittelbar mit der Implementierung.

Stelle nur dann Rückfragen, wenn eine wichtige technische Entscheidung ohne weitere Informationen nicht sinnvoll getroffen werden kann.

---

# 2. Grundlegende Vision

Wir entwickeln eine universelle Desktop-Shell, die als zentrale Steuerungs- und Managementebene für externe Anwendungen dient.

Die Shell selbst enthält möglichst wenig Anwendungslogik.

Stattdessen verwaltet sie eigenständige Komponenten:

- Frontend
- Backend
- Datenbank
- Weitere Services
- Extensions
- AI-Agents

Das Frontend wird direkt in einem integrierten WebView innerhalb der Desktop-Anwendung dargestellt.

Die eigentlichen Komponenten bleiben außerhalb der Desktop-Binary und können unabhängig voneinander verändert, aktualisiert und weiterentwickelt werden.

Die Shell soll als fertige Infrastruktur für andere Entwickler dienen.

Entwickler sollen keine eigene Rust-Desktop-Anwendung und keine eigene Prozessverwaltung schreiben müssen.

Sie sollen lediglich ihre Anwendung konfigurieren und die Shell verwenden können.

## Zentrale Philosophie

Die Desktop-Shell ist nicht die Anwendung selbst.

Sie ist der Manager, die native Benutzeroberfläche und die Runtime für die eigentliche Anwendung.

Alle wesentlichen Komponenten bleiben unabhängig und austauschbar.

---

# 3. Architektur

Die Architektur soll aus folgenden Hauptkomponenten bestehen:

## A. Desktop Core

Technologie: Rust + Tauri 2.

Verantwortlichkeiten:

- Native Desktop-Fenster
- Eingebetteter WebView
- Fensterverwaltung
- System Tray
- Minimieren und Schließen
- Autostart
- Native Notifications
- Plattformintegration
- Kommunikation mit der Runtime
- Verwaltung des Anwendungslebenszyklus

Die Desktop-Shell soll möglichst unabhängig vom verwendeten Frontend-Framework sein.

React, Vue, Svelte und andere Webtechnologien sollen unterstützt werden.

## B. Runtime Manager

Das Herzstück des Projekts.

Der Runtime Manager verwaltet alle externen Komponenten einer Anwendung.

Funktionen:

- Services starten und stoppen
- Prozessüberwachung
- Automatischer Neustart bei Abstürzen
- Health Checks
- Abhängigkeitsverwaltung
- Startup-Reihenfolge
- Shutdown-Verhalten
- Dynamische Portverwaltung
- Prozessisolation
- Logs sammeln
- Runtime-Status bereitstellen

Es dürfen keine bestimmten Programmiersprachen vorausgesetzt werden.

Unterstützt werden sollen beliebige ausführbare Prozesse, beispielsweise:

- Python
- Node.js
- Go
- Rust
- Andere native Binaries

Bereits extern laufende Services müssen ebenfalls eingebunden werden können.

## C. Frontend Manager

Das Frontend ist eine eigenständige Webanwendung.

Die Shell lädt dieses Frontend direkt in ihren WebView.

Unterstützte Betriebsarten:

- Development Server
- Produktions-Build
- Lokaler Webserver
- Externe URL

Funktionen:

- Frontend starten
- Frontend-Status überwachen
- Automatisches Reload
- Hot Reload im Development Mode
- Fehlererkennung
- Frontend neu laden

Die Shell darf keine Abhängigkeit von einem bestimmten Frontend-Framework haben.

## D. Service Manager

Verwaltet zusätzliche externe Komponenten.

Beispiele:

- Backend-Server
- Datenbanken
- Worker
- Automatisierungsdienste
- Lokale AI-Server
- Weitere Hintergrundprozesse

Es muss möglich sein, lokale Services zu starten und bereits vorhandene externe Services zu verbinden.

## E. Configuration System

Jede Anwendung soll über eine zentrale Konfigurationsdatei definiert werden.

Beispiel:

```json
{
  "app": {
    "name": "My Application",
    "version": "1.0.0"
  },
  "window": {
    "width": 1400,
    "height": 900
  },
  "frontend": {
    "type": "web",
    "command": "npm run dev",
    "url": "http://localhost:3000",
    "embedded": true
  },
  "services": [
    {
      "name": "backend",
      "command": "python backend/main.py",
      "healthcheck": "http://localhost:8000/health",
      "restart": true
    }
  ],
  "runtime": {
    "autostart": true,
    "background_mode": true
  }
}
```

Dies ist ein konzeptionelles Beispiel. Entwickle ein sauberes, versioniertes Konfigurationsschema.

Es muss Validierung, sinnvolle Fehlermeldungen und spätere Erweiterbarkeit unterstützen.

---

# 4. Neue Kernfunktion: Agent Interaction Layer

Dies ist ein besonders wichtiger Teil des Projekts.

Die Shell soll eine eigene standardisierte Ebene bereitstellen, über die AI-Agents nicht nur den Quellcode verändern, sondern auch die laufende Anwendung verwenden und bedienen können.

Das Projekt soll langfristig eine Art universelle Schnittstelle zwischen Desktop-Anwendungen und AI-Agents bereitstellen.

Die Agent Interaction Layer soll unabhängig vom verwendeten AI-Modell funktionieren.

Es dürfen keine bestimmten Anbieter vorausgesetzt werden.

Unterstützt werden sollen beispielsweise:

- Lokale AI-Modelle
- Ollama
- LM Studio
- Claude
- OpenAI
- Andere externe AI-Provider
- Coding-Agents
- MCP-kompatible Agents
- Eigene Agent-Implementierungen

Wichtig: Die Shell soll nicht selbst ein bestimmtes AI-Modell voraussetzen.

Sie stellt lediglich die Infrastruktur und Schnittstellen bereit.

## A. Agent Interface

Entwickle eine standardisierte Schnittstelle, über die Agents mit der Anwendung kommunizieren können.

Mögliche Funktionen:

- Anwendungsstatus abfragen
- Runtime-Status abfragen
- Services starten und stoppen
- Logs abrufen
- Frontend neu laden
- Anwendung navigieren
- Verfügbare Funktionen entdecken
- Native Desktop-Funktionen aufrufen
- Benutzeroberfläche bedienen
- Aktionen ausführen
- Ergebnisse zurückgeben

Die Schnittstelle soll maschinenlesbar sein und möglichst gut mit MCP zusammenarbeiten.

Untersuche, ob MCP als primäre Integrationsmöglichkeit oder als zusätzlicher Adapter sinnvoller ist.

## B. UI Interaction

Agents sollen die laufende Anwendung ähnlich wie ein menschlicher Nutzer verwenden können.

Entwickle dafür eine möglichst universelle Schnittstelle.

Mögliche Fähigkeiten:

- UI-Elemente erkennen
- Buttons identifizieren
- Formulare ausfüllen
- Navigation durchführen
- Texte lesen
- Aktionen ausführen
- UI-Zustände abfragen
- Screenshots erstellen
- Ergebnisse überprüfen

Bevorzuge semantische Schnittstellen wie Accessibility Tree, DOM oder strukturierte UI-Metadaten gegenüber rein visueller Interaktion.

Visuelle Interaktion über Screenshots und Maus-/Tastatursteuerung kann als zusätzliche Möglichkeit vorgesehen werden.

Die Lösung soll sowohl mit WebView-basierten Anwendungen als auch perspektivisch mit externen Browseroberflächen funktionieren.

## C. Application Capabilities

Die Anwendung soll selbst deklarieren können, welche Funktionen Agents zur Verfügung stehen.

Beispielsweise:

```json
{
  "capabilities": [
    {
      "name": "create_project",
      "description": "Create a new project",
      "permission": "write"
    },
    {
      "name": "get_dashboard_data",
      "description": "Retrieve dashboard information",
      "permission": "read"
    }
  ]
}
```

Entwickle ein Konzept, mit dem Anwendungen ihre Fähigkeiten registrieren und Agents diese automatisch entdecken können.

Die Anwendung soll nicht gezwungen sein, sämtliche Funktionen über UI-Interaktion zugänglich zu machen.

Eine direkte semantische API ist für Agents häufig zuverlässiger.

## D. Agent Management

Die Shell soll perspektivisch auch lokale und externe Agents verwalten können.

Mögliche Funktionen:

- Agents registrieren
- Agent-Verbindungen verwalten
- Agent-Status anzeigen
- Agent-Aufgaben übergeben
- Verfügbare Modelle erkennen
- Lokale AI-Provider verbinden
- Agent-Logs verwalten
- Berechtigungen verwalten

Wichtig: Agent-Management und Agent-Ausführung sollen modular bleiben.

Die Shell soll nicht zu einem eigenen AI-Agent-Framework werden.

Sie soll unterschiedliche bestehende Agent-Systeme integrieren können.

## E. Security

Sicherheit ist ein grundlegender Bestandteil.

Implementiere ein Berechtigungssystem mit:

- Read-only
- UI Interaction
- Service Management
- File Access
- Code Modification
- Native System Access

Kritische Aktionen müssen explizit freigegeben werden können.

Agents dürfen nicht automatisch uneingeschränkten Zugriff auf das Betriebssystem erhalten.

Unterstütze nachvollziehbare Logs und eine klare Trennung zwischen Development- und Production-Modus.

---

# 5. Developer Experience

Die Verwendung der Shell muss extrem einfach sein.

Ein Entwickler soll eine bestehende Webanwendung innerhalb weniger Minuten integrieren können.

## CLI

Entwickle eine CLI mit folgenden Befehlen:

```bash
openshell init
openshell create my-app
openshell dev
openshell start
openshell stop
openshell restart backend
openshell status
openshell logs
openshell doctor
```

Die Befehle sollen intuitiv und gut dokumentiert sein.

## Projektgenerator

Biete Templates für unterschiedliche Technologie-Stacks an.

Beispielsweise:

- React + Python
- React + Node.js
- Vue + Python
- Svelte + Go
- Custom Application

Die Templates dienen nur als Einstieg und dürfen die Shell selbst nicht einschränken.

## SDK

Entwickle ein optionales SDK für Frontend-Anwendungen.

Beispielsweise:

```typescript
import { OpenShell } from "@openshell/sdk";

await OpenShell.window.minimize();

const status = await OpenShell.runtime.status();

await OpenShell.runtime.restart("backend");
```

Das SDK soll Zugriff auf freigegebene native Funktionen und Runtime-Informationen bieten.

---

# 6. Betriebsmodi

Die Shell soll drei Hauptbetriebsmodi unterstützen.

## Development Mode

- Projektdateien direkt verwenden
- Frontend Hot Reload
- Backend-Neustarts
- Logs
- Direkter Zugriff für Coding-Agents
- Entwicklungswerkzeuge

## Production Mode

- Fertig konfigurierte Anwendung starten
- Automatischer Runtime-Start
- Eingebettetes Frontend
- Hintergrundbetrieb
- Stabiler Prozesslebenszyklus
- Updates

## Headless Mode

- Kein sichtbares Desktop-Fenster notwendig
- Services im Hintergrund verwalten
- Zugriff über Browser oder API
- Remote-Management optional

Alle Modi sollen möglichst dieselbe Runtime-Architektur verwenden.

---

# 7. Projektstruktur

Entwickle eine saubere Monorepo-Struktur.

Ein möglicher Aufbau:

```text
openshell/
│
├── apps/
│   └── desktop/
│       ├── src-tauri/
│       └── frontend/
│
├── packages/
│   ├── core/
│   ├── runtime/
│   ├── process-manager/
│   ├── service-manager/
│   ├── agent-layer/
│   ├── mcp-server/
│   ├── cli/
│   ├── sdk/
│   └── config/
│
├── templates/
│   ├── react-python/
│   ├── react-node/
│   └── custom/
│
├── examples/
│   ├── basic-app/
│   ├── backend-app/
│   └── agent-managed-app/
│
├── docs/
│
├── scripts/
│
├── LICENSE
├── README.md
└── CONTRIBUTING.md
```

Passe die Struktur an die tatsächliche technische Architektur an. Vermeide unnötige Abstraktionen.

---

# 8. Open-Source-Strategie

Das Projekt soll von Anfang an für externe Entwickler attraktiv sein.

Berücksichtige:

- MIT- oder Apache-2.0-Lizenz evaluieren
- Ausführliches README
- Getting Started Guide
- Architekturdiagramme
- API-Dokumentation
- CLI-Dokumentation
- Beispiele
- Contribution Guidelines
- Roadmap
- GitHub Discussions
- Issue Templates

Die Einstiegshürde muss möglichst niedrig sein.

Ein Entwickler sollte innerhalb von fünf Minuten eine funktionierende Beispielanwendung starten können.

---

# 9. Technische Grundentscheidungen

Bevorzuge:

| Bereich | Technologie |
|---|---|
| Desktop | Tauri 2 |
| Core | Rust |
| Runtime | Rust |
| Configuration | JSON / TOML |
| Frontend | Framework-unabhängig |
| Backend | Sprachunabhängig |
| Agent Interface | MCP + eigene APIs |
| CLI | Möglichst schlank |
| Communication | HTTP / WebSocket / IPC |

Prüfe die Eignung jeder Technologie kritisch.

Übernimm keine Technologie nur deshalb, weil sie hier vorgeschlagen wurde.

Achte auf Plattformunterstützung für Windows, macOS und Linux.

---

# 10. Entwicklungsphasen

Implementiere das Projekt schrittweise.

## Phase 1: Foundation

- Repository aufbauen
- Tauri Desktop Shell
- WebView
- Konfigurationssystem
- Einfaches Frontend laden

## Phase 2: Runtime Manager

- Prozesse starten
- Prozesse stoppen
- Health Checks
- Logs
- Automatischer Neustart
- Backend-Verwaltung

## Phase 3: Developer Experience

- CLI
- Projektgenerator
- Templates
- Development Mode
- Hot Reload

## Phase 4: Production

- Packaging
- Installer
- Runtime-Bundling
- Autostart
- System Tray
- Hintergrundbetrieb
- Updates

## Phase 5: Agent Interaction Layer

- Agent API
- MCP-Integration
- Capability Discovery
- UI Interaction
- Agent Permissions
- Lokale AI-Provider
- Agent Management

## Phase 6: Ecosystem

- SDK
- Erweiterungssystem
- Weitere Templates
- Dokumentation
- Beispielprojekte

---

# 11. Arbeitsweise und Erwartungen

Arbeite nach folgenden Regeln:

1. Analysiere zunächst die technischen Anforderungen.
2. Erstelle eine konkrete Architekturentscheidung.
3. Dokumentiere die Architektur in einer ARCHITECTURE.md.
4. Erstelle eine nachvollziehbare Roadmap.
5. Implementiere anschließend direkt die erste funktionsfähige Version.
6. Arbeite modular und vermeide unnötige Komplexität.
7. Schreibe Tests für kritische Runtime-Funktionen.
8. Dokumentiere alle öffentlichen Schnittstellen.
9. Halte die Shell unabhängig von bestimmten Frontend- und Backend-Technologien.
10. Berücksichtige AI-Agent-Kompatibilität von Anfang an.

Wichtig: Verliere dich nicht in Architekturplanung oder abstrakten Konzepten.

Baue zuerst einen funktionierenden vertikalen Prototyp:

- Desktop-Shell startet.
- Backend wird gestartet.
- Frontend wird geladen.
- Frontend erscheint im Desktop-WebView.
- Runtime kann den Backend-Prozess überwachen.
- Eine einfache CLI kann den Status abfragen.

Danach wird schrittweise erweitert.

Triff sinnvolle Entscheidungen selbstständig und dokumentiere sie.

Wenn eine vorgeschlagene Funktion für den MVP unnötig komplex ist, verschiebe sie in eine spätere Phase.

---

# 12. Definition of Success

Das Projekt ist erfolgreich, wenn ein externer Entwickler Folgendes machen kann:

1. OpenShell installieren.
2. Ein bestehendes Webprojekt integrieren.
3. Eine Konfigurationsdatei erstellen.
4. OpenShell starten.
5. Die Webanwendung erscheint automatisch in einem nativen Desktop-Fenster.
6. Das Backend läuft automatisch im Hintergrund.
7. Die Runtime überwacht alle Services.
8. Ein AI-Agent kann über die standardisierte Schnittstelle mit der Anwendung interagieren.
9. Ein Coding-Agent kann den Quellcode verändern.
10. Änderungen werden übernommen, ohne die Desktop-Shell neu bauen zu müssen.

Das langfristige Ziel:

Eine universelle Open-Source-Infrastruktur, mit der Entwickler Desktop-Anwendungen bauen können, die sowohl von Menschen als auch von AI-Agents verwendet, verwaltet und erweitert werden können.

**Build the shell once. Build applications on top of it. Let agents use and evolve them.**
