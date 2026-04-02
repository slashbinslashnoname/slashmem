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
    // TODO: implement in slashmem-6188acc3-1su.3
    let out = output::DistillOutput::default();
    println!("{}", serde_json::to_string(&out).unwrap());
    Ok(())
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
}
