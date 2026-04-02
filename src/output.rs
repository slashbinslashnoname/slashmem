use serde::Serialize;

/// JSON output for the `context` command.
#[derive(Debug, Serialize, PartialEq)]
pub struct ContextOutput {
    pub relevant_rules: Vec<String>,
    pub anti_patterns: Vec<String>,
    pub history_snippets: Vec<String>,
}

impl Default for ContextOutput {
    fn default() -> Self {
        Self {
            relevant_rules: vec![],
            anti_patterns: vec![],
            history_snippets: vec![],
        }
    }
}

/// JSON output for the `ingest` command.
#[derive(Debug, Serialize, PartialEq)]
pub struct IngestOutput {
    pub episodic_id: Option<String>,
    pub proposed_rules: Vec<String>,
    pub validated_rules: Vec<String>,
}

impl Default for IngestOutput {
    fn default() -> Self {
        Self {
            episodic_id: None,
            proposed_rules: vec![],
            validated_rules: vec![],
        }
    }
}

/// JSON output for the `distill` command.
#[derive(Debug, Serialize, PartialEq)]
pub struct DistillOutput {
    pub decayed: u64,
    pub pruned: u64,
    pub transitioned: u64,
}

impl Default for DistillOutput {
    fn default() -> Self {
        Self {
            decayed: 0,
            pruned: 0,
            transitioned: 0,
        }
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
