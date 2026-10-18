use super::*;
use lycheedev_core::{
    control::OperationControl,
    selection::{SelectionSpec, SourcePin},
    workspace,
};
use std::{
    fs,
    path::PathBuf,
    process::{Command, Stdio},
};

fn invocation(home: PathBuf) -> Invocation {
    Invocation {
        command: "source batch".into(),
        positional: None,
        flags: BTreeMap::new(),
        cwd: home.clone(),
        home,
        control: OperationControl::default(),
    }
}
#[test]
fn fixed_go_batch_json_admission_and_known_field_merging() {
    for raw in [
        r#"{"requests":[]}"#,
        r#"{"requests":[{}]}"#,
        r#"{"requests":[null]}"#,
        r#"{"requests":[{"query":{"text":"Foo"},"refs":{"symbol":"Foo"}}]}"#,
        r#"{"requests":[{"query":{"text":"Foo","unexpected":1}}]}"#,
        r#"{"requests":[{"refs":{"symbol":"Foo","unknown":null}}]}"#,
        r#"{"requests":[{"context":{"symbol":"Foo","unknown":1}}]}"#,
        r#"{"requests":[{"query":{"text":"Foo"}}]} {}"#,
        r#"{"requests":[{"query":{"text":"Foo","limit":1.0}}]}"#,
        r#"{"requests":[{"query":{"text":"Foo","limit":9223372036854775808}}]}"#,
        r#"{"requests":[{"id":7,"query":{"text":"Foo"}}]}"#,
        r#"{"requests":[{"query":{"text":"Foo"},"other":false}]}"#,
        r#"{"requests":[{"query":{"text":"Foo"}}],"other":false}"#,
    ] {
        let error = parse(raw.as_bytes()).unwrap_err();
        assert_eq!(error.code, "codebase.invalid_batch", "{raw}: {error}");
        assert_eq!(error.exit_code, 2);
    }
    assert_eq!(
        parse(&vec![b' '; FILE_BYTES + 1]).unwrap_err().code,
        "codebase.invalid_batch"
    );
    for count in [1, 32, 33] {
        let raw = json!({"requests":(0..count).map(|_|json!({"query":{"text":"Foo"}})).collect::<Vec<_>>()});
        assert_eq!(
            parse(&serde_json::to_vec(&raw).unwrap()).is_ok(),
            count <= 32
        );
    }
    let merged = parse(br#"{"REQUESTS":[{"ID":"kept","id":null,"Query":{"TEXT":"Foo","limit":1},"query":{"topic":"LUA","text":null},"refs":null}]}"#).unwrap();
    assert_eq!(merged.requests[0].id, "kept");
    let query = merged.requests[0].query.as_ref().unwrap();
    assert_eq!(query.text, "Foo");
    assert_eq!(query.limit, 1);
    assert_eq!(query.topic, "LUA");
    assert!(!merged.needs_research());
}
#[test]
fn go_json_reuses_request_elements_and_replaces_invalid_unicode() {
    let merged = parse(br#"{"requests":[{"id":"first","query":{"text":"Foo"}}],"requests":[{"query":{"limit":1}}]}"#).unwrap();
    assert_eq!(merged.requests[0].id, "first");
    assert_eq!(merged.requests[0].query.as_ref().unwrap().text, "Foo");
    assert_eq!(merged.requests[0].query.as_ref().unwrap().limit, 1);
    assert_eq!(parse(br#"{"requests":[{"query":{"text":"Foo"}}],"requests":[],"requests":[{"query":{"limit":1}}]}"#).unwrap_err().code, "codebase.invalid_query");
    let replaced = parse(br#"{"requests":[{"query":{"text":"\ud800x\udc00"}}]}"#).unwrap();
    assert_eq!(
        replaced.requests[0].query.as_ref().unwrap().text,
        "\u{fffd}x\u{fffd}"
    );
    let replaced = parse(b"{\"requests\":[{\"query\":{\"text\":\"\xff\xe2\x82\"}}]}").unwrap();
    assert_eq!(
        replaced.requests[0].query.as_ref().unwrap().text,
        "\u{fffd}\u{fffd}\u{fffd}"
    );
    let folded = parse("{\"requeſts\":[{\"query\":{\"text\":\"Foo\"}}]}".as_bytes()).unwrap();
    assert_eq!(folded.requests.len(), 1);
}

#[test]
fn every_item_is_validated_before_preparation_or_runtime_admission() {
    let home = unique_home("invalid");
    let mut inv = invocation(home.clone());
    inv.flags.insert("--snapshot".into(), "absent-pin".into());
    let request = BatchRequest {
        requests: vec![
            BatchItem {
                query: Some(SearchQuery {
                    text: "Good".into(),
                    ..Default::default()
                }),
                ..Default::default()
            },
            BatchItem {
                query: Some(SearchQuery {
                    text: "Bad".into(),
                    topic: "missing".into(),
                    ..Default::default()
                }),
                ..Default::default()
            },
        ],
    };
    let error = dispatch_request(&inv, request, SourceTools::default()).unwrap_err();
    assert_eq!(error.code, "codebase.invalid_query");
    assert_eq!(error.exit_code, 2);
    assert!(error.message.contains("request 2"));
    assert!(!home.exists());
}
#[test]
fn frozen_json_defaults_and_invalid_queries_are_distinct_from_cli_defaults() {
    let request = parse(br#"{"requests":[{"query":{"text":" Foo ","limit":-5}},{"refs":{"symbol":"Foo"}},{"context":{"symbol":"Foo"}}]}"#).unwrap();
    let inv = invocation(PathBuf::new());
    let calls = request
        .requests
        .iter()
        .map(|item| item_invocation(&inv, item).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(calls[0].flag("--limit"), Some("10"));
    assert_eq!(calls[0].argument().unwrap(), "Foo");
    assert_eq!(calls[1].flag("--limit"), Some("25"));
    assert_eq!(calls[2].flag("--limit"), Some("25"));
    assert_eq!(calls[2].flag("--max-lines"), Some("80"));
    assert_eq!(calls[2].flag("--max-bytes"), Some("16384"));
    assert_eq!(calls[2].flag("--depth"), Some("0"));
    for (body, code) in [
        (json!({"query":{"text":""}}), "codebase.invalid_query"),
        (
            json!({"query":{"text":"Foo","mode":"bad"}}),
            "codebase.invalid_query",
        ),
        (
            json!({"query":{"text":"Foo","limit":201}}),
            "codebase.invalid_query",
        ),
        (
            json!({"refs":{"symbol":"Foo","symbolId":"id"}}),
            "codebase.invalid_symbol",
        ),
        (
            json!({"refs":{"symbol":"Foo","limit":-1}}),
            "codebase.invalid_query",
        ),
        (
            json!({"refs":{"symbol":"Foo","direction":"bad"}}),
            "codebase.invalid_query",
        ),
        (
            json!({"context":{"symbol":"Foo","depth":5}}),
            "codebase.invalid_query",
        ),
        (
            json!({"context":{"symbol":"Foo","maxBytes":-1}}),
            "codebase.invalid_query",
        ),
        (
            json!({"context":{"symbol":"Foo","maxLines":2001}}),
            "codebase.invalid_query",
        ),
        (
            json!({"query":{"text":"Foo","cursor":"invalid"}}),
            "codebase.invalid_search_cursor",
        ),
        (
            json!({"context":{"symbol":"Foo","cursor":"0000000000000000:!:0:0:0"}}),
            "codebase.invalid_search_cursor",
        ),
    ] {
        assert_eq!(
            parse(&serde_json::to_vec(&json!({"requests":[body]})).unwrap())
                .unwrap_err()
                .code,
            code
        );
    }
}
#[test]
fn static_options_and_expired_controls_fail_before_preparation() {
    let request = parse(br#"{"requests":[{"query":{"text":"Foo"}}]}"#).unwrap();
    let home = unique_home("stopped");
    let mut inv = invocation(home.clone());
    inv.flags.insert("--static-only".into(), "true".into());
    inv.flags.insert("--session-reuse".into(), "true".into());
    assert_eq!(
        dispatch_request(&inv, request.clone(), SourceTools::default())
            .unwrap_err()
            .code,
        "command.invalid_arguments"
    );
    inv.flags.clear();
    inv.control = OperationControl::uncancelable(Instant::now());
    assert_eq!(
        dispatch_request(&inv, request, SourceTools::default())
            .unwrap_err()
            .code,
        "command.deadline"
    );
    assert!(!home.exists());
}
#[test]
fn batch_deadline_retains_live_caller_cancellation_for_owned_processes() {
    let parent = OperationControl::default();
    let token = parent.cancel_handle();
    let batch = parent.child(
        Instant::now() + Duration::from_secs(90),
        parent.cancel_handle(),
    );
    assert!(batch.deadline() <= parent.deadline());
    // The process owner polls the token while its caller is blocked, so this
    // must be visible without calling batch.check() to copy a parent signal.
    let process_token = batch.token();
    token.store(true, std::sync::atomic::Ordering::Release);
    assert!(process_token.load(std::sync::atomic::Ordering::Acquire));
    assert_eq!(batch.check().unwrap_err().code, "command.cancelled");
}

fn unique_home(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "lycheedev-batch-{name}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ))
}
struct Fixture {
    inv: Invocation,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        if std::env::var_os("LYCHEEDEV_RESEARCH_KEEP_FIXTURE").is_none() {
            let _ = fs::remove_dir_all(&self.inv.home);
        }
    }
}
fn fixture() -> Fixture {
    let home = unique_home("fixed-source");
    workspace::initialize(&home).unwrap();
    let mirror = crate::git::mirror(&home, "elvui");
    fs::create_dir_all(mirror.parent().unwrap()).unwrap();
    assert!(
        Command::new("git")
            .args(["init", "--bare"])
            .arg(&mirror)
            .output()
            .unwrap()
            .status
            .success()
    );
    let run = |input: &str, args: &[&str]| {
        let mut child = Command::new("git")
            .arg(format!("--git-dir={}", mirror.display()))
            .args(args)
            .env("GIT_AUTHOR_NAME", "Batch Fixture")
            .env("GIT_AUTHOR_EMAIL", "batch@example.invalid")
            .env("GIT_COMMITTER_NAME", "Batch Fixture")
            .env("GIT_COMMITTER_EMAIL", "batch@example.invalid")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(input.as_bytes())
            .unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap().trim().to_owned()
    };
    let mut tree = String::new();
    for (path, text) in [
        (
            "a.lua",
            "function Caller()\n Target()\n Target()\nend\nfunction Target() end\nfunction CallerTwo()\n Caller()\nend\n",
        ),
        ("Main.toc", "## Interface: 120100\na.lua\n"),
        (
            "unicode.lua",
            "function Unicode()\n return '汉字🙂abcdef'\nend\n",
        ),
    ] {
        let blob = run(text, &["hash-object", "-w", "--stdin"]);
        tree.push_str(&format!("100644 blob {blob}\t{path}\n"));
    }
    let tree = run(&tree, &["mktree"]);
    let commit = run("batch fixture", &["commit-tree", &tree]);
    let pin = selection::pin_selection(
        &home,
        SelectionSpec {
            source: Some(SourcePin {
                repository: "elvui".into(),
                product: "main".into(),
                requested_ref: "refs/heads/main".into(),
                exact_commit: commit,
                parser_revision: "source-v1".into(),
            }),
            ..Default::default()
        },
    )
    .unwrap();
    let mut inv = invocation(home);
    inv.flags.insert("--snapshot".into(), pin.id);
    inv.flags.insert("--static-only".into(), "true".into());
    Fixture { inv }
}
#[test]
fn one_capture_shared_snapshot_and_independent_pages_match_single_evaluators() {
    let f = fixture();
    let request=parse(br#"{"requests":[{"id":"find","query":{"text":"Caller","limit":1}},{"id":"uses","refs":{"symbol":"Target","limit":1}},{"id":"read","context":{"symbol":"Caller","limit":1,"maxBytes":12,"depth":1}}]}"#).unwrap();
    let result = dispatch_request(&f.inv, request.clone(), SourceTools::default()).unwrap();
    assert_eq!(result.provenance.len(), 1);
    assert_eq!(
        lycheedev_core::metadata::Metadata::open(&f.inv.home, false)
            .unwrap()
            .list::<Value>("capture/", 100)
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        result.selection,
        json!({"snapshot":f.inv.flag("--snapshot").unwrap()})
    );
    assert_eq!(result.data["snapshot"], result.selection["snapshot"]);
    assert_eq!(result.data["results"].as_array().unwrap().len(), 3);
    assert_eq!(result.data["complete"], false);
    assert_eq!(result.data["truncated"], true);
    let pin = selection::resolve_pinned(&f.inv).unwrap();
    let source = pin.source.as_ref().unwrap();
    let index = index::build_with_control(&f.inv.home, source, &f.inv.control).unwrap();
    let query = index
        .search(source, &pin.id, "Caller", "", "", 1, "")
        .unwrap();
    // Timings describe preparation, not different search answers.
    let mut batch_query = result.data["results"][0]["query"].clone();
    let mut query = query;
    batch_query["coverage"]
        .as_object_mut()
        .unwrap()
        .remove("io");
    query["coverage"].as_object_mut().unwrap().remove("io");
    assert_eq!(batch_query, query);
    let refs =
        crate::refs::dispatch(&item_invocation(&f.inv, &request.requests[1]).unwrap()).unwrap();
    assert_eq!(result.data["results"][1]["refs"], refs.data);
    let context =
        crate::context::dispatch(&item_invocation(&f.inv, &request.requests[2]).unwrap()).unwrap();
    assert_eq!(result.data["results"][2]["context"], context.data);
    let mut continued = request.clone();
    continued.requests[1].refs.as_mut().unwrap().cursor =
        result.data["results"][1]["refs"]["nextCursor"]
            .as_str()
            .unwrap()
            .into();
    continued.requests[2].context.as_mut().unwrap().cursor =
        result.data["results"][2]["context"]["nextCursor"]
            .as_str()
            .unwrap()
            .into();
    let next = dispatch_request(&f.inv, continued, SourceTools::default()).unwrap();
    assert_eq!(next.provenance.len(), 1);
    assert_ne!(
        next.data["results"][1]["refs"]["relations"],
        result.data["results"][1]["refs"]["relations"]
    );
    assert_ne!(
        next.data["results"][2]["context"]["snippets"],
        result.data["results"][2]["context"]["snippets"]
    );
}
#[test]
fn query_only_needs_no_runtime_and_failed_later_item_never_publishes_a_batch() {
    let f = fixture();
    let mut inv = f.inv.clone();
    inv.flags.remove("--static-only");
    inv.flags
        .insert("--release".into(), "does-not-exist".into());
    let request = parse(br#"{"requests":[{"query":{"text":"Caller"}}]}"#).unwrap();
    assert!(dispatch_request(&inv, request, SourceTools::default()).is_ok());
    let captures = || {
        lycheedev_core::metadata::Metadata::open(&inv.home, false)
            .unwrap()
            .list::<Value>("capture/", 100)
            .unwrap()
            .len()
    };
    let before = captures();
    let request =
        parse(br#"{"requests":[{"query":{"text":"Caller"}},{"refs":{"symbol":"Target"}}]}"#)
            .unwrap();
    assert_eq!(
        dispatch_request(&inv, request, SourceTools::default())
            .unwrap_err()
            .code,
        "source.native_unavailable"
    );
    let request =
        parse(br#"{"requests":[{"query":{"text":"Caller"}},{"refs":{"symbolId":"absent"}}]}"#)
            .unwrap();
    let error = dispatch_request(&f.inv, request, SourceTools::default()).unwrap_err();
    assert_eq!(error.code, "codebase.invalid_symbol");
    assert!(error.message.contains("request 2"));
    assert_eq!(
        captures(),
        before,
        "failed item published a partial batch capture"
    );
}
#[test]
fn request_scoped_native_batch_uses_one_prepared_session_and_preserves_typed_error() {
    let fixture = || native_research::SnapshotRequest {
        files: BTreeMap::from([(
            "a.lua".into(),
            "function Caller() Target() end\nfunction Target() end\n".into(),
        )]),
        load_documents: BTreeMap::new(),
        environment_documents: BTreeMap::new(),
        source: json!({"repository":"batch-test","product":"retail"}),
        luac: crate::native::LuacIdentity {
            version: "5.1.5".into(),
            platform: "windows-amd64".into(),
            executable_sha256: "a".repeat(64),
            executable_bytes: 1,
        },
        api: vec![],
        environment_manifest: None,
        load_plans: vec![],
        input_gaps: vec![],
    };
    let query = || native_research::QueryRequest {
        symbol: index::Symbol {
            id: String::new(),
            kind: "declaration".into(),
            name: "Caller".into(),
            target: String::new(),
            category: "function".into(),
            confidence: String::new(),
            path: "a.lua".into(),
            line: 1,
            end_line: 1,
            signature: String::new(),
            scope: String::new(),
            ordinal: 0,
        },
        direction: "outgoing".into(),
    };
    let mut phases = Vec::new();
    let result = run_native_observed(
        fixture(),
        vec![
            NativeQuery {
                item: 1,
                query: query(),
            },
            NativeQuery {
                item: 4,
                query: query(),
            },
        ],
        &OperationControl::default(),
        |phase| phases.push(phase),
    )
    .unwrap_or_else(|failure| panic!("{}", failure.error));
    assert_eq!(
        phases
            .iter()
            .filter(|phase| **phase == "emmy_indexed")
            .count(),
        1
    );
    assert_eq!(
        phases
            .iter()
            .filter(|phase| **phase == "query_inputs_renewed")
            .count(),
        2
    );
    assert_eq!(
        result.iter().map(|(item, _)| *item).collect::<Vec<_>>(),
        [1, 4]
    );
    assert_eq!(result[0].1["inputIdentity"], result[1].1["inputIdentity"]);
    assert_eq!(result[0].1["relations"], result[1].1["relations"]);
    let stopped = OperationControl::uncancelable(Instant::now());
    let failure = run_native(
        fixture(),
        vec![NativeQuery {
            item: 1,
            query: query(),
        }],
        &stopped,
    )
    .err()
    .unwrap();
    assert_eq!(failure.error.code, "command.deadline");
    assert_eq!(failure.item, None);
}

#[test]
fn implicit_unavailable_runtime_warns_and_keeps_static_and_flow_evidence_partial() {
    let f = fixture();
    let mut inv = f.inv.clone();
    inv.flags.remove("--static-only");
    inv.flags.insert("--session-reuse".into(), "true".into());
    let request = parse(
        br#"{"requests":[{"refs":{"symbol":"Target"}},{"context":{"symbol":"Caller","depth":1}}]}"#,
    )
    .unwrap();
    let unavailable = || {
        ToolkitError::new(
            "delivery.release_integrity",
            "default resource digest differs",
            4,
        )
    };
    let result = dispatch_unavailable(&inv, request.clone(), unavailable()).unwrap();
    assert_eq!(result.warnings.len(), 1);
    assert!(
        result.warnings[0]
            .as_str()
            .unwrap()
            .contains("default resource digest differs")
    );
    assert_eq!(result.provenance.len(), 1);
    assert!(!result.complete);
    assert_eq!(
        result.data["results"][0]["refs"]["coverage"]["semantic"],
        "unavailable"
    );
    assert!(
        result.data["results"][0]["refs"]
            .get("nativeResearch")
            .is_none()
    );
    inv.flags.insert("--flow".into(), "true".into());
    let flow = dispatch_unavailable(&inv, request.clone(), unavailable()).unwrap();
    assert_eq!(
        flow.data["results"][1]["context"]["flow"]["complete"],
        false
    );
    assert_eq!(
        flow.data["results"][1]["context"]["flow"]["gaps"][0]["code"],
        "flow.compiler_unavailable"
    );
    inv.flags
        .insert("--release".into(), "explicit-release".into());
    assert_eq!(
        dispatch_unavailable(&inv, request.clone(), unavailable())
            .unwrap_err()
            .code,
        "delivery.release_integrity"
    );
    inv.flags.remove("--release");
    let mut conflicting = inv.clone();
    conflicting
        .flags
        .insert("--static-only".into(), "true".into());
    assert_eq!(
        dispatch_unavailable(&conflicting, request.clone(), unavailable())
            .unwrap_err()
            .code,
        "command.invalid_arguments",
        "fallback must not remove a conflicting session-reuse flag before admission"
    );
    for code in [
        "command.cancelled",
        "command.deadline",
        "operation.cancelled",
        "process.cancelled",
    ] {
        let error = ToolkitError::new(
            code,
            "must remain terminal",
            if code == "command.deadline" { 5 } else { 7 },
        );
        assert_eq!(
            dispatch_unavailable(&inv, request.clone(), error)
                .unwrap_err()
                .code,
            code
        );
    }
}
#[test]
fn cancellation_inside_batch_literal_scan_has_no_capture_and_can_recover() {
    let f = fixture();
    // Warm index preparation is outside the observed query cancellation point.
    let pin = selection::resolve_pinned(&f.inv).unwrap();
    index::build_with_control(&f.inv.home, pin.source.as_ref().unwrap(), &f.inv.control).unwrap();
    let request =
        parse(br#"{"requests":[{"query":{"text":"absent-literal","mode":"exploratory"}}]}"#)
            .unwrap();
    let cancellation = f.inv.control.cancel_handle();
    let mut reached = false;
    let error = dispatch_observed(
        &f.inv,
        request.clone(),
        SourceTools::default(),
        None,
        &mut |phase, number| {
            if phase == "line" && number == 1 {
                reached = true;
                cancellation.store(true, std::sync::atomic::Ordering::Release);
            }
        },
    )
    .unwrap_err();
    assert!(reached, "query never reached literal scan");
    assert_eq!(error.code, "command.cancelled");
    assert!(
        lycheedev_core::metadata::Metadata::open(&f.inv.home, false)
            .unwrap()
            .list::<Value>("capture/", 100)
            .unwrap()
            .is_empty()
    );
    let mut recovered = f.inv.clone();
    recovered.control = OperationControl::default();
    assert_eq!(
        dispatch_request(&recovered, request, SourceTools::default())
            .unwrap()
            .provenance
            .len(),
        1
    );
}
#[test]
fn deadline_inside_query_scan_has_no_capture_and_leaves_index_reusable() {
    let mut f = fixture();
    let pin = selection::resolve_pinned(&f.inv).unwrap();
    let source = pin.source.as_ref().unwrap();
    let index = index::build_with_control(&f.inv.home, source, &f.inv.control).unwrap();
    let request =
        parse(br#"{"requests":[{"query":{"text":"absent-literal","mode":"exploratory"}}]}"#)
            .unwrap();
    // Warm preparation is outside the deadline; the actual batch dispatcher
    // must reach its query scan before the original one-second budget expires.
    f.inv.control = OperationControl::uncancelable(Instant::now() + Duration::from_secs(1));
    let deadline = f.inv.control.deadline();
    let mut reached = false;
    let error = dispatch_observed(
        &f.inv,
        request.clone(),
        SourceTools::default(),
        None,
        &mut |phase, number| {
            if phase == "line" && number == 1 {
                reached = true;
                std::thread::sleep(
                    deadline.saturating_duration_since(Instant::now()) + Duration::from_millis(1),
                );
            }
        },
    )
    .unwrap_err();
    assert!(reached, "batch query never reached literal scan");
    assert_eq!(error.code, "command.deadline");
    assert!(
        lycheedev_core::metadata::Metadata::open(&f.inv.home, false)
            .unwrap()
            .list::<Value>("capture/", 100)
            .unwrap()
            .is_empty()
    );
    let recovered = index
        .search_with_control(
            source,
            &pin.id,
            "Caller",
            "",
            "",
            10,
            "",
            &OperationControl::default(),
        )
        .unwrap();
    assert!(!recovered["results"].as_array().unwrap().is_empty());
    let mut recovered_inv = f.inv.clone();
    recovered_inv.control = OperationControl::default();
    assert_eq!(
        dispatch_request(&recovered_inv, request, SourceTools::default())
            .unwrap()
            .provenance
            .len(),
        1
    );
}
fn budget_queries(count: usize) -> Vec<NativeQuery> {
    (0..count)
        .map(|item| NativeQuery {
            item,
            query: native_research::QueryRequest {
                symbol: index::Symbol {
                    id: String::new(),
                    kind: "declaration".into(),
                    name: "Fixture".into(),
                    target: String::new(),
                    category: "function".into(),
                    confidence: String::new(),
                    path: "a.lua".into(),
                    line: 1,
                    end_line: 1,
                    signature: String::new(),
                    scope: String::new(),
                    ordinal: 0,
                },
                direction: "both".into(),
            },
        })
        .collect()
}
#[test]
fn aggregate_admission_stops_after_oversized_item_and_releases_request_owner() {
    struct Owner(Arc<std::sync::atomic::AtomicBool>);
    impl Drop for Owner {
        fn drop(&mut self) {
            self.0.store(true, std::sync::atomic::Ordering::Release);
        }
    }
    for mode in ["request_scoped_batch", "session_reuse"] {
        let control = OperationControl::default();
        let value = json!({"schema":"lycheedev.source-native-research.v1","payload":"\"".repeat(128),"mode":mode});
        let empty = NativeReply {
            schema: WORKER_SCHEMA.into(),
            result: Ok(Vec::new()),
        };
        let baseline = admit_json(&empty, &control, usize::MAX, false).unwrap().0;
        let item_bytes = admit_json(&(0usize, &value), &control, usize::MAX, false)
            .unwrap()
            .0;
        let limit = baseline + item_bytes * 2 + 1;
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let calls_inner = calls.clone();
        let retired = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let owner = Owner(retired.clone());
        let error = collect_native_results(budget_queries(5), &control, limit, move |_| {
            let _owner = &owner;
            calls_inner.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            Ok(value.clone())
        })
        .err()
        .unwrap();
        assert_eq!(error.item, Some(2));
        assert_eq!(error.error.code, "source.native_budget");
        assert_eq!(
            calls.load(std::sync::atomic::Ordering::Relaxed),
            3,
            "evaluated items after aggregate exhaustion"
        );
        assert!(
            retired.load(std::sync::atomic::Ordering::Acquire),
            "request-scoped owner was retained after failure"
        );
    }
}
#[test]
fn aggregate_measurement_matches_exact_wire_and_keeps_control_errors() {
    let control = OperationControl::default();
    let values = collect_native_results(budget_queries(2), &control, RESULT_BYTES, |query| {
        Ok(json!({"id":query.item,"unicode":"汉字🙂","escaped":"\"\n"}))
    })
    .unwrap_or_else(|failure| panic!("{}", failure.error));
    let reply = NativeReply {
        schema: WORKER_SCHEMA.into(),
        result: Ok(values),
    };
    let measured = admit_json(&reply, &control, RESULT_BYTES, false).unwrap().0;
    let (_, bytes) = admit_json(&reply, &control, measured, true).unwrap();
    assert_eq!(bytes.len(), measured);
    assert_eq!(bytes, serde_json::to_vec(&reply).unwrap());
    assert_eq!(
        admit_json(&reply, &control, measured - 1, false)
            .unwrap_err()
            .code,
        "source.native_budget"
    );
    let token = control.cancel_handle();
    token.store(true, std::sync::atomic::Ordering::Release);
    assert_eq!(
        admit_json(&reply, &control, 0, true).unwrap_err().code,
        "command.cancelled"
    );
    let deadline = OperationControl::uncancelable(Instant::now());
    assert_eq!(
        admit_json(&reply, &deadline, 0, true).unwrap_err().code,
        "command.deadline"
    );
}


#[test]
fn cold_context_multiple_sites_borrow_each_original_once() {
    let f = fixture();
    let request = parse(br#"{"requests":[{"context":{"symbol":"Target","depth":1,"limit":25,"maxLines":120,"maxBytes":16384}}]}"#).unwrap();
    crate::index::take_read_observations();
    let result = dispatch_request(&f.inv, request, SourceTools::default()).unwrap();
    let reads = crate::index::take_read_observations();
    let snippets = result.data["results"][0]["context"]["snippets"].as_array().unwrap();
    assert!(snippets.len() > 1, "fixture must exercise distinct same-file sites");
    assert!(snippets.iter().all(|snippet| snippet["path"] == "a.lua"));
    assert_eq!(reads, vec![("a.lua".to_owned(), true)], "cold bytes stay borrowed and per-path memoized");
    for text in ["## Interface: 120100\na.lua\n", "function Unicode()\n return '汉字🙂abcdef'\nend\n"] {
        assert!(!f.inv.home.join("cache/source-originals-rust").join(lycheedev_core::storage::sha256(text.as_bytes())).exists());
    }
}
