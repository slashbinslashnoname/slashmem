use rusqlite::{Connection, Row, params};
use serde::Serialize;

use crate::error;

/// A row from the `episodic` table.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct EpisodicRow {
    pub id: i64,
    pub content: String,
    pub context: Option<String>,
    pub agent: Option<String>,
    pub timestamp: String,
    pub source: Option<String>,
}

/// Parameters for inserting a new episodic record.
pub struct InsertEpisodic<'a> {
    pub content: &'a str,
    pub context: Option<&'a str>,
    pub agent: Option<&'a str>,
    pub source: Option<&'a str>,
}

fn map_row(row: &Row<'_>) -> rusqlite::Result<EpisodicRow> {
    Ok(EpisodicRow {
        id: row.get(0)?,
        content: row.get(1)?,
        context: row.get(2)?,
        agent: row.get(3)?,
        timestamp: row.get(4)?,
        source: row.get(5)?,
    })
}

/// Insert an episodic record and return its rowid.
pub fn insert(conn: &Connection, rec: &InsertEpisodic<'_>) -> error::Result<i64> {
    conn.execute(
        "INSERT INTO episodic (content, context, agent, source) VALUES (?1, ?2, ?3, ?4)",
        params![rec.content, rec.context, rec.agent, rec.source],
    )?;
    Ok(conn.last_insert_rowid())
}

/// Return the most recent `limit` episodic entries, newest first.
pub fn recent(conn: &Connection, limit: u32) -> error::Result<Vec<EpisodicRow>> {
    let mut stmt = conn.prepare(
        "SELECT id, content, context, agent, timestamp, source \
         FROM episodic ORDER BY timestamp DESC, id DESC LIMIT ?1",
    )?;
    stmt.query_map(params![limit], map_row)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(Into::into)
}

/// Return episodic entries filtered by agent, newest first.
pub fn by_agent(conn: &Connection, agent: &str, limit: u32) -> error::Result<Vec<EpisodicRow>> {
    let mut stmt = conn.prepare(
        "SELECT id, content, context, agent, timestamp, source \
         FROM episodic WHERE agent = ?1 ORDER BY timestamp DESC, id DESC LIMIT ?2",
    )?;
    stmt.query_map(params![agent, limit], map_row)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(Into::into)
}

