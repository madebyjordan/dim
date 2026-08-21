-- User-authored fields that are not part of provider metadata. Keeping this separate lets the
-- existing media schema and automatic matcher remain unchanged while the mediafile override flag
-- protects the chosen identity during reconciliation.
CREATE TABLE manual_media_metadata (
    media_id INTEGER PRIMARY KEY NOT NULL,
    language TEXT,
    artwork_source TEXT,
    FOREIGN KEY (media_id) REFERENCES _tblmedia(id) ON DELETE CASCADE
);
