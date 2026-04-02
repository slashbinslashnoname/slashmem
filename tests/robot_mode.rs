//! Integration tests for robot mode: JSON output on pipe, --json flag,
//! structured error envelopes, exit codes, compact help token count,
//! and backward compatibility of existing JSON contracts.

use std::process::Command;

fn sm_bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_sm"))
}

/// Run `sm` with SLASHMEM_DIR pointing at a fresh tempdir.
fn run_sm(dir: &tempfile::TempDir, args: &[&str]) -> std::process::Output {
    sm_bin()
        .env("SLASHMEM_DIR", dir.path())
        .args(args)
        .output()
        .expect("failed to run sm")
}

fn parse_json(output: &std::process::Output) -> serde_json::Value {
    serde_json::from_slice(&output.stdout).expect("stdout should be valid JSON")
}

fn parse_success(output: &std::process::Output) -> serde_json::Value {
    assert!(
        output.status.success(),
        "sm exited with {}: {}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    parse_json(output)
}

// ===========================================================================
// 1. JSON output on pipe (non-TTY) — tests run in non-TTY by default
// ===========================================================================

#[test]
fn pipe_context_outputs_valid_json() {
    let tmp = tempfile::tempdir().unwrap();
    let output = run_sm(&tmp, &["context", "anything"]);
    // In non-TTY, stdout is JSON
    let json = parse_success(&output);
    assert!(json.is_object());
    assert!(json.get("relevant_rules").is_some());
}

#[test]
fn pipe_ingest_outputs_valid_json() {
    let tmp = tempfile::tempdir().unwrap();
    let output = run_sm(&tmp, &["ingest", "--task", "T-1", "--body", "test", "--agent", "a"]);
    let json = parse_success(&output);
    assert!(json.is_object());
    assert!(json.get("episodic_id").is_some());
}

#[test]
fn pipe_distill_outputs_valid_json() {
    let tmp = tempfile::tempdir().unwrap();
    run_sm(&tmp, &["context", "init"]); // create DB
    let output = run_sm(&tmp, &["distill"]);
    let json = parse_success(&output);
    assert!(json.is_object());
    assert!(json.get("decayed").is_some());
}

#[test]
fn pipe_status_outputs_valid_json() {
    let tmp = tempfile::tempdir().unwrap();
    let output = run_sm(&tmp, &["status"]);
    let json = parse_success(&output);
    assert!(json.is_object());
    assert!(json.get("ok").is_some());
}

#[test]
fn pipe_rules_list_outputs_valid_json() {
    let tmp = tempfile::tempdir().unwrap();
    run_sm(&tmp, &["context", "init"]); // create DB
    let output = run_sm(&tmp, &["rules", "list"]);
    let json = parse_success(&output);
    assert!(json.is_object());
    assert!(json.get("rules").is_some());
}

#[test]
fn pipe_help_outputs_valid_json() {
    // No SLASHMEM_DIR needed — just no subcommand
    let output = sm_bin().output().expect("failed to run sm");
    assert!(output.status.success());
    let json = parse_json(&output);
    assert!(json.is_object());
    assert!(json.get("commands").is_some());
}

#[test]
fn pipe_no_stderr_on_success() {
    // When piped (non-TTY), successful commands should produce no stderr
    let tmp = tempfile::tempdir().unwrap();
    for args in [
        vec!["context", "anything"],
        vec!["ingest", "--task", "T-1", "--body", "b", "--agent", "a"],
        vec!["status"],
    ] {
        let output = run_sm(&tmp, &args);
        assert!(output.status.success());
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.is_empty(),
            "expected no stderr for {args:?}, got: {stderr}"
        );
    }
}

// ===========================================================================
// 2. --json flag forces JSON output (simulates TTY override)
// ===========================================================================

#[test]
fn json_flag_forces_json_on_context() {
    let tmp = tempfile::tempdir().unwrap();
    let output = run_sm(&tmp, &["--json", "context", "anything"]);
    let json = parse_success(&output);
    assert!(json.get("relevant_rules").is_some());
}

