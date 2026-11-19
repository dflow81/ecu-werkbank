-- Schema-Version 4: Ablage — alle Daten-Dateien des Datenordners (Fahrzeuge,
-- Scans, Sicherungen, Einstellungen) als Einträge, Schlüssel = Pfad relativ
-- zum Datenordner („fahrzeuge/WVW…/historie/1795….toml“). Dokumente (PDF, CSV,
-- Mitschnitte) bleiben Dateien.
CREATE TABLE ablage (
    pfad      TEXT PRIMARY KEY,
    inhalt    TEXT NOT NULL,
    geaendert TEXT NOT NULL DEFAULT ''
);
