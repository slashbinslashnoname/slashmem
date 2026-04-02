//! Integration tests verifying graceful degradation when the DB is missing or corrupt.
//!
//! Each test points SLASHMEM_DIR at a broken path, runs a subcommand, and asserts
//! that the process exits 0 with valid default JSON on stdout.

use std::process::Command;

fn sm_bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_sm"))
}

/// Point SLASHMEM_DIR at a path nested under a regular file so that
/// `create_dir_all` fails (cannot create a directory inside a file).
fn bad_slashmem_dir() -> tempfile::TempDir {
    let tmp = tempfile::tempdir().unwrap();
    let blocker = tmp.path().join("blocker");
    std::fs::write(&blocker, b"not a directory").unwrap();
    // Now "blocker/nested" can never be created as a directory.
    tmp
}

fn bad_dir_path(tmp: &tempfile::TempDir) -> String {
    tmp.path()
        .join("blocker")
        .join("nested")
        .to_string_lossy()
        .into_owned()
}

#[test]
fn context_exits_zero_with_default_json_on_missing_db() {
    let tmp = bad_slashmem_dir();
    let output = sm_bin()
        .env("SLASHMEM_DIR", bad_dir_path(&tmp))
        .args(["context", "anything"])
        .output()
        .expect("failed to run sm");

    assert!(output.status.success(), "expected exit 0, got {:?}", output.status);
    let json: serde_json::Value = serde_json::from_slice(&output.stdout)
        .expect("stdout should be valid JSON");
    assert_eq!(json["relevant_rules"], serde_json::json!([]));
    assert_eq!(json["anti_patterns"], serde_json::json!([]));
    assert_eq!(json["history_snippets"], serde_json::json!([]));
}

#[test]
fn ingest_exits_zero_with_default_json_on_missing_db() {
    let tmp = bad_slashmem_dir();
    let output = sm_bin()
        .env("SLASHMEM_DIR", bad_dir_path(&tmp))
        .args(["ingest", "--task", "T-1", "--body", "hello", "--agent", "a"])
        .output()
        .expect("failed to run sm");

    assert!(output.status.success(), "expected exit 0, got {:?}", output.status);
    let json: serde_json::Value = serde_json::from_slice(&output.stdout)
        .expect("stdout should be valid JSON");
    assert!(json["episodic_id"].is_null());
    assert_eq!(json["proposed_rules"], serde_json::json!([]));
    assert_eq!(json["validated_rules"], serde_json::json!([]));
}

#[test]
fn distill_exits_zero_with_default_json_on_missing_db() {
    let tmp = bad_slashmem_dir();
    let output = sm_bin()
        .env("SLASHMEM_DIR", bad_dir_path(&tmp))
        .args(["distill"])
        .output()
        .expect("failed to run sm");

    assert!(output.status.success(), "expected exit 0, got {:?}", output.status);
    let json: serde_json::Value = serde_json::from_slice(&output.stdout)
        .expect("stdout should be valid JSON");
    assert_eq!(json["decayed"], 0);
    assert_eq!(json["pruned"], 0);
    assert_eq!(json["transitioned"], 0);
}

#[test]
fn context_stderr_contains_diagnostic_on_missing_db() {
    let tmp = bad_slashmem_dir();
    let output = sm_bin()
        .env("SLASHMEM_DIR", bad_dir_path(&tmp))
        .args(["context", "test"])
        .output()
        .expect("failed to run sm");

    assert!(output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!stderr.is_empty(), "expected diagnostic on stderr");
}
