-- Transactional application event boundary. Trigger rows share the writer transaction, so a
-- rollback cannot become externally visible. Rows remain until the ordered dispatcher succeeds.
CREATE TABLE runtime_event_outbox (
    sequence INTEGER PRIMARY KEY AUTOINCREMENT,
    source_table TEXT NOT NULL,
    row_id INTEGER NOT NULL,
    event_type TEXT NOT NULL,
    created_at INTEGER NOT NULL DEFAULT (unixepoch())
);

CREATE TRIGGER runtime_event_library_insert AFTER INSERT ON library BEGIN
    INSERT INTO runtime_event_outbox(source_table, row_id, event_type) VALUES ('library', NEW.id, 'insert');
END;
CREATE TRIGGER runtime_event_library_update AFTER UPDATE ON library BEGIN
    INSERT INTO runtime_event_outbox(source_table, row_id, event_type) VALUES ('library', NEW.id, 'update');
END;
CREATE TRIGGER runtime_event_library_delete AFTER DELETE ON library BEGIN
    INSERT INTO runtime_event_outbox(source_table, row_id, event_type) VALUES ('library', OLD.id, 'delete');
END;

CREATE TRIGGER runtime_event_media_insert AFTER INSERT ON _tblmedia BEGIN
    INSERT INTO runtime_event_outbox(source_table, row_id, event_type) VALUES ('_tblmedia', NEW.id, 'insert');
END;
CREATE TRIGGER runtime_event_media_update AFTER UPDATE ON _tblmedia BEGIN
    INSERT INTO runtime_event_outbox(source_table, row_id, event_type) VALUES ('_tblmedia', NEW.id, 'update');
END;
CREATE TRIGGER runtime_event_media_delete AFTER DELETE ON _tblmedia BEGIN
    INSERT INTO runtime_event_outbox(source_table, row_id, event_type) VALUES ('_tblmedia', OLD.id, 'delete');
END;

CREATE TRIGGER runtime_event_assets_insert AFTER INSERT ON assets BEGIN
    INSERT INTO runtime_event_outbox(source_table, row_id, event_type) VALUES ('assets', NEW.id, 'insert');
END;
