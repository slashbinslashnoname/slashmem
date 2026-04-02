//! Integration tests for CLI commands.
//!
//! Each test uses SLASHMEM_DIR=<tempdir> for full isolation.
//! Tests invoke the built `sm` binary via std::process::Command.

use std::process::Command;

fn sm_bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_sm"))
}

/// Run `sm` with SLASHMEM_DIR pointing at a fresh tempdir.
/// Returns (tempdir_handle, stdout_json).
fn run_sm(dir: &tempfile::TempDir, args: &[&str]) -> std::process::Output {
    sm_bin()
        .env("SLASHMEM_DIR", dir.path())
        .args(args)
        .output()
        .expect("failed to run sm")
}

fn parse_stdout(output: &std::process::Output) -> serde_json::Value {
    assert!(
        output.status.success(),
        "sm exited with {}: {}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("stdout should be valid JSON")
}

/// Open the DB file inside the tempdir directly (for seeding/verifying).
fn open_db(dir: &tempfile::TempDir) -> rusqlite::Connection {
    let db_path = dir.path().join("mem.db");
    rusqlite::Connection::open(db_path).unwrap()
}

// ---------------------------------------------------------------------------
// 1. context on empty DB returns valid empty JSON
// ---------------------------------------------------------------------------

#[test]
fn context_empty_db_returns_valid_empty_json() {
    let tmp = tempfile::tempdir().unwrap();
    let output = run_sm(&tmp, &["context", "anything"]);
    let json = parse_stdout(&output);

    assert_eq!(json["relevant_rules"], serde_json::json!([]));
    assert_eq!(json["anti_patterns"], serde_json::json!([]));
    assert_eq!(json["history_snippets"], serde_json::json!([]));
}

// ---------------------------------------------------------------------------
// 2. ingest writes to episodic and returns episodic_id
// ---------------------------------------------------------------------------

#[test]
fn ingest_writes_episodic_and_returns_id() {
    let tmp = tempfile::tempdir().unwrap();
    let output = run_sm(
        &tmp,
        &["ingest", "--task", "TASK-42", "--body", "deployed canary", "--agent", "bot-1"],
    );
    let json = parse_stdout(&output);

    // Must have a non-null episodic_id
    assert!(json["episodic_id"].is_string(), "expected episodic_id string, got {:?}", json["episodic_id"]);
    let ep_id: i64 = json["episodic_id"].as_str().unwrap().parse().expect("episodic_id should be numeric");
    assert!(ep_id > 0);

    // Verify the row landed in the DB
    let conn = open_db(&tmp);
    let content: String = conn
        .query_row("SELECT content FROM episodic WHERE id = ?1", [ep_id], |r| r.get(0))
        .unwrap();
    assert_eq!(content, "deployed canary");
}

// ---------------------------------------------------------------------------
// 3. ingest with --success updates procedural success_count
// ---------------------------------------------------------------------------

#[test]
fn ingest_success_updates_procedural_count() {
    let tmp = tempfile::tempdir().unwrap();

    // First ingest to create the DB/schema, then seed a procedural rule directly
    run_sm(&tmp, &["ingest", "--task", "T-0", "--body", "init", "--agent", "a"]);
    let conn = open_db(&tmp);
    conn.execute(
        "INSERT INTO procedural (id, rule) VALUES ('rule-1', 'Always run tests')",
        [],
    )
    .unwrap();
    // Sync FTS manually since we bypassed the trigger? No — the trigger fires on INSERT.
    drop(conn);

    // Now ingest with --success rule-1
    let output = run_sm(
        &tmp,
        &[
            "ingest", "--task", "T-1", "--body", "ran tests", "--agent", "a",
            "--success", "rule-1",
        ],
    );
    let json = parse_stdout(&output);
    assert!(json["validated_rules"].as_array().unwrap().contains(&serde_json::json!("rule-1")));

    // Verify in DB
    let conn = open_db(&tmp);
    let count: i64 = conn
        .query_row("SELECT success_count FROM procedural WHERE id = 'rule-1'", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 1);
}

// ---------------------------------------------------------------------------
// 4. distill marks rules as is_anti_pattern after enough harm
// ---------------------------------------------------------------------------

#[test]
fn distill_marks_anti_pattern_after_harm() {
    let tmp = tempfile::tempdir().unwrap();

    // Create DB via a dummy ingest, then seed a rule with 3+ failures
    run_sm(&tmp, &["ingest", "--task", "T-0", "--body", "init", "--agent", "a"]);
    let conn = open_db(&tmp);
    conn.execute(
        "INSERT INTO procedural (id, rule, failure_count, last_validated) \
         VALUES ('bad-rule', 'Skip code review', 4, datetime('now'))",
        [],
    )
    .unwrap();
    drop(conn);

    let output = run_sm(&tmp, &["distill"]);
    let json = parse_stdout(&output);

    // distill should have processed rules
    assert!(json["decayed"].as_u64().unwrap() >= 1);

    // Verify the rule is now marked as anti-pattern
    let conn = open_db(&tmp);
    let is_anti: i64 = conn
        .query_row(
            "SELECT is_anti_pattern FROM procedural WHERE id = 'bad-rule'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(is_anti, 1, "rule with 4 failures should be marked as anti-pattern");
}

// ---------------------------------------------------------------------------
// 5. distill prunes rules below C<0.05 threshold
// ---------------------------------------------------------------------------

#[test]
fn distill_prunes_low_confidence_rules() {
    let tmp = tempfile::tempdir().unwrap();

    // Create DB, then seed a stale rule that will decay below 0.05
    run_sm(&tmp, &["ingest", "--task", "T-0", "--body", "init", "--agent", "a"]);
    let conn = open_db(&tmp);
    // Rule with 1 success from 6 years ago → confidence ≈ 0 after decay
    conn.execute(
        "INSERT INTO procedural (id, rule, success_count, last_validated) \
         VALUES ('stale-rule', 'Outdated guideline', 1, '2020-01-01 00:00:00')",
        [],
    )
    .unwrap();
    // Also insert a healthy rule that should survive
    conn.execute(
        "INSERT INTO procedural (id, rule, success_count, last_validated) \
         VALUES ('good-rule', 'Always validate input', 10, datetime('now'))",
        [],
    )
    .unwrap();
    drop(conn);

    let output = run_sm(&tmp, &["distill"]);
    let json = parse_stdout(&output);

    assert!(json["pruned"].as_u64().unwrap() >= 1, "should prune at least the stale rule");

    let conn = open_db(&tmp);
    // Stale rule should be gone
    let stale_exists: bool = conn
        .query_row(
            "SELECT COUNT(*) FROM procedural WHERE id = 'stale-rule'",
            [],
            |r| r.get::<_, i64>(0),
        )
        .unwrap()
        > 0;
    assert!(!stale_exists, "stale rule should have been pruned");

    // Good rule should still exist
    let good_exists: bool = conn
        .query_row(
            "SELECT COUNT(*) FROM procedural WHERE id = 'good-rule'",
            [],
            |r| r.get::<_, i64>(0),
        )
        .unwrap()
        > 0;
    assert!(good_exists, "healthy rule should survive pruning");
}

// ---------------------------------------------------------------------------
// 6. context returns anti-patterns in separate array
// ---------------------------------------------------------------------------

#[test]
fn context_returns_anti_patterns_in_separate_array() {
    let tmp = tempfile::tempdir().unwrap();

    // Create DB and seed both a proven rule and an anti-pattern rule
    run_sm(&tmp, &["ingest", "--task", "T-0", "--body", "init", "--agent", "a"]);
    let conn = open_db(&tmp);

    // Proven rule: high confidence, is_proven=1
    // Note: no manual FTS insert — the schema trigger handles sync automatically.
    conn.execute(
        "INSERT INTO procedural (id, rule, success_count, confidence, is_proven, last_validated) \
         VALUES ('proven-deploy', 'Always run deploy checks', 10, 5.0, 1, datetime('now'))",
        [],
    )
    .unwrap();

    // Anti-pattern rule: is_anti_pattern=1, not proven
    conn.execute(
        "INSERT INTO procedural (id, rule, failure_count, confidence, is_anti_pattern, last_validated) \
         VALUES ('anti-deploy', 'Skip deploy validation', 4, -12.0, 1, datetime('now'))",
        [],
    )
    .unwrap();

    drop(conn);

    let output = run_sm(&tmp, &["context", "deploy"]);
    let json = parse_stdout(&output);

    let relevant = json["relevant_rules"].as_array().unwrap();
    let anti = json["anti_patterns"].as_array().unwrap();

    assert!(
        relevant.iter().any(|v| v.as_str().unwrap().contains("deploy checks")),
        "proven rule should appear in relevant_rules: {relevant:?}"
    );
    assert!(
        anti.iter().any(|v| v.as_str().unwrap().contains("Skip deploy")),
        "anti-pattern should appear in anti_patterns: {anti:?}"
    );
    // Anti-pattern should NOT appear in relevant_rules (it's not proven)
    assert!(
        !relevant.iter().any(|v| v.as_str().unwrap().contains("Skip deploy")),
        "anti-pattern should not be in relevant_rules"
    );
}

// ---------------------------------------------------------------------------
// Bonus: end-to-end ingest → context round-trip via working memory
// ---------------------------------------------------------------------------

#[test]
fn ingest_then_context_returns_history_snippet() {
    let tmp = tempfile::tempdir().unwrap();

    // Ingest a record
    let ingest_out = run_sm(
        &tmp,
        &["ingest", "--task", "AUTH-99", "--body", "Fixed token refresh logic", "--agent", "bot-1"],
    );
    parse_stdout(&ingest_out); // assert success

    // Query context for matching terms
    let ctx_out = run_sm(&tmp, &["context", "token refresh"]);
    let json = parse_stdout(&ctx_out);

    let snippets = json["history_snippets"].as_array().unwrap();
    assert!(
        snippets.iter().any(|v| v.as_str().unwrap().contains("token refresh")),
        "ingested text should appear in history_snippets: {snippets:?}"
    );
}

// ---------------------------------------------------------------------------
// Bonus: multiple ingests accumulate
// ---------------------------------------------------------------------------

#[test]
fn multiple_ingests_accumulate() {
    let tmp = tempfile::tempdir().unwrap();

    for i in 0..3 {
        let body = format!("migration step {i}");
        let out = run_sm(
            &tmp,
            &["ingest", "--task", "MIG-1", "--body", &body, "--agent", "a"],
        );
        parse_stdout(&out);
    }

    let conn = open_db(&tmp);
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM episodic", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 3);
}

// ---------------------------------------------------------------------------
// 7. status on empty DB returns ok with zero counts
// ---------------------------------------------------------------------------

#[test]
fn status_empty_db_returns_ok_json() {
    let tmp = tempfile::tempdir().unwrap();
    let output = run_sm(&tmp, &["status"]);
    let json = parse_stdout(&output);

    assert_eq!(json["ok"], true);
    assert!(json["db_path"].as_str().unwrap().contains("mem.db"));
    assert_eq!(json["counts"]["episodic"], 0);
    assert_eq!(json["counts"]["working"], 0);
    assert_eq!(json["counts"]["procedural"], 0);
    assert_eq!(json["schema_version"], 1);
}

// ---------------------------------------------------------------------------
// 8. status reflects record counts after ingestion
// ---------------------------------------------------------------------------

#[test]
fn status_counts_after_ingest() {
    let tmp = tempfile::tempdir().unwrap();

    // Ingest a few records
    run_sm(&tmp, &["ingest", "--task", "T-1", "--body", "one", "--agent", "a"]);
    run_sm(&tmp, &["ingest", "--task", "T-2", "--body", "two", "--agent", "a"]);

    let output = run_sm(&tmp, &["status"]);
    let json = parse_stdout(&output);

    assert_eq!(json["ok"], true);
    assert_eq!(json["counts"]["episodic"], 2);
    assert_eq!(json["counts"]["working"], 2);
    assert_eq!(json["counts"]["procedural"], 0);
}

// ---------------------------------------------------------------------------
// Bonus: distill on empty DB returns zeros
// ---------------------------------------------------------------------------

#[test]
fn distill_empty_db_returns_zeros() {
    let tmp = tempfile::tempdir().unwrap();
    // Run context first to create the DB/schema
    run_sm(&tmp, &["context", "anything"]);

    let output = run_sm(&tmp, &["distill"]);
    let json = parse_stdout(&output);

    assert_eq!(json["decayed"], 0);
    assert_eq!(json["pruned"], 0);
    assert_eq!(json["transitioned"], 0);
}
