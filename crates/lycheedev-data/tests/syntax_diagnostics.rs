#[path = "support/syntax_diagnostics.rs"]
mod cases;
use lycheedev_core::Invocation;
use lycheedev_data::relational::Program;
use serde_json::json;
use std::{collections::BTreeMap, path::PathBuf};

#[test]
fn grammar_diagnostics_retain_typed_locations_and_original_source_offsets() {
    for &(sql, offset, line, column, detail) in cases::CASES {
        let error = Program::compile(sql).unwrap_err();
        assert_eq!(error.code, "relational.invalid_syntax", "{sql}: {error:?}");
        assert_eq!(
            error.message,
            format!("relational.invalid_syntax at {line}:{column}: {detail}"),
            "{sql}"
        );
        assert_eq!(
            error.details,
            json!({"location":{"offset":offset,"line":line,"column":column}}),
            "{sql}"
        );
    }
}
#[test]
fn data_sql_dispatch_preserves_syntax_message_location_exit_and_stage() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../.tmp")
        .join(format!(
            "syntax-diagnostic-unused-home-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
    assert!(!root.exists());
    for &(sql, offset, line, column, detail) in cases::CASES {
        let inv = Invocation {
            command: "data sql".into(),
            positional: None,
            flags: BTreeMap::from([("--sql".into(), sql.into())]),
            home: root.clone(),
            cwd: root.clone(),
            control: Default::default(),
        };
        let error = lycheedev_data::migration_oracle::dispatch(&inv).err().unwrap();
        assert_eq!(
            (error.code.as_str(), error.exit_code),
            ("query.invalid_syntax", 2),
            "{sql}"
        );
        assert_eq!(
            error.message,
            format!("relational.invalid_syntax at {line}:{column}: {detail}"),
            "{sql}"
        );
        assert_eq!(
            error.details,
            json!({"stage":"query","location":{"offset":offset,"line":line,"column":column}}),
            "{sql}"
        );
        assert!(
            !root.exists(),
            "malformed SQL must not initialize a workspace or publish a capture"
        );
    }
}
#[test]
fn syntax_without_location_keeps_the_original_message_at_dispatch() {
    let inv = Invocation {
        command: "data sql".into(),
        positional: None,
        flags: BTreeMap::from([("--sql".into(), "SELECT ID FROM Sample\0".into())]),
        home: PathBuf::new(),
        cwd: PathBuf::new(),
        control: Default::default(),
    };
    let error = lycheedev_data::migration_oracle::dispatch(&inv).err().unwrap();
    assert_eq!(
        (error.code.as_str(), error.exit_code),
        ("query.invalid_syntax", 2)
    );
    assert_eq!(error.message, "Invalid SQL text");
    assert_eq!(error.details, json!({"stage":"query"}));
}