/// Return episodic entries newer than the given ISO-8601 timestamp, newest first.
pub fn since(conn: &Connection, since: &str, limit: u32) -> error::Result<Vec<EpisodicRow>> {
    let mut stmt = conn.prepare(
        "SELECT id, content, context, agent, timestamp, source \
         FROM episodic WHERE timestamp > ?1 ORDER BY timestamp DESC, id DESC LIMIT ?2",
    )?;
    stmt.query_map(params![since, limit], map_row)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(Into::into)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::ensure_schema;

    fn setup() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        ensure_schema(&conn).unwrap();
        conn
    }

    #[test]
    fn insert_returns_rowid() {
        let conn = setup();
        let id = insert(&conn, &InsertEpisodic {
            content: "did a thing",
            context: None,
            agent: None,
            source: None,
        }).unwrap();
        assert_eq!(id, 1);

        let id2 = insert(&conn, &InsertEpisodic {
            content: "did another thing",
            context: Some("ctx"),
            agent: Some("bot-1"),
            source: Some("ingest"),
        }).unwrap();
        assert_eq!(id2, 2);
    }

    #[test]
    fn insert_empty_content_is_valid() {
        let conn = setup();
        let id = insert(&conn, &InsertEpisodic {
            content: "",
            context: None,
            agent: None,
            source: None,
        }).unwrap();
        let rows = recent(&conn, 10).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, id);
        assert_eq!(rows[0].content, "");
    }

    #[test]
    fn recent_returns_newest_first() {
        let conn = setup();
        // Insert with explicit timestamps to control ordering
        conn.execute(
            "INSERT INTO episodic (content, timestamp) VALUES ('first', '2025-01-01 00:00:00')",
            [],
        ).unwrap();
        conn.execute(
            "INSERT INTO episodic (content, timestamp) VALUES ('second', '2025-01-02 00:00:00')",
            [],
        ).unwrap();
        conn.execute(
            "INSERT INTO episodic (content, timestamp) VALUES ('third', '2025-01-03 00:00:00')",
            [],
        ).unwrap();

        let rows = recent(&conn, 10).unwrap();
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].content, "third");
        assert_eq!(rows[1].content, "second");
        assert_eq!(rows[2].content, "first");
    }

    #[test]
    fn recent_respects_limit() {
        let conn = setup();
        for i in 0..5 {
            insert(&conn, &InsertEpisodic {
                content: &format!("entry {i}"),
                context: None,
                agent: None,
                source: None,
            }).unwrap();
        }
        let rows = recent(&conn, 2).unwrap();
        assert_eq!(rows.len(), 2);
    }

    #[test]
    fn recent_empty_table() {
        let conn = setup();
        let rows = recent(&conn, 10).unwrap();
        assert!(rows.is_empty());
    }

    #[test]
    fn by_agent_filters_correctly() {
        let conn = setup();
        insert(&conn, &InsertEpisodic {
            content: "from alpha",
            context: None,
            agent: Some("alpha"),
            source: None,
        }).unwrap();
        insert(&conn, &InsertEpisodic {
            content: "from beta",
            context: None,
            agent: Some("beta"),
            source: None,
        }).unwrap();
        insert(&conn, &InsertEpisodic {
            content: "also alpha",
            context: None,
            agent: Some("alpha"),
            source: None,
        }).unwrap();

        let rows = by_agent(&conn, "alpha", 10).unwrap();
        assert_eq!(rows.len(), 2);
        assert!(rows.iter().all(|r| r.agent.as_deref() == Some("alpha")));

        let rows = by_agent(&conn, "beta", 10).unwrap();
        assert_eq!(rows.len(), 1);

        let rows = by_agent(&conn, "gamma", 10).unwrap();
        assert!(rows.is_empty());
    }

    #[test]
    fn since_filters_by_timestamp() {
        let conn = setup();
        conn.execute(
            "INSERT INTO episodic (content, timestamp) VALUES ('old', '2025-01-01 00:00:00')",
            [],
        ).unwrap();
        conn.execute(
            "INSERT INTO episodic (content, timestamp) VALUES ('mid', '2025-06-01 00:00:00')",
            [],
        ).unwrap();
        conn.execute(
            "INSERT INTO episodic (content, timestamp) VALUES ('new', '2025-12-01 00:00:00')",
            [],
        ).unwrap();

        let rows = since(&conn, "2025-05-01 00:00:00", 10).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].content, "new");
        assert_eq!(rows[1].content, "mid");
    }

    #[test]
    fn since_returns_empty_when_all_entries_are_older() {
        let conn = setup();
        conn.execute(
            "INSERT INTO episodic (content, timestamp) VALUES ('old', '2025-01-01 00:00:00')",
            [],
        ).unwrap();

        let rows = since(&conn, "2025-12-01 00:00:00", 10).unwrap();
        assert!(rows.is_empty());
    }

    #[test]
    fn all_optional_fields_round_trip() {
        let conn = setup();
        let id = insert(&conn, &InsertEpisodic {
            content: "full record",
            context: Some("test context"),
            agent: Some("agent-42"),
            source: Some("cli"),
        }).unwrap();

        let rows = recent(&conn, 1).unwrap();
        assert_eq!(rows.len(), 1);
        let row = &rows[0];
        assert_eq!(row.id, id);
        assert_eq!(row.content, "full record");
        assert_eq!(row.context.as_deref(), Some("test context"));
        assert_eq!(row.agent.as_deref(), Some("agent-42"));
        assert_eq!(row.source.as_deref(), Some("cli"));
        // timestamp is auto-set
        assert!(!row.timestamp.is_empty());
    }

    #[test]
    fn row_serializes_to_json() {
        let row = EpisodicRow {
            id: 1,
            content: "test".into(),
            context: None,
            agent: Some("bot".into()),
            timestamp: "2025-01-01 00:00:00".into(),
            source: None,
        };
        let json = serde_json::to_value(&row).unwrap();
        assert_eq!(json["id"], 1);
        assert_eq!(json["content"], "test");
        assert!(json["context"].is_null());
        assert_eq!(json["agent"], "bot");
    }
}
