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

/// Default limits for context queries.
const PROCEDURAL_SEARCH_LIMIT: u32 = 20;
const WORKING_SEARCH_LIMIT: u32 = 10;

fn cmd_context(args: cli::ContextArgs) -> error::Result<()> {
    let out = match build_context(&args.description) {
        Ok(ctx) => ctx,
        Err(e) => {
            eprintln!("sm context: {e}");
            output::ContextOutput::default()
        }
    };
    println!("{}", serde_json::to_string(&out)?);
    Ok(())
}

fn build_context(description: &str) -> error::Result<output::ContextOutput> {
    let conn = db::init::open_db()?;
    schema::ensure_schema(&conn)?;
    build_context_with(&conn, description)
}

fn build_context_with(
    conn: &rusqlite::Connection,
    description: &str,
) -> error::Result<output::ContextOutput> {
    // FTS5 search on procedural rules, split into proven rules and anti-patterns.
    let procedural_hits = db::procedural::search(conn, description, PROCEDURAL_SEARCH_LIMIT)?;

    let mut relevant_rules = Vec::new();
    let mut anti_patterns = Vec::new();
    for rule in &procedural_hits {
        if rule.is_anti_pattern {
            anti_patterns.push(rule.rule.clone());
        }
        if rule.is_proven {
            relevant_rules.push(rule.rule.clone());
        }
    }

    // FTS5 search on working memory, take N most-recent.
    let working_hits = db::working::search(conn, description, WORKING_SEARCH_LIMIT)?;
    let history_snippets: Vec<String> = working_hits.into_iter().map(|e| e.summary).collect();

    Ok(output::ContextOutput {
        relevant_rules,
        anti_patterns,
        history_snippets,
    })
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
    // TODO: implement in slashmem-6188acc3-1su.3
    let out = output::DistillOutput::default();
    println!("{}", serde_json::to_string(&out).unwrap());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use rusqlite::Connection;

    fn setup() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        schema::ensure_schema(&conn).unwrap();
        conn
    }

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
    fn cmd_distill_stub_returns_ok() {
        assert!(cmd_distill().is_ok());
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

    // --- build_context_with tests ---

    #[test]
    fn context_empty_db_returns_empty_output() {
        let conn = setup();
        let out = build_context_with(&conn, "anything").unwrap();
        assert_eq!(out, output::ContextOutput::default());
    }

    #[test]
    fn context_returns_proven_rules() {
        let conn = setup();
        db::procedural::insert(&conn, "r1", "Always validate deploy inputs", None).unwrap();
        // Make it proven: 10 successes
        for _ in 0..10 {
            db::procedural::record_success(&conn, "r1").unwrap();
        }
        db::procedural::recalculate_confidence(&conn, Utc::now()).unwrap();

        let out = build_context_with(&conn, "validate deploy").unwrap();
        assert_eq!(out.relevant_rules, vec!["Always validate deploy inputs"]);
        assert!(out.anti_patterns.is_empty());
    }

    #[test]
    fn context_returns_anti_patterns() {
        let conn = setup();
        db::procedural::insert(&conn, "r1", "Skip deploy tests for speed", None).unwrap();
        // Make it an anti-pattern: 4 failures
        for _ in 0..4 {
            db::procedural::record_failure(&conn, "r1").unwrap();
        }
        db::procedural::recalculate_confidence(&conn, Utc::now()).unwrap();

        let out = build_context_with(&conn, "deploy tests").unwrap();
        assert_eq!(out.anti_patterns, vec!["Skip deploy tests for speed"]);
        assert!(out.relevant_rules.is_empty());
    }

    #[test]
    fn context_separates_proven_and_anti_pattern() {
        let conn = setup();
        db::procedural::insert(&conn, "good", "Always run deploy checks", None).unwrap();
        db::procedural::insert(&conn, "bad", "Skip deploy validation", None).unwrap();

        for _ in 0..10 {
            db::procedural::record_success(&conn, "good").unwrap();
        }
        for _ in 0..4 {
            db::procedural::record_failure(&conn, "bad").unwrap();
        }
        db::procedural::recalculate_confidence(&conn, Utc::now()).unwrap();

        let out = build_context_with(&conn, "deploy").unwrap();
        assert!(out.relevant_rules.contains(&"Always run deploy checks".to_string()));
        assert!(out.anti_patterns.contains(&"Skip deploy validation".to_string()));
    }

    #[test]
    fn context_excludes_unclassified_rules() {
        let conn = setup();
        // A rule with no successes or failures stays at default confidence (0.5)
        // — not proven (needs >0.8) and not anti-pattern (needs >=3 failures).
        db::procedural::insert(&conn, "r1", "Some deploy guideline", None).unwrap();

        let out = build_context_with(&conn, "deploy").unwrap();
        assert!(out.relevant_rules.is_empty());
        assert!(out.anti_patterns.is_empty());
    }

    #[test]
    fn context_returns_working_summaries() {
        let conn = setup();
        db::working::insert(&conn, "Fixed auth token refresh", Some("body"), Some("T-1"), None).unwrap();
        db::working::insert(&conn, "Unrelated deploy work", None, None, None).unwrap();

        let out = build_context_with(&conn, "auth token").unwrap();
        assert_eq!(out.history_snippets, vec!["Fixed auth token refresh"]);
    }

    #[test]
    fn context_no_matching_working_entries() {
        let conn = setup();
        db::working::insert(&conn, "deploy pipeline fix", None, None, None).unwrap();

        let out = build_context_with(&conn, "authentication").unwrap();
        assert!(out.history_snippets.is_empty());
    }

    #[test]
    fn context_combined_procedural_and_working() {
        let conn = setup();
        // Proven procedural rule
        db::procedural::insert(&conn, "r1", "Always check tokens before API calls", None).unwrap();
        for _ in 0..10 {
            db::procedural::record_success(&conn, "r1").unwrap();
        }
        db::procedural::recalculate_confidence(&conn, Utc::now()).unwrap();

        // Working memory
        db::working::insert(&conn, "Refactored tokens validation", None, None, None).unwrap();

        let out = build_context_with(&conn, "tokens").unwrap();
        assert!(!out.relevant_rules.is_empty());
        assert!(!out.history_snippets.is_empty());
    }

    #[test]
    fn context_graceful_on_bad_fts_query() {
        let conn = setup();
        // FTS5 returns a syntax error for empty queries. cmd_context catches errors
        // and falls back to ContextOutput::default(). Mirror that logic here.
        let out = match build_context_with(&conn, "") {
            Ok(ctx) => ctx,
            Err(_) => output::ContextOutput::default(),
        };
        assert!(out.relevant_rules.is_empty());
        assert!(out.anti_patterns.is_empty());
        assert!(out.history_snippets.is_empty());
    }
}
