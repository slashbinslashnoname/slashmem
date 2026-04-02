use chrono::{DateTime, Utc};
use rusqlite::{Connection, params};

use crate::confidence;

/// A row from the `procedural` table.
#[derive(Debug, PartialEq)]
pub struct ProceduralRule {
    pub id: String,
    pub rule: String,
    pub success_count: u32,
    pub failure_count: u32,
    pub confidence: f64,
    pub is_anti_pattern: bool,
    pub is_proven: bool,
    pub last_validated: Option<String>,
    pub source: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

fn map_row(row: &rusqlite::Row) -> rusqlite::Result<ProceduralRule> {
    Ok(ProceduralRule {
        id: row.get(0)?,
        rule: row.get(1)?,
        success_count: row.get(2)?,
        failure_count: row.get(3)?,
        confidence: row.get(4)?,
        is_anti_pattern: row.get::<_, i32>(5)? != 0,
        is_proven: row.get::<_, i32>(6)? != 0,
        last_validated: row.get(7)?,
        source: row.get(8)?,
        created_at: row.get(9)?,
        updated_at: row.get(10)?,
    })
}

const SELECT_COLS: &str =
    "id, rule, success_count, failure_count, confidence, is_anti_pattern, is_proven, \
     last_validated, source, created_at, updated_at";

/// Insert a new procedural rule. Returns the text id.
pub fn insert(
    conn: &Connection,
    id: &str,
    rule: &str,
    source: Option<&str>,
) -> crate::error::Result<String> {
    conn.execute(
        "INSERT INTO procedural (id, rule, source) VALUES (?1, ?2, ?3)",
        params![id, rule, source],
    )?;
    Ok(id.to_string())
}

/// Update the rule text for an existing procedural entry.
pub fn update_rule(
    conn: &Connection,
    id: &str,
    rule: &str,
) -> crate::error::Result<bool> {
    let changed = conn.execute(
        "UPDATE procedural SET rule = ?2 WHERE id = ?1",
        params![id, rule],
    )?;
    Ok(changed > 0)
}

/// Increment the success count for a rule.
pub fn record_success(conn: &Connection, id: &str) -> crate::error::Result<bool> {
    let changed = conn.execute(
        "UPDATE procedural SET success_count = success_count + 1, \
         last_validated = datetime('now') WHERE id = ?1",
        params![id],
    )?;
    Ok(changed > 0)
}

/// Increment the failure count for a rule.
pub fn record_failure(conn: &Connection, id: &str) -> crate::error::Result<bool> {
    let changed = conn.execute(
        "UPDATE procedural SET failure_count = failure_count + 1, \
         last_validated = datetime('now') WHERE id = ?1",
        params![id],
    )?;
    Ok(changed > 0)
}

/// Get a single rule by id.
pub fn get(conn: &Connection, id: &str) -> crate::error::Result<Option<ProceduralRule>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {SELECT_COLS} FROM procedural WHERE id = ?1"
    ))?;
    let mut rows = stmt.query_map(params![id], map_row)?;
    match rows.next() {
        Some(row) => Ok(Some(row?)),
        None => Ok(None),
    }
}

