use serde::Serialize;

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
}
