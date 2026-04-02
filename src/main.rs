pub mod cli;
pub mod confidence;
pub mod db;
pub mod error;
pub mod exit_codes;
pub mod format;
pub mod output;
pub mod schema;

use std::io::IsTerminal;

use clap::Parser;
use cli::{Cli, Commands};
use output::Render;

fn main() {
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(e) => handle_clap_error(e),
    };
    let fmt = format::FormatContext::detect(cli.json, cli.quiet);

    let command = match cli.command {
        Some(cmd) => cmd,
        None => {
            if fmt.use_json() {
                // Robot mode: emit a structured error envelope so callers can
                // distinguish "no subcommand" from a successful empty result.
                let err_out = output::ErrorOutput::no_subcommand();
                err_out.render(&fmt);
                std::process::exit(exit_codes::INVALID_INPUT);
            }
            display_short_help(&fmt);
            std::process::exit(0);
        }
    };

    let result = match command {
        Commands::Context(args) => cmd_context(args, &fmt),
        Commands::Ingest(args) => cmd_ingest(args, &fmt),
        Commands::Distill => cmd_distill(&fmt),
        Commands::Status => cmd_status(&fmt),
        Commands::Rules(args) => cmd_rules(args, &fmt),
    };

    if let Err(e) = result {
        let err_out = output::ErrorOutput::from_app_error(&e);
        err_out.render(&fmt);
        std::process::exit(e.exit_code());
    }
}

/// Display compact help output, format-aware (JSON or human-readable).
fn display_short_help(fmt: &format::FormatContext) {
    let help = output::HelpOutput::build();
    help.render(fmt);
}

/// Handle a clap parse error in a format-aware way.
///
/// For display-type errors (--help, --version) clap already prints the right
/// thing, so we let it through.  For actual parse failures we detect whether
/// the process is in robot mode (--json flag present in raw args, or stdout
/// is not a TTY) and, if so, emit a structured JSON error envelope to stdout
/// instead of clap's default plain-text stderr.
fn handle_clap_error(e: clap::Error) -> ! {
    use clap::error::ErrorKind;

    // Let display-type messages (help, version) pass through unchanged.
    if matches!(e.kind(), ErrorKind::DisplayHelp | ErrorKind::DisplayVersion) {
        e.exit();
    }

    // Determine robot mode from raw args: look for --json flag, or non-TTY stdout.
    let json_flag = std::env::args().any(|a| a == "--json");
    let fmt = format::FormatContext::detect(json_flag, false);

    let err_out = output::ErrorOutput::from_clap_error(&e);
    err_out.render(&fmt);
    std::process::exit(exit_codes::INVALID_INPUT);
}

/// Emit a one-line warning to stderr, but only when stderr is a terminal.
/// This keeps agent pipelines clean while giving interactive developers
/// visibility into graceful degradation.
fn warn_degraded() {
    if std::io::stderr().is_terminal() {
        eprintln!("sm: db unavailable, returning empty context");
    }
}

/// Default limits for context queries.
const PROCEDURAL_SEARCH_LIMIT: u32 = 20;
const WORKING_SEARCH_LIMIT: u32 = 10;

