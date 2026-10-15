use super::*;
use lycheedev_core::{
    control::OperationControl, evidence, metadata::Metadata, selection, storage, workspace,
};
use std::{
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};
const BUILD_KEY: &str = "13361dee4d33a553f01cc58226b71eb3";
const CDN_KEY: &str = "cdf3a7e8977c4bee31264ff20de6d19e";
const OBSERVED: &str = "2026-10-14T22:00:00Z";
fn words(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).into()).collect()
}
fn home() -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../.tmp/target-default-fixtures")
        .join(format!(
            "{}-{}-{}",
            std::process::id(),
            storage::utc_now().replace([':', '.'], "-"),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ))
}
fn invocation(home: &Path) -> Invocation {
    super::super::parse(&words(&[
        "target",
        "resolve",
        "--product",
        "retail",
        "--region",
        "cn",
        "--locale",
        "zhCN",
        "--definitions",
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "--offline",
        "--home",
        home.to_str().unwrap(),
        "--format",
        "json",
    ]))
    .unwrap()
}
fn seed(home: &Path) {
    workspace::initialize(home).unwrap();
    let build = format!(
        "root = {}\nencoding = {} {}\nencoding-size = 123 456\nbuild-uid = wow\n",
        "1".repeat(32),
        "2".repeat(32),
        "3".repeat(32)
    );
    let cdn = b"archives = 44444444444444444444444444444444\n";
    let versions = format!(
        "Region!STRING:0|BuildConfig!HEX:16|CDNConfig!HEX:16|BuildId!DEC:4|VersionsName!STRING:0\ncn|{BUILD_KEY}|{CDN_KEY}|70000|12.1.5.70000\n"
    );
    let distribution = b"Name!STRING:0|Path!STRING:0|Hosts!STRING:0\ncn|tpr/wow|cdn.example.test\n";
    let b = evidence::publish_blob(home, build.as_bytes(), 4096).unwrap();
    let c = evidence::publish_blob(home, cdn, 4096).unwrap();
    let v = evidence::publish_blob(home, versions.as_bytes(), 4096).unwrap();
    let d = evidence::publish_blob(home, distribution, 4096).unwrap();
    let mut metadata = Metadata::open(home, true).unwrap();
    metadata
        .put(&format!("remote-config/{BUILD_KEY}"), &b, None)
        .unwrap();
    metadata
        .put(&format!("remote-config/{CDN_KEY}"), &c, None)
        .unwrap();
    metadata.put("remote-catalog/wow/cn", &json!({"productCode":"wow", "region":"cn", "observedAt":OBSERVED, "versions":v, "distribution":d}), None).unwrap();
    metadata.close().unwrap();
}

#[test]
fn real_argv_preserves_fixed_target_selector_exclusions() {
    for flag in [
        "--target",
        "--installation",
        "--product",
        "--build",
        "--region",
        "--locale",
        "--definitions",
        "--from",
        "--offline",
    ] {
        let mut args = words(&["target", "resolve", "--file", "selection.json", flag]);
        if flag != "--offline" {
            args.push("value".into());
        }
        let error = super::super::parse(&args).err().unwrap();
        assert_eq!(error.exit_code, 2);
        assert_eq!(
            error.message,
            format!("target resolve --file cannot be combined with {flag}")
        );
    }
    for flag in [
        "--installation",
        "--product",
        "--build",
        "--region",
        "--locale",
    ] {
        let error = super::super::parse(&words(&[
            "target", "resolve", "--target", "named", flag, "value",
        ]))
        .err()
        .unwrap();
        assert_eq!(
            error.message,
            format!("target resolve --target cannot be combined with {flag}")
        );
    }
    let error = super::super::parse(&words(&["target", "resolve", "--build", "12.1.5.70000"]))
        .err()
        .unwrap();
    assert_eq!(error.message, "target resolve --build requires --product");
}

#[test]
fn direct_request_keeps_all_explicit_fields_and_never_takes_local_or_named_path() {
    let path = home();
    let mut inv = invocation(&path);
    inv.flags.insert("--build".into(), "12.1.5.70000".into());
    inv.flags.insert("--from".into(), "PIN-parent".into());
    let q = request(&inv).unwrap();
    assert_eq!(
        (q.product.as_str(), q.region.as_str(), q.locale.as_str()),
        ("retail", "cn", "zhCN")
    );
    assert_eq!(q.full_build, "12.1.5.70000");
    assert_eq!(q.definitions, "a".repeat(40));
    assert_eq!(q.parent, "PIN-parent");
    assert!(q.offline);
    inv.flags
        .insert("--installation".into(), "explicit-local".into());
    assert!(request(&inv).is_none());
    inv.flags.remove("--installation");
    inv.flags.insert("--target".into(), "named".into());
    assert!(request(&inv).is_none());
    inv.flags.remove("--target");
    inv.flags.insert("--installation".into(), String::new());
    assert!(
        request(&inv).is_some(),
        "Go dispatch compares the installation value, not presence"
    );
    assert!(!path.exists());
}

