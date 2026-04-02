use std::io::Write;

use serde::Serialize;

use crate::error::AppError;
use crate::format::FormatContext;

/// Trait for rendering output according to FormatContext.
pub trait Render {
    /// Write this value to the appropriate output stream.
    fn render(&self, fmt: &FormatContext);
}

/// Inner detail of a structured error envelope.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ErrorDetail {
    pub code: String,
    pub message: String,
    pub suggestions: Vec<String>,
    pub exit_code: i32,
}

/// Structured error envelope for JSON mode.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ErrorOutput {
    pub error: ErrorDetail,
}

impl ErrorOutput {
    /// Build an ErrorOutput from an AppError.
    pub fn from_app_error(e: &AppError) -> Self {
        Self {
            error: ErrorDetail {
                code: e.error_code().to_string(),
                message: e.to_string(),
                suggestions: e.suggestions().into_iter().map(|s| s.to_string()).collect(),
                exit_code: e.exit_code(),
            },
        }
    }
}

impl Render for ErrorOutput {
    fn render(&self, fmt: &FormatContext) {
        if fmt.use_json() {
            // Structured JSON to stdout
            if let Ok(json) = serde_json::to_string(self) {
                println!("{json}");
            }
        } else {
            // Human-readable to stderr
            let d = &self.error;
            let _ = writeln!(std::io::stderr(), "error [{}]: {}", d.code, d.message);
            for s in &d.suggestions {
                let _ = writeln!(std::io::stderr(), "  → {s}");
            }
        }
    }
}

/// JSON output for the `context` command.
#[derive(Debug, Default, Serialize, PartialEq)]
pub struct ContextOutput {
    pub relevant_rules: Vec<String>,
    pub anti_patterns: Vec<String>,
    pub history_snippets: Vec<String>,
}

/// JSON output for the `ingest` command.
#[derive(Debug, Default, Serialize, PartialEq)]
pub struct IngestOutput {
    pub episodic_id: Option<String>,
    pub proposed_rules: Vec<String>,
    pub validated_rules: Vec<String>,
}

/// JSON output for the `distill` command.
#[derive(Debug, Default, Serialize, PartialEq)]
pub struct DistillOutput {
    pub decayed: u64,
    pub pruned: u64,
    pub transitioned: u64,
}

/// Record counts per memory table.
#[derive(Debug, Default, Serialize, PartialEq)]
pub struct RecordCounts {
    pub episodic: u64,
    pub working: u64,
    pub procedural: u64,
}

/// JSON output for the `status` command.
#[derive(Debug, Serialize, PartialEq)]
pub struct StatusOutput {
    pub ok: bool,
    pub db_path: String,
    pub counts: RecordCounts,
    pub schema_version: u32,
}

/// Summary of a single procedural rule (used in list output).
#[derive(Debug, Serialize, PartialEq)]
pub struct RuleSummary {
    pub id: String,
    pub rule: String,
    pub confidence: f64,
    pub is_proven: bool,
    pub is_anti_pattern: bool,
}

/// JSON output for the `rules list` command.
#[derive(Debug, Default, Serialize, PartialEq)]
pub struct RulesListOutput {
    pub rules: Vec<RuleSummary>,
    pub count: u64,
}

impl RulesListOutput {
    /// Render as compact human-readable text.
    pub fn to_human(&self) -> String {
        if self.rules.is_empty() {
            return "no rules".to_string();
        }
        let mut lines: Vec<String> = Vec::with_capacity(self.rules.len() + 1);
        lines.push(format!("{} rule(s)", self.count));
        for r in &self.rules {
            let flags = match (r.is_proven, r.is_anti_pattern) {
                (true, _) => " [proven]",
                (_, true) => " [anti]",
                _ => "",
            };
            lines.push(format!(
                "  {} | C={:.2}{} | {}",
                r.id, r.confidence, flags, r.rule
            ));
        }
        lines.join("\n")
    }
}

