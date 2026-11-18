-- ECUWerkbank Fahrzeug-Datenbank, Schema-Version 1.
-- Plan und Begründungen: ROADMAP.md, „Plan: Fahrzeug-Datenbank“.
--
-- Jede Wissenstabelle hat source_id → data_source. Wird eine Quelle entfernt,
-- verschwinden ihre Zeilen mit (ON DELETE CASCADE). Hersteller und Modelle
-- bleiben stehen (SET NULL), weil sie oft von mehreren Quellen genutzt werden.

CREATE TABLE data_source (
    id          INTEGER PRIMARY KEY,
    source_type TEXT NOT NULL CHECK (source_type IN ('COMMUNITY', 'ODX', 'PDX', 'DBC', 'USER', 'MANUAL')),
    source_name TEXT NOT NULL,
    version     TEXT NOT NULL DEFAULT '',
    license     TEXT NOT NULL DEFAULT '',
    -- woher genau (Datei, Mitschnitt, Datum) — für Nachvollziehbarkeit
    origin      TEXT NOT NULL DEFAULT '',
    imported_at TEXT NOT NULL
);

CREATE TABLE manufacturer (
    id                  INTEGER PRIMARY KEY,
    name                TEXT NOT NULL UNIQUE,
    country             TEXT NOT NULL DEFAULT '',
    brand_group         TEXT NOT NULL DEFAULT '',
    logo                TEXT NOT NULL DEFAULT '',
    -- kommagetrennt, z. B. "UDS,TP20,OBD"
    supported_protocols TEXT NOT NULL DEFAULT '',
    -- erste drei Zeichen der Fahrgestellnummer, kommagetrennt (WVW,WV1,…)
    vin_wmi             TEXT NOT NULL DEFAULT '',
    source_id           INTEGER REFERENCES data_source (id) ON DELETE SET NULL,
    created_at          TEXT NOT NULL,
    updated_at          TEXT NOT NULL
);

CREATE TABLE vehicle_model (
    id               INTEGER PRIMARY KEY,
    manufacturer_id  INTEGER NOT NULL REFERENCES manufacturer (id) ON DELETE CASCADE,
    model_name       TEXT NOT NULL,
    platform         TEXT NOT NULL DEFAULT '',
    generation       TEXT NOT NULL DEFAULT '',
    production_start INTEGER,
    production_end   INTEGER,
    source_id        INTEGER REFERENCES data_source (id) ON DELETE SET NULL,
    UNIQUE (manufacturer_id, model_name, generation)
);

CREATE TABLE vehicle_variant (
    id               INTEGER PRIMARY KEY,
    vehicle_model_id INTEGER NOT NULL REFERENCES vehicle_model (id) ON DELETE CASCADE,
    engine_code      TEXT NOT NULL DEFAULT '',
    gearbox_code     TEXT NOT NULL DEFAULT '',
    drive_type       TEXT NOT NULL DEFAULT '',
    fuel_type        TEXT NOT NULL DEFAULT '',
    -- Muster wie "WVWZZZ1K?A*" (? = ein Zeichen, * = Rest)
    vin_pattern      TEXT NOT NULL DEFAULT '',
    source_id        INTEGER REFERENCES data_source (id) ON DELETE SET NULL
);

CREATE TABLE ecu (
    id                 INTEGER PRIMARY KEY,
    manufacturer_id    INTEGER REFERENCES manufacturer (id) ON DELETE CASCADE,
    ecu_name           TEXT NOT NULL,
    ecu_family         TEXT NOT NULL DEFAULT '',
    supplier           TEXT NOT NULL DEFAULT '',
    hardware_number    TEXT NOT NULL DEFAULT '',
    software_number    TEXT NOT NULL DEFAULT '',
    -- UDS | TP20 | KWP1281 | OBD
    protocol           TEXT NOT NULL DEFAULT '',
    -- Diagnoseadresse (VW: 0x01 Motor, 0x09 Bordnetz …)
    diagnostic_address INTEGER,
    can_tx             INTEGER,
    can_rx             INTEGER,
    coding_did         INTEGER,
    source_id          INTEGER NOT NULL REFERENCES data_source (id) ON DELETE CASCADE
);
CREATE INDEX ecu_adresse ON ecu (manufacturer_id, diagnostic_address);

