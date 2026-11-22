-- First migration of the app database. OpenShell applies every *.sql file in this folder once,
-- in file-name order (001_…, 002_…). Add new files for schema changes; never edit applied ones.
CREATE TABLE notes (
    id         INTEGER PRIMARY KEY,
    title      TEXT    NOT NULL,
    done       INTEGER NOT NULL DEFAULT 0,
    created_at TEXT    NOT NULL DEFAULT CURRENT_TIMESTAMP
);
