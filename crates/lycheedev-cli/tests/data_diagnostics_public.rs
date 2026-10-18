//! Actual binary entrypoint/command mapping/public JSON, without source or game access.
#[path = "../../lycheedev-data/tests/support/syntax_diagnostics.rs"]
mod cases;
use serde_json::{Value, json};
use std::{path::PathBuf, process::Command};

#[test]
fn actual_data_sql_cli_preserves_diagnostic_message_location_stage_and_exit() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../.tmp")
        .join(format!(
            "public-syntax-unused-home-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
    assert!(!root.exists());
    for &(sql, offset, line, column, detail) in cases::CASES {
        let output = Command::new(env!("CARGO_BIN_EXE_lycheedev"))
            .args([
                "data",
                "sql",
                "--sql",
                sql,
                "--home",
                root.to_str().unwrap(),
                "--format",
                "json",
            ])
            .output()
            .unwrap();
        let value: Value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
            panic!(
                "{sql}: {error}: {} / {}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            )
        });
        assert_eq!(output.status.code(), Some(2), "{sql}: {value}");
        assert_eq!(value["schema"], "lycheedev.result.v1");
        assert_eq!(value["ok"], false);
        assert_eq!(value["result"], Value::Null);
        assert_eq!(value["captures"], json!([]));
        assert_eq!(
            value["error"],
            json!({
                "code":"query.invalid_syntax",
                "message":format!("relational.invalid_syntax at {line}:{column}: {detail}"),
                "stage":"query", "retryable":false, "resumeOperationId":"",
                "location":{"offset":offset,"line":line,"column":column}
            }),
            "{sql}"
        );
        assert!(
            !root.exists(),
            "syntax rejection must precede workspace or capture publication"
        );
    }
}
