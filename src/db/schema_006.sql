ALTER TABLE data_source ADD COLUMN local_only INTEGER NOT NULL DEFAULT 0 CHECK (local_only IN (0, 1));

CREATE TABLE ecu_diagnostic_reference (
    ecu_id INTEGER PRIMARY KEY REFERENCES ecu (id) ON DELETE CASCADE,
    content TEXT NOT NULL
);

CREATE TABLE dtc_alternative (
    source_id INTEGER NOT NULL REFERENCES data_source (id) ON DELETE CASCADE,
    code TEXT NOT NULL,
    text TEXT NOT NULL,
    PRIMARY KEY (source_id, code, text)
);
CREATE INDEX dtc_alternative_code ON dtc_alternative (code);

CREATE TABLE dtc_combination (
    source_id INTEGER NOT NULL REFERENCES data_source (id) ON DELETE CASCADE,
    code TEXT NOT NULL,
    failure_type INTEGER NOT NULL CHECK (failure_type BETWEEN 0 AND 255),
    groups_count INTEGER NOT NULL CHECK (groups_count > 0),
    PRIMARY KEY (source_id, code, failure_type)
);
CREATE INDEX dtc_combination_code ON dtc_combination (code, failure_type);
