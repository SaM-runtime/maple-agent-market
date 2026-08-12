//! Process-level contracts for the source-management CLI.
//!
//! These tests exercise clap parsing, isolated configuration, source probing,
//! JSON presentation, and the process exit code together. Unix owns this test
//! seam because HOME isolation and hook-install failure setup differ on Windows.
#![cfg(unix)]

#[test]
fn sources_json_lists_every_source_in_an_isolated_home() {
    let home = tempfile::tempdir().expect("tempdir");

    let output = std::process::Command::new(env!("CARGO_BIN_EXE_maple-agent-market"))
        .args(["sources", "--json"])
        .env_clear()
        .env("HOME", home.path())
        .env("PATH", "/usr/bin:/bin")
        .output()
        .expect("run maple-agent-market sources --json");

    assert!(
        output.status.success(),
        "sources --json exited non-zero: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8(output.stdout).expect("utf-8 stdout");
    snapbox::assert_data_eq!(stdout, snapbox::file!["snapshots/cli/sources.json"]);
}

#[test]
fn a_failing_connect_emits_the_outcome_rows_and_exits_nonzero() {
    let home = tempfile::tempdir().expect("tempdir");
    std::fs::write(home.path().join(".claude"), b"not a directory").expect("seed .claude file");

    let output = std::process::Command::new(env!("CARGO_BIN_EXE_maple-agent-market"))
        .args(["connect", "claude-code", "--json"])
        .env_clear()
        .env("HOME", home.path())
        .env("PATH", "/usr/bin:/bin")
        .output()
        .expect("run maple-agent-market connect --json");

    assert!(
        !output.status.success(),
        "a failing connect must exit non-zero; stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8(output.stdout).expect("utf-8 stdout");
    let rows: Vec<serde_json::Value> =
        serde_json::from_str(stdout.trim()).unwrap_or_else(|error| {
            panic!("failing connect must print OutcomeRow JSON to stdout: {error}: {stdout:?}")
        });
    assert_eq!(rows.len(), 1, "one row per requested source: {rows:?}");
    assert_eq!(rows[0]["id"], "claude-code");
    assert_eq!(rows[0]["outcome"], "failed");
}
