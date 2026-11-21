-- Schema-Version 5: Fehlerarten (was mit einem Bauteil los ist: „defekt“,
-- „Kurzschluss nach Plus“ …). Die Nummern hängen vom Protokoll ab:
--   TP20   = KWP2000 über TP 2.0, untere 4 Bit des Statusbytes (0–15)
--   UDS    = 3. Byte des Fehlers („Failure Type Byte“, genormt)
--   KW1281 = alte K-Line-Steuergeräte (eigene Nummern)
-- Eigene Einträge haben Vorrang vor den eingebauten Texten.
CREATE TABLE fault_type (
    id        INTEGER PRIMARY KEY,
    protocol  TEXT NOT NULL,
    nr        INTEGER NOT NULL,
    text      TEXT NOT NULL,
    source_id INTEGER NOT NULL REFERENCES data_source (id) ON DELETE CASCADE
);
CREATE INDEX fault_type_nr ON fault_type (protocol, nr);
