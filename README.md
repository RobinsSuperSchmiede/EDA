# EDA
# Architekturkonzept: Desktop App als zentraler Manager

## 1. Problem

Eine Anwendung besteht aus mehreren Komponenten, die unabhängig voneinander entwickelt und verändert werden können.

Werden Backend, Frontend und Datenbank fest in die Desktop-App integriert, entstehen mehrere Probleme:

- Änderungen am Backend oder Frontend erfordern möglicherweise einen erneuten Build der Desktop-App.
- AI-Agents und Entwickler können die Anwendung nicht ohne Weiteres zur Laufzeit erweitern.
- Die einzelnen Komponenten sind nicht mehr sauber voneinander getrennt.
- Updates und Erweiterungen werden unnötig kompliziert.
- Die Anwendung wird weniger flexibel und schwieriger zu warten.

**Ziel:** Die Anwendung soll wie eine normale Desktop-Software funktionieren, gleichzeitig aber vollständig modular, unabhängig und erweiterbar bleiben.

## 2. Lösung: Desktop App als zentraler Manager

Die Desktop-App übernimmt ausschließlich die Steuerung und Verwaltung der Anwendung.

Sie ist der zentrale Einstiegspunkt, enthält aber nicht die eigentliche Anwendungslogik.

Alle wesentlichen Komponenten bleiben eigenständige, externe Projekte:

- **Desktop App:** Zentrale Steuerung, Prozessverwaltung und Benutzerzugang.
- **Backend:** Eigenständiger Server mit API und Anwendungslogik.
- **Frontend:** Eigenständige Webanwendung.
- **Datenbank:** Separater Daten- und Persistenzlayer.

Die Desktop-App verwaltet diese Komponenten und sorgt dafür, dass sie korrekt gestartet, überwacht und beendet werden.

## 3. Architektur

```text
                 USER
                   |
                   v
        +----------------------+
        |     DESKTOP APP      |
        |                      |
        |    Central Manager   |
        |                      |
        |  - Runtime Manager   |
        |  - Process Manager   |
        |  - Health Checks     |
        |  - Lifecycle Manager |
        |  - System Tray       |
        |                      |
        |  +----------------+  |
        |  |  WebView       |  |
        |  |  Frontend UI   |  |
        |  +----------------+  |
        +----------------------+
                   |
          Manages Components
                   |
       +-----------+-----------+
       |           |           |
       v           v           v
  +---------+ +---------+ +----------+
  |Frontend | | Backend | | Database |
  |         | |         | |          |
  | Web UI  | | API     | | Storage  |
  |         | | Logic   | |          |
  +---------+ +---------+ +----------+
                   |
                   v
             Communication
              via API
```

## 4. Funktionsweise

Beim Start der Desktop-App:

1. Die Desktop-App wird gestartet.
2. Sie überprüft die vorhandenen Komponenten und deren Status.
3. Das Backend wird gestartet.
4. Die Datenbank wird bereitgestellt bzw. verbunden.
5. Das Frontend wird gestartet und bereitgestellt.
6. Die Desktop-App lädt das Frontend direkt in ihren integrierten WebView.
7. Die Anwendung ist vollständig einsatzbereit.

Der Nutzer muss keine Server, Prozesse oder URLs manuell verwalten.

## 5. Trennung und Veränderbarkeit

Alle Komponenten bleiben unabhängig voneinander.

Das bedeutet:

- Frontend kann ohne Build der Desktop-App verändert werden.
- Backend kann unabhängig weiterentwickelt werden.
- Datenbank und Daten bleiben außerhalb der Desktop-App.
- Neue Funktionen können direkt in den jeweiligen Komponenten implementiert werden.
- AI-Agents können direkt auf die Projektdateien zugreifen.
- Änderungen können automatisch durch Reload oder Neustart übernommen werden.

Die Desktop-App selbst muss nur dann neu gebaut werden, wenn ihre eigene Funktionalität verändert wird.

## 6. Betriebsmodi

Die Architektur unterstützt verschiedene Nutzungsszenarien:

| Modus | Beschreibung |
|---|---|
| Normal Mode | Desktop-App startet alle Komponenten und zeigt das Frontend direkt an. |
| Developer Mode | Entwickler und AI-Agents können alle externen Komponenten direkt verändern. |
| Headless Mode | Desktop-App läuft im Hintergrund und verwaltet die Komponenten ohne eigene sichtbare Oberfläche. |
| Browser Mode | Frontend kann unabhängig von der Desktop-App über eine lokale oder freigegebene Netzwerkadresse aufgerufen werden. |

## 7. Grundprinzipien

Die Architektur basiert auf fünf zentralen Prinzipien:

1. **Separation of Concerns:** Jede Komponente hat eine klar definierte Aufgabe.
2. **Modularität:** Backend, Frontend und Datenbank sind unabhängig.
3. **Runtime Management:** Die Desktop-App verwaltet den Lebenszyklus aller Komponenten.
4. **AI-Readiness:** Entwickler und AI-Agents können die Anwendung direkt erweitern.
5. **User Experience:** Die technische Komplexität bleibt für normale Nutzer vollständig verborgen.

---

### Zentrale Architekturentscheidung

**Die Desktop-App ist nicht die Anwendung selbst, sondern der Manager der Anwendung.**

Sie stellt den Einstiegspunkt und die native Benutzerumgebung bereit, während das eigentliche Produkt durch externe, unabhängig veränderbare Komponenten gebildet wird.

Das Frontend wird direkt in die Desktop-App eingebettet, sodass der Nutzer eine normale Desktop-Anwendung erlebt, obwohl die eigentliche Benutzeroberfläche vollständig unabhängig entwickelt und bereitgestellt wird.
