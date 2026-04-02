pub mod cli;
pub mod confidence;
pub mod db;
pub mod error;
pub mod output;
pub mod schema;

use clap::Parser;
use cli::{Cli, Commands};

fn main() {
    let cli = Cli::parse();

    let result = match cli.command {
        Commands::Context(args) => cmd_context(args),
        Commands::Ingest(args) => cmd_ingest(args),
        Commands::Distill => cmd_distill(),
    };

    if let Err(e) = result {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

fn cmd_context(_args: cli::ContextArgs) -> error::Result<()> {
    // TODO: implement in slashmem-6188acc3-1su.1
    let out = output::ContextOutput::default();
    println!("{}", serde_json::to_string(&out).unwrap());
    Ok(())
}

fn cmd_ingest(args: cli::IngestArgs) -> error::Result<()> {
    let conn = db::init::open_db()?;
    schema::ensure_schema(&conn)?;
    let out = run_ingest(&conn, &args)?;
    println!("{}", serde_json::to_string(&out)?);
    Ok(())
}

fn run_ingest(
    conn: &rusqlite::Connection,
    args: &cli::IngestArgs,
) -> error::Result<output::IngestOutput> {
    // Insert body into episodic table
    let ep_id = db::episodic::insert(
        conn,
        &db::episodic::InsertEpisodic {
            content: &args.body,
            context: Some(&args.task),
            agent: Some(&args.agent),
            source: Some("ingest"),
        },
    )?;

    // Insert into working table
    db::working::insert(conn, &args.body, None, Some(&args.task), Some(&args.agent))?;

    // Process --success and --harm flags
    let mut validated_rules = Vec::new();
    for rule_id in &args.success {
        if db::procedural::record_success(conn, rule_id)? {
            validated_rules.push(rule_id.clone());
        }
    }
    for rule_id in &args.harm {
        if db::procedural::record_failure(conn, rule_id)? {
            validated_rules.push(rule_id.clone());
        }
    }

    Ok(output::IngestOutput {
        episodic_id: Some(ep_id.to_string()),
        proposed_rules: vec![],
        validated_rules,
    })
}

fn cmd_distill() -> error::Result<()> {
    let conn = db::init::open_db()?;
    schema::ensure_schema(&conn)?;
    let out = run_distill(&conn, chrono::Utc::now())?;
    println!("{}", serde_json::to_string(&out)?);
    Ok(())
}

fn run_distill(
    conn: &rusqlite::Connection,
    now: chrono::DateTime<chrono::Utc>,
) -> error::Result<output::DistillOutput> {
    // Snapshot current state to detect transitions
    let rules = db::procedural::all(conn)?;
    let before: std::collections::HashMap<String, (bool, bool)> = rules
        .iter()
        .map(|r| (r.id.clone(), (r.is_proven, r.is_anti_pattern)))
        .collect();

    // Recalculate confidence, is_proven, is_anti_pattern for all rules
    let decayed = db::procedural::recalculate_confidence(conn, now)? as u64;

    // Count transitions (is_proven or is_anti_pattern changed)
    let updated_rules = db::procedural::all(conn)?;
    let mut transitioned = 0u64;
    for rule in &updated_rules {
        if let Some(&(was_proven, was_anti)) = before.get(&rule.id) {
            if rule.is_proven != was_proven || rule.is_anti_pattern != was_anti {
                transitioned += 1;
            }
        }
    }

    // Prune low-confidence rules that are NOT anti-patterns
    let pruned = db::procedural::prune_safe(conn, 0.05)? as u64;

    Ok(output::DistillOutput {
        decayed,
        pruned,
        transitioned,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_conn() -> rusqlite::Connection {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        schema::ensure_schema(&conn).unwrap();
        conn
    }

    #[test]
    fn cli_parses() {
        use clap::CommandFactory;
        Cli::command().debug_assert();
    }

    #[test]
    fn cmd_context_stub_returns_ok() {
        let args = cli::ContextArgs {
            description: "test".into(),
            json: false,
        };
        assert!(cmd_context(args).is_ok());
    }

    #[test]
    fn distill_empty_db_returns_zeros() {
        let conn = test_conn();
        let out = run_distill(&conn, chrono::Utc::now()).unwrap();
        assert_eq!(out.decayed, 0);
        assert_eq!(out.pruned, 0);
        assert_eq!(out.transitioned, 0);
    }

    #[test]
    fn distill_recalculates_confidence() {
        let conn = test_conn();
        db::procedural::insert(&conn, "r1", "rule one", None).unwrap();
        for _ in 0..10 {
            db::procedural::record_success(&conn, "r1").unwrap();
        }

        let out = run_distill(&conn, chrono::Utc::now()).unwrap();
        assert_eq!(out.decayed, 1);

        let rule = db::procedural::get(&conn, "r1").unwrap().unwrap();
        assert!(rule.confidence > 0.8);
        assert!(rule.is_proven);
    }

    #[test]
    fn distill_marks_anti_pattern() {
        let conn = test_conn();
        db::procedural::insert(&conn, "r1", "bad rule", None).unwrap();
        for _ in 0..3 {
            db::procedural::record_failure(&conn, "r1").unwrap();
        }

        let out = run_distill(&conn, chrono::Utc::now()).unwrap();
        assert_eq!(out.decayed, 1);
        assert_eq!(out.transitioned, 1); // became anti-pattern

        let rule = db::procedural::get(&conn, "r1").unwrap().unwrap();
        assert!(rule.is_anti_pattern);
    }

    #[test]
    fn distill_prunes_low_confidence_non_anti_patterns() {
        let conn = test_conn();
        // Rule with no validations and old timestamp → will decay to ~0
        db::procedural::insert(&conn, "stale", "stale rule", None).unwrap();
        conn.execute(
            "UPDATE procedural SET success_count = 1, last_validated = '2020-01-01 00:00:00' WHERE id = 'stale'",
            [],
        ).unwrap();

        let now = chrono::TimeZone::with_ymd_and_hms(&chrono::Utc, 2026, 4, 1, 0, 0, 0).unwrap();
        let out = run_distill(&conn, now).unwrap();
        assert_eq!(out.pruned, 1);
        assert!(db::procedural::get(&conn, "stale").unwrap().is_none());
    }

    #[test]
    fn distill_preserves_anti_patterns_even_with_low_confidence() {
        let conn = test_conn();
        db::procedural::insert(&conn, "anti", "harmful rule", None).unwrap();
        // 3 failures makes it an anti-pattern, but confidence will be very negative
        for _ in 0..3 {
            db::procedural::record_failure(&conn, "anti").unwrap();
        }

        let out = run_distill(&conn, chrono::Utc::now()).unwrap();
        // Anti-pattern should NOT be pruned even though confidence < 0.05
        assert_eq!(out.pruned, 0);
        assert!(db::procedural::get(&conn, "anti").unwrap().is_some());
        assert!(db::procedural::get(&conn, "anti").unwrap().unwrap().is_anti_pattern);
    }

    #[test]
    fn distill_counts_transitions_correctly() {
        let conn = test_conn();
        // Rule that will become proven
        db::procedural::insert(&conn, "good", "good rule", None).unwrap();
        for _ in 0..10 {
            db::procedural::record_success(&conn, "good").unwrap();
        }
        // Rule that will become anti-pattern
        db::procedural::insert(&conn, "bad", "bad rule", None).unwrap();
        for _ in 0..4 {
            db::procedural::record_failure(&conn, "bad").unwrap();
        }
        // Rule that stays the same (no changes)
        db::procedural::insert(&conn, "neutral", "neutral rule", None).unwrap();

        let out = run_distill(&conn, chrono::Utc::now()).unwrap();
        assert_eq!(out.transitioned, 2); // good→proven, bad→anti_pattern
    }

    #[test]
    fn distill_output_serializes_correctly() {
        let conn = test_conn();
        db::procedural::insert(&conn, "r1", "rule", None).unwrap();
        let out = run_distill(&conn, chrono::Utc::now()).unwrap();
        let json: serde_json::Value = serde_json::to_value(&out).unwrap();
        assert!(json["decayed"].is_u64());
        assert!(json["pruned"].is_u64());
        assert!(json["transitioned"].is_u64());
    }

    #[test]
    fn distill_no_transition_on_second_run() {
        let conn = test_conn();
        db::procedural::insert(&conn, "r1", "rule", None).unwrap();
        for _ in 0..10 {
            db::procedural::record_success(&conn, "r1").unwrap();
        }

        let now = chrono::Utc::now();
        // First run: transitions from not-proven to proven
        let out1 = run_distill(&conn, now).unwrap();
        assert_eq!(out1.transitioned, 1);

        // Second run: no transition since already proven
        let out2 = run_distill(&conn, now).unwrap();
        assert_eq!(out2.transitioned, 0);
    }

    #[test]
    fn ingest_inserts_episodic_and_working() {
        let conn = test_conn();
        let args = cli::IngestArgs {
            task: "T-1".into(),
            body: "did stuff".into(),
            agent: "agent-0".into(),
            success: vec![],
            harm: vec![],
        };
        let out = run_ingest(&conn, &args).unwrap();

        assert!(out.episodic_id.is_some());
        assert!(out.validated_rules.is_empty());
        assert!(out.proposed_rules.is_empty());

        // Verify episodic row
        let eps = db::episodic::recent(&conn, 1).unwrap();
        assert_eq!(eps.len(), 1);
        assert_eq!(eps[0].content, "did stuff");
        assert_eq!(eps[0].context.as_deref(), Some("T-1"));
        assert_eq!(eps[0].agent.as_deref(), Some("agent-0"));
        assert_eq!(eps[0].source.as_deref(), Some("ingest"));

        // Verify working row
        let ws = db::working::recent(&conn, 1).unwrap();
        assert_eq!(ws.len(), 1);
        assert_eq!(ws[0].summary, "did stuff");
        assert_eq!(ws[0].task_id.as_deref(), Some("T-1"));
        assert_eq!(ws[0].agent.as_deref(), Some("agent-0"));
    }

    #[test]
    fn ingest_records_success() {
        let conn = test_conn();
        db::procedural::insert(&conn, "rule-1", "always test", None).unwrap();

        let args = cli::IngestArgs {
            task: "T-1".into(),
            body: "tested it".into(),
            agent: "a".into(),
            success: vec!["rule-1".into()],
            harm: vec![],
        };
        let out = run_ingest(&conn, &args).unwrap();
        assert_eq!(out.validated_rules, vec!["rule-1"]);

        let rule = db::procedural::get(&conn, "rule-1").unwrap().unwrap();
        assert_eq!(rule.success_count, 1);
        assert!(rule.last_validated.is_some());
    }

    #[test]
    fn ingest_records_harm() {
        let conn = test_conn();
        db::procedural::insert(&conn, "rule-2", "never panic", None).unwrap();

        let args = cli::IngestArgs {
            task: "T-1".into(),
            body: "panicked".into(),
            agent: "a".into(),
            success: vec![],
            harm: vec!["rule-2".into()],
        };
        let out = run_ingest(&conn, &args).unwrap();
        assert_eq!(out.validated_rules, vec!["rule-2"]);

        let rule = db::procedural::get(&conn, "rule-2").unwrap().unwrap();
        assert_eq!(rule.failure_count, 1);
        assert!(rule.last_validated.is_some());
    }

    #[test]
    fn ingest_skips_missing_rules() {
        let conn = test_conn();
        let args = cli::IngestArgs {
            task: "T-1".into(),
            body: "body".into(),
            agent: "a".into(),
            success: vec!["nonexistent".into()],
            harm: vec!["also-missing".into()],
        };
        let out = run_ingest(&conn, &args).unwrap();
        assert!(out.validated_rules.is_empty());
    }

    #[test]
    fn ingest_success_and_harm_together() {
        let conn = test_conn();
        db::procedural::insert(&conn, "r-ok", "good rule", None).unwrap();
        db::procedural::insert(&conn, "r-bad", "bad rule", None).unwrap();

        let args = cli::IngestArgs {
            task: "T-1".into(),
            body: "mixed".into(),
            agent: "a".into(),
            success: vec!["r-ok".into()],
            harm: vec!["r-bad".into()],
        };
        let out = run_ingest(&conn, &args).unwrap();
        assert_eq!(out.validated_rules, vec!["r-ok", "r-bad"]);

        let ok = db::procedural::get(&conn, "r-ok").unwrap().unwrap();
        assert_eq!(ok.success_count, 1);
        assert_eq!(ok.failure_count, 0);

        let bad = db::procedural::get(&conn, "r-bad").unwrap().unwrap();
        assert_eq!(bad.success_count, 0);
        assert_eq!(bad.failure_count, 1);
    }

    #[test]
    fn ingest_output_serializes_correctly() {
        let conn = test_conn();
        let args = cli::IngestArgs {
            task: "T-1".into(),
            body: "hello".into(),
            agent: "a".into(),
            success: vec![],
            harm: vec![],
        };
        let out = run_ingest(&conn, &args).unwrap();
        let json: serde_json::Value = serde_json::to_value(&out).unwrap();
        assert!(json["episodic_id"].is_string());
        assert_eq!(json["proposed_rules"], serde_json::json!([]));
        assert_eq!(json["validated_rules"], serde_json::json!([]));
    }
}
