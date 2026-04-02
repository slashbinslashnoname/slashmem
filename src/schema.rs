use rusqlite::Connection;

/// Core table DDL — all use IF NOT EXISTS for idempotency.
const CREATE_EPISODIC: &str = "
CREATE TABLE IF NOT EXISTS episodic (
    id        INTEGER PRIMARY KEY,
    content   TEXT NOT NULL,
    context   TEXT,
    agent     TEXT,
    timestamp TEXT NOT NULL DEFAULT (datetime('now')),
    source    TEXT
)";

const CREATE_WORKING: &str = "
CREATE TABLE IF NOT EXISTS working (
    id         INTEGER PRIMARY KEY,
    summary    TEXT NOT NULL,
    body       TEXT,
    task_id    TEXT,
    agent      TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
)";

const CREATE_PROCEDURAL: &str = "
CREATE TABLE IF NOT EXISTS procedural (
    id              TEXT PRIMARY KEY,
    rule            TEXT NOT NULL,
    success_count   INTEGER NOT NULL DEFAULT 0,
    failure_count   INTEGER NOT NULL DEFAULT 0,
    confidence      REAL NOT NULL DEFAULT 0.5,
    is_anti_pattern INTEGER NOT NULL DEFAULT 0,
    is_proven       INTEGER NOT NULL DEFAULT 0,
    last_validated  TEXT,
    source          TEXT,
    created_at      TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at      TEXT NOT NULL DEFAULT (datetime('now'))
)";

// ---------- FTS5 virtual tables (content-sync mode) ----------

const CREATE_PROCEDURAL_FTS: &str = "
CREATE VIRTUAL TABLE IF NOT EXISTS procedural_fts USING fts5(
    rule,
    content='procedural',
    content_rowid='rowid'
)";

const CREATE_WORKING_FTS: &str = "
CREATE VIRTUAL TABLE IF NOT EXISTS working_fts USING fts5(
    summary,
    body,
    content='working',
    content_rowid='id'
)";

// ---------- Triggers — updated_at + FTS sync ----------

const TRIGGER_PROCEDURAL_UPDATED_AT: &str = "
CREATE TRIGGER IF NOT EXISTS procedural_updated_at
AFTER UPDATE ON procedural
BEGIN
    UPDATE procedural SET updated_at = datetime('now') WHERE id = NEW.id;
END";

// Procedural FTS sync
const TRIGGER_PROCEDURAL_FTS_INSERT: &str = "
CREATE TRIGGER IF NOT EXISTS procedural_fts_insert
AFTER INSERT ON procedural
BEGIN
    INSERT INTO procedural_fts(rowid, rule) VALUES (NEW.rowid, NEW.rule);
END";

const TRIGGER_PROCEDURAL_FTS_UPDATE: &str = "
CREATE TRIGGER IF NOT EXISTS procedural_fts_update
AFTER UPDATE ON procedural
BEGIN
    INSERT INTO procedural_fts(procedural_fts, rowid, rule) VALUES ('delete', OLD.rowid, OLD.rule);
    INSERT INTO procedural_fts(rowid, rule) VALUES (NEW.rowid, NEW.rule);
END";

const TRIGGER_PROCEDURAL_FTS_DELETE: &str = "
CREATE TRIGGER IF NOT EXISTS procedural_fts_delete
AFTER DELETE ON procedural
BEGIN
    INSERT INTO procedural_fts(procedural_fts, rowid, rule) VALUES ('delete', OLD.rowid, OLD.rule);
END";

// Working FTS sync
const TRIGGER_WORKING_FTS_INSERT: &str = "
CREATE TRIGGER IF NOT EXISTS working_fts_insert
AFTER INSERT ON working
BEGIN
    INSERT INTO working_fts(rowid, summary, body) VALUES (NEW.id, NEW.summary, NEW.body);
END";

const TRIGGER_WORKING_FTS_UPDATE: &str = "
CREATE TRIGGER IF NOT EXISTS working_fts_update
AFTER UPDATE ON working
BEGIN
    INSERT INTO working_fts(working_fts, rowid, summary, body) VALUES ('delete', OLD.id, OLD.summary, OLD.body);
    INSERT INTO working_fts(rowid, summary, body) VALUES (NEW.id, NEW.summary, NEW.body);
END";

const TRIGGER_WORKING_FTS_DELETE: &str = "
CREATE TRIGGER IF NOT EXISTS working_fts_delete
AFTER DELETE ON working
BEGIN
    INSERT INTO working_fts(working_fts, rowid, summary, body) VALUES ('delete', OLD.id, OLD.summary, OLD.body);
