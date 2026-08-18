CREATE TABLE IF NOT EXISTS show_metadata (
    media_id INTEGER PRIMARY KEY NOT NULL REFERENCES _tblmedia(id) ON DELETE CASCADE,
    end_year INTEGER,
    ongoing BOOLEAN
);
