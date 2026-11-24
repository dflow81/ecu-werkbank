-- Service-Rückstellung (Inspektion/Ölwechsel) je Kombi-Teilenummer:
-- Kennungen, Umrechnung und Rückstellwerte als JSON (service::Config).
CREATE TABLE ecu_service_config (
    ecu_id INTEGER PRIMARY KEY REFERENCES ecu (id) ON DELETE CASCADE,
    content TEXT NOT NULL
);