-- welches Steuergerät steckt in welcher Variante (viele zu viele)
CREATE TABLE vehicle_ecu (
    vehicle_variant_id INTEGER NOT NULL REFERENCES vehicle_variant (id) ON DELETE CASCADE,
    ecu_id             INTEGER NOT NULL REFERENCES ecu (id) ON DELETE CASCADE,
    PRIMARY KEY (vehicle_variant_id, ecu_id)
);

CREATE TABLE did (
    id           INTEGER PRIMARY KEY,
    ecu_id       INTEGER REFERENCES ecu (id) ON DELETE CASCADE,
    did          INTEGER NOT NULL,
    name         TEXT NOT NULL,
    description  TEXT NOT NULL DEFAULT '',
    datatype     TEXT NOT NULL DEFAULT '',
    unit         TEXT NOT NULL DEFAULT '',
    factor       REAL NOT NULL DEFAULT 1.0,
    "offset"     REAL NOT NULL DEFAULT 0.0,
    access_level TEXT NOT NULL DEFAULT '',
    source_id    INTEGER NOT NULL REFERENCES data_source (id) ON DELETE CASCADE
);
CREATE INDEX did_ecu ON did (ecu_id, did);
CREATE INDEX did_wert ON did (did);

CREATE TABLE dtc (
    id                    INTEGER PRIMARY KEY,
    ecu_id                INTEGER REFERENCES ecu (id) ON DELETE CASCADE,
    dtc_code              TEXT NOT NULL,
    title                 TEXT NOT NULL,
    description           TEXT NOT NULL DEFAULT '',
    severity              TEXT NOT NULL DEFAULT '',
    category              TEXT NOT NULL DEFAULT '',
    manufacturer_specific INTEGER NOT NULL DEFAULT 0,
    source_id             INTEGER NOT NULL REFERENCES data_source (id) ON DELETE CASCADE
);
CREATE INDEX dtc_code ON dtc (dtc_code);

-- Normdaten (ISO 14229), gehören keinem Hersteller
CREATE TABLE uds_service (
    id              INTEGER PRIMARY KEY,
    service_id      INTEGER NOT NULL UNIQUE,
    name            TEXT NOT NULL,
    description     TEXT NOT NULL DEFAULT '',
    request_format  TEXT NOT NULL DEFAULT '',
    response_format TEXT NOT NULL DEFAULT ''
);

CREATE TABLE diagnostic_session (
    id                      INTEGER PRIMARY KEY,
    session_id              INTEGER NOT NULL UNIQUE,
    name                    TEXT NOT NULL,
    required_security_level INTEGER NOT NULL DEFAULT 0
);

-- Nur Beschreibung. KEINE Schlüssel-Verfahren von Herstellern.
CREATE TABLE security_access (
    id          INTEGER PRIMARY KEY,
    ecu_id      INTEGER NOT NULL REFERENCES ecu (id) ON DELETE CASCADE,
    level       INTEGER NOT NULL,
    seed_length INTEGER,
    key_length  INTEGER,
    notes       TEXT NOT NULL DEFAULT '',
    source_id   INTEGER NOT NULL REFERENCES data_source (id) ON DELETE CASCADE
);

CREATE TABLE routine (
    id          INTEGER PRIMARY KEY,
    ecu_id      INTEGER REFERENCES ecu (id) ON DELETE CASCADE,
    routine_id  INTEGER NOT NULL,
    name        TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    -- JSON
    parameters  TEXT NOT NULL DEFAULT '',
    source_id   INTEGER NOT NULL REFERENCES data_source (id) ON DELETE CASCADE
);

CREATE TABLE coding_item (
    id            INTEGER PRIMARY KEY,
    ecu_id        INTEGER NOT NULL REFERENCES ecu (id) ON DELETE CASCADE,
    -- LANG | KURZ
    coding_type   TEXT NOT NULL DEFAULT 'LANG',
    byte_index    INTEGER NOT NULL,
    bit_index     INTEGER,
    label         TEXT NOT NULL,
    description   TEXT NOT NULL DEFAULT '',
    default_value TEXT NOT NULL DEFAULT '',
    source_id     INTEGER NOT NULL REFERENCES data_source (id) ON DELETE CASCADE
);
CREATE INDEX coding_ecu ON coding_item (ecu_id);

-- Anpassungskanäle (TP 2.0 Kanal, UDS Kennung)
CREATE TABLE adaptation (
    id          INTEGER PRIMARY KEY,
    ecu_id      INTEGER NOT NULL REFERENCES ecu (id) ON DELETE CASCADE,
    channel     INTEGER NOT NULL,
    name        TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    source_id   INTEGER NOT NULL REFERENCES data_source (id) ON DELETE CASCADE
);

