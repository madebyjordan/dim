-- A full scan can establish authority for one configured root even when another root fails.
-- Keep those outcomes durable so a partial failure remains diagnosable after the worker exits.
CREATE TABLE ingestion_scan_root (
    id INTEGER PRIMARY KEY NOT NULL,
    scan_id INTEGER NOT NULL,
    ordinal INTEGER NOT NULL,
    root TEXT NOT NULL,
    normalized_root TEXT,
    status TEXT NOT NULL DEFAULT 'running',
    discovered INTEGER NOT NULL DEFAULT 0,
    error TEXT,
    finished_at TEXT,
    FOREIGN KEY (scan_id) REFERENCES ingestion_scan(id) ON DELETE CASCADE,
    UNIQUE(scan_id, ordinal),
    CHECK (status IN ('running', 'authoritative', 'missing', 'failed', 'cancelled'))
);

CREATE INDEX ingestion_scan_root_scan_status_idx
    ON ingestion_scan_root(scan_id, status);

ALTER TABLE ingestion_item ADD COLUMN root_id INTEGER REFERENCES ingestion_scan_root(id);

CREATE INDEX ingestion_item_scan_root_idx
    ON ingestion_item(scan_id, root_id);