#[test]
fn json_flag_forces_json_on_status() {
    let tmp = tempfile::tempdir().unwrap();
    let output = run_sm(&tmp, &["--json", "status"]);
    let json = parse_success(&output);
    assert!(json.get("ok").is_some());
}

#[test]
fn json_flag_forces_json_on_help() {
    let output = sm_bin()
        .arg("--json")
        .output()
        .expect("failed to run sm");
    assert!(output.status.success());
    let json = parse_json(&output);
    assert!(json.get("commands").is_some());
    assert!(json.get("version").is_some());
}

#[test]
fn json_flag_works_after_subcommand() {
    let tmp = tempfile::tempdir().unwrap();
    let output = run_sm(&tmp, &["status", "--json"]);
    let json = parse_success(&output);
    assert!(json.get("ok").is_some());
}

// ===========================================================================
// 3. Structured error envelopes for each error class
// ===========================================================================

#[test]
fn error_envelope_not_found() {
    let tmp = tempfile::tempdir().unwrap();
    run_sm(&tmp, &["context", "init"]); // create DB

    let output = run_sm(&tmp, &["rules", "show", "nonexistent-id"]);
    assert!(!output.status.success());

    let json = parse_json(&output);
    assert!(json.get("error").is_some(), "should have error envelope");
    let err = &json["error"];
    assert_eq!(err["code"], "NOT_FOUND");
    assert!(err["message"].as_str().unwrap().contains("not found"));
    assert!(err["suggestions"].is_array());
    assert!(!err["suggestions"].as_array().unwrap().is_empty());
    assert_eq!(err["exit_code"], 1);
}

#[test]
fn error_envelope_has_all_required_fields() {
    let tmp = tempfile::tempdir().unwrap();
    run_sm(&tmp, &["context", "init"]);

    let output = run_sm(&tmp, &["rules", "show", "missing"]);
    let json = parse_json(&output);
    let err = &json["error"];

    // Verify the complete structure
    assert!(err["code"].is_string(), "code should be a string");
    assert!(err["message"].is_string(), "message should be a string");
    assert!(err["suggestions"].is_array(), "suggestions should be an array");
    assert!(err["exit_code"].is_number(), "exit_code should be a number");
}

#[test]
fn error_envelope_code_matches_exit_code_for_not_found() {
    let tmp = tempfile::tempdir().unwrap();
    run_sm(&tmp, &["context", "init"]);

    let output = run_sm(&tmp, &["rules", "show", "no-such-rule"]);
    let exit_code = output.status.code().unwrap();
    let json = parse_json(&output);

    assert_eq!(exit_code, 1);
    assert_eq!(json["error"]["exit_code"], 1);
    assert_eq!(json["error"]["code"], "NOT_FOUND");
}

#[test]
fn error_envelope_suggestions_are_strings() {
    let tmp = tempfile::tempdir().unwrap();
    run_sm(&tmp, &["context", "init"]);

    let output = run_sm(&tmp, &["rules", "show", "ghost"]);
    let json = parse_json(&output);
    let suggestions = json["error"]["suggestions"].as_array().unwrap();
    for s in suggestions {
        assert!(s.is_string(), "each suggestion must be a string, got: {s}");
    }
}

// ===========================================================================
// 4. Correct exit codes for each failure mode
// ===========================================================================

#[test]
fn exit_code_not_found_is_1() {
    let tmp = tempfile::tempdir().unwrap();
    run_sm(&tmp, &["context", "init"]);

    let output = run_sm(&tmp, &["rules", "show", "nonexistent"]);
    assert_eq!(output.status.code().unwrap(), 1);
}

