use rusqlite::{Connection, params};

/// A row from the `working` table.
#[derive(Debug, PartialEq)]
pub struct WorkingEntry {
    pub id: i64,
    pub summary: String,
    pub body: Option<String>,
    pub task_id: Option<String>,
    pub agent: Option<String>,
    pub created_at: String,
}

/// Insert a new working-memory entry. Returns the new row id.
pub fn insert(
    conn: &Connection,
    summary: &str,
    body: Option<&str>,
    task_id: Option<&str>,
    agent: Option<&str>,
) -> crate::error::Result<i64> {
    conn.execute(
        "INSERT INTO working (summary, body, task_id, agent) VALUES (?1, ?2, ?3, ?4)",
        params![summary, body, task_id, agent],
    )?;
    Ok(conn.last_insert_rowid())
}

/// Return the most recent `limit` working entries, newest first.
pub fn recent(conn: &Connection, limit: u32) -> crate::error::Result<Vec<WorkingEntry>> {
    let mut stmt = conn.prepare(
        "SELECT id, summary, body, task_id, agent, created_at
         FROM working ORDER BY created_at DESC, id DESC LIMIT ?1",
    )?;
    let rows = stmt.query_map(params![limit], |row| {
        Ok(WorkingEntry {
            id: row.get(0)?,
            summary: row.get(1)?,
            body: row.get(2)?,
            task_id: row.get(3)?,
            agent: row.get(4)?,
            created_at: row.get(5)?,
        })
    })?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

/// Full-text search across summary and body. Returns matching entries ordered by recency.
pub fn search(conn: &Connection, query: &str, limit: u32) -> crate::error::Result<Vec<WorkingEntry>> {
    let mut stmt = conn.prepare(
        "SELECT w.id, w.summary, w.body, w.task_id, w.agent, w.created_at
         FROM working_fts f
         JOIN working w ON w.id = f.rowid
         WHERE working_fts MATCH ?1
         ORDER BY w.created_at DESC, w.id DESC
         LIMIT ?2",
    )?;
    let rows = stmt.query_map(params![query, limit], |row| {
        Ok(WorkingEntry {
            id: row.get(0)?,
            summary: row.get(1)?,
            body: row.get(2)?,
            task_id: row.get(3)?,
            agent: row.get(4)?,
            created_at: row.get(5)?,
        })
    })?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

/// Return all entries for a given task_id, newest first.
pub fn by_task(conn: &Connection, task_id: &str) -> crate::error::Result<Vec<WorkingEntry>> {
    let mut stmt = conn.prepare(
        "SELECT id, summary, body, task_id, agent, created_at
         FROM working WHERE task_id = ?1
         ORDER BY created_at DESC, id DESC",
    )?;
    let rows = stmt.query_map(params![task_id], |row| {
        Ok(WorkingEntry {
            id: row.get(0)?,
            summary: row.get(1)?,
            body: row.get(2)?,
            task_id: row.get(3)?,
            agent: row.get(4)?,
            created_at: row.get(5)?,
        })
    })?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::ensure_schema;
    use rusqlite::Connection;

    fn setup() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        ensure_schema(&conn).unwrap();
        conn
    }

    #[test]
    fn insert_returns_id() {
        let conn = setup();
        let id = insert(&conn, "first entry", None, None, None).unwrap();
        assert_eq!(id, 1);
        let id2 = insert(&conn, "second entry", None, None, None).unwrap();
        assert_eq!(id2, 2);
    }

    #[test]
    fn insert_with_all_fields() {
        let conn = setup();
        let id = insert(&conn, "auth refactor", Some("moved to JWT"), Some("T-42"), Some("agent-0")).unwrap();
        assert!(id > 0);

        let entries = recent(&conn, 1).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].summary, "auth refactor");
        assert_eq!(entries[0].body.as_deref(), Some("moved to JWT"));
        assert_eq!(entries[0].task_id.as_deref(), Some("T-42"));
        assert_eq!(entries[0].agent.as_deref(), Some("agent-0"));
    }

    #[test]
    fn recent_returns_newest_first() {
        let conn = setup();
        // Insert entries with explicit timestamps to guarantee ordering
        conn.execute(
            "INSERT INTO working (summary, created_at) VALUES ('old', '2025-01-01 00:00:00')",
            [],
        ).unwrap();
        conn.execute(
            "INSERT INTO working (summary, created_at) VALUES ('mid', '2025-06-01 00:00:00')",
            [],
        ).unwrap();
        conn.execute(
            "INSERT INTO working (summary, created_at) VALUES ('new', '2025-12-01 00:00:00')",
            [],
        ).unwrap();

        let entries = recent(&conn, 10).unwrap();
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].summary, "new");
        assert_eq!(entries[1].summary, "mid");
        assert_eq!(entries[2].summary, "old");
    }

    #[test]
    fn recent_respects_limit() {
        let conn = setup();
        for i in 0..5 {
            insert(&conn, &format!("entry {i}"), None, None, None).unwrap();
        }
        let entries = recent(&conn, 2).unwrap();
        assert_eq!(entries.len(), 2);
    }

    #[test]
    fn recent_empty_table() {
        let conn = setup();
        let entries = recent(&conn, 10).unwrap();
        assert!(entries.is_empty());
    }

    #[test]
    fn search_finds_by_summary() {
        let conn = setup();
        insert(&conn, "deploy pipeline fix", None, None, None).unwrap();
        insert(&conn, "auth refactor", None, None, None).unwrap();

        let results = search(&conn, "pipeline", 10).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].summary, "deploy pipeline fix");
    }

    #[test]
    fn search_finds_by_body() {
        let conn = setup();
        insert(&conn, "task summary", Some("added canary stage to deploy"), None, None).unwrap();

        let results = search(&conn, "canary", 10).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].summary, "task summary");
    }

    #[test]
    fn search_no_results() {
        let conn = setup();
        insert(&conn, "something", None, None, None).unwrap();

        let results = search(&conn, "nonexistent", 10).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn search_respects_limit() {
        let conn = setup();
        for i in 0..5 {
            insert(&conn, &format!("deploy variant {i}"), None, None, None).unwrap();
        }
        let results = search(&conn, "deploy", 3).unwrap();
        assert_eq!(results.len(), 3);
    }

    #[test]
    fn search_returns_newest_first() {
        let conn = setup();
        // Insert entries with explicit timestamps; "old" has more keyword repetition
        // (higher FTS5 relevance) but should still appear last due to recency ordering.
        conn.execute(
            "INSERT INTO working (summary, body, created_at) VALUES ('deploy deploy deploy', 'deploy deploy', '2025-01-01 00:00:00')",
            [],
        ).unwrap();
        conn.execute(
            "INSERT INTO working (summary, created_at) VALUES ('unrelated mid entry', '2025-06-01 00:00:00')",
            [],
        ).unwrap();
        conn.execute(
            "INSERT INTO working (summary, created_at) VALUES ('single deploy mention', '2025-12-01 00:00:00')",
            [],
        ).unwrap();

        let results = search(&conn, "deploy", 10).unwrap();
        assert_eq!(results.len(), 2);
        // Most recent matching entry first, even though the older one is more relevant by FTS5 rank
        assert_eq!(results[0].summary, "single deploy mention");
        assert_eq!(results[1].summary, "deploy deploy deploy");
    }

    #[test]
    fn by_task_filters_correctly() {
        let conn = setup();
        insert(&conn, "task A work", None, Some("T-1"), None).unwrap();
        insert(&conn, "task B work", None, Some("T-2"), None).unwrap();
        insert(&conn, "more task A", None, Some("T-1"), None).unwrap();

        let results = by_task(&conn, "T-1").unwrap();
        assert_eq!(results.len(), 2);
        assert!(results.iter().all(|e| e.task_id.as_deref() == Some("T-1")));
    }

    #[test]
    fn by_task_empty_result() {
        let conn = setup();
        insert(&conn, "something", None, Some("T-1"), None).unwrap();

        let results = by_task(&conn, "T-999").unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn by_task_returns_newest_first() {
        let conn = setup();
        conn.execute(
            "INSERT INTO working (summary, task_id, created_at) VALUES ('early', 'T-1', '2025-01-01 00:00:00')",
            [],
        ).unwrap();
        conn.execute(
            "INSERT INTO working (summary, task_id, created_at) VALUES ('later', 'T-1', '2025-06-01 00:00:00')",
            [],
        ).unwrap();

        let results = by_task(&conn, "T-1").unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].summary, "later");
        assert_eq!(results[1].summary, "early");
    }

    #[test]
    fn created_at_is_populated() {
        let conn = setup();
        insert(&conn, "test entry", None, None, None).unwrap();

        let entries = recent(&conn, 1).unwrap();
        assert!(!entries[0].created_at.is_empty());
    }
}
