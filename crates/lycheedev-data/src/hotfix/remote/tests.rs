//! Fixed Go3.2.0 provider, parser, output and source-entry fixtures. No game or public network.
use super::*;
use std::{
    cell::Cell,
    collections::BTreeMap,
    fs,
    path::PathBuf,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
struct Fixture {
    root: PathBuf,
    ctx: DataContext,
    request: Request,
}
impl Fixture {
    fn new(source: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "lychee-remote-hotfix-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&root).unwrap();
        let home = root.join("home");
        lycheedev_core::workspace::initialize(&home).unwrap();
        let ctx = DataContext {
            home,
            cwd: root.clone(),
            control: OperationControl::default(),
            source: Default::default(),
        };
        let request = Request {
            provider: if source == "wago" {
                Provider::Wago
            } else {
                Provider::Raidbots(root.join("DBCache.bin"))
            },
            query: q(),
            max_pages: 2048,
            fetch: FetchBudget::default(),
            csv: None,
        };
        Self { root, ctx, request }
    }
    fn cache(&mut self, rows: &[(i32, u32, u8, &[u8])]) -> PathBuf {
        let path = self.root.join("DBCache.bin");
        let mut raw = b"XFTH".to_vec();
        raw.extend(9u32.to_le_bytes());
        raw.extend(68974u32.to_le_bytes());
        raw.resize(44, 0);
        for (push, id, status, payload) in rows {
            raw.extend(b"XFTH");
            raw.extend(71u32.to_le_bytes());
            raw.extend(push.to_le_bytes());
            raw.extend(id.to_le_bytes());
            raw.extend(42u32.to_le_bytes());
            raw.extend(id.to_le_bytes());
            raw.extend((payload.len() as u32).to_le_bytes());
            raw.extend([*status, 0, 0, 0]);
            raw.extend(*payload);
        }
        fs::write(&path, raw).unwrap();
        self.request.provider = Provider::Raidbots(path.clone());
        path
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}
fn q() -> Query {
    Query {
        product: "retail".into(),
        full_build: "12.0.7.68974".into(),
        region: "us".into(),
        locale: "enUS".into(),
        limit: 50,
        ..Default::default()
    }
}
#[test]
fn current_hotfix_query_accepts_formal_forever_and_rejects_retired_products() {
    let mut query = q();
    query.product = "forever".into();
    query.full_build = "1.60.1.70707".into();
    query.validate().unwrap();
    for product in ["forever-asia-beta", "forever-cn-beta"] {
        query.product = product.into();
        let error = query.validate().unwrap_err();
        assert_eq!(error.code, "records.hotfix_query");
        assert!(error.message.contains("selection.unsupported_data_product"));
        assert_eq!(error.exit_code, 2);
    }
}
fn html(payload: &str) -> Vec<u8> {
    format!(
        "<html><div id=\"app\" data-page=\"{}\"></div></html>",
        payload
            .replace('&', "&amp;")
            .replace('"', "&#34;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('\'', "&#39;")
    )
    .into_bytes()
}
fn row(id: u64, push: i32, status: u8) -> Value {
    json!({"id":id,"push_id":push,"record_id":id+40,"status":status,"build":68974,"table_name":"SpellEffect","data":[1,2],"created_at":"2026-08-05 22:09:07","region_id":71,"locale":"enUS"})
}
fn page(current: i64, last: i64, total: i64, rows: Vec<Value>) -> Vec<u8> {
    html(&json!({"component":"Hotfixes","props":{"hotfixes":{"current_page":current,"last_page":last,"per_page":25,"total":total,"data":rows,"next_page_url":if current<last{json!(format!("/hotfixes?page={}",current+1))}else{Value::Null}}}}).to_string())
}
struct Pages {
    pages: BTreeMap<i64, Vec<u8>>,
    calls: Cell<usize>,
}
impl Pages {
    fn new(pages: Vec<(i64, Vec<u8>)>) -> Self {
        Self {
            pages: pages.into_iter().collect(),
            calls: Cell::new(0),
        }
    }
}
impl wago::Fetcher for Pages {
    fn fetch(&self, url: &str, control: &OperationControl, _: &Budget) -> Result<Vec<u8>> {
        control.check()?;
        self.calls.set(self.calls.get() + 1);
        let n = url
            .split("page=")
            .nth(1)
            .unwrap()
            .split('&')
            .next()
            .unwrap()
            .parse::<i64>()
            .unwrap();
        self.pages
            .get(&n)
            .cloned()
            .ok_or_else(|| error("wago_http", 5, "status 404"))
    }
}
fn run_wago(f: &Fixture, query: Query, pages: &Pages) -> Result<RemoteResult> {
    wago::query(
        &f.ctx,
        query,
        f.request.fetch.clone(),
        f.request.max_pages,
        &budget(&f.ctx, &f.request),
        pages,
        "http://fixture.invalid",
    )
}
fn two() -> Pages {
    Pages::new(vec![
        (1, page(1, 2, 4, vec![row(1, 10, 1), row(2, 10, 1)])),
        (2, page(2, 2, 4, vec![row(3, 9, 3), row(4, 9, 3)])),
    ])
}
#[test]
fn query_validation_and_scope_are_fixed() {
    let good = q();
    good.validate().unwrap();
    let mut bad = good.clone();
    bad.product = "ptr".into();
    assert_eq!(bad.validate().unwrap_err().code, "records.hotfix_query");
    for n in [0, 201] {
        let mut bad = good.clone();
        bad.limit = n;
        assert!(bad.validate().is_err());
    }
    let mut changed = good.clone();
    changed.limit = 1;
    changed.page = 9;
    changed.cursor = "position".into();
    assert_eq!(
        good.scope_digest().unwrap(),
        changed.scope_digest().unwrap()
    );
    changed.table = "SPELLEFFECT".into();
    assert!(changed.scope().unwrap().ends_with("table:spelleffect"));
    changed.table_hash = Some(42);
    assert!(changed.validate().is_err());
}
#[test]
fn time_formats_are_not_guessed() {
    let a = model::parse_time("2026-08-05T22:09:07Z").unwrap();
    assert_eq!(a, model::parse_time("2026-08-05 22:09:07").unwrap());
    assert!(model::parse_time("yesterday").is_err());
    let mut query = q();
    query.from = Some("2026-08-06T00:00:00Z".into());
    query.to = Some("2026-08-05T00:00:00Z".into());
    assert!(query.validate().is_err());
}
#[test]
fn cursor_roundtrip_pins_scope_and_source() {
    let f = Fixture::new("wago");
    let c = Cursor {
        source: "wago".into(),
        reference: "a".repeat(64),
        scope: q().scope_digest().unwrap(),
        resume_page: 3,
        after: "id:9".into(),
        ..Default::default()
    }
    .encode(&budget(&f.ctx, &f.request))
    .unwrap();
    assert_eq!(Cursor::decode(&c, &q(), "wago").unwrap().resume_page, 3);
    assert!(Cursor::decode("broken", &q(), "wago").is_err());
    assert!(Cursor::decode(&c, &q(), "raidbots").is_err());
    let mut other = q();
    other.status = Some(1);
    assert!(Cursor::decode(&c, &other, "wago").is_err());
}
#[test]
fn raidbots_preserves_physical_metadata_and_declared_coverage() {
    let mut f = Fixture::new("raidbots");
    let path = f.cache(&[(-3, 55, 1, b"\xde\xad"), (4, 56, 3, b"")]);
    let out = dispatch(&f.ctx, &f.request).unwrap();
    assert_eq!(out.data["records"][0]["push"], -3);
    assert_eq!(out.data["records"][0]["payloadHex"], "dead");
    assert_eq!(out.data["records"][0]["regionId"], 71);
    assert_eq!(out.data["coverage"]["complete"], false);
    assert_eq!(out.data["coverage"]["fullQueryComplete"], false);
    assert_eq!(out.data["complete"], true);
    assert_eq!(
        out.data["change"]["sha256"],
        storage::sha256(&fs::read(path).unwrap())
    );
    assert!(out.data["snapshot"].as_str().unwrap().starts_with("PIN-"));
    assert!(out.provenance.is_empty());
}
#[test]
fn raidbots_latest_is_signed_batch_before_cursor_paging() {
    let mut f = Fixture::new("raidbots");
    f.cache(&[(-3, 1, 1, b"a"), (4, 2, 1, b"b"), (4, 3, 3, b"")]);
    f.request.query.latest = true;
    f.request.query.limit = 1;
    let first = dispatch(&f.ctx, &f.request).unwrap();
    assert_eq!(first.data["records"].as_array().unwrap().len(), 1);
    assert_eq!(first.data["records"][0]["recordId"], 2);
    assert_eq!(first.data["coverage"]["selectedPush"], 4);
    assert!(!first.complete);
    assert_eq!(first.data["complete"], false);
    assert_eq!(first.data["truncated"], true);
    assert_eq!(first.data["coverage"]["complete"], false);
    assert_eq!(first.data["coverage"]["fullQueryComplete"], false);
    assert_eq!(first.data["coverage"]["matched"], 2);
    assert_eq!(first.data["coverage"]["returned"], 1);
    f.request.query.cursor = first.data["nextCursor"].as_str().unwrap().into();
    let second = dispatch(&f.ctx, &f.request).unwrap();
    assert_eq!(second.data["records"].as_array().unwrap().len(), 1);
    assert_eq!(second.data["records"][0]["recordId"], 3);
    // Retained scan semantics require no AfterIndex for Complete.
    // Reaching the tail ends pagination; it does not make this response the
    // entire selected batch or establish Raidbots server coverage.
    assert!(!second.complete);
    assert_eq!(second.data["complete"], false);
    assert_eq!(second.data["truncated"], false);
    assert!(second.data.get("nextCursor").is_none());
    assert_eq!(second.data["coverage"]["selectedPush"], 4);
    assert_eq!(second.data["coverage"]["complete"], false);
    assert_eq!(second.data["coverage"]["fullQueryComplete"], false);
    assert_eq!(second.data["coverage"]["truncated"], false);
    assert_eq!(second.data["coverage"]["matched"], 2);
    assert_eq!(second.data["coverage"]["returned"], 1);

    f.request.query.cursor.clear();
    f.request.query.limit = 2;
    let whole = dispatch(&f.ctx, &f.request).unwrap();
    assert_eq!(whole.data["records"].as_array().unwrap().len(), 2);
    assert_eq!(whole.data["records"][0], first.data["records"][0]);
    assert_eq!(whole.data["records"][1], second.data["records"][0]);
    assert!(whole.complete);
    assert_eq!(whole.data["complete"], true);
    assert_eq!(whole.data["truncated"], false);
    assert!(whole.data.get("nextCursor").is_none());
    assert_eq!(whole.data["coverage"]["complete"], false);
    assert_eq!(whole.data["coverage"]["fullQueryComplete"], false);
}
#[test]
fn raidbots_stale_snapshot_refused() {
    let mut f = Fixture::new("raidbots");
    let path = f.cache(&[(1, 1, 1, b"a")]);
    let file = fs::OpenOptions::new().write(true).open(&path).unwrap();
    file.set_times(
        fs::FileTimes::new().set_modified(SystemTime::now() - Duration::from_secs(31 * 86400)),
    )
    .unwrap();
    assert_eq!(
        dispatch(&f.ctx, &f.request).unwrap_err().code,
        "records.hotfix_snapshot_stale"
    );
}
#[test]
fn raidbots_rejects_unsupported_filters_and_changed_cursor_bytes() {
    let mut f = Fixture::new("raidbots");
    let path = f.cache(&[(1, 1, 1, b"a"), (1, 2, 1, b"b")]);
    for field in ["search", "page", "table"] {
        let mut request = f.request.clone();
        match field {
            "search" => request.query.search = "x".into(),
            "page" => request.query.page = 1,
            "table" => request.query.table = "SpellEffect".into(),
            _ => unreachable!(),
        };
        assert_eq!(
            dispatch(&f.ctx, &request).unwrap_err().code,
            "records.hotfix_filter_unsupported"
        );
    }
    f.request.query.limit = 1;
    let first = dispatch(&f.ctx, &f.request).unwrap();
    f.request.query.cursor = first.data["nextCursor"].as_str().unwrap().into();
    let mut bytes = fs::read(&path).unwrap();
    *bytes.last_mut().unwrap() = b'c';
    fs::write(path, bytes).unwrap();
    assert_eq!(
        dispatch(&f.ctx, &f.request).unwrap_err().code,
        "records.hotfix_cursor"
    );
}
#[test]
fn raidbots_parent_change_reuse_and_conflict() {
    let mut f = Fixture::new("raidbots");
    f.cache(&[(1, 1, 1, b"a")]);
    let first = dispatch(&f.ctx, &f.request).unwrap();
    let id = first.data["snapshot"].as_str().unwrap().to_owned();
    f.ctx.source.snapshot = Some(id.clone());
    assert_eq!(dispatch(&f.ctx, &f.request).unwrap().data["snapshot"], id);
    f.request.query.record_id = Some(1);
    assert_eq!(
        dispatch(&f.ctx, &f.request).unwrap_err().code,
        "records.hotfix_identity"
    );
}
#[test]
fn parser_reads_fixed_real_pages() {
    let f = Fixture::new("wago");
    let b = budget(&f.ctx, &f.request);
    for (raw, current, last, total) in [
        (
            include_bytes!("../../../../../tests/fixtures/hotfix-wago/response-page-1.html")
                .as_slice(),
            1,
            680096,
            17002386,
        ),
        (
            include_bytes!("../../../../../tests/fixtures/hotfix-wago/response-page-2-real.html")
                .as_slice(),
            2,
            680111,
            17002775,
        ),
    ] {
        let p = parse::parse(raw, &b, &f.ctx.control).unwrap();
        assert_eq!(
            (p.current_page, p.last_page, p.total),
            (current, last, total)
        );
        assert!(!p.records.is_empty());
        if current == 1 {
            assert_eq!(p.records[0].id, 25567226914);
            assert_eq!(p.records[0].table_name, "VehicleSeat");
            assert_eq!(
                p.records[0].payload_length,
                p.records[0].payload.as_ref().unwrap().get().len()
            );
        }
    }
}
#[test]
fn parser_preserves_raw_payload_and_flexible_numbers() {
    let f = Fixture::new("wago");
    let b = budget(&f.ctx, &f.request);
    let raw = html(
        r#"{"component":"Hotfixes","props":{"hotfixes":{"current_page":1,"last_page":1,"per_page":25,"total":"1","data":[{"id":"25567226914","push_id":"-3","record_id":"55","status":null,"build":68974,"table_name":"SpellEffect","region_id":null,"data":[ 18446744073709551615, 1.2300 ]}]}}}"#,
    );
    let p = parse::parse(&raw, &b, &f.ctx.control).unwrap();
    assert_eq!(
        p.records[0].payload.as_ref().unwrap().get(),
        "[ 18446744073709551615, 1.2300 ]"
    );
    assert_eq!(p.records[0].push, -3);
    assert_eq!(p.records[0].region_id, Some(0));
}
#[test]
fn parser_rejects_protocol_changes_instead_of_empty_success() {
    let f = Fixture::new("wago");
    let b = budget(&f.ctx, &f.request);
    for raw in [
        b"<div id='app'></div>".to_vec(),
        html("{oops"),
        page(0, 1, 0, vec![]),
        page(3, 2, 0, vec![]),
        html(r#"{"component":"Other","props":{}}"#),
    ] {
        assert_eq!(
            parse::parse(&raw, &b, &f.ctx.control).unwrap_err().code,
            "records.hotfix_wago_protocol"
        );
    }
    for (key, value) in [
        ("id", json!(-1)),
        ("push_id", json!(2147483648u64)),
        ("record_id", json!(4294967296u64)),
        ("status", json!(256)),
        ("table_name", json!("")),
        ("build", json!(null)),
    ] {
        let mut r = row(1, 2, 1);
        r[key] = value;
        assert!(parse::parse(&page(1, 1, 1, vec![r]), &b, &f.ctx.control).is_err());
    }
}
#[test]
fn parser_accepts_single_quote_unquoted_and_casefolded_tags() {
    let f = Fixture::new("wago");
    let raw = page(1, 1, 0, vec![]);
    let text = String::from_utf8(raw)
        .unwrap()
        .replace("<div id=\"app\"", "<DIV ID=app")
        .replace("data-page=\"", "DATA-PAGE='")
        .replace("\"></div>", "'></DIV>");
    assert_eq!(
        parse::parse(text.as_bytes(), &budget(&f.ctx, &f.request), &f.ctx.control)
            .unwrap()
            .current_page,
        1
    );
}
#[test]
fn wago_multpage_cache_hit_and_complete_scope() {
    let f = Fixture::new("wago");
    let pages = two();
    let result = run_wago(&f, q(), &pages).unwrap();
    assert_eq!(result.records.len(), 4);
    assert!(result.complete && result.coverage.full_query_complete);
    assert_eq!(result.coverage.fetch.as_ref().unwrap().requests, 2);
    let cached = run_wago(&f, q(), &pages).unwrap();
    assert_eq!(pages.calls.get(), 2);
    assert_eq!(cached.change, result.change);
    assert!(cached.warnings.contains(&"records.wago_cache_hit".into()));
}
#[test]
fn wago_late_page_window_is_not_full_query() {
    let f = Fixture::new("wago");
    let pages = two();
    let mut query = q();
    query.page = 2;
    let result = run_wago(&f, query, &pages).unwrap();
    assert!(result.complete);
    assert!(!result.coverage.full_query_complete);
}
#[test]
fn wago_limit_cursor_resumes_and_refuses_scope_change() {
    let f = Fixture::new("wago");
    let pages = two();
    let mut query = q();
    query.limit = 2;
    let first = run_wago(&f, query.clone(), &pages).unwrap();
    assert!(first.truncated);
    query.cursor = first.next_cursor;
    let second = run_wago(&f, query.clone(), &pages).unwrap();
    assert_eq!(
        second.records.iter().map(|r| r.id).collect::<Vec<_>>(),
        vec![3, 4]
    );
    assert!(second.complete && !second.coverage.full_query_complete);
    query.status = Some(1);
    assert_eq!(
        run_wago(&f, query, &pages).unwrap_err().code,
        "records.hotfix_cursor"
    );
}
#[test]
fn wago_budget_boundary_cursor_and_latest_refusal() {
    let mut f = Fixture::new("wago");
    let pages = two();
    f.request.max_pages = 1;
    let first = run_wago(&f, q(), &pages).unwrap();
    assert_eq!(first.coverage.truncation_reason, "page_budget");
    let mut query = q();
    query.cursor = first.next_cursor;
    let second = run_wago(&f, query, &pages).unwrap();
    assert!(second.complete && !second.coverage.full_query_complete);
    let mut latest = q();
    latest.latest = true;
    assert_eq!(
        run_wago(&f, latest, &pages).unwrap_err().code,
        "records.hotfix_budget"
    );
}
#[test]
fn wago_network_byte_budget_never_publishes_failed_page() {
    let mut f = Fixture::new("wago");
    let pages = two();
    f.request.fetch.max_bytes = 1;
    assert_eq!(
        run_wago(&f, q(), &pages).unwrap_err().code,
        "records.hotfix_budget"
    );
    f.ctx.source.offline = true;
    assert_eq!(
        run_wago(&f, q(), &pages).unwrap_err().code,
        "records.hotfix_offline"
    );
    assert_eq!(pages.calls.get(), 1);
}
#[test]
fn wago_offline_miss_and_hit_make_no_network_call() {
    let mut f = Fixture::new("wago");
    let pages = two();
    f.ctx.source.offline = true;
    assert_eq!(
        run_wago(&f, q(), &pages).unwrap_err().code,
        "records.hotfix_offline"
    );
    assert_eq!(pages.calls.get(), 0);
    f.ctx.source.offline = false;
    run_wago(&f, q(), &pages).unwrap();
    f.ctx.source.offline = true;
    run_wago(&f, q(), &pages).unwrap();
    assert_eq!(pages.calls.get(), 2);
}
#[test]
fn wago_page_total_drift_and_duplicate_ids_are_errors() {
    for duplicate in [false, true] {
        let f = Fixture::new("wago");
        let pages = Pages::new(vec![
            (1, page(1, 2, 2, vec![row(1, 1, 1)])),
            (
                2,
                page(
                    2,
                    2,
                    if duplicate { 2 } else { 3 },
                    vec![row(if duplicate { 1 } else { 2 }, 1, 1)],
                ),
            ),
        ]);
        assert_eq!(
            run_wago(&f, q(), &pages).unwrap_err().code,
            "records.hotfix_page_drift"
        );
    }
}
#[test]
fn wago_latest_batch_and_unknown_status_are_preserved() {
    let f = Fixture::new("wago");
    let pages = Pages::new(vec![(
        1,
        page(1, 1, 3, vec![row(1, -2, 1), row(2, 7, 99), row(3, 7, 1)]),
    )]);
    let mut query = q();
    query.latest = true;
    let r = run_wago(&f, query, &pages).unwrap();
    assert_eq!(r.coverage.selected_push, Some(7));
    assert_eq!(r.records.len(), 2);
    assert!(
        r.warnings
            .contains(&"records.hotfix_unknown_status:99".into())
    );
}
#[test]
fn wago_exact_filters_search_miss_and_hash_capability() {
    let f = Fixture::new("wago");
    let pages = two();
    let mut query = q();
    query.record_id = Some(41);
    query.region_id = Some(71);
    query.table = "spelleffect".into();
    query.from = Some("2026-08-05T22:09:07Z".into());
    query.to = query.from.clone();
    let r = run_wago(&f, query, &pages).unwrap();
    assert_eq!(r.records.len(), 1);
    let mut query = q();
    query.status = Some(255);
    let r = run_wago(&f, query, &pages).unwrap();
    assert!(r.warnings.contains(&"records.wago_search_miss".into()));
    let mut query = q();
    query.table_hash = Some(42);
    assert_eq!(
        run_wago(&f, query, &pages).unwrap_err().code,
        "records.hotfix_filter_unsupported"
    );
}
#[test]
fn wago_corrupt_blob_fails_without_refetch() {
    let f = Fixture::new("wago");
    let pages = two();
    run_wago(&f, q(), &pages).unwrap();
    let docs = crate::cache_fixture::Cache::new(&f.ctx.home)
        .list::<Value>("hotfix-wago-page/", 10)
        .unwrap();
    let blob = &docs[0]["response"];
    let path = evidence::blob_path(&f.ctx.home, blob["sha256"].as_str().unwrap()).unwrap();
    fs::write(path, b"broken").unwrap();
    assert_eq!(
        run_wago(&f, q(), &pages).unwrap_err().code,
        "records.hotfix_cache_corrupt"
    );
    assert_eq!(pages.calls.get(), 2);
}
#[test]
fn wago_resumed_self_page_refuses_different_bytes() {
    let f = Fixture::new("wago");
    let pages = two();
    let mut query = q();
    query.limit = 1;
    let first = run_wago(&f, query.clone(), &pages).unwrap();
    let cursor = Cursor::decode(&first.next_cursor, &query, "wago").unwrap();
    let metadata = crate::cache_fixture::Cache::new(&f.ctx.home);
    metadata
        .delete(&format!("hotfix-wago-page/{}", cursor.current.unwrap().key))
        .unwrap();
    query.cursor = first.next_cursor;
    let changed = Pages::new(vec![(1, page(1, 2, 4, vec![row(1, 11, 1), row(2, 10, 1)]))]);
    assert_eq!(
        run_wago(&f, query, &changed).unwrap_err().code,
        "records.hotfix_page_drift"
    );
}
#[test]
fn cancellation_before_network_or_snapshot_has_no_side_effect() {
    let mut f = Fixture::new("raidbots");
    f.cache(&[(1, 1, 1, b"a")]);
    f.ctx.control.tracked_cancel_handle().unwrap().cancel();
    assert_eq!(
        dispatch(&f.ctx, &f.request).unwrap_err().code,
        "command.cancelled"
    );
    let f = Fixture::new("wago");
    f.ctx.control.tracked_cancel_handle().unwrap().cancel();
    let pages = two();
    assert_eq!(
        run_wago(&f, q(), &pages).unwrap_err().code,
        "command.cancelled"
    );
    assert_eq!(pages.calls.get(), 0);
}
#[test]
fn csv_columns_quoting_manifest_and_implicit_overwrite() {
    let f = Fixture::new("wago");
    let pages = two();
    let mut r = run_wago(&f, q(), &pages).unwrap();
    r.records[0].table_name = " Vehicle,\"Seat\"\nX\rY".into();
    let path = f.root.join("hotfix.csv");
    fs::write(&path, b"prior").unwrap();
    output::csv(&f.ctx, &path, &r, &budget(&f.ctx, &f.request)).unwrap();
    let raw = fs::read(&path).unwrap();
    let m: Value =
        serde_json::from_slice(&fs::read(path.with_extension("csv.manifest.json")).unwrap())
            .unwrap();
    assert_eq!(
        raw,
        concat!(
            "id,push_id,record_id,table_name,status,build,region_id,locale,payload_length\r\n",
            "1,10,41,\" Vehicle,\"\"Seat\"\"\r\nXY\",1,68974,71,enUS,5\r\n",
            "2,10,42,SpellEffect,1,68974,71,enUS,5\r\n",
            "3,9,43,SpellEffect,3,68974,71,enUS,5\r\n",
            "4,9,44,SpellEffect,3,68974,71,enUS,5\r\n",
        )
        .as_bytes()
    );
    assert_eq!(m["schema"], "lycheedev.csv-export.v1");
    assert_eq!(m["rowCount"], 4);
    assert_eq!(m["complete"], true);
    assert_eq!(m["truncated"], false);
    assert_eq!(
        m["columns"],
        json!([
            "id",
            "push_id",
            "record_id",
            "table_name",
            "status",
            "build",
            "region_id",
            "locale",
            "payload_length"
        ])
    );
    assert_eq!(m["source"]["provider"], "wago");
    assert_eq!(m["source"]["product"], "retail");
    assert_eq!(m["source"]["fullBuild"], "12.0.7.68974");
    assert_eq!(m["source"]["region"], "us");
    assert_eq!(m["source"]["locale"], "enUS");
    assert_eq!(m["source"]["ref"], r.change.sha256);
    assert_eq!(m["source"]["fetchedAt"], r.change.fetched_at);
    assert_eq!(m["content"]["sha256"], storage::sha256(&raw));
    assert_eq!(m["content"]["bytes"], raw.len());
}
#[test]
fn csv_refuses_workspace_and_sidecar_conflict_before_data() {
    let f = Fixture::new("wago");
    let r = run_wago(&f, q(), &two()).unwrap();
    let path = f.ctx.home.join("evil.csv");
    assert_eq!(
        output::csv(&f.ctx, &path, &r, &budget(&f.ctx, &f.request))
            .unwrap_err()
            .code,
        "records.export_path"
    );
    let path = f.root.join("prior.csv");
    fs::write(&path, b"original").unwrap();
    fs::create_dir(f.root.join("prior.csv.manifest.json")).unwrap();
    assert_eq!(
        output::csv(&f.ctx, &path, &r, &budget(&f.ctx, &f.request))
            .unwrap_err()
            .code,
        "records.export_conflict"
    );
    assert_eq!(fs::read(path).unwrap(), b"original");
}

#[test]
fn parser_merges_duplicate_objects_and_reuses_row_slots_like_go() {
    let f = Fixture::new("wago");
    let payload = r#"{"component":"Hotfixes","props":{"hotfixes":{"current_page":1,"last_page":1,"per_page":25,"total":1,"data":[{"id":1,"build":"68974","table_name":"SpellEffect","status":1}]}},"props":{"hotfixes":{"data":[{"record_id":42}]}}}"#;
    let b = budget(&f.ctx, &f.request);
    let p = parse::parse(&html(payload), &b, &f.ctx.control).unwrap();
    assert_eq!(p.records.len(), 1);
    assert_eq!(p.records[0].id, 1);
    assert_eq!(p.records[0].record_id, 42);
    assert_eq!(p.records[0].table_name, "SpellEffect");
}
#[test]
fn parser_supports_all_wire_html_entity_classes() {
    let f = Fixture::new("wago");
    let b = budget(&f.ctx, &f.request);
    let raw = page(1, 1, 1, vec![row(1, 1, 1)]);
    let raw = String::from_utf8(raw)
        .unwrap()
        .replace("SpellEffect", "A&NotEqualTilde;B&#x80;C&copy D");
    let p = parse::parse(raw.as_bytes(), &b, &f.ctx.control).unwrap();
    assert_eq!(p.records[0].table_name, "A≂̸B€C© D");
}
#[test]
fn page_sequence_decodes_query_keys_and_skips_malformed_pairs() {
    let f = Fixture::new("wago");
    let b = budget(&f.ctx, &f.request);
    let next = parse::parse(&page(2, 2, 2, vec![row(2, 1, 1)]), &b, &f.ctx.control).unwrap();
    let mut previous = CursorPage {
        page: 1,
        total: 2,
        last_page: 2,
        next_page_url: "/hotfixes?bad=x;y&broken=%QZ&%70age=%32".into(),
        ..Default::default()
    };
    parse::sequence(&previous, &next).unwrap();
    previous.next_page_url = "http://[invalid/hotfixes?page=2".into();
    assert_eq!(
        parse::sequence(&previous, &next).unwrap_err().code,
        "records.hotfix_page_drift"
    );
    previous.next_page_url = "/hotfixes?page=3&page=2".into();
    assert!(parse::sequence(&previous, &next).is_err());
}
#[test]
fn capture_wire_preserves_payload_tokens_and_duplicate_members() {
    let f = Fixture::new("wago");
    let b = budget(&f.ctx, &f.request);
    let mut r = run_wago(&f, q(), &two()).unwrap();
    r.records[0].payload = Some(
        serde_json::value::RawValue::from_string(
            r#" { "dup":1e+02, "dup":2, "html":"<&>" } "#.into(),
        )
        .unwrap(),
    );
    let raw = wire_bytes(&r, &b, 32 << 20).unwrap();
    let raw = String::from_utf8(raw).unwrap();
    assert!(raw.contains(r#""payload":{"dup":1e+02,"dup":2,"html":"\u003c\u0026\u003e"}"#));
}
#[test]
fn optional_definition_keeps_short_rows_and_status_decode_states() {
    let f = Fixture::new("wago");
    let b = budget(&f.ctx, &f.request);
    let definition = crate::schema::Definition {
        sha256: "a".repeat(64),
        r#match: "build".into(),
        build: q().full_build,
        layout: String::new(),
        fields: vec![crate::schema::Field {
            name: "Value".into(),
            kind: "int".into(),
            foreign_table: String::new(),
            foreign_column: String::new(),
            verified: false,
            bits: 32,
            signed: true,
            elements: 0,
            array: false,
            identity: false,
            inline: true,
            relation: false,
        }],
    };
    let mut records = vec![
        Record {
            status: 1,
            payload: Some(serde_json::value::RawValue::from_string("[-2]".into()).unwrap()),
            ..Default::default()
        },
        Record {
            status: 3,
            payload: Some(serde_json::value::RawValue::from_string("[4]".into()).unwrap()),
            ..Default::default()
        },
        Record::default(),
    ];
    decode::apply_with_check(&mut records, &definition, &b, || Ok(())).unwrap();
    assert_eq!(records[0].fields.as_ref().unwrap()["Value"], -2);
    assert_eq!(records[0].decode_state, "decoded");
    assert_eq!(records[1].decode_state, "not_valid");
    assert_eq!(records[2].decode_state, "no_payload");
    let mut empty = [Record {
        status: 1,
        payload: Some(serde_json::value::RawValue::from_string("[]".into()).unwrap()),
        ..Default::default()
    }];
    decode::apply_with_check(&mut empty, &definition, &b, || Ok(())).unwrap();
    assert_eq!(empty[0].decode_state, "decoded");
    assert_eq!(empty[0].fields.as_ref().unwrap(), &BTreeMap::new());
    let mut overwide = [Record {
        status: 1,
        payload: Some(serde_json::value::RawValue::from_string("[1,2]".into()).unwrap()),
        ..Default::default()
    }];
    assert!(decode::apply_with_check(&mut overwide, &definition, &b, || Ok(())).is_err());
}

#[test]
fn offline_wago_pins_change_and_capture_from_verified_pages() {
    let mut f = Fixture::new("wago");
    let fetched = run_wago(&f, q(), &two()).unwrap();
    f.ctx.source.offline = true;
    let r = run(&f.ctx, &f.request, &budget(&f.ctx, &f.request)).unwrap();
    assert_eq!(r.change, fetched.change);
    assert!(r.snapshot.starts_with("PIN-"));
    let pin = selection::load_pinned(&f.ctx.home, &r.snapshot).unwrap();
    assert_eq!(pin.changes.as_ref(), Some(&r.change));
    let capture = r.capture.as_ref().unwrap();
    assert_eq!(capture["provenance"]["kind"], "hotfix-records");
    assert_eq!(capture["provenance"]["snapshot"], r.snapshot);
    let bytes =
        evidence::read_blob_bounded(&f.ctx.home, &capture["blob"], 32 << 20, &f.ctx.control)
            .unwrap();
    let body: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(body["snapshot"], r.snapshot);
    assert!(body.get("capture").is_none());
    f.ctx.source.snapshot = Some(r.snapshot.clone());
    let again = run(&f.ctx, &f.request, &budget(&f.ctx, &f.request)).unwrap();
    assert_eq!(again.snapshot, r.snapshot);
}
#[test]
fn every_build_segment_is_u32_and_invalid_query_time_is_rejected() {
    for part in 0..4 {
        let mut query = q();
        let mut segments = ["12", "0", "7", "68974"];
        segments[part] = "4294967296";
        query.full_build = segments.join(".");
        assert!(query.validate().is_err(), "segment {part}");
    }
    // Current typed time accepts RFC3339 and the documented space-separated
    // form only; non-RFC3339 offsets and non-time tokens are rejected.
    for stamp in [
        "2026-01-01T00:00:00+24:00",
        "2026-01-01T00:00:00",
        "not-a-time",
    ] {
        assert!(model::parse_time(stamp).is_err(), "{stamp}");
    }
    assert!(model::parse_time("2026-01-01T00:00:59.9999999999Z").is_ok());
    assert!(model::parse_time("2026-01-01 00:00:59").is_ok());
}
#[test]
fn duplicate_row_arrays_keep_hidden_backing_slots_and_null_empty_reset() {
    let f = Fixture::new("wago");
    for reset in ["", ",\"data\":null", ",\"data\":[]"] {
        // Without a reset, omitted fields must come from the original backing slots.
        // A reset starts zeroed rows, so supply the parser's required identity fields.
        let final_rows = if reset.is_empty() {
            r#"[{"record_id":12},{"record_id":22}]"#
        } else {
            r#"[{"record_id":12,"table_name":"Fresh","build":"68974"},{"record_id":22,"table_name":"Fresh","build":"68974"}]"#
        };
        let raw = format!(
            r#"{{"component":"Hotfixes","props":{{"hotfixes":{{"current_page":1,"last_page":1,"per_page":25,"total":2,"data":[{{"id":1,"record_id":10,"table_name":"First","build":"68974","locale":"enUS"}},{{"id":2,"record_id":20,"table_name":"Second","build":"68974","locale":"enUS"}}],"data":[{{"record_id":11}}]{reset},"data":{final_rows}}}}}}}"#
        );
        let b = budget(&f.ctx, &f.request);
        let parsed = parse::parse(&html(&raw), &b, &f.ctx.control).unwrap();
        assert_eq!(parsed.records.len(), 2);
        assert_eq!(parsed.records[1].record_id, 22);
        assert_eq!(parsed.records[1].id, if reset.is_empty() { 2 } else { 0 });
        assert_eq!(
            parsed.records[1].table_name,
            if reset.is_empty() { "Second" } else { "Fresh" }
        );
        assert_eq!(
            parsed.records[1].locale,
            if reset.is_empty() { "enUS" } else { "" }
        );
        assert_eq!(parsed.records[1].build, "68974");
    }
}
#[test]
fn pathological_json_depth_is_rejected_for_payload_and_cursor() {
    use base64::Engine as _;
    let f = Fixture::new("wago");
    // Current bounded JSON admission rejects pathological nesting in raw payload
    // members and cursor envelopes instead of scanning them unboundedly.
    let nested = format!("{}0{}", "[".repeat(10_000), "]".repeat(10_000));
    for contents in [
        format!(
            r#""data":[{{"id":1,"table_name":"SpellEffect","build":"68974","data":{nested}}}]"#
        ),
        format!(r#""data":[],"ignored":{nested}"#),
    ] {
        let raw = format!(
            r#"{{"component":"Hotfixes","props":{{"hotfixes":{{"current_page":1,"last_page":1,"per_page":25,"total":1,{contents}}}}}}}"#
        );
        let b = budget(&f.ctx, &f.request);
        assert!(parse::parse(&html(&raw), &b, &f.ctx.control).is_err());
    }
    let raw = format!(
        r#"{{"schema":"{CURSOR_SCHEMA}","source":"wago","ref":"{}","scope":"{}","ignored":{nested}}}"#,
        "a".repeat(64),
        q().scope_digest().unwrap(),
    );
    let encoded = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(raw);
    assert!(Cursor::decode(&encoded, &q(), "wago").is_err());
}
#[test]
fn zero_id_limit_cursor_uses_first_matching_key_in_original_window() {
    for latest in [false, true] {
        let f = Fixture::new("wago");
        let same = row(0, 10, 1);
        let pages = Pages::new(vec![
            (1, page(1, 2, 3, vec![same.clone()])),
            (2, page(2, 2, 3, vec![same, row(3, 10, 1)])),
        ]);
        let query = Query {
            latest,
            limit: 2,
            ..q()
        };
        let result = run_wago(&f, query.clone(), &pages).unwrap();
        assert_eq!(result.records.len(), 2);
        let cursor = Cursor::decode(&result.next_cursor, &query, "wago").unwrap();
        assert_eq!(cursor.resume_page, 1);
        assert_eq!(cursor.current.unwrap().page, 1);
        assert!(cursor.previous.is_none());
    }
}
#[test]
fn raidbots_source_bytes_do_not_consume_metadata_allowance() {
    let mut f = Fixture::new("raidbots");
    let path = f.cache(&[(1, 1, 1, b"payload")]);
    let b = Budget::controlled(
        Cost {
            metadata_bytes: 0,
            ..Cost::defaults()
        },
        f.ctx.control.deadline(),
        f.ctx.control.clone(),
    );
    assert_eq!(
        super::super::local::read_source_file_controlled(
            &path,
            512 << 20,
            &b,
            &f.ctx.control,
            |_| Ok(())
        )
        .unwrap(),
        fs::read(path).unwrap()
    );
    assert_eq!(b.snapshot().unwrap().metadata_bytes, 0);
}
#[test]
fn empty_optional_fields_are_omitted_in_public_and_capture_bytes() {
    let f = Fixture::new("wago");
    let r = Record {
        fields: Some(BTreeMap::new()),
        ..Default::default()
    };
    assert!(
        !serde_json::to_value(&r)
            .unwrap()
            .as_object()
            .unwrap()
            .contains_key("fields")
    );
    assert!(
        !String::from_utf8(wire_bytes(&r, &budget(&f.ctx, &f.request), 1 << 20).unwrap())
            .unwrap()
            .contains("\"fields\"")
    );
}

#[test]
fn provider_zero_byte_budget_retains_256_mib_coverage_default() {
    let f = Fixture::new("wago");
    let pages = Pages::new(vec![(1, page(1, 1, 1, vec![row(1, 1, 1)]))]);
    let result = run_wago(&f, q(), &pages).unwrap();
    let fetch = result.coverage.fetch.unwrap();
    assert_eq!(fetch.max_bytes, 256 << 20);
    assert_eq!(fetch.max_requests, 2048);
    assert_eq!(fetch.requests, 1);
    assert_eq!(fetch.bytes, pages.pages[&1].len() as i64);
}

#[test]
fn presentation_callback_runs_once_with_live_budget_and_retains_committed_capture() {
    let mut f = Fixture::new("wago");
    let pages = two();
    run_wago(&f, q(), &pages).unwrap();
    f.ctx.source.offline = true;
    let mut calls = 0;
    let mut committed = None;
    let error = with_result(&f.ctx, &f.request, |result, budget| {
        calls += 1;
        budget.check()?;
        committed = result.capture.clone();
        Err(ToolkitError::new(
            "fixture.presentation_failed",
            "closed output",
            5,
        ))
    })
    .unwrap_err();
    assert_eq!(calls, 1);
    assert_eq!(pages.calls.get(), 2);
    let committed = committed.unwrap();
    assert_eq!(error.details["completedCaptures"], json!([committed["id"]]));
    assert_eq!(
        evidence::read_capture(&f.ctx.home, committed["id"].as_str().unwrap()).unwrap(),
        committed
    );
    assert_eq!(
        error.details["primary"]["code"],
        "fixture.presentation_failed"
    );
}
