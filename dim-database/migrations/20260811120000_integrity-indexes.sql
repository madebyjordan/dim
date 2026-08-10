-- Index foreign-key columns and the high-frequency library/media/progress lookup paths used by
-- the scanner, dashboard, playback progress, and library deletion flows.
CREATE INDEX IF NOT EXISTS idx_mediafile_library_media ON mediafile(library_id, media_id);
CREATE INDEX IF NOT EXISTS idx_mediafile_media_duration ON mediafile(media_id, duration DESC);
CREATE INDEX IF NOT EXISTS idx_indexed_paths_library ON indexed_paths(library_id);
CREATE INDEX IF NOT EXISTS idx_media_library_type_name ON _tblmedia(library_id, media_type, name);
CREATE INDEX IF NOT EXISTS idx_progress_media ON progress(media_id);

-- These joins are used when resolving show episodes and media genres. Their existing unique
-- indexes have the opposite leading column and therefore cannot serve these lookups/cascades.
CREATE INDEX IF NOT EXISTS idx_season_tvshow ON _tblseason(tvshowid);
CREATE INDEX IF NOT EXISTS idx_genre_media_media ON genre_media(media_id);