/// JSON output for the `rules show` command.
#[derive(Debug, Serialize, PartialEq)]
pub struct RuleShowOutput {
    pub id: String,
    pub rule: String,
    pub success_count: u32,
    pub failure_count: u32,
    pub confidence: f64,
    pub is_proven: bool,
    pub is_anti_pattern: bool,
    pub last_validated: Option<String>,
    pub source: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

impl RuleShowOutput {
    /// Render as compact human-readable text.
    pub fn to_human(&self) -> String {
        let flags = match (self.is_proven, self.is_anti_pattern) {
            (true, _) => " [proven]",
            (_, true) => " [anti]",
            _ => "",
        };
        format!(
            "{} | C={:.2}{} | S={} F={} | src={} | {}\n  {}",
            self.id,
            self.confidence,
            flags,
            self.success_count,
            self.failure_count,
            self.source.as_deref().unwrap_or("-"),
            self.created_at,
            self.rule,
        )
    }
}

/// JSON output for the `rules add` command.
#[derive(Debug, Serialize, PartialEq)]
pub struct RuleAddOutput {
    pub id: String,
    pub created: bool,
}

impl RuleAddOutput {
    pub fn to_human(&self) -> String {
        format!("added rule {}", self.id)
    }
}

/// JSON output for the `rules rm` command.
#[derive(Debug, Serialize, PartialEq)]
pub struct RuleRmOutput {
    pub id: String,
    pub deleted: bool,
}

impl RuleRmOutput {
    pub fn to_human(&self) -> String {
        if self.deleted {
            format!("removed rule {}", self.id)
        } else {
            format!("rule {} not found", self.id)
        }
    }
}

impl StatusOutput {
    /// Render as compact human-readable text.
    pub fn to_human(&self) -> String {
        let status = if self.ok { "ok" } else { "ERR" };
        format!(
            "{} | db: {} | episodic: {}, working: {}, procedural: {} | schema: v{}",
            status,
            self.db_path,
            self.counts.episodic,
            self.counts.working,
            self.counts.procedural,
            self.schema_version,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn context_output_default_serializes() {
        let out = ContextOutput::default();
        let json = serde_json::to_value(&out).unwrap();
        assert_eq!(json["relevant_rules"], serde_json::json!([]));
        assert_eq!(json["anti_patterns"], serde_json::json!([]));
        assert_eq!(json["history_snippets"], serde_json::json!([]));
    }

    #[test]
    fn context_output_field_names_locked() {
        let out = ContextOutput {
            relevant_rules: vec!["r1".into()],
            anti_patterns: vec!["a1".into()],
            history_snippets: vec!["h1".into()],
        };
        let json = serde_json::to_string(&out).unwrap();
        assert!(json.contains("\"relevant_rules\""));
        assert!(json.contains("\"anti_patterns\""));
        assert!(json.contains("\"history_snippets\""));
    }

    #[test]
    fn ingest_output_default_serializes() {
        let out = IngestOutput::default();
        let json = serde_json::to_value(&out).unwrap();
        assert_eq!(json["episodic_id"], serde_json::Value::Null);
        assert_eq!(json["proposed_rules"], serde_json::json!([]));
        assert_eq!(json["validated_rules"], serde_json::json!([]));
    }

    #[test]
    fn ingest_output_with_values() {
        let out = IngestOutput {
            episodic_id: Some("ep-123".into()),
            proposed_rules: vec!["p1".into()],
            validated_rules: vec!["v1".into(), "v2".into()],
        };
        let json = serde_json::to_value(&out).unwrap();
        assert_eq!(json["episodic_id"], "ep-123");
        assert_eq!(json["proposed_rules"], serde_json::json!(["p1"]));
        assert_eq!(json["validated_rules"], serde_json::json!(["v1", "v2"]));
    }

    #[test]
    fn distill_output_default_serializes() {
        let out = DistillOutput::default();
        let json = serde_json::to_value(&out).unwrap();
        assert_eq!(json["decayed"], 0);
        assert_eq!(json["pruned"], 0);
        assert_eq!(json["transitioned"], 0);
    }

    #[test]
    fn distill_output_with_values() {
        let out = DistillOutput {
            decayed: 5,
            pruned: 3,
            transitioned: 1,
        };
        let json = serde_json::to_value(&out).unwrap();
        assert_eq!(json["decayed"], 5);
        assert_eq!(json["pruned"], 3);
        assert_eq!(json["transitioned"], 1);
    }

    // --- ErrorOutput / ErrorDetail tests ---

    #[test]
    fn error_output_from_not_found() {
        let err = AppError::NotFound("rule-42".into());
        let out = ErrorOutput::from_app_error(&err);
        assert_eq!(out.error.code, "NOT_FOUND");
        assert_eq!(out.error.message, "not found: rule-42");
        assert_eq!(out.error.exit_code, 1);
        assert!(!out.error.suggestions.is_empty());
    }

    #[test]
    fn error_output_from_invalid_input() {
        let err = AppError::InvalidInput("missing arg".into());
        let out = ErrorOutput::from_app_error(&err);
        assert_eq!(out.error.code, "INVALID_INPUT");
        assert_eq!(out.error.exit_code, 2);
    }

    #[test]
    fn error_output_from_database() {
        let err: AppError = rusqlite::Error::InvalidColumnName("x".into()).into();
        let out = ErrorOutput::from_app_error(&err);
        assert_eq!(out.error.code, "DB_ERROR");
        assert_eq!(out.error.exit_code, 3);
    }

    #[test]
    fn error_output_from_io() {
        let err: AppError = std::io::Error::new(std::io::ErrorKind::NotFound, "gone").into();
        let out = ErrorOutput::from_app_error(&err);
        assert_eq!(out.error.code, "IO_ERROR");
        assert_eq!(out.error.exit_code, 4);
    }

    #[test]
    fn error_output_from_json_parse() {
        let err: AppError = serde_json::from_str::<String>("bad").unwrap_err().into();
        let out = ErrorOutput::from_app_error(&err);
        assert_eq!(out.error.code, "PARSE_ERROR");
        assert_eq!(out.error.exit_code, 5);
    }

    #[test]
    fn error_output_from_other() {
        let err = AppError::Other("kaboom".into());
        let out = ErrorOutput::from_app_error(&err);
        assert_eq!(out.error.code, "INTERNAL");
        assert_eq!(out.error.exit_code, 127);
    }

    #[test]
    fn error_output_json_envelope_shape() {
        let err = AppError::NotFound("rule-42".into());
        let out = ErrorOutput::from_app_error(&err);
        let json: serde_json::Value = serde_json::to_value(&out).unwrap();

        // Must have top-level "error" key
        assert!(json.get("error").is_some());
        let inner = &json["error"];
        assert_eq!(inner["code"], "NOT_FOUND");
        assert_eq!(inner["message"], "not found: rule-42");
        assert!(inner["suggestions"].is_array());
        assert!(inner["exit_code"].is_i64());
        assert_eq!(inner["exit_code"], 1);
    }

    #[test]
    fn error_detail_field_count() {
        let err = AppError::NotFound("x".into());
        let out = ErrorOutput::from_app_error(&err);
        let json: serde_json::Value = serde_json::to_value(&out).unwrap();
        let inner = json["error"].as_object().unwrap();
        assert_eq!(inner.len(), 4, "ErrorDetail must have exactly 4 fields");
    }

    #[test]
    fn error_output_suggestions_are_strings() {
        let err = AppError::NotFound("x".into());
        let out = ErrorOutput::from_app_error(&err);
        for s in &out.error.suggestions {
            assert!(!s.is_empty());
        }
    }

    #[test]
    fn error_output_render_json_mode() {
        // In JSON mode, render produces valid JSON on stdout.
        // We can't easily capture stdout in tests, but we can verify the
        // serialization path doesn't panic.
        let err = AppError::NotFound("x".into());
        let out = ErrorOutput::from_app_error(&err);
        let fmt = FormatContext::new(false, true, false);
        out.render(&fmt); // should not panic
    }

    #[test]
    fn error_output_render_human_mode() {
        // In human mode, render writes to stderr.
        let err = AppError::NotFound("rule-42".into());
        let out = ErrorOutput::from_app_error(&err);
        let fmt = FormatContext::new(true, false, false);
        out.render(&fmt); // should not panic
    }

    #[test]
    fn error_output_all_variants_have_suggestions() {
        let errors: Vec<AppError> = vec![
            AppError::NotFound("x".into()),
            AppError::InvalidInput("x".into()),
            AppError::Database(rusqlite::Error::InvalidColumnName("x".into())),
            AppError::Json(serde_json::from_str::<String>("bad").unwrap_err()),
            AppError::Io(std::io::Error::new(std::io::ErrorKind::Other, "x")),
            AppError::Other("x".into()),
        ];
        for err in &errors {
            let out = ErrorOutput::from_app_error(err);
            assert!(
                !out.error.suggestions.is_empty(),
                "ErrorOutput for {:?} must have suggestions",
                err
            );
        }
    }

    #[test]
    fn status_output_serializes_json_shape() {
        let out = StatusOutput {
            ok: true,
            db_path: "/Users/slashbin/.slashmem/mem.db".into(),
            counts: RecordCounts {
                episodic: 142,
                working: 37,
                procedural: 18,
            },
            schema_version: 1,
        };
        let json = serde_json::to_value(&out).unwrap();
        assert_eq!(json["ok"], true);
        assert_eq!(json["db_path"], "/Users/slashbin/.slashmem/mem.db");
        assert_eq!(json["counts"]["episodic"], 142);
        assert_eq!(json["counts"]["working"], 37);
        assert_eq!(json["counts"]["procedural"], 18);
        assert_eq!(json["schema_version"], 1);
    }

    #[test]
    fn status_output_field_count() {
        let out = StatusOutput {
            ok: true,
            db_path: "/tmp/mem.db".into(),
            counts: RecordCounts::default(),
            schema_version: 1,
        };
        let map: serde_json::Map<String, serde_json::Value> =
            serde_json::from_str(&serde_json::to_string(&out).unwrap()).unwrap();
        assert_eq!(map.len(), 4); // ok, db_path, counts, schema_version
    }

    #[test]
    fn record_counts_default_is_zeros() {
        let counts = RecordCounts::default();
        assert_eq!(counts.episodic, 0);
        assert_eq!(counts.working, 0);
        assert_eq!(counts.procedural, 0);
    }

    #[test]
    fn status_to_human_ok() {
        let out = StatusOutput {
            ok: true,
            db_path: "~/.slashmem/mem.db".into(),
            counts: RecordCounts {
                episodic: 142,
                working: 37,
                procedural: 18,
            },
            schema_version: 1,
        };
        assert_eq!(
            out.to_human(),
            "ok | db: ~/.slashmem/mem.db | episodic: 142, working: 37, procedural: 18 | schema: v1"
        );
    }

    #[test]
    fn status_to_human_err() {
        let out = StatusOutput {
            ok: false,
            db_path: "/tmp/mem.db".into(),
            counts: RecordCounts::default(),
            schema_version: 1,
        };
        assert!(out.to_human().starts_with("ERR"));
    }

    #[test]
    fn all_outputs_roundtrip_exact_field_count() {
        // Ensure no extra fields sneak in
        let ctx: serde_json::Map<String, serde_json::Value> =
            serde_json::from_str(&serde_json::to_string(&ContextOutput::default()).unwrap()).unwrap();
        assert_eq!(ctx.len(), 3);

        let ing: serde_json::Map<String, serde_json::Value> =
            serde_json::from_str(&serde_json::to_string(&IngestOutput::default()).unwrap()).unwrap();
        assert_eq!(ing.len(), 3);

        let dis: serde_json::Map<String, serde_json::Value> =
            serde_json::from_str(&serde_json::to_string(&DistillOutput::default()).unwrap()).unwrap();
        assert_eq!(dis.len(), 3);
    }

    // --- rules output tests ---

    #[test]
    fn rules_list_output_default_serializes() {
        let out = RulesListOutput::default();
        let json = serde_json::to_value(&out).unwrap();
        assert_eq!(json["rules"], serde_json::json!([]));
        assert_eq!(json["count"], 0);
    }

    #[test]
    fn rules_list_output_with_rules() {
        let out = RulesListOutput {
            rules: vec![RuleSummary {
                id: "r1".into(),
                rule: "Always test".into(),
                confidence: 0.85,
                is_proven: true,
                is_anti_pattern: false,
            }],
            count: 1,
        };
        let json = serde_json::to_value(&out).unwrap();
        assert_eq!(json["count"], 1);
        assert_eq!(json["rules"][0]["id"], "r1");
        assert_eq!(json["rules"][0]["is_proven"], true);
    }

    #[test]
    fn rules_list_output_field_count() {
        let out = RulesListOutput::default();
        let map: serde_json::Map<String, serde_json::Value> =
            serde_json::from_str(&serde_json::to_string(&out).unwrap()).unwrap();
        assert_eq!(map.len(), 2); // rules, count
    }

    #[test]
    fn rule_summary_field_count() {
        let summary = RuleSummary {
            id: "r1".into(),
            rule: "test".into(),
            confidence: 0.5,
            is_proven: false,
            is_anti_pattern: false,
        };
        let map: serde_json::Map<String, serde_json::Value> =
            serde_json::from_str(&serde_json::to_string(&summary).unwrap()).unwrap();
        assert_eq!(map.len(), 5);
    }

    #[test]
    fn rules_list_to_human_empty() {
        let out = RulesListOutput::default();
        assert_eq!(out.to_human(), "no rules");
    }

    #[test]
    fn rules_list_to_human_with_rules() {
        let out = RulesListOutput {
            rules: vec![
                RuleSummary {
                    id: "r1".into(),
                    rule: "Always test".into(),
                    confidence: 0.85,
                    is_proven: true,
                    is_anti_pattern: false,
                },
                RuleSummary {
                    id: "r2".into(),
                    rule: "Skip reviews".into(),
                    confidence: -3.0,
                    is_proven: false,
                    is_anti_pattern: true,
                },
            ],
            count: 2,
        };
        let human = out.to_human();
        assert!(human.contains("2 rule(s)"));
        assert!(human.contains("[proven]"));
        assert!(human.contains("[anti]"));
    }

    #[test]
    fn rule_show_output_serializes() {
        let out = RuleShowOutput {
            id: "r1".into(),
            rule: "Always test".into(),
            success_count: 5,
            failure_count: 1,
            confidence: 0.75,
            is_proven: false,
            is_anti_pattern: false,
            last_validated: Some("2026-04-01".into()),
            source: Some("review".into()),
            created_at: "2026-01-01".into(),
            updated_at: "2026-04-01".into(),
        };
        let json = serde_json::to_value(&out).unwrap();
        assert_eq!(json["id"], "r1");
        assert_eq!(json["success_count"], 5);
        assert_eq!(json["failure_count"], 1);
        assert_eq!(json["source"], "review");
    }

    #[test]
    fn rule_show_output_field_count() {
        let out = RuleShowOutput {
            id: "r1".into(),
            rule: "test".into(),
            success_count: 0,
            failure_count: 0,
            confidence: 0.5,
            is_proven: false,
            is_anti_pattern: false,
            last_validated: None,
            source: None,
            created_at: "2026-01-01".into(),
            updated_at: "2026-01-01".into(),
        };
        let map: serde_json::Map<String, serde_json::Value> =
            serde_json::from_str(&serde_json::to_string(&out).unwrap()).unwrap();
        assert_eq!(map.len(), 11);
    }

    #[test]
    fn rule_add_output_serializes() {
        let out = RuleAddOutput {
            id: "r1".into(),
            created: true,
        };
        let json = serde_json::to_value(&out).unwrap();
        assert_eq!(json["id"], "r1");
        assert_eq!(json["created"], true);
    }

    #[test]
    fn rule_add_to_human() {
        let out = RuleAddOutput {
            id: "r1".into(),
            created: true,
        };
        assert_eq!(out.to_human(), "added rule r1");
    }

    #[test]
    fn rule_rm_output_serializes() {
        let out = RuleRmOutput {
            id: "r1".into(),
            deleted: true,
        };
        let json = serde_json::to_value(&out).unwrap();
        assert_eq!(json["id"], "r1");
        assert_eq!(json["deleted"], true);
    }

    #[test]
    fn rule_rm_to_human_deleted() {
        let out = RuleRmOutput {
            id: "r1".into(),
            deleted: true,
        };
        assert_eq!(out.to_human(), "removed rule r1");
    }

    #[test]
    fn rule_rm_to_human_not_found() {
        let out = RuleRmOutput {
            id: "r1".into(),
            deleted: false,
        };
        assert_eq!(out.to_human(), "rule r1 not found");
    }

    #[test]
    fn rule_show_to_human() {
        let out = RuleShowOutput {
            id: "r1".into(),
            rule: "Always test".into(),
            success_count: 5,
            failure_count: 0,
            confidence: 0.85,
            is_proven: true,
            is_anti_pattern: false,
            last_validated: None,
            source: Some("review".into()),
            created_at: "2026-01-01".into(),
            updated_at: "2026-04-01".into(),
        };
        let human = out.to_human();
        assert!(human.contains("r1"));
        assert!(human.contains("[proven]"));
        assert!(human.contains("Always test"));
        assert!(human.contains("src=review"));
    }
}
