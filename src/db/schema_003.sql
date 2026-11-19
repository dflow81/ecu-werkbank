-- Schema-Version 3: Datenbank als einziger Speicher für eigenes Wissen (Weg A, 19.11.2026).
--
-- Eigene Texte (früher je eine TOML-Datei im Datenordner):
--   bereich = fehler | messwert | codier | kennung | wert
CREATE TABLE eigener_text (
    bereich    TEXT NOT NULL,
    schluessel TEXT NOT NULL,
    text       TEXT NOT NULL,
    geaendert  TEXT NOT NULL DEFAULT '',
    PRIMARY KEY (bereich, schluessel)
);

-- Welche alten Dateien schon übernommen wurden (nur einmal übernehmen).
CREATE TABLE uebernahme (
    datei     TEXT PRIMARY KEY,
    zeitpunkt TEXT NOT NULL
);

-- Eigene Labels: was die Label-Dateien zusätzlich kannten.
CREATE TABLE sollwert (
    id        INTEGER PRIMARY KEY,
    ecu_id    INTEGER REFERENCES ecu (id) ON DELETE CASCADE,
    -- kwp | uds | obd
    quelle    TEXT NOT NULL,
    kennung   INTEGER NOT NULL,
    feld      INTEGER,
    min       REAL,
    max       REAL,
    name      TEXT NOT NULL DEFAULT '',
    source_id INTEGER NOT NULL REFERENCES data_source (id) ON DELETE CASCADE
);

-- Stellfunktionen (Ablauf als TOML-Text, wie in der Label-Datei)
CREATE TABLE stellfunktion (
    id        INTEGER PRIMARY KEY,
    ecu_id    INTEGER REFERENCES ecu (id) ON DELETE CASCADE,
    inhalt    TEXT NOT NULL,
    source_id INTEGER NOT NULL REFERENCES data_source (id) ON DELETE CASCADE
);