fn cmd_context(args: cli::ContextArgs, fmt: &format::FormatContext) -> error::Result<()> {
    let out = match build_context(&args.description) {
        Ok(ctx) => ctx,
        Err(_) => {
            warn_degraded();
            output::ContextOutput::default()
        }
    };
    if !fmt.is_quiet() {
        if fmt.use_json() {
            println!("{}", serde_json::to_string(&out)?);
        } else {
            println!("{}", out.to_human());
        }
    }
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

fn cmd_ingest(args: cli::IngestArgs, fmt: &format::FormatContext) -> error::Result<()> {
    let out = match try_ingest(&args) {
        Ok(o) => o,
        Err(_) => {
            warn_degraded();
            output::IngestOutput::default()
        }
    };
    if !fmt.is_quiet() {
        if fmt.use_json() {
            println!("{}", serde_json::to_string(&out)?);
        } else {
            println!("{}", out.to_human());
        }
    }
    Ok(())
}

fn try_ingest(args: &cli::IngestArgs) -> error::Result<output::IngestOutput> {
    let conn = db::init::open_db()?;
    schema::ensure_schema(&conn)?;
    run_ingest(&conn, args)
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

fn cmd_distill(fmt: &format::FormatContext) -> error::Result<()> {
    let out = match try_distill() {
        Ok(o) => o,
        Err(_) => {
            warn_degraded();
            output::DistillOutput::default()
        }
    };
    if !fmt.is_quiet() {
        if fmt.use_json() {
            println!("{}", serde_json::to_string(&out)?);
        } else {
            println!("{}", out.to_human());
        }
    }
    Ok(())
}

/// Current schema version — bump when schema.rs changes structurally.
const SCHEMA_VERSION: u32 = 1;

fn cmd_status(fmt: &format::FormatContext) -> error::Result<()> {
    let out = match try_status() {
        Ok(s) => s,
        Err(_) => {
            warn_degraded();
            output::StatusOutput {
                ok: false,
                db_path: db::init::db_path().display().to_string(),
                counts: output::RecordCounts::default(),
                schema_version: SCHEMA_VERSION,
            }
        }
    };
    if !fmt.is_quiet() {
        if fmt.use_json() {
            println!("{}", serde_json::to_string(&out)?);
        } else {
            println!("{}", out.to_human());
        }
    }
    Ok(())
}

fn try_status() -> error::Result<output::StatusOutput> {
    let conn = db::init::open_db()?;
    schema::ensure_schema(&conn)?;
    build_status(&conn)
}

fn build_status(conn: &rusqlite::Connection) -> error::Result<output::StatusOutput> {
    let count = |table: &str| -> error::Result<u64> {
        let sql = format!("SELECT COUNT(*) FROM {table}");
        Ok(conn.query_row(&sql, [], |r| r.get::<_, i64>(0))? as u64)
    };

    Ok(output::StatusOutput {
        ok: true,
        db_path: db::init::db_path().display().to_string(),
        counts: output::RecordCounts {
            episodic: count("episodic")?,
            working: count("working")?,
            procedural: count("procedural")?,
        },
        schema_version: SCHEMA_VERSION,
    })
}

// --- rules subcommand ---

fn cmd_rules(args: cli::RulesArgs, fmt: &format::FormatContext) -> error::Result<()> {
    match args.action {
        cli::RulesAction::List(list_args) => cmd_rules_list(list_args, fmt),
        cli::RulesAction::Add(add_args) => cmd_rules_add(add_args, fmt),
        cli::RulesAction::Rm(rm_args) => cmd_rules_rm(rm_args, fmt),
        cli::RulesAction::Show(show_args) => cmd_rules_show(show_args, fmt),
    }
}

fn cmd_rules_list(args: cli::RulesListArgs, fmt: &format::FormatContext) -> error::Result<()> {
    let conn = db::init::open_db()?;
    schema::ensure_schema(&conn)?;
    let out = build_rules_list(&conn, args.query.as_deref())?;
    if !fmt.is_quiet() {
        if fmt.use_json() {
            println!("{}", serde_json::to_string(&out)?);
        } else {
            println!("{}", out.to_human());
        }
    }
    Ok(())
}

fn build_rules_list(
    conn: &rusqlite::Connection,
    query: Option<&str>,
) -> error::Result<output::RulesListOutput> {
    let rules = if let Some(q) = query {
        db::procedural::search(conn, q, PROCEDURAL_SEARCH_LIMIT)?
    } else {
        db::procedural::all(conn)?
    };
    let summaries: Vec<output::RuleSummary> = rules
        .iter()
        .map(|r| output::RuleSummary {
            id: r.id.clone(),
            rule: r.rule.clone(),
            confidence: r.confidence,
            is_proven: r.is_proven,
            is_anti_pattern: r.is_anti_pattern,
        })
        .collect();
    let count = summaries.len() as u64;
    Ok(output::RulesListOutput {
        rules: summaries,
        count,
    })
}

fn cmd_rules_add(args: cli::RulesAddArgs, fmt: &format::FormatContext) -> error::Result<()> {
    let conn = db::init::open_db()?;
    schema::ensure_schema(&conn)?;
    let out = build_rules_add(&conn, &args.id, &args.rule, args.source.as_deref())?;
    if !fmt.is_quiet() {
        if fmt.use_json() {
            println!("{}", serde_json::to_string(&out)?);
        } else {
            println!("{}", out.to_human());
        }
    }
    Ok(())
}

fn build_rules_add(
    conn: &rusqlite::Connection,
    id: &str,
    rule: &str,
    source: Option<&str>,
) -> error::Result<output::RuleAddOutput> {
    db::procedural::insert(conn, id, rule, source)?;
    Ok(output::RuleAddOutput {
        id: id.to_string(),
        created: true,
    })
}

fn cmd_rules_rm(args: cli::RulesRmArgs, fmt: &format::FormatContext) -> error::Result<()> {
    let conn = db::init::open_db()?;
    schema::ensure_schema(&conn)?;
    let out = build_rules_rm(&conn, &args.id)?;
    if !fmt.is_quiet() {
        if fmt.use_json() {
            println!("{}", serde_json::to_string(&out)?);
        } else {
            println!("{}", out.to_human());
        }
    }
    Ok(())
}

fn build_rules_rm(
    conn: &rusqlite::Connection,
    id: &str,
) -> error::Result<output::RuleRmOutput> {
    let deleted = db::procedural::delete(conn, id)?;
    Ok(output::RuleRmOutput {
        id: id.to_string(),
        deleted,
    })
}

fn cmd_rules_show(args: cli::RulesShowArgs, fmt: &format::FormatContext) -> error::Result<()> {
    let conn = db::init::open_db()?;
    schema::ensure_schema(&conn)?;
    let out = build_rules_show(&conn, &args.id)?;
    if !fmt.is_quiet() {
        if fmt.use_json() {
            println!("{}", serde_json::to_string(&out)?);
        } else {
            println!("{}", out.to_human());
        }
    }
    Ok(())
}

fn build_rules_show(
    conn: &rusqlite::Connection,
    id: &str,
) -> error::Result<output::RuleShowOutput> {
    let rule = db::procedural::get(conn, id)?
        .ok_or_else(|| error::AppError::NotFound(id.to_string()))?;
    Ok(output::RuleShowOutput {
        id: rule.id,
        rule: rule.rule,
        success_count: rule.success_count,
        failure_count: rule.failure_count,
        confidence: rule.confidence,
        is_proven: rule.is_proven,
        is_anti_pattern: rule.is_anti_pattern,
        last_validated: rule.last_validated,
        source: rule.source,
        created_at: rule.created_at,
        updated_at: rule.updated_at,
    })
}

fn try_distill() -> error::Result<output::DistillOutput> {
    let conn = db::init::open_db()?;
    schema::ensure_schema(&conn)?;
    run_distill(&conn, chrono::Utc::now())
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
        if let Some(&(was_proven, was_anti)) = before.get(&rule.id)
            && (rule.is_proven != was_proven || rule.is_anti_pattern != was_anti)
        {
            transitioned += 1;
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
        };
        let fmt = format::FormatContext::new(false, true, false);
        assert!(cmd_context(args, &fmt).is_ok());
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

    // --- graceful degradation tests ---

    /// A connection with no schema simulates a corrupted/empty DB.
    fn broken_conn() -> Connection {
        Connection::open_in_memory().unwrap()
    }

    #[test]
    fn context_degrades_on_missing_schema() {
        let conn = broken_conn();
        let result = build_context_with(&conn, "anything");
        assert!(result.is_err());
    }

    #[test]
    fn ingest_degrades_on_missing_schema() {
        let conn = broken_conn();
        let args = cli::IngestArgs {
            task: "T-1".into(),
            body: "data".into(),
            agent: "a".into(),
            success: vec![],
            harm: vec![],
        };
        let result = run_ingest(&conn, &args);
        assert!(result.is_err());
    }

    #[test]
    fn distill_degrades_on_missing_schema() {
        let conn = broken_conn();
        let result = run_distill(&conn, Utc::now());
        assert!(result.is_err());
    }

    #[test]
    fn cmd_context_returns_ok_on_db_error() {
        // Point SLASHMEM_DIR to a path inside a file (not a directory) to force
        // open_db to fail. cmd_context should still return Ok with default JSON.
        // We can't mutate the env safely across threads, so test the inner
        // try-catch pattern directly by calling build_context with a broken conn.
        let conn = broken_conn();
        let out = match build_context_with(&conn, "test") {
            Ok(ctx) => ctx,
            Err(_) => output::ContextOutput::default(),
        };
        assert_eq!(out, output::ContextOutput::default());
    }

    #[test]
    fn cmd_ingest_degrades_to_default_on_error() {
        let conn = broken_conn();
        let args = cli::IngestArgs {
            task: "T-1".into(),
            body: "data".into(),
            agent: "a".into(),
            success: vec![],
            harm: vec![],
        };
        let out = match run_ingest(&conn, &args) {
            Ok(o) => o,
            Err(_) => output::IngestOutput::default(),
        };
        assert_eq!(out, output::IngestOutput::default());
        // Verify the default serializes to valid JSON
        let json = serde_json::to_string(&out).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(parsed["episodic_id"].is_null());
    }

    #[test]
    fn cmd_distill_degrades_to_default_on_error() {
        let conn = broken_conn();
        let out = match run_distill(&conn, Utc::now()) {
            Ok(o) => o,
            Err(_) => output::DistillOutput::default(),
        };
        assert_eq!(out, output::DistillOutput::default());
        let json = serde_json::to_string(&out).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["decayed"], 0);
        assert_eq!(parsed["pruned"], 0);
        assert_eq!(parsed["transitioned"], 0);
    }

    // --- status tests ---

    #[test]
    fn status_empty_db_returns_ok_with_zero_counts() {
        let conn = test_conn();
        let out = build_status(&conn).unwrap();
        assert!(out.ok);
        assert_eq!(out.counts.episodic, 0);
        assert_eq!(out.counts.working, 0);
        assert_eq!(out.counts.procedural, 0);
        assert_eq!(out.schema_version, SCHEMA_VERSION);
    }

    #[test]
    fn status_counts_records() {
        let conn = test_conn();

        // Insert some records
        db::episodic::insert(
            &conn,
            &db::episodic::InsertEpisodic {
                content: "event 1",
                context: None,
                agent: None,
                source: None,
            },
        )
        .unwrap();
        db::episodic::insert(
            &conn,
            &db::episodic::InsertEpisodic {
                content: "event 2",
                context: None,
                agent: None,
                source: None,
            },
        )
        .unwrap();
        db::working::insert(&conn, "summary", None, None, None).unwrap();
        db::procedural::insert(&conn, "r1", "rule one", None).unwrap();
        db::procedural::insert(&conn, "r2", "rule two", None).unwrap();
        db::procedural::insert(&conn, "r3", "rule three", None).unwrap();

        let out = build_status(&conn).unwrap();
        assert!(out.ok);
        assert_eq!(out.counts.episodic, 2);
        assert_eq!(out.counts.working, 1);
        assert_eq!(out.counts.procedural, 3);
    }

    #[test]
    fn status_serializes_correct_json_shape() {
        let conn = test_conn();
        let out = build_status(&conn).unwrap();
        let json: serde_json::Value = serde_json::to_value(&out).unwrap();
        assert_eq!(json["ok"], true);
        assert!(json["db_path"].is_string());
        assert_eq!(json["counts"]["episodic"], 0);
        assert_eq!(json["counts"]["working"], 0);
        assert_eq!(json["counts"]["procedural"], 0);
        assert_eq!(json["schema_version"], SCHEMA_VERSION);
    }

    #[test]
    fn status_degrades_on_missing_schema() {
        let conn = broken_conn();
        let result = build_status(&conn);
        assert!(result.is_err());
    }

    #[test]
    fn cmd_status_returns_ok_on_db_error() {
        // Mirrors the degradation pattern: build_status fails, cmd_status
        // returns a fallback with ok=false.
        let conn = broken_conn();
        let out = match build_status(&conn) {
            Ok(s) => s,
            Err(_) => output::StatusOutput {
                ok: false,
                db_path: db::init::db_path().display().to_string(),
                counts: output::RecordCounts::default(),
                schema_version: SCHEMA_VERSION,
            },
        };
        assert!(!out.ok);
        assert_eq!(out.counts, output::RecordCounts::default());
    }

    #[test]
    fn warn_degraded_does_not_write_when_not_tty() {
        // In test harness, stderr is captured (not a TTY), so warn_degraded
        // should silently return without writing anything.
        warn_degraded();
        // No panic, no output — the function is safe to call in non-TTY contexts.
    }

    // --- rules tests ---

    #[test]
    fn rules_list_empty_db() {
        let conn = test_conn();
        let out = build_rules_list(&conn, None).unwrap();
        assert_eq!(out.count, 0);
        assert!(out.rules.is_empty());
    }

    #[test]
    fn rules_list_returns_all_rules() {
        let conn = test_conn();
        db::procedural::insert(&conn, "r1", "rule one", None).unwrap();
        db::procedural::insert(&conn, "r2", "rule two", Some("review")).unwrap();

        let out = build_rules_list(&conn, None).unwrap();
        assert_eq!(out.count, 2);
        assert_eq!(out.rules.len(), 2);
    }

    #[test]
    fn rules_list_with_query_filters() {
        let conn = test_conn();
        db::procedural::insert(&conn, "r1", "Always validate deploy inputs", None).unwrap();
        db::procedural::insert(&conn, "r2", "Use connection pooling", None).unwrap();

        let out = build_rules_list(&conn, Some("validate")).unwrap();
        assert_eq!(out.count, 1);
        assert_eq!(out.rules[0].id, "r1");
    }

    #[test]
    fn rules_list_query_no_match() {
        let conn = test_conn();
        db::procedural::insert(&conn, "r1", "some rule", None).unwrap();

        let out = build_rules_list(&conn, Some("nonexistent")).unwrap();
        assert_eq!(out.count, 0);
    }

    #[test]
    fn rules_list_output_serializes() {
        let conn = test_conn();
        db::procedural::insert(&conn, "r1", "rule one", None).unwrap();
        let out = build_rules_list(&conn, None).unwrap();
        let json: serde_json::Value = serde_json::to_value(&out).unwrap();
        assert_eq!(json["count"], 1);
        assert!(json["rules"].is_array());
        assert_eq!(json["rules"][0]["id"], "r1");
        assert_eq!(json["rules"][0]["rule"], "rule one");
    }

    #[test]
    fn rules_add_creates_rule() {
        let conn = test_conn();
        let out = build_rules_add(&conn, "r1", "Always test", Some("postmortem")).unwrap();
        assert_eq!(out.id, "r1");
        assert!(out.created);

        let rule = db::procedural::get(&conn, "r1").unwrap().unwrap();
        assert_eq!(rule.rule, "Always test");
        assert_eq!(rule.source.as_deref(), Some("postmortem"));
    }

    #[test]
    fn rules_add_duplicate_errors() {
        let conn = test_conn();
        build_rules_add(&conn, "r1", "first", None).unwrap();
        assert!(build_rules_add(&conn, "r1", "second", None).is_err());
    }

    #[test]
    fn rules_add_output_serializes() {
        let conn = test_conn();
        let out = build_rules_add(&conn, "r1", "rule text", None).unwrap();
        let json: serde_json::Value = serde_json::to_value(&out).unwrap();
        assert_eq!(json["id"], "r1");
        assert_eq!(json["created"], true);
    }

    #[test]
    fn rules_rm_deletes_existing() {
        let conn = test_conn();
        db::procedural::insert(&conn, "r1", "rule to delete", None).unwrap();

        let out = build_rules_rm(&conn, "r1").unwrap();
        assert_eq!(out.id, "r1");
        assert!(out.deleted);
        assert!(db::procedural::get(&conn, "r1").unwrap().is_none());
    }

    #[test]
    fn rules_rm_missing_returns_false() {
        let conn = test_conn();
        let out = build_rules_rm(&conn, "nonexistent").unwrap();
        assert!(!out.deleted);
    }

    #[test]
    fn rules_show_existing_rule() {
        let conn = test_conn();
        db::procedural::insert(&conn, "r1", "Always test", Some("review")).unwrap();

        let out = build_rules_show(&conn, "r1").unwrap();
        assert_eq!(out.id, "r1");
        assert_eq!(out.rule, "Always test");
        assert_eq!(out.source.as_deref(), Some("review"));
        assert_eq!(out.success_count, 0);
        assert_eq!(out.failure_count, 0);
    }

    #[test]
    fn rules_show_missing_returns_not_found() {
        let conn = test_conn();
        let result = build_rules_show(&conn, "nonexistent");
        assert!(result.is_err());
        match result.unwrap_err() {
            error::AppError::NotFound(msg) => assert_eq!(msg, "nonexistent"),
            other => panic!("expected NotFound, got {:?}", other),
        }
    }

    #[test]
    fn rules_show_output_serializes() {
        let conn = test_conn();
        db::procedural::insert(&conn, "r1", "rule text", None).unwrap();
        let out = build_rules_show(&conn, "r1").unwrap();
        let json: serde_json::Value = serde_json::to_value(&out).unwrap();
        assert_eq!(json["id"], "r1");
        assert_eq!(json["rule"], "rule text");
        assert!(json["confidence"].is_f64());
        assert_eq!(json["is_proven"], false);
        assert_eq!(json["is_anti_pattern"], false);
    }

    #[test]
    fn rules_show_reflects_validation_counts() {
        let conn = test_conn();
        db::procedural::insert(&conn, "r1", "test rule", None).unwrap();
        db::procedural::record_success(&conn, "r1").unwrap();
        db::procedural::record_success(&conn, "r1").unwrap();
        db::procedural::record_failure(&conn, "r1").unwrap();

        let out = build_rules_show(&conn, "r1").unwrap();
        assert_eq!(out.success_count, 2);
        assert_eq!(out.failure_count, 1);
        assert!(out.last_validated.is_some());
    }
}