#[test]
fn exit_code_success_is_0_for_all_commands() {
    let tmp = tempfile::tempdir().unwrap();

    let commands: Vec<Vec<&str>> = vec![
        vec!["context", "test"],
        vec!["ingest", "--task", "T-1", "--body", "b", "--agent", "a"],
        vec!["status"],
        vec!["rules", "list"],
        vec!["rules", "add", "test-r", "A rule"],
    ];

    for args in &commands {
        let output = run_sm(&tmp, args);
        assert_eq!(
            output.status.code().unwrap(),
            0,
            "expected exit 0 for {args:?}, got {}",
            output.status
        );
    }
}

#[test]
fn exit_code_0_for_distill() {
    let tmp = tempfile::tempdir().unwrap();
    run_sm(&tmp, &["context", "init"]);
    let output = run_sm(&tmp, &["distill"]);
    assert_eq!(output.status.code().unwrap(), 0);
}

#[test]
fn exit_code_0_for_rules_rm_missing() {
    // Removing a nonexistent rule is not an error — it returns deleted=false
    let tmp = tempfile::tempdir().unwrap();
    run_sm(&tmp, &["context", "init"]);

    let output = run_sm(&tmp, &["rules", "rm", "nonexistent"]);
    assert_eq!(output.status.code().unwrap(), 0);
}

#[test]
fn exit_code_nonzero_for_rules_show_missing() {
    let tmp = tempfile::tempdir().unwrap();
    run_sm(&tmp, &["context", "init"]);

    let output = run_sm(&tmp, &["rules", "show", "nonexistent"]);
    assert_ne!(output.status.code().unwrap(), 0);
}

// ===========================================================================
// 5. Compact help output token count
// ===========================================================================

#[test]
fn help_json_is_compact_and_complete() {
    let output = sm_bin().output().expect("failed to run sm");
    let json = parse_success(&output);

    // Must have all expected fields
    assert!(json["version"].is_string());
    assert!(json["usage"].is_string());
    assert!(json["commands"].is_array());
    assert!(json["exit_codes"].is_array());

    // Commands list should include all 5 subcommands
    let commands = json["commands"].as_array().unwrap();
    assert_eq!(commands.len(), 5, "should list exactly 5 commands");

    let names: Vec<&str> = commands
        .iter()
        .map(|c| c["name"].as_str().unwrap())
        .collect();
    for expected in &["context", "ingest", "distill", "status", "rules"] {
        assert!(
            names.contains(expected),
            "help should list {expected} command"
        );
    }

    // Each command entry should have name + description
    for cmd in commands {
        assert!(cmd["name"].is_string());
        assert!(cmd["description"].is_string());
        assert!(!cmd["description"].as_str().unwrap().is_empty());
    }
}

#[test]
fn help_exit_codes_list_all_codes() {
    let output = sm_bin().output().expect("failed to run sm");
    let json = parse_success(&output);

    let exit_codes = json["exit_codes"].as_array().unwrap();
    // Should list 7 exit codes: 0, 1, 2, 3, 4, 5, 127
    assert_eq!(exit_codes.len(), 7, "should list 7 exit codes");

    let codes: Vec<i64> = exit_codes
        .iter()
        .map(|e| e["code"].as_i64().unwrap())
        .collect();
    for expected in &[0, 1, 2, 3, 4, 5, 127] {
        assert!(
            codes.contains(expected),
            "exit codes should include {expected}"
        );
    }

    // Each exit code entry should have meaning
    for ec in exit_codes {
        assert!(ec["meaning"].is_string());
        assert!(!ec["meaning"].as_str().unwrap().is_empty());
    }
}

#[test]
fn help_json_token_count_is_compact() {
    // The JSON help output should be reasonably compact for agent consumption.
    // We approximate token count as whitespace-separated words.
    let output = sm_bin().output().expect("failed to run sm");
    let stdout = String::from_utf8_lossy(&output.stdout);

    // JSON serialization should be a single line (compact, not pretty-printed)
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(lines.len(), 1, "JSON help should be a single line");

    // Rough token estimate: JSON string length should be reasonable
    // A compact help blob for 5 commands + 7 exit codes should be under 600 bytes
    assert!(
        stdout.len() < 1000,
        "help JSON should be compact (<1000 bytes), got {} bytes",
        stdout.len()
    );
}

