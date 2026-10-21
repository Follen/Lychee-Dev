//! Actual parser/formal-dispatch/disk-retirement/output fixtures, all NOT_RUN.
use super::*;
use lycheedev_core::{metadata::Metadata, storage};
fn execute(inv: &Invocation) -> Response {
    crate::dispatch_formal_entry_with(inv, |inv| {
        dispatch_with::<Fake>(inv, |_| {
            panic!("historical retirement opened native services")
        })
    })
    .expect("formal entry")
}
fn seed(inv: &Invocation, id: &str, stage: &str, status: &str) {
    let identity = lycheedev_core::workspace::initialize(&inv.home).unwrap();
    let game = inv.cwd.join("game");
    let parent = game.join("Interface/AddOns");
    fs::create_dir_all(&parent).unwrap();
    let canonical = fs::canonicalize(&parent).unwrap();
    let display = canonical.to_string_lossy();
    let ordinary = display.strip_prefix(r"\\?\").unwrap_or(&display);
    let record = json!({"schema":"lycheedev.work.v1","operationId":id,"generation":1,"intent":{"kind":"probe","resource":"window/1/2/3","snapshot":"PIN-test","session":"session","request":{"installation":game},"admission":{"parent":ordinary,"workspaceId":identity["workspaceId"]}},"stage":stage,"status":status,"observation":null});
    let mut m = Metadata::open(&inv.home, true).unwrap();
    m.create(&format!("work/{id}"), &record).unwrap();
    m.create("ownership/window/1/2/3", &json!({"operationId":id}))
        .unwrap();
    if stage != "prepared" {
        let owner = lycheedev_live::go_mailbox_v1::durable_journal::WindowOwner {
            schema: "lycheedev.window-owner.v1".into(),
            workspace_id: identity["workspaceId"].as_str().unwrap().into(),
            resource: "window/1/2/3".into(),
            operation_id: id.into(),
            intent_sha256: String::new(),
        };
        let scope = parent.join(".lycheedev-window-owners");
        fs::create_dir_all(&scope).unwrap();
        fs::write(
            scope.join(format!("{}.json", storage::sha256(b"window/1/2/3"))),
            serde_json::to_vec(&owner).unwrap(),
        )
        .unwrap();
    }
}
fn json_envelope(response: &Response) -> Value {
    serde_json::from_str(&rendered(response, "json")).unwrap()
}
#[test]
fn describe_exposes_exact_71_agent_era_commands_without_empty_handlers() {
    let all = crate::contract::implemented();
    assert_eq!(all.len(), 71);
    for route in ["live cancel", "live abandon"] {
        let d = all.iter().find(|d| d.path == route).unwrap();
        assert!(d.mutates);
        assert_eq!(
            d.flags.iter().map(|f| f.name()).collect::<Vec<_>>(),
            ["--home", "--format"]
        );
        for words in [
            [route.split_whitespace().nth(1).unwrap(), "--no-cache", ""],
            [route.split_whitespace().nth(1).unwrap(), "--project", "x"],
            [
                route.split_whitespace().nth(1).unwrap(),
                "--wait-seconds",
                "1",
            ],
        ] {
            let argv = ["live", words[0], "OP-test", words[1], words[2]]
                .into_iter()
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
                .collect::<Vec<_>>();
            assert!(crate::parse(&argv).is_err());
        }
    }
}
#[test]
fn actual_cancel_default_entry_writes_terminal_stage_and_returns_typed_result() {
    let f = Fixture::new();
    let id = format!("OP-{}", "1".repeat(32));
    let inv = invocation(&["live", "cancel", &id], &f.0);
    seed(&inv, &id, "prepared", "pending");
    let response = execute(&inv);
    assert_eq!(response.exit_code(), 0, "{:?}", response.error);
    let out = json_envelope(&response);
    assert_eq!(out["operationId"], id);
    assert_eq!(out["context"]["stage"], "cleaned");
    assert_eq!(out["result"]["status"], "cancelled");
    assert_eq!(out["result"]["complete"], false);
    assert_eq!(out["result"]["report"]["state"], "unavailable");
    assert!(out["warnings"].as_array().unwrap().is_empty());
}
#[test]
fn actual_abandon_default_entry_keeps_unknown_result_and_explicit_warning() {
    let f = Fixture::new();
    let id = format!("OP-{}", "2".repeat(32));
    let inv = invocation(&["live", "abandon", &id], &f.0);
    seed(&inv, &id, "dispatch_requested", "unresolved");
    let response = execute(&inv);
    assert_eq!(response.exit_code(), 0, "{:?}", response.error);
    let out = json_envelope(&response);
    assert_eq!(out["result"]["cleanup"], "abandoned");
    assert_eq!(out["result"]["complete"], false);
    assert_eq!(
        out["warnings"],
        json!([
            "Cleanup abandoned explicitly; available evidence retained; missing report results remain unknown. No game ACK or runtime unload was performed."
        ])
    );
    let jsonl = rendered(&response, "jsonl");
    let lines: Vec<Value> = jsonl
        .lines()
        .map(|s| serde_json::from_str(s).unwrap())
        .collect();
    assert_eq!(lines.last().unwrap()["type"], "end");
    assert!(jsonl.contains("abandoned"));
}
#[test]
fn cancel_after_effect_returns_original_stage_partial_result_and_exit_three() {
    let f = Fixture::new();
    let id = format!("OP-{}", "3".repeat(32));
    let inv = invocation(&["live", "cancel", &id], &f.0);
    seed(&inv, &id, "dispatch_requested", "unresolved");
    let response = execute(&inv);
    assert_eq!(response.exit_code(), 3);
    let out = json_envelope(&response);
    assert_eq!(out["error"]["code"], "journal.invalid_transition");
    assert_eq!(out["error"]["stage"], "dispatch_requested");
    assert_eq!(out["error"]["resumeOperationId"], id);
    assert_eq!(out["error"]["retryable"], false);
    assert_eq!(out["result"]["status"], "unresolved");
}
#[test]
fn cancel_and_abandon_con_or_missing_id_reject_before_workspace_or_native_factory() {
    let f = Fixture::new();
    for route in ["cancel", "abandon"] {
        for id in [None, Some("CON-id")] {
            let mut words = vec!["live", route];
            if let Some(id) = id {
                words.push(id);
            }
            if id.is_none() {
                // Missing positional arguments are rejected by the real parser,
                // before an Invocation or a native service factory exists.
                let mut args = words.iter().map(|word| word.to_string()).collect::<Vec<_>>();
                args.extend(["--home".into(), f.0.join("home").to_string_lossy().into_owned()]);
                let err = crate::parse(&args).err().expect("missing operation ID must fail parsing");
                assert_eq!(err.code, "command.invalid_arguments");
                assert_eq!(err.exit_code, 2);
                let argument = if route == "cancel" { "<operation-id>" } else { "<operation-or-bootstrap-id>" };
                assert_clap_error(&err, "MissingRequiredArgument", argument);
                assert!(!f.0.join("home").exists());
                continue;
            }
            let inv = invocation(&words, &f.0);
            let response = execute(&inv);
            assert_eq!(response.exit_code(), 2);
            assert!(response.result.is_none());
            assert!(!inv.home.exists());
        }
    }
}
#[test]
fn btp_cancel_is_op_lookup_while_abandon_is_real_bootstrap_mutation() {
    let f = Fixture::new();
    let id = format!("BTP-{}", "4".repeat(32));
    let inv = invocation(&["live", "abandon", &id], &f.0);
    lycheedev_core::workspace::initialize(&inv.home).unwrap();
    let resource = "window/0/0/0";
    let mut m = Metadata::open(&inv.home, true).unwrap();
    m.create(&format!("bootstrap/attempt/{id}"),&json!({"schema":"lycheedev.bootstrap-receiver.v1","id":id,"phase":"prepared","action":"identify"})).unwrap();
    m.create(
        &format!(
            "bootstrap/current/{}",
            storage::sha256(format!("{resource}\0identify").as_bytes())
        ),
        &json!({"id":id}),
    )
    .unwrap();
    m.create(
        &format!("bootstrap/window/{}", storage::sha256(resource.as_bytes())),
        &json!({"id":id}),
    )
    .unwrap();
    drop(m);
    let cancel = invocation(&["live", "cancel", &id], &f.0);
    let response = execute(&cancel);
    assert_eq!(response.error.unwrap().code, "vault.record_missing");
    assert!(response.result.is_none());
    let response = execute(&inv);
    assert_eq!(response.exit_code(), 0);
    let out = json_envelope(&response);
    assert_eq!(out["result"]["phase"], "abandoned");
    assert!(out["context"].get("stage").is_none());
    assert!(out["warnings"].as_array().unwrap().is_empty());
}
#[test]
fn historical_joined_failures_use_go_priority_but_keep_original_cause_tree() {
    use lycheedev_core::metadata::historical_write::finish_historical;
    let f = Fixture::new();
    let inv = invocation(&["live", "abandon", "OP-id"], &f.0);
    for (code, exit, want) in [
        ("command.cancelled", 7, "command.cancelled"),
        ("command.deadline", 6, "live.deadline_pending"),
        ("vault.generation_conflict", 3, "vault.generation_conflict"),
    ] {
        let error = finish_historical::<()>(
            Err(ToolkitError::new("external.io", "first IO failure", 5)),
            Err(ToolkitError::new(code, "second cause", exit)),
        )
        .unwrap_err();
        let mapped = historical_failure(&inv, error);
        assert_eq!(mapped.code, want);
        assert_eq!(mapped.exit_code, exit);
        assert_eq!(mapped.message, "first IO failure\nsecond cause");
        assert_eq!(mapped.details["primary"]["code"], "external.io");
        assert_eq!(mapped.details["cleanup"]["code"], code);
    }
    let joined = finish_historical::<()>(
        Err(ToolkitError::new("command.cancelled", "cancelled", 7)),
        Err(ToolkitError::new("journal.resource_busy", "busy", 3)),
    )
    .unwrap_err();
    assert_eq!(
        historical_failure(&inv, joined).code,
        "journal.resource_busy"
    );
}
