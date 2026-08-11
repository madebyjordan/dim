-- Preserve the complete successful probe so playback can avoid repeating expensive I/O.
-- Nullable records are legacy and continue through the safe ffprobe fallback.
ALTER TABLE mediafile ADD COLUMN probe_metadata TEXT;
