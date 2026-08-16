-- A scan's aggregate counts are useful only when its durable lifecycle also says what it is
-- doing and when the owning worker last demonstrated liveness. Heartbeats are deliberately
-- coalesced by the scanner; this table stores the latest observation, not a heartbeat history.
ALTER TABLE ingestion_scan ADD COLUMN stage TEXT NOT NULL DEFAULT 'queued';
ALTER TABLE ingestion_scan ADD COLUMN last_progress_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP;
ALTER TABLE ingestion_scan ADD COLUMN processed INTEGER NOT NULL DEFAULT 0;

CREATE INDEX ingestion_scan_active_progress_idx
    ON ingestion_scan(status, last_progress_at)
    WHERE status IN ('queued', 'running');
