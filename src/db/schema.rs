use rusqlite::Connection;

/// SQL to create the episodic table.
const CREATE_EPISODIC: &str = "
CREATE TABLE IF NOT EXISTS episodic (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    content    TEXT    NOT NULL,
    context    TEXT,
    timestamp  TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    source     TEXT
)";

/// SQL to create the procedural table.
const CREATE_PROCEDURAL: &str = "
CREATE TABLE IF NOT EXISTS procedural (
    id          TEXT PRIMARY KEY,
    rule        TEXT    NOT NULL,
    confidence  REAL    NOT NULL DEFAULT 1.0,
    source      TEXT,
    created_at  TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    updated_at  TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
)";

/// SQL trigger to auto-update procedural.updated_at on row changes.
const CREATE_UPDATED_AT_TRIGGER: &str = "
CREATE TRIGGER IF NOT EXISTS procedural_updated_at
AFTER UPDATE ON procedural
FOR EACH ROW
BEGIN
    UPDATE procedural
       SET updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
     WHERE id = NEW.id;
END";

/// Run all schema migrations: create tables, enable WAL, install triggers.
pub fn ensure_schema(conn: &Connection) -> crate::error::Result<()> {
    conn.execute_batch("PRAGMA journal_mode=WAL;")?;
    conn.execute(CREATE_EPISODIC, [])?;
    conn.execute(CREATE_PROCEDURAL, [])?;
    conn.execute_batch(CREATE_UPDATED_AT_TRIGGER)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_conn() -> Connection {
        Connection::open_in_memory().unwrap()
    }

    #[test]
    fn ensure_schema_creates_tables() {
        let conn = test_conn();
        ensure_schema(&conn).unwrap();

        // Verify both tables exist by querying sqlite_master
        let tables: Vec<String> = conn
            .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();
        assert!(tables.contains(&"episodic".to_string()));
        assert!(tables.contains(&"procedural".to_string()));
    }

    #[test]
    fn ensure_schema_creates_trigger() {
        let conn = test_conn();
        ensure_schema(&conn).unwrap();

        let triggers: Vec<String> = conn
            .prepare("SELECT name FROM sqlite_master WHERE type='trigger'")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();
        assert!(triggers.contains(&"procedural_updated_at".to_string()));
    }

    #[test]
    fn ensure_schema_is_idempotent() {
        let conn = test_conn();
        ensure_schema(&conn).unwrap();
        ensure_schema(&conn).unwrap(); // must not fail
    }

    #[test]
    fn trigger_updates_updated_at_on_row_change() {
        let conn = test_conn();
        ensure_schema(&conn).unwrap();

        // Insert a row with explicit timestamps so we can detect the change.
        conn.execute(
            "INSERT INTO procedural (id, rule, confidence, created_at, updated_at)
             VALUES ('RULE-001', 'test rule', 1.0, '2020-01-01T00:00:00Z', '2020-01-01T00:00:00Z')",
            [],
        )
        .unwrap();

        // Verify initial state
        let (created, updated): (String, String) = conn
            .query_row(
                "SELECT created_at, updated_at FROM procedural WHERE id = 'RULE-001'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(created, "2020-01-01T00:00:00Z");
        assert_eq!(updated, "2020-01-01T00:00:00Z");

        // Update the rule text — trigger should bump updated_at
        conn.execute(
            "UPDATE procedural SET rule = 'updated rule' WHERE id = 'RULE-001'",
            [],
        )
        .unwrap();

        let (created_after, updated_after): (String, String) = conn
            .query_row(
                "SELECT created_at, updated_at FROM procedural WHERE id = 'RULE-001'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();

        // created_at must not change
        assert_eq!(created_after, "2020-01-01T00:00:00Z");
        // updated_at must have been bumped past the original value
        assert_ne!(updated_after, "2020-01-01T00:00:00Z");
        assert!(updated_after.as_str() > "2020-01-01T00:00:00Z");
    }

    #[test]
    fn trigger_fires_on_confidence_update() {
        let conn = test_conn();
        ensure_schema(&conn).unwrap();

        conn.execute(
            "INSERT INTO procedural (id, rule, confidence, created_at, updated_at)
             VALUES ('RULE-002', 'another rule', 0.8, '2020-01-01T00:00:00Z', '2020-01-01T00:00:00Z')",
            [],
        )
        .unwrap();

        conn.execute(
            "UPDATE procedural SET confidence = 0.5 WHERE id = 'RULE-002'",
            [],
        )
        .unwrap();

        let updated: String = conn
            .query_row(
                "SELECT updated_at FROM procedural WHERE id = 'RULE-002'",
                [],
                |row| row.get(0),
            )
            .unwrap();

        assert_ne!(updated, "2020-01-01T00:00:00Z");
    }
}
