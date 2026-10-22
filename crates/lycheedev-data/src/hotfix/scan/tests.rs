use super::*;
use lycheedev_core::{
    metadata::Metadata,
    selection::{self, DataPin, SelectionSpec},
    workspace,
};
use std::{fs, path::PathBuf};

struct Fixture {
    root: PathBuf,
    ctx: DataContext,
    request: ScanRequest,
}
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "lychee-hotfix-scan-fault-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&root).unwrap();
        let home = root.join("home");
        workspace::initialize(&home).unwrap();
        let pin = selection::pin_selection(
            &home,
            SelectionSpec {
                data: Some(DataPin {
                    product: "retail".into(),
                    region: "cn".into(),
                    language: "zhCN".into(),
                    full_build: "12.1.0.69875".into(),
                    build_config: "a".repeat(32),
                    cdn_config: "b".repeat(32),
                    definition_commit: "c".repeat(40),
                }),
                ..Default::default()
            },
        )
        .unwrap()
        .id;
        let mut raw = b"XFTH".to_vec();
        raw.extend(9u32.to_le_bytes());
        raw.extend(69875u32.to_le_bytes());
        raw.resize(44, 0);
        for id in 1u32..=3 {
            let mut header = [0; 32];
            header[..4].copy_from_slice(b"XFTH");
            header[16..20].copy_from_slice(&42u32.to_le_bytes());
            header[20..24].copy_from_slice(&id.to_le_bytes());
            header[28] = 2;
            raw.extend(header);
        }
        let file = root.join("DBCache.bin");
        fs::write(&file, raw).unwrap();
        let ctx = DataContext { home, cwd: root.clone(), control:Default::default(), source:Default::default() };
        let request = ScanRequest { page:LocalRequest {snapshot:pin,input:LocalInput::File(file),table:None,offline:false,max_bytes:128<<20,
            filter:CacheFilter {limit:1,..Default::default()}},cursor:String::new(),max_pages:3};
        Self { root, ctx, request }
    }
    fn resume(&self, error: &ToolkitError) -> ScanRequest {
        let reading = &error.details["result"];
        let mut request = self.request.clone();
        request.page.input = LocalInput::Capture(reading["result"]["source"]["id"].as_str().unwrap().into());
        request.page.snapshot = reading["result"]["snapshot"].as_str().unwrap().into();
        request.cursor = reading["resume"].as_str().unwrap().into();
        request
    }
    fn fresh_context(&self) -> DataContext {
        let mut ctx=self.ctx.clone();ctx.control=Default::default();ctx
    }
    fn count_scans(&self) -> usize {
        Metadata::open(&self.ctx.home, false)
            .unwrap()
            .list::<Value>("capture/", 1000)
            .unwrap()
            .iter()
            .filter(|value| value["provenance"]["kind"] == "hotfix-scan")
            .count()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn cancellation_after_a_saved_page_returns_that_checkpoint_and_can_resume() {
    let fixture = Fixture::new();
    let cancellation = fixture.ctx.control.tracked_cancel_handle().unwrap();
    let mut calls = 0;
    let error = dispatch_with(&fixture.ctx, &fixture.request, |ctx, request| {
        calls += 1;
        if calls == 2 {
            cancellation.cancel();
        }
        local::dispatch(ctx, request)
    })
    .unwrap_err();
    assert_eq!(error.exit_code, 7);
    assert_eq!(error.details["result"]["result"]["returned"], 1);
    assert_eq!(
        error.details["result"]["result"]["pages"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        error.details["result"]["resume"],
        error.details["result"]["capture"]["id"]
    );
    assert_eq!(fixture.count_scans(), 1);
    let done = dispatch(&fixture.fresh_context(), &fixture.resume(&error)).unwrap();
    assert!(done.complete);
    assert_eq!(done.data["result"]["returned"], 3);
}

#[test]
fn removed_page_before_checkpoint_commit_returns_previous_published_coverage() {
    let fixture = Fixture::new();
    let mut calls = 0;
    let error = dispatch_with(&fixture.ctx, &fixture.request, |ctx, request| {
        let page = local::dispatch(ctx, request)?;
        calls += 1;
        if calls == 2 {
            evidence::remove_capture(
                &ctx.home,
                page.provenance.last().unwrap()["id"].as_str().unwrap(),
            )?;
        }
        Ok(page)
    })
    .unwrap_err();
    assert_eq!(error.code, "vault.record_missing");
    assert_eq!(error.details["result"]["result"]["returned"], 1);
    assert_eq!(fixture.count_scans(), 1);
    let done = dispatch(&fixture.fresh_context(), &fixture.resume(&error)).unwrap();
    assert!(done.complete);
    assert_eq!(done.data["result"]["returned"], 3);
}

#[test]
fn removed_first_source_before_publication_does_not_invent_a_resume() {
    let fixture = Fixture::new();
    let error = dispatch_with(&fixture.ctx, &fixture.request, |ctx, request| {
        let page = local::dispatch(ctx, request)?;
        evidence::remove_capture(
            &ctx.home,
            page.provenance.last().unwrap()["id"].as_str().unwrap(),
        )?;
        evidence::remove_capture(&ctx.home, page.data["source"]["id"].as_str().unwrap())?;
        Ok(page)
    })
    .unwrap_err();
    assert_eq!(error.code, "vault.record_missing");
    assert!(error.details.get("result").is_none());
    assert_eq!(fixture.count_scans(), 0);
}

#[test]
fn source_coverage_and_cursor_faults_never_publish_mutated_progress() {
    for fault_kind in [
        "source",
        "matched",
        "backward",
        "next_missing",
        "next_wrong",
        "short_final",
    ] {
        let fixture = Fixture::new();
        let mut calls = 0;
        let error = dispatch_with(&fixture.ctx, &fixture.request, |ctx, request| {
            let mut page = local::dispatch(ctx, request)?;
            calls += 1;
            if calls == 2 {
                match fault_kind {
                    "source" => page.data["source"]["id"] = json!("CAP-changed"),
                    "matched" => page.data["page"]["matched"] = json!(4),
                    "backward" => page.data["page"]["entries"][0]["index"] = json!(0),
                    "next_missing" => {
                        page.data["page"]
                            .as_object_mut()
                            .unwrap()
                            .remove("nextIndex");
                    }
                    "next_wrong" => page.data["page"]["nextIndex"] = json!(2),
                    "short_final" => page.data["page"]["truncated"] = json!(false),
                    _ => unreachable!(),
                }
            }
            Ok(page)
        })
        .unwrap_err();
        assert_eq!(
            error.code,
            if matches!(fault_kind, "source" | "matched") {
                "records.hotfix_identity"
            } else {
                "records.hotfix_cursor"
            },
            "{fault_kind}"
        );
        assert_eq!(
            error.details["result"]["result"]["returned"], 1,
            "{fault_kind}"
        );
        assert_eq!(fixture.count_scans(), 1);
    }
}

#[test]
fn manifest_output_budget_is_enforced_before_checkpoint_publication() {
    let fixture = Fixture::new();
    let budget = ledger(&fixture.ctx);
    let state = State {
        snapshot: "x".repeat(MANIFEST_BYTES),
        ..Default::default()
    };
    assert_eq!(
        encoded_state(&state, &budget).unwrap_err().code,
        "records.hotfix_limit"
    );
    assert_eq!(fixture.count_scans(), 0);
}

#[test]
fn all_10000_page_dependencies_keep_one_live_reservation_and_cumulative_work() {
    use lycheedev_core::evidence::CaptureAccounting;
    let fixture = Fixture::new();
    let budget = ledger(&fixture.ctx);
    let meter = ScanCaptureAccounting::new(&budget);
    let length = 1 << 20;
    for _ in 0..=MAX_PAGES {
        meter.admit_dependency(length).unwrap();
        assert_eq!(
            budget.snapshot().unwrap().retained_bytes,
            (length * 24 + 4096) as u64
        );
    }
    let used = budget.snapshot().unwrap();
    assert_eq!(used.metadata_bytes, ((MAX_PAGES + 1) * length) as u64);
    assert_eq!(used.decode_work, ((MAX_PAGES + 1) * length * 8) as u64);
    drop(meter);
    assert_eq!(budget.snapshot().unwrap().retained_bytes, 0);
    assert_eq!(
        budget.snapshot().unwrap().metadata_bytes,
        used.metadata_bytes
    );
    assert_eq!(budget.snapshot().unwrap().decode_work, used.decode_work);
}

#[test]
fn dependency_peak_metadata_and_work_limits_fail_independently() {
    use lycheedev_core::evidence::CaptureAccounting;
    let fixture = Fixture::new();
    for limits in [
        Cost {
            retained_bytes: 24 * 1024 + 4095,
            ..Cost::defaults()
        },
        Cost {
            metadata_bytes: 1023,
            ..Cost::defaults()
        },
        Cost {
            decode_work: 8191,
            ..Cost::defaults()
        },
    ] {
        let budget = Budget::controlled(
            limits,
            fixture.ctx.control.deadline(),
            fixture.ctx.control.clone(),
        );
        let meter = ScanCaptureAccounting::new(&budget);
        assert_eq!(
            meter.admit_dependency(1024).unwrap_err().code,
            "records.query_resource_budget"
        );
        assert_eq!(budget.snapshot().unwrap().retained_bytes, 0);
    }
    let budget = ledger(&fixture.ctx);
    let meter = ScanCaptureAccounting::new(&budget);
    let existing = budget.reserve_retained((512 << 20) - 4096).unwrap();
    assert_eq!(
        meter.admit_dependency(1).unwrap_err().code,
        "records.query_resource_budget"
    );
    drop(existing);
    meter.admit_dependency(1).unwrap();
    assert_eq!(budget.snapshot().unwrap().retained_bytes, 4120);
}

#[test]
fn cumulative_scan_page_limit_is_checked_before_another_page_is_inspected() {
    let mut state = State::default();
    for _ in 0..MAX_PAGES - 1 {
        state.pages.push("CAP-previous".into());
    }
    admit_page(&state).unwrap();
    state.pages.push("CAP-last".into());
    assert_eq!(admit_page(&state).unwrap_err().code, "records.hotfix_limit");
}