-- Messwerte: OBD-PID, UDS-DID oder TP-2.0-Messwertblock + Feld
CREATE TABLE measurement (
    id                 INTEGER PRIMARY KEY,
    ecu_id             INTEGER REFERENCES ecu (id) ON DELETE CASCADE,
    -- OBD | UDS | KWP
    kind               TEXT NOT NULL,
    pid                INTEGER NOT NULL,
    field              INTEGER,
    name               TEXT NOT NULL,
    unit               TEXT NOT NULL DEFAULT '',
    datatype           TEXT NOT NULL DEFAULT '',
    factor             REAL NOT NULL DEFAULT 1.0,
    "offset"           REAL NOT NULL DEFAULT 0.0,
    -- frei, z. B. "x*0.25" (nur Anzeige; gerechnet wird mit factor/offset)
    conversion_formula TEXT NOT NULL DEFAULT '',
    source_id          INTEGER NOT NULL REFERENCES data_source (id) ON DELETE CASCADE
);
CREATE INDEX measurement_ecu ON measurement (ecu_id, kind, pid);

-- Nur Metadaten und Prüfsumme. Flash-Dateien liefert das Projekt nie.
CREATE TABLE flash_container (
    id                 INTEGER PRIMARY KEY,
    ecu_id             INTEGER NOT NULL REFERENCES ecu (id) ON DELETE CASCADE,
    version            TEXT NOT NULL,
    checksum           TEXT NOT NULL DEFAULT '',
    hardware_reference TEXT NOT NULL DEFAULT '',
    software_reference TEXT NOT NULL DEFAULT '',
    source_id          INTEGER NOT NULL REFERENCES data_source (id) ON DELETE CASCADE
);

CREATE TABLE can_message (
    id              INTEGER PRIMARY KEY,
    manufacturer_id INTEGER REFERENCES manufacturer (id) ON DELETE CASCADE,
    can_id          INTEGER NOT NULL,
    extended        INTEGER NOT NULL DEFAULT 0,
    name            TEXT NOT NULL DEFAULT '',
    dlc             INTEGER NOT NULL DEFAULT 8,
    cycle_time      INTEGER,
    source_id       INTEGER NOT NULL REFERENCES data_source (id) ON DELETE CASCADE
);
CREATE INDEX can_message_id ON can_message (manufacturer_id, can_id);

CREATE TABLE can_signal (
    id             INTEGER PRIMARY KEY,
    can_message_id INTEGER NOT NULL REFERENCES can_message (id) ON DELETE CASCADE,
    signal_name    TEXT NOT NULL,
    start_bit      INTEGER NOT NULL,
    bit_length     INTEGER NOT NULL,
    -- 1 = Intel (little endian), 0 = Motorola
    little_endian  INTEGER NOT NULL DEFAULT 1,
    is_signed      INTEGER NOT NULL DEFAULT 0,
    factor         REAL NOT NULL DEFAULT 1.0,
    "offset"       REAL NOT NULL DEFAULT 0.0,
    unit           TEXT NOT NULL DEFAULT '',
    -- Wertetexte als JSON {"0":"aus","1":"an"}
    value_table    TEXT NOT NULL DEFAULT ''
);
CREATE INDEX can_signal_message ON can_signal (can_message_id);

-- Übersetzungen: schluessel z. B. "dtc:P0301", sprache "de"/"en"
CREATE TABLE text (
    schluessel TEXT NOT NULL,
    sprache    TEXT NOT NULL,
    inhalt     TEXT NOT NULL,
    source_id  INTEGER REFERENCES data_source (id) ON DELETE CASCADE,
    PRIMARY KEY (schluessel, sprache)
);

CREATE TABLE permission (
    id             INTEGER PRIMARY KEY,
    role           TEXT NOT NULL UNIQUE,
    "read"         INTEGER NOT NULL DEFAULT 1,
    "write"        INTEGER NOT NULL DEFAULT 0,
    flash          INTEGER NOT NULL DEFAULT 0,
    coding         INTEGER NOT NULL DEFAULT 0,
    administration INTEGER NOT NULL DEFAULT 0
);
INSERT INTO permission (role, "read", "write", flash, coding, administration) VALUES
    ('lesen',     1, 0, 0, 0, 0),
    ('werkstatt', 1, 1, 0, 1, 0),
    ('verwalten', 1, 1, 0, 1, 1);