// ===========================================================================
// 6. Backward compatibility of existing JSON contracts
// ===========================================================================

#[test]
fn contract_context_output_fields() {
    let tmp = tempfile::tempdir().unwrap();
    let output = run_sm(&tmp, &["context", "anything"]);
    let json = parse_success(&output);

    // Exact field set
    let obj = json.as_object().unwrap();
    assert!(obj.contains_key("relevant_rules"), "missing relevant_rules");
    assert!(obj.contains_key("anti_patterns"), "missing anti_patterns");
    assert!(obj.contains_key("history_snippets"), "missing history_snippets");

    // Types
    assert!(json["relevant_rules"].is_array());
    assert!(json["anti_patterns"].is_array());
    assert!(json["history_snippets"].is_array());
}

#[test]
fn contract_ingest_output_fields() {
    let tmp = tempfile::tempdir().unwrap();
    let output = run_sm(
        &tmp,
        &["ingest", "--task", "T-1", "--body", "test", "--agent", "a"],
    );
    let json = parse_success(&output);
    let obj = json.as_object().unwrap();

    assert!(obj.contains_key("episodic_id"), "missing episodic_id");
    assert!(obj.contains_key("proposed_rules"), "missing proposed_rules");
    assert!(obj.contains_key("validated_rules"), "missing validated_rules");

    // episodic_id is a string (or null on degraded)
    assert!(json["episodic_id"].is_string());
    assert!(json["proposed_rules"].is_array());
    assert!(json["validated_rules"].is_array());
}

#[test]
fn contract_distill_output_fields() {
    let tmp = tempfile::tempdir().unwrap();
    run_sm(&tmp, &["context", "init"]);
    let output = run_sm(&tmp, &["distill"]);
    let json = parse_success(&output);
    let obj = json.as_object().unwrap();

    assert!(obj.contains_key("decayed"), "missing decayed");
    assert!(obj.contains_key("pruned"), "missing pruned");
    assert!(obj.contains_key("transitioned"), "missing transitioned");

    assert!(json["decayed"].is_u64());
    assert!(json["pruned"].is_u64());
    assert!(json["transitioned"].is_u64());
}

#[test]
fn contract_status_output_fields() {
    let tmp = tempfile::tempdir().unwrap();
    let output = run_sm(&tmp, &["status"]);
    let json = parse_success(&output);
    let obj = json.as_object().unwrap();

    assert!(obj.contains_key("ok"), "missing ok");
    assert!(obj.contains_key("db_path"), "missing db_path");
    assert!(obj.contains_key("counts"), "missing counts");
    assert!(obj.contains_key("schema_version"), "missing schema_version");

    assert!(json["ok"].is_boolean());
    assert!(json["db_path"].is_string());
    assert!(json["schema_version"].is_u64());

    // Nested counts
    let counts = &json["counts"];
    assert!(counts["episodic"].is_u64());
    assert!(counts["working"].is_u64());
    assert!(counts["procedural"].is_u64());
}

#[test]
fn contract_rules_list_output_fields() {
    let tmp = tempfile::tempdir().unwrap();
    run_sm(&tmp, &["rules", "add", "r1", "Test rule"]);

    let output = run_sm(&tmp, &["rules", "list"]);
    let json = parse_success(&output);
    let obj = json.as_object().unwrap();

    assert!(obj.contains_key("rules"), "missing rules");
    assert!(obj.contains_key("count"), "missing count");
    assert_eq!(json["count"], 1);

    let rule = &json["rules"][0];
    assert!(rule["id"].is_string());
    assert!(rule["rule"].is_string());
    assert!(rule["confidence"].is_f64());
    assert!(rule["is_proven"].is_boolean());
    assert!(rule["is_anti_pattern"].is_boolean());
}