END";

/// Columns that may be missing on pre-existing databases.
/// Each entry: (table, column, column_def).
const MIGRATIONS: &[(&str, &str, &str)] = &[
    ("episodic", "source", "TEXT"),
    ("working", "task_id", "TEXT"),
    ("working", "agent", "TEXT"),
    ("procedural", "is_anti_pattern", "INTEGER NOT NULL DEFAULT 0"),
    ("procedural", "is_proven", "INTEGER NOT NULL DEFAULT 0"),
    ("procedural", "last_validated", "TEXT"),
    ("procedural", "source", "TEXT"),
];

/// Add a column if it doesn't already exist. Returns Ok(true) if added.
fn add_column_if_missing(
    conn: &Connection,
    table: &str,
    column: &str,
    column_def: &str,
) -> rusqlite::Result<bool> {
    let has_column: bool = {
        let mut stmt = conn.prepare(&format!("PRAGMA table_info('{table}')"))?;
        let names: Vec<String> = stmt
            .query_map([], |row| row.get::<_, String>(1))?
            .collect::<Result<Vec<_>, _>>()?;
        names.iter().any(|n| n == column)
    };
    if has_column {
        return Ok(false);
    }
    conn.execute_batch(&format!(
        "ALTER TABLE {table} ADD COLUMN {column} {column_def}"
    ))?;
    Ok(true)
}