/// Full-text search across rule text. Returns matching entries ranked by relevance.
pub fn search(
    conn: &Connection,
    query: &str,
    limit: u32,
) -> crate::error::Result<Vec<ProceduralRule>> {
    let mut stmt = conn.prepare(
        "SELECT p.id, p.rule, p.success_count, p.failure_count, p.confidence, \
         p.is_anti_pattern, p.is_proven, p.last_validated, p.source, p.created_at, p.updated_at \
         FROM procedural_fts f \
         JOIN procedural p ON p.rowid = f.rowid \
         WHERE procedural_fts MATCH ?1 \
         ORDER BY rank \
         LIMIT ?2",
    )?;
    let rows = stmt.query_map(params![query, limit], map_row)?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

/// Recalculate confidence scores for all rules (or those needing update).
/// Updates `confidence`, `is_proven`, and `is_anti_pattern` flags.
/// Returns the number of rules updated.
pub fn recalculate_confidence(conn: &Connection, now: DateTime<Utc>) -> crate::error::Result<u32> {
    let mut stmt = conn.prepare(
        "SELECT id, success_count, failure_count, last_validated FROM procedural",
    )?;
    let rules: Vec<(String, u32, u32, Option<String>)> = stmt
        .query_map([], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;

    let mut update_stmt = conn.prepare(
        "UPDATE procedural SET confidence = ?2, is_proven = ?3, is_anti_pattern = ?4 WHERE id = ?1",
    )?;

    let mut count = 0u32;
    for (id, success, failure, last_val) in &rules {
        let last_validated = last_val
            .as_deref()
            .and_then(|s| DateTime::parse_from_rfc3339(s).ok().map(|d| d.with_timezone(&Utc)))
            .or_else(|| {
                last_val.as_deref().and_then(|s| {
                    chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S")
                        .ok()
                        .map(|dt| dt.and_utc())
                })
            })
            .unwrap_or(now);

        let score = confidence::confidence(*success, *failure, last_validated, now);
        let proven = confidence::is_proven(score);
        let anti = confidence::is_anti_pattern(*failure);

        update_stmt.execute(params![id, score, proven as i32, anti as i32])?;
        count += 1;
    }
    Ok(count)
}

/// Delete rules with confidence below `threshold`.
/// Returns the number of rules pruned.
pub fn prune(conn: &Connection, threshold: f64) -> crate::error::Result<u32> {
    let deleted = conn.execute(
        "DELETE FROM procedural WHERE confidence < ?1",
        params![threshold],
    )?;
    Ok(deleted as u32)
}

/// Return all procedural rules.
pub fn all(conn: &Connection) -> crate::error::Result<Vec<ProceduralRule>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {SELECT_COLS} FROM procedural"
    ))?;
    let rows = stmt.query_map([], map_row)?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

/// Delete rules with confidence below `threshold` that are NOT anti-patterns.
/// Returns the number of rules pruned.
pub fn prune_safe(conn: &Connection, threshold: f64) -> crate::error::Result<u32> {
    let deleted = conn.execute(
        "DELETE FROM procedural WHERE confidence < ?1 AND is_anti_pattern = 0",
        params![threshold],
    )?;
    Ok(deleted as u32)
}

/// Delete a single rule by id. Returns true if a row was deleted.
pub fn delete(conn: &Connection, id: &str) -> crate::error::Result<bool> {
    let changed = conn.execute("DELETE FROM procedural WHERE id = ?1", params![id])?;
    Ok(changed > 0)
}

/// Return all rules currently marked as proven (is_proven = 1).
pub fn proven_rules(conn: &Connection) -> crate::error::Result<Vec<ProceduralRule>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {SELECT_COLS} FROM procedural WHERE is_proven = 1 ORDER BY confidence DESC"
    ))?;
    let rows = stmt.query_map([], map_row)?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

