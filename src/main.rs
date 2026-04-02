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

fn cmd_ingest(_args: cli::IngestArgs) -> error::Result<()> {
    // TODO: implement in slashmem-6188acc3-1su.2
    let out = output::IngestOutput::default();
    println!("{}", serde_json::to_string(&out).unwrap());
    Ok(())
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

    #[test]
    fn cli_parses() {
        use clap::CommandFactory;
        Cli::command().debug_assert();
    }

    #[test]
    fn cmd_ingest_stub_returns_ok() {
        let args = cli::IngestArgs {
            task: "T-1".into(),
            body: "body".into(),
            agent: "a".into(),
            success: vec![],
            harm: vec![],
        };
        assert!(cmd_ingest(args).is_ok());
    }

    #[test]
    fn cmd_distill_stub_returns_ok() {
        assert!(cmd_distill().is_ok());
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
    fn context_output_serializes_to_json() {
        let out = output::ContextOutput {
            relevant_rules: vec!["rule1".into()],
            anti_patterns: vec!["anti1".into()],
            history_snippets: vec!["snippet1".into()],
        };
        let json = serde_json::to_string(&out).unwrap();
        assert!(json.contains("\"relevant_rules\""));
        assert!(json.contains("\"anti_patterns\""));
        assert!(json.contains("\"history_snippets\""));
    }

    #[test]
    fn context_graceful_on_bad_fts_query() {
        let conn = setup();
        // FTS5 with empty query or special chars — should not panic
        let result = build_context_with(&conn, "");
        // Empty query may error in FTS5 — that's ok, we just need no panic
        // If it errors, the cmd_context wrapper catches it
        drop(result);
    }
}