#[test]
fn direct_validation_precedes_workspace_and_retains_command_exit_two() {
    let path = home();
    for (flag, value, code) in [
        ("--product", "unknown", "selection.unsupported_data_product"),
        ("--locale", "unknown", "selection.unsupported_data_language"),
        ("--region", "unknown", "selection.unsupported_data_region"),
        ("--build", "+1.2.3.4", "records.invalid_metadata"),
        ("--definitions", "master", "records.definition_identity"),
    ] {
        let mut inv = invocation(&path);
        inv.flags.insert(flag.into(), value.into());
        let error = super::super::dispatch(&inv).unwrap_err();
        assert_eq!(error.code, code);
        assert_eq!(error.exit_code, 2);
        assert!(!path.exists());
    }
}

#[test]
fn actual_default_offline_remote_target_returns_pin_context_capture_and_warning() {
    let path = home();
    seed(&path);
    let inv = invocation(&path);
    let outcome = super::super::dispatch(&inv).unwrap();
    let pin: selection::PinnedSet = serde_json::from_value(outcome.data.clone()).unwrap();
    let data = pin.data.as_ref().unwrap();
    assert_eq!(data.product, "retail");
    assert_eq!(data.region, "cn");
    assert_eq!(data.language, "zhCN");
    assert_eq!(data.full_build, "12.1.5.70000");
    assert_eq!(data.build_config, BUILD_KEY);
    assert_eq!(data.cdn_config, CDN_KEY);
    assert_eq!(
        outcome.selection,
        json!({"snapshot":pin.id,"observedAt":OBSERVED,"offline":true})
    );
    assert_eq!(outcome.provenance.len(), 1);
    let capture = &outcome.provenance[0];
    assert_eq!(capture["provenance"]["kind"], "remote-target");
    assert_eq!(capture["provenance"]["snapshot"], pin.id);
    assert_eq!(capture["provenance"]["dataBuild"], data.full_build);
    assert_eq!(
        evidence::read_capture(&path, capture["id"].as_str().unwrap()).unwrap(),
        *capture
    );
    let body: Value =
        serde_json::from_slice(&evidence::read_blob(&path, &capture["blob"], 65536).unwrap())
            .unwrap();
    assert_eq!(body["pin"], outcome.data);
    assert_eq!(body["offline"], true);
    assert_eq!(
        outcome.warnings,
        vec![json!(
            "Remote target metadata is pinned; this does not download data archives. Offline resolution reuses a recorded observation, not the server's current state."
        )]
    );
    let second = super::super::dispatch(&inv).unwrap();
    assert_eq!(outcome.data, second.data);
    std::fs::remove_dir_all(path).unwrap();
}

#[test]
fn actual_default_remote_preserves_parent_build_and_definition_inheritance() {
    let path = home();
    seed(&path);
    let parent = selection::pin_selection(
        &path,
        selection::SelectionSpec {
            data: Some(selection::DataPin {
                product: "retail".into(),
                region: "cn".into(),
                language: "zhCN".into(),
                full_build: "12.1.5.70000".into(),
                build_config: BUILD_KEY.into(),
                cdn_config: CDN_KEY.into(),
                definition_commit: "b".repeat(40),
            }),
            ..Default::default()
        },
    )
    .unwrap();
    let mut inv = invocation(&path);
    inv.flags.remove("--definitions");
    inv.flags.insert("--from".into(), parent.id.clone());
    let output = super::super::dispatch(&inv).unwrap();
    assert_eq!(output.data["parent"], parent.id);
    assert_eq!(output.data["data"]["definitionCommit"], "b".repeat(40));
    assert_eq!(output.data["data"]["fullBuild"], "12.1.5.70000");
    inv.flags.insert("--build".into(), "12.1.5.70001".into());
    let error = super::super::dispatch(&inv).unwrap_err();
    assert_eq!(error.code, "records.build_unavailable");
    std::fs::remove_dir_all(path).unwrap();
}

#[test]
fn actual_default_offline_missing_observation_does_not_request_installation() {
    let path = home();
    workspace::initialize(&path).unwrap();
    let error = super::super::dispatch(&invocation(&path)).unwrap_err();
    assert_eq!(error.code, "records.remote_unavailable_offline");
    std::fs::remove_dir_all(path).unwrap();
}

#[test]
fn actual_default_remote_original_cancel_and_deadline_precede_provider_io() {
    let path = home();
    for expired in [false, true] {
        let mut inv = invocation(&path);
        inv.control = OperationControl::uncancelable(if expired {
            Instant::now() - Duration::from_secs(1)
        } else {
            Instant::now() + Duration::from_secs(30)
        });
        if !expired {
            inv.control.tracked_cancel_handle().unwrap().cancel();
        }
        let error = super::super::dispatch(&inv).unwrap_err();
        assert_eq!(
            error.code,
            if expired {
                "command.deadline"
            } else {
                "command.cancelled"
            }
        );
        assert!(!path.exists());
    }
}

mod named;
