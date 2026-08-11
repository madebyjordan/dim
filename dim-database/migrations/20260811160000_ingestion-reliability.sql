-- Durable, inspectable ingestion state. Existing catalog rows remain valid and are not
-- assigned provider identities that cannot be proven from historical data.
CREATE TABLE ingestion_scan (
    id INTEGER PRIMARY KEY NOT NULL,
    library_id INTEGER NOT NULL,
    kind TEXT NOT NULL DEFAULT 'full',
    status TEXT NOT NULL DEFAULT 'queued',
    requested_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    started_at TEXT,
    finished_at TEXT,
    discovered INTEGER NOT NULL DEFAULT 0,
    committed INTEGER NOT NULL DEFAULT 0,
    failed INTEGER NOT NULL DEFAULT 0,
    skipped INTEGER NOT NULL DEFAULT 0,
    error TEXT,
    FOREIGN KEY (library_id) REFERENCES library(id) ON DELETE CASCADE,
    CHECK (kind IN ('full', 'watcher', 'recovery')),
    CHECK (status IN ('queued', 'running', 'complete', 'failed', 'cancelled'))
);

CREATE INDEX ingestion_scan_library_status_idx
    ON ingestion_scan(library_id, status, id DESC);

CREATE TABLE ingestion_item (
    id INTEGER PRIMARY KEY NOT NULL,
    scan_id INTEGER NOT NULL,
    library_id INTEGER NOT NULL,
    path TEXT NOT NULL,
    fingerprint TEXT,
    stage TEXT NOT NULL DEFAULT 'discovery',
    status TEXT NOT NULL DEFAULT 'pending',
    attempts INTEGER NOT NULL DEFAULT 0,
    error_class TEXT,
    error_message TEXT,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE(scan_id, path),
    FOREIGN KEY (scan_id) REFERENCES ingestion_scan(id) ON DELETE CASCADE,
    FOREIGN KEY (library_id) REFERENCES library(id) ON DELETE CASCADE,
    CHECK (stage IN ('discovery', 'stability', 'probing', 'matching', 'artwork', 'commit')),
    CHECK (status IN ('pending', 'running', 'retryable', 'complete', 'skipped', 'failed'))
);

CREATE INDEX ingestion_item_recovery_idx
    ON ingestion_item(library_id, status, stage);

ALTER TABLE mediafile ADD COLUMN file_size INTEGER;
ALTER TABLE mediafile ADD COLUMN modified_ns INTEGER;
ALTER TABLE mediafile ADD COLUMN metadata_provider TEXT;
ALTER TABLE mediafile ADD COLUMN provider_external_id TEXT;
ALTER TABLE mediafile ADD COLUMN match_provenance TEXT;
ALTER TABLE mediafile ADD COLUMN match_confidence REAL;
ALTER TABLE mediafile ADD COLUMN manual_override BOOLEAN NOT NULL DEFAULT 0;
ALTER TABLE mediafile ADD COLUMN missing_since TEXT;

ALTER TABLE assets ADD COLUMN download_status TEXT NOT NULL DEFAULT 'pending';
ALTER TABLE assets ADD COLUMN download_error TEXT;
ALTER TABLE assets ADD COLUMN downloaded_at TEXT;
ALTER TABLE assets ADD COLUMN orphaned_at TEXT;