/// Return all rules currently marked as anti-patterns (is_anti_pattern = 1).
pub fn anti_patterns(conn: &Connection) -> crate::error::Result<Vec<ProceduralRule>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {SELECT_COLS} FROM procedural WHERE is_anti_pattern = 1 ORDER BY failure_count DESC"
    ))?;
    let rows = stmt.query_map([], map_row)?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::ensure_schema;
    use chrono::TimeZone;
    use rusqlite::Connection;

    fn setup() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        ensure_schema(&conn).unwrap();
        conn
    }

    fn utc(y: i32, m: u32, d: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(y, m, d, 0, 0, 0).unwrap()
    }

    #[test]
    fn insert_returns_id() {
        let conn = setup();
        let id = insert(&conn, "rule-1", "Always run tests before deploy", None).unwrap();
        assert_eq!(id, "rule-1");
    }

    #[test]
    fn insert_with_source() {
        let conn = setup();
        insert(&conn, "rule-2", "Use prepared statements", Some("code-review")).unwrap();
        let rule = get(&conn, "rule-2").unwrap().unwrap();
        assert_eq!(rule.source.as_deref(), Some("code-review"));
        assert_eq!(rule.success_count, 0);
        assert_eq!(rule.failure_count, 0);
    }

    #[test]
    fn insert_duplicate_id_errors() {
        let conn = setup();
        insert(&conn, "dup", "first rule", None).unwrap();
        assert!(insert(&conn, "dup", "second rule", None).is_err());
    }

    #[test]
    fn get_missing_returns_none() {
        let conn = setup();
        assert!(get(&conn, "nonexistent").unwrap().is_none());
    }

    #[test]
    fn update_rule_changes_text() {
        let conn = setup();
        insert(&conn, "r1", "old text", None).unwrap();
        assert!(update_rule(&conn, "r1", "new text").unwrap());

        let rule = get(&conn, "r1").unwrap().unwrap();
        assert_eq!(rule.rule, "new text");
    }

    #[test]
    fn update_rule_missing_returns_false() {
        let conn = setup();
        assert!(!update_rule(&conn, "nope", "text").unwrap());
    }

    #[test]
    fn record_success_increments() {
        let conn = setup();
        insert(&conn, "r1", "test rule", None).unwrap();
        record_success(&conn, "r1").unwrap();
        record_success(&conn, "r1").unwrap();

        let rule = get(&conn, "r1").unwrap().unwrap();
        assert_eq!(rule.success_count, 2);
        assert!(rule.last_validated.is_some());
    }

    #[test]
    fn record_failure_increments() {
        let conn = setup();
        insert(&conn, "r1", "test rule", None).unwrap();
        record_failure(&conn, "r1").unwrap();
        record_failure(&conn, "r1").unwrap();
        record_failure(&conn, "r1").unwrap();

        let rule = get(&conn, "r1").unwrap().unwrap();
        assert_eq!(rule.failure_count, 3);
    }

    #[test]
    fn record_on_missing_returns_false() {
        let conn = setup();
        assert!(!record_success(&conn, "nope").unwrap());
        assert!(!record_failure(&conn, "nope").unwrap());
    }

    #[test]
    fn search_finds_matching_rules() {
        let conn = setup();
        insert(&conn, "r1", "Always validate user input", None).unwrap();
        insert(&conn, "r2", "Use connection pooling for databases", None).unwrap();

        let results = search(&conn, "validate", 10).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "r1");
    }

    #[test]
    fn search_no_results() {
        let conn = setup();
        insert(&conn, "r1", "some rule", None).unwrap();
        let results = search(&conn, "nonexistent", 10).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn search_respects_limit() {
        let conn = setup();
        for i in 0..5 {
            insert(&conn, &format!("r{i}"), &format!("deploy rule variant {i}"), None).unwrap();
        }
        let results = search(&conn, "deploy", 3).unwrap();
        assert_eq!(results.len(), 3);
    }

    #[test]
    fn recalculate_confidence_updates_scores() {
        let conn = setup();
        insert(&conn, "r1", "test rule", None).unwrap();
        // Give it successes
        for _ in 0..10 {
            record_success(&conn, "r1").unwrap();
        }

        let now = Utc::now();
        let updated = recalculate_confidence(&conn, now).unwrap();
        assert_eq!(updated, 1);

        let rule = get(&conn, "r1").unwrap().unwrap();
        // 10 successes with recent validation should give high confidence
        assert!(rule.confidence > 0.0);
        assert!(rule.is_proven);
        assert!(!rule.is_anti_pattern);
    }

    #[test]
    fn recalculate_marks_anti_pattern() {
        let conn = setup();
        insert(&conn, "r1", "bad rule", None).unwrap();
        for _ in 0..4 {
            record_failure(&conn, "r1").unwrap();
        }

        recalculate_confidence(&conn, Utc::now()).unwrap();
        let rule = get(&conn, "r1").unwrap().unwrap();
        assert!(rule.is_anti_pattern);
        assert!(rule.confidence < 0.0);
    }

    #[test]
    fn prune_removes_low_confidence() {
        let conn = setup();
        insert(&conn, "good", "good rule", None).unwrap();
        insert(&conn, "bad", "bad rule", None).unwrap();

        // Make "good" have high confidence
        for _ in 0..10 {
            record_success(&conn, "good").unwrap();
        }
        // Make "bad" have negative confidence
        for _ in 0..5 {
            record_failure(&conn, "bad").unwrap();
        }

        recalculate_confidence(&conn, Utc::now()).unwrap();
        let pruned = prune(&conn, 0.0).unwrap();
        assert_eq!(pruned, 1);

        assert!(get(&conn, "good").unwrap().is_some());
        assert!(get(&conn, "bad").unwrap().is_none());
    }

    #[test]
    fn proven_rules_returns_promoted() {
        let conn = setup();
        insert(&conn, "r1", "proven rule", None).unwrap();
        insert(&conn, "r2", "unproven rule", None).unwrap();
        for _ in 0..10 {
            record_success(&conn, "r1").unwrap();
        }
        recalculate_confidence(&conn, Utc::now()).unwrap();

        let proven = proven_rules(&conn).unwrap();
        assert_eq!(proven.len(), 1);
        assert_eq!(proven[0].id, "r1");
    }

    #[test]
    fn anti_patterns_returns_flagged() {
        let conn = setup();
        insert(&conn, "r1", "good rule", None).unwrap();
        insert(&conn, "r2", "bad rule", None).unwrap();
        for _ in 0..3 {
            record_failure(&conn, "r2").unwrap();
        }
        recalculate_confidence(&conn, Utc::now()).unwrap();

        let anti = anti_patterns(&conn).unwrap();
        assert_eq!(anti.len(), 1);
        assert_eq!(anti[0].id, "r2");
    }

    #[test]
    fn recalculate_with_explicit_timestamps() {
        let conn = setup();
        insert(&conn, "r1", "decaying rule", None).unwrap();
        // Set last_validated to 180 days ago manually
        conn.execute(
            "UPDATE procedural SET success_count = 8, last_validated = '2025-07-01 00:00:00' WHERE id = 'r1'",
            [],
        ).unwrap();

        let now = utc(2026, 1, 1); // ~180 days later
        recalculate_confidence(&conn, now).unwrap();
        let rule = get(&conn, "r1").unwrap().unwrap();
        // 8 * 0.25 = 2.0 (two half-lives)
        assert!(rule.confidence < 3.0 && rule.confidence > 1.0);
    }

    #[test]
    fn default_confidence_is_half() {
        let conn = setup();
        insert(&conn, "r1", "new rule", None).unwrap();
        let rule = get(&conn, "r1").unwrap().unwrap();
        assert!((rule.confidence - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn boolean_fields_default_false() {
        let conn = setup();
        insert(&conn, "r1", "new rule", None).unwrap();
        let rule = get(&conn, "r1").unwrap().unwrap();
        assert!(!rule.is_proven);
        assert!(!rule.is_anti_pattern);
    }

    #[test]
    fn timestamps_are_populated() {
        let conn = setup();
        insert(&conn, "r1", "rule", None).unwrap();
        let rule = get(&conn, "r1").unwrap().unwrap();
        assert!(!rule.created_at.is_empty());
        assert!(!rule.updated_at.is_empty());
    }

    #[test]
    fn all_returns_every_rule() {
        let conn = setup();
        insert(&conn, "r1", "rule one", None).unwrap();
        insert(&conn, "r2", "rule two", None).unwrap();
        insert(&conn, "r3", "rule three", None).unwrap();
        let rules = all(&conn).unwrap();
        assert_eq!(rules.len(), 3);
    }

    #[test]
    fn all_empty_db() {
        let conn = setup();
        let rules = all(&conn).unwrap();
        assert!(rules.is_empty());
    }

    #[test]
    fn prune_safe_keeps_anti_patterns() {
        let conn = setup();
        insert(&conn, "good", "good rule", None).unwrap();
        insert(&conn, "anti", "anti rule", None).unwrap();
        insert(&conn, "stale", "stale rule", None).unwrap();

        // Make "good" high confidence
        for _ in 0..10 {
            record_success(&conn, "good").unwrap();
        }
        // Make "anti" an anti-pattern with low confidence
        for _ in 0..3 {
            record_failure(&conn, "anti").unwrap();
        }
        // "stale" stays at default 0.5, but let's set it low
        conn.execute(
            "UPDATE procedural SET success_count = 1, last_validated = '2020-01-01 00:00:00' WHERE id = 'stale'",
            [],
        ).unwrap();

        recalculate_confidence(&conn, Utc::now()).unwrap();

        let pruned = prune_safe(&conn, 0.05).unwrap();
        // stale should be pruned, anti should NOT be pruned (is_anti_pattern=1)
        assert_eq!(pruned, 1);
        assert!(get(&conn, "good").unwrap().is_some());
        assert!(get(&conn, "anti").unwrap().is_some());
        assert!(get(&conn, "stale").unwrap().is_none());
    }
}