/// Initialise or migrate the schema to the target state.
///
/// Safe to call on a fresh database or an existing one — every statement
/// is guarded with IF NOT EXISTS or a column-existence check.
pub fn ensure_schema(conn: &Connection) -> rusqlite::Result<()> {
    // Core tables
    conn.execute_batch(CREATE_EPISODIC)?;
    conn.execute_batch(CREATE_WORKING)?;
    conn.execute_batch(CREATE_PROCEDURAL)?;

    // Migrations — add missing columns
    for &(table, column, col_def) in MIGRATIONS {
        add_column_if_missing(conn, table, column, col_def)?;
    }

    // FTS5 virtual tables
    conn.execute_batch(CREATE_PROCEDURAL_FTS)?;
    conn.execute_batch(CREATE_WORKING_FTS)?;

    // Triggers
    conn.execute_batch(TRIGGER_PROCEDURAL_UPDATED_AT)?;
    conn.execute_batch(TRIGGER_PROCEDURAL_FTS_INSERT)?;
    conn.execute_batch(TRIGGER_PROCEDURAL_FTS_UPDATE)?;
    conn.execute_batch(TRIGGER_PROCEDURAL_FTS_DELETE)?;
    conn.execute_batch(TRIGGER_WORKING_FTS_INSERT)?;
    conn.execute_batch(TRIGGER_WORKING_FTS_UPDATE)?;
    conn.execute_batch(TRIGGER_WORKING_FTS_DELETE)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn mem_db() -> Connection {
        Connection::open_in_memory().unwrap()
    }

    #[test]
    fn ensure_schema_creates_all_tables() {
        let conn = mem_db();
        ensure_schema(&conn).unwrap();

        let tables: Vec<String> = conn
            .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();

        assert!(tables.contains(&"episodic".into()));
        assert!(tables.contains(&"working".into()));
        assert!(tables.contains(&"procedural".into()));
        assert!(tables.contains(&"procedural_fts".into()));
        assert!(tables.contains(&"working_fts".into()));
    }

    #[test]
    fn ensure_schema_is_idempotent() {
        let conn = mem_db();
        ensure_schema(&conn).unwrap();
        ensure_schema(&conn).unwrap(); // must not error
    }

    #[test]
    fn episodic_columns() {
        let conn = mem_db();
        ensure_schema(&conn).unwrap();

        conn.execute(
            "INSERT INTO episodic (content, context, agent, source) VALUES (?1,?2,?3,?4)",
            ["hello", "ctx", "agent-0", "test"],
        )
        .unwrap();

        let (id, ts): (i64, String) = conn
            .query_row(
                "SELECT id, timestamp FROM episodic WHERE id=1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(id, 1);
        assert!(!ts.is_empty());
    }

    #[test]
    fn working_columns() {
        let conn = mem_db();
        ensure_schema(&conn).unwrap();

        conn.execute(
            "INSERT INTO working (summary, body, task_id, agent) VALUES (?1,?2,?3,?4)",
            ["sum", "body text", "T-1", "agent-0"],
        )
        .unwrap();

        let created: String = conn
            .query_row("SELECT created_at FROM working WHERE id=1", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert!(!created.is_empty());
    }

    #[test]
    fn procedural_columns_and_defaults() {
        let conn = mem_db();
        ensure_schema(&conn).unwrap();

        conn.execute(
            "INSERT INTO procedural (id, rule) VALUES ('r1', 'always test')",
            [],
        )
        .unwrap();

        let (sc, fc, conf, anti, proven): (i64, i64, f64, i64, i64) = conn
            .query_row("SELECT success_count, failure_count, confidence, is_anti_pattern, is_proven FROM procedural WHERE id='r1'", [], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
            })
            .unwrap();

        assert_eq!(sc, 0);
        assert_eq!(fc, 0);
        assert!((conf - 0.5).abs() < f64::EPSILON);
        assert_eq!(anti, 0);
        assert_eq!(proven, 0);
    }

    #[test]
    fn procedural_fts_sync() {
        let conn = mem_db();
        ensure_schema(&conn).unwrap();

        // Insert
        conn.execute(
            "INSERT INTO procedural (id, rule) VALUES ('r1', 'always write tests')",
            [],
        )
        .unwrap();

        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM procedural_fts WHERE procedural_fts MATCH 'tests'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);

        // Update
        conn.execute(
            "UPDATE procedural SET rule = 'always write unit tests' WHERE id = 'r1'",
            [],
        )
        .unwrap();

        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM procedural_fts WHERE procedural_fts MATCH 'unit'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);

        // Delete
        conn.execute("DELETE FROM procedural WHERE id = 'r1'", [])
            .unwrap();

        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM procedural_fts WHERE procedural_fts MATCH 'unit'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn working_fts_sync() {
        let conn = mem_db();
        ensure_schema(&conn).unwrap();

        // Insert
        conn.execute(
            "INSERT INTO working (summary, body) VALUES ('auth refactor', 'moved to JWT tokens')",
            [],
        )
        .unwrap();

        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM working_fts WHERE working_fts MATCH 'JWT'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);

        // Update
        conn.execute(
            "UPDATE working SET body = 'switched to OAuth2' WHERE id = 1",
            [],
        )
        .unwrap();

        let jwt_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM working_fts WHERE working_fts MATCH 'JWT'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(jwt_count, 0);

        let oauth_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM working_fts WHERE working_fts MATCH 'OAuth2'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(oauth_count, 1);

        // Delete
        conn.execute("DELETE FROM working WHERE id = 1", []).unwrap();

        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM working_fts WHERE working_fts MATCH 'OAuth2'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn migration_adds_missing_columns() {
        let conn = mem_db();

        // Create a minimal procedural table missing several columns
        conn.execute_batch(
            "CREATE TABLE procedural (
                id TEXT PRIMARY KEY,
                rule TEXT NOT NULL,
                success_count INTEGER NOT NULL DEFAULT 0,
                failure_count INTEGER NOT NULL DEFAULT 0,
                confidence REAL NOT NULL DEFAULT 0.5,
                created_at TEXT NOT NULL DEFAULT (datetime('now')),
                updated_at TEXT NOT NULL DEFAULT (datetime('now'))
            )",
        )
        .unwrap();

        // ensure_schema should add the missing columns without error
        ensure_schema(&conn).unwrap();

        // Verify the new columns exist by inserting a full row
        conn.execute(
            "INSERT INTO procedural (id, rule, is_anti_pattern, is_proven, last_validated, source)
             VALUES ('r1', 'test', 1, 0, '2026-01-01', 'migration-test')",
            [],
        )
        .unwrap();

        let (anti, source): (i64, String) = conn
            .query_row(
                "SELECT is_anti_pattern, source FROM procedural WHERE id='r1'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(anti, 1);
        assert_eq!(source, "migration-test");
    }

    #[test]
    fn procedural_updated_at_trigger() {
        let conn = mem_db();
        ensure_schema(&conn).unwrap();

        conn.execute(
            "INSERT INTO procedural (id, rule, updated_at) VALUES ('r1', 'old rule', '2020-01-01 00:00:00')",
            [],
        )
        .unwrap();

        conn.execute(
            "UPDATE procedural SET rule = 'new rule' WHERE id = 'r1'",
            [],
        )
        .unwrap();

        let updated: String = conn
            .query_row(
                "SELECT updated_at FROM procedural WHERE id='r1'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_ne!(updated, "2020-01-01 00:00:00");
    }
}
