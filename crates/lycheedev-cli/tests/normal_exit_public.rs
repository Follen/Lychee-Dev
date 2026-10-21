//! Exercise the ordinary main return with real CLI output and exit statuses.
//! Inherit the test environment, including any collector-owned profile path.
use lycheedev_core::VERSION;
use serde_json::{Value, json};
use std::{fs, path::PathBuf, process::Command};

struct Fixture(PathBuf);
impl Fixture {
    fn unused() -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let home = std::env::temp_dir().join(format!(
            "lycheedev-normal-exit-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
        ));
        assert!(!home.exists());
        Self(home)
    }

    fn invoke(&self, args: &[&str], exit: i32) -> (Vec<u8>, Value) {
        let output = Command::new(env!("CARGO_BIN_EXE_lycheedev"))
            .args(args)
            .arg("--home")
            .arg(&self.0)
            .args(["--format", "json"])
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(exit), "{args:?}: {output:?}");
        assert!(output.stderr.is_empty(), "{args:?}: {output:?}");
        assert_eq!(output.stdout.last(), Some(&b'\n'), "{args:?}");
        let value = serde_json::from_slice(&output.stdout).unwrap();
        (output.stdout, value)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        if self.0.exists() {
            fs::remove_dir_all(&self.0).unwrap();
        }
    }
}

fn success(result: Value) -> Value {
    json!({
        "schema":"lycheedev.result.v1", "ok":true, "operationId":"",
        "context":{}, "result":result, "captures":[], "warnings":[], "error":null
    })
}

fn failure(code: &str, message: &str) -> Value {
    json!({
        "schema":"lycheedev.result.v1", "ok":false, "operationId":"",
        "context":{}, "result":null, "captures":[], "warnings":[],
        "error":{"code":code,"message":message,"stage":"dispatch","retryable":false,"resumeOperationId":""}
    })
}

#[test]
fn actual_normal_exit_preserves_default_help_and_success_output() {
    let fixture = Fixture::unused();
    let (default_bytes, default) = fixture.invoke(&[], 0);
    let (help_bytes, help) = fixture.invoke(&["--help"], 0);
    assert_eq!(default_bytes, help_bytes);
    assert_eq!(default, help);
    assert_eq!(help, success(help["result"].clone()));
    assert_eq!(help["result"]["schema"], "lycheedev.cli-help.v1");
    assert_eq!(help["result"]["kind"], "group");
    assert_eq!(help["result"]["command"], "");
    let children = help["result"]["children"].as_array().unwrap();
    assert!(!children.is_empty());
    assert_eq!(help["result"]["flags"], json!(["--home <root>", "--format text|json|jsonl", "--help"]));

    let (_, version) = fixture.invoke(&["version"], 0);
    assert_eq!(version, success(version["result"].clone()));
    assert_eq!(version["result"]["version"], VERSION);

    let (describe_bytes, describe) = fixture.invoke(&["describe"], 0);
    // A complete multi-buffer response must survive the ordinary stdout-lock
    // drop and runtime cleanup, not merely a short status message.
    assert!(describe_bytes.len() > 8192);
    assert_eq!(describe, success(describe["result"].clone()));
    let commands = describe["result"]["commands"].as_array().unwrap();
    assert_eq!(commands.len(), 71);
    for child in children {
        let path = child["command"].as_str().unwrap();
        assert!(commands.iter().any(|command| {
            let command = command["path"].as_str().unwrap();
            command == path || command.starts_with(&format!("{path} "))
        }), "help child missing from complete command directory: {path}");
    }
    assert_eq!(describe["result"]["formats"], json!(["text", "json", "jsonl"]));
    assert_eq!(describe["result"]["development"], VERSION.contains('-'));
    assert!(!fixture.0.exists(), "information commands must not create a workspace");
}

#[test]
fn actual_normal_exit_preserves_argument_failure() {
    let fixture = Fixture::unused();
    let (_, value) = fixture.invoke(&["not-a-command"], 2);
    let details = &value["error"]["details"];
    assert_eq!(details["parser"], "clap");
    assert_eq!(details["kind"], "InvalidSubcommand");
    assert!(details["diagnostic"].as_str().unwrap().contains("not-a-command"));
    assert_eq!(details["discovery"], "lycheedev cli search \"<task>\"");
    assert!(!details["suggestions"].as_array().unwrap().is_empty());
    let mut expected = failure(
        "command.invalid_arguments",
        "invalid command arguments",
    );
    expected["error"]["details"] = details.clone();
    assert_eq!(value, expected);
    assert!(!fixture.0.exists(), "parse rejection must not create a workspace");
}

#[test]
fn actual_normal_exit_preserves_workspace_failure() {
    let fixture = Fixture::unused();
    fs::create_dir(&fixture.0).unwrap();
    let marker = fixture.0.join("workspace.json");
    fs::write(&marker, b"{}\n").unwrap();
    let (_, value) = fixture.invoke(&["config", "show"], 4);
    assert_eq!(value, failure("vault.unsupported_workspace", "vault.unsupported_workspace"));
    assert_eq!(fs::read(&marker).unwrap(), b"{}\n");
    assert_eq!(fs::read_dir(&fixture.0).unwrap().count(), 1);
}
