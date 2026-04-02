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
}