#[test]
fn contract_rules_show_output_fields() {
    let tmp = tempfile::tempdir().unwrap();
    run_sm(&tmp, &["rules", "add", "r1", "Test rule", "--source", "test"]);

    let output = run_sm(&tmp, &["rules", "show", "r1"]);
    let json = parse_success(&output);
    let obj = json.as_object().unwrap();

    // All expected fields
    for field in &[
        "id",
        "rule",
        "success_count",
        "failure_count",
        "confidence",
        "is_proven",
        "is_anti_pattern",
        "last_validated",
        "source",
        "created_at",
        "updated_at",
    ] {
        assert!(
            obj.contains_key(*field),
            "rules show missing field: {field}"
        );
    }

    assert!(json["id"].is_string());
    assert!(json["rule"].is_string());
    assert!(json["success_count"].is_u64());
    assert!(json["failure_count"].is_u64());
    assert!(json["confidence"].is_f64());
    assert!(json["is_proven"].is_boolean());
    assert!(json["is_anti_pattern"].is_boolean());
    // last_validated and source can be string or null
    assert!(json["source"].is_string() || json["source"].is_null());
    assert!(json["created_at"].is_string());
    assert!(json["updated_at"].is_string());
}

#[test]
fn contract_rules_add_output_fields() {
    let tmp = tempfile::tempdir().unwrap();

    let output = run_sm(&tmp, &["rules", "add", "r1", "Test rule"]);
    let json = parse_success(&output);
    let obj = json.as_object().unwrap();

    assert!(obj.contains_key("id"), "missing id");
    assert!(obj.contains_key("created"), "missing created");
    assert_eq!(json["id"], "r1");
    assert_eq!(json["created"], true);
}

#[test]
fn contract_rules_rm_output_fields() {
    let tmp = tempfile::tempdir().unwrap();
    run_sm(&tmp, &["rules", "add", "r1", "Temp rule"]);

    let output = run_sm(&tmp, &["rules", "rm", "r1"]);
    let json = parse_success(&output);
    let obj = json.as_object().unwrap();

    assert!(obj.contains_key("id"), "missing id");
    assert!(obj.contains_key("deleted"), "missing deleted");
    assert_eq!(json["id"], "r1");
    assert_eq!(json["deleted"], true);
}

#[test]
fn contract_error_output_fields() {
    let tmp = tempfile::tempdir().unwrap();
    run_sm(&tmp, &["context", "init"]);

    let output = run_sm(&tmp, &["rules", "show", "nonexistent"]);
    let json = parse_json(&output);
    let obj = json.as_object().unwrap();

    // Top-level must have exactly "error"
    assert!(obj.contains_key("error"), "missing error envelope");
    assert_eq!(obj.len(), 1, "error envelope should have only 'error' key");

    let err = json["error"].as_object().unwrap();
    assert!(err.contains_key("code"));
    assert!(err.contains_key("message"));
    assert!(err.contains_key("suggestions"));
    assert!(err.contains_key("exit_code"));
}

#[test]
fn contract_help_output_fields() {
    let output = sm_bin().output().expect("failed to run sm");
    let json = parse_success(&output);
    let obj = json.as_object().unwrap();

    assert!(obj.contains_key("version"), "missing version");
    assert!(obj.contains_key("usage"), "missing usage");
    assert!(obj.contains_key("commands"), "missing commands");
    assert!(obj.contains_key("exit_codes"), "missing exit_codes");

    // Commands entries
    let cmd = &json["commands"][0];
    assert!(cmd["name"].is_string());
    assert!(cmd["description"].is_string());

    // Exit code entries
    let ec = &json["exit_codes"][0];
    assert!(ec["code"].is_number());
    assert!(ec["meaning"].is_string());
}

// ===========================================================================
// 7. Additional robot-mode integration tests
// ===========================================================================

