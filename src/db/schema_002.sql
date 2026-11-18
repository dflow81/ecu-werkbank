-- Schema-Version 2: Abhängigkeiten zwischen Codier-Bits (Roadmap 29).
-- Kommagetrennt "Byte.Bit", z. B. "1.3,2.0".
ALTER TABLE coding_item ADD COLUMN requires TEXT NOT NULL DEFAULT '';
ALTER TABLE coding_item ADD COLUMN excludes TEXT NOT NULL DEFAULT '';