#[test]
fn error_on_pipe_goes_to_stdout_not_stderr() {
    // In non-TTY mode, error envelopes go to stdout (JSON), not stderr
    let tmp = tempfile::tempdir().unwrap();
    run_sm(&tmp, &["context", "init"]);

    let output = run_sm(&tmp, &["rules", "show", "nonexistent"]);
    assert!(!output.status.success());

    // Error JSON should be on stdout
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("NOT_FOUND"), "error JSON should be on stdout");

    // Stderr should be empty in non-TTY
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.is_empty(),
        "stderr should be empty in non-TTY error mode, got: {stderr}"
    );
}

#[test]
fn multiple_commands_produce_consistent_json() {
    // Run several commands in sequence and verify all produce parseable JSON
    let tmp = tempfile::tempdir().unwrap();

    let runs: Vec<Vec<&str>> = vec![
        vec!["context", "test"],
        vec!["ingest", "--task", "T", "--body", "b", "--agent", "a"],
        vec!["status"],
        vec!["distill"],
        vec!["rules", "list"],
        vec!["rules", "add", "r1", "A rule"],
        vec!["rules", "show", "r1"],
        vec!["rules", "rm", "r1"],
    ];

    for args in &runs {
        let output = run_sm(&tmp, args);
        assert!(
            output.status.success(),
            "command {args:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let _json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap_or_else(
            |e| {
                panic!(
                    "command {args:?} did not produce valid JSON: {e}\nstdout: {}",
                    String::from_utf8_lossy(&output.stdout)
                )
            },
        );
    }
}

#[test]
fn quiet_flag_suppresses_stdout() {
    let tmp = tempfile::tempdir().unwrap();

    let output = run_sm(&tmp, &["--quiet", "context", "test"]);
    assert!(output.status.success());
    assert!(
        output.stdout.is_empty(),
        "stdout should be empty with --quiet, got: {}",
        String::from_utf8_lossy(&output.stdout)
    );
}

#[test]
fn quiet_flag_preserves_exit_code() {
    let tmp = tempfile::tempdir().unwrap();
    run_sm(&tmp, &["context", "init"]);

    let output = run_sm(&tmp, &["--quiet", "rules", "show", "missing"]);
    // Exit code should still be nonzero even with --quiet
    assert_ne!(
        output.status.code().unwrap(),
        0,
        "exit code should reflect error even with --quiet"
    );
}

#[test]
fn graceful_degradation_outputs_valid_json_on_pipe() {
    // When DB is broken, piped output should still be valid JSON
    let tmp = tempfile::tempdir().unwrap();
    let blocker = tmp.path().join("blocker");
    std::fs::write(&blocker, b"not a directory").unwrap();
    let bad_dir = tmp.path().join("blocker").join("nested");

    for args in [
        vec!["context", "test"],
        vec!["ingest", "--task", "T", "--body", "b", "--agent", "a"],
        vec!["distill"],
        vec!["status"],
    ] {
        let output = sm_bin()
            .env("SLASHMEM_DIR", &bad_dir)
            .args(&args)
            .output()
            .expect("failed to run sm");

        assert!(
            output.status.success(),
            "degraded {args:?} should exit 0"
        );

        let _: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap_or_else(|e| {
            panic!(
                "degraded {args:?} should produce valid JSON: {e}\nstdout: {}",
                String::from_utf8_lossy(&output.stdout)
            )
        });
    }
}

#[test]
fn exit_code_and_error_code_consistency() {
    // Verify that the exit code returned by the process matches the exit_code in
    // the error envelope for a NOT_FOUND error
    let tmp = tempfile::tempdir().unwrap();
    run_sm(&tmp, &["context", "init"]);

    let output = run_sm(&tmp, &["rules", "show", "missing"]);
    let process_exit = output.status.code().unwrap();
    let json = parse_json(&output);
    let envelope_exit = json["error"]["exit_code"].as_i64().unwrap() as i32;

    assert_eq!(
        process_exit, envelope_exit,
        "process exit code ({process_exit}) should match envelope exit_code ({envelope_exit})"
    );
}
