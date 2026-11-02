use lycheedev_core::{
    evidence,
    selection::{self, SelectionSpec, SourcePin},
    workspace,
};
use lycheedev_source::request::{
    ContextRequest, RelationDirection, RelationRequest, ResearchMode, ResearchSelection,
    SourceContext, SymbolSelector,
};
use lycheedev_source::{context, git, refs};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    fs,
    io::Write,
    path::PathBuf,
    process::{Command, Stdio},
};
struct Fixture {
    home: PathBuf,
    pin: String,
}
const RESEARCH_A: &str = "function Caller()\n  Target()\n  Target()\nend\nfunction Target() end\n-- Target() fake comment\nlocal label='Target() fake string'\n";
impl Drop for Fixture {
    fn drop(&mut self) {
        if std::env::var_os("LYCHEEDEV_RESEARCH_KEEP_FIXTURE").is_some() {
            return;
        }
        let _ = fs::remove_dir_all(&self.home);
    }
}
fn fixture() -> Fixture {
    fixture_with(&[])
}
fn fixture_with(extra: &[(&str, &str)]) -> Fixture {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let home = std::env::temp_dir().join(format!(
        "lycheedev-research-{}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    fs::create_dir(&home).expect("fresh research fixture");
    workspace::initialize(&home).unwrap();
    let mirror = git::mirror(&home, "elvui");
    fs::create_dir_all(mirror.parent().unwrap()).unwrap();
    assert!(
        Command::new("git")
            .args(["init", "--bare", mirror.to_str().unwrap()])
            .output()
            .unwrap()
            .status
            .success()
    );
    let run = |input: &str, args: &[&str]| {
        let mut child = Command::new("git")
            .arg(format!("--git-dir={}", mirror.display()))
            .args(args)
            .env("GIT_AUTHOR_NAME", "Research Fixture")
            .env("GIT_AUTHOR_EMAIL", "fixture@example.invalid")
            .env("GIT_COMMITTER_NAME", "Research Fixture")
            .env("GIT_COMMITTER_EMAIL", "fixture@example.invalid")
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
        let result = child.wait_with_output().unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        String::from_utf8(result.stdout).unwrap().trim().to_owned()
    };
    let files = [
        ("a.lua", RESEARCH_A),
        ("Main.toc", "## Interface: 120100\na.lua\n"),
        ("b.lua", "function Neighbor()\n  Caller()\nend\n"),
        (
            "unicode.lua",
            "function Unicode()\r\n  return '汉字abcdef🙂'\r\nend\r\n",
        ),
    ];
    let mut tree = String::new();
    // Explicit per-test originals may replace a default document. The Git pin
    // authenticates the final originals, including the selected TOC load order.
    let files = files
        .into_iter()
        .chain(extra.iter().copied())
        .collect::<BTreeMap<_, _>>();
    for (path, text) in files {
        let blob = run(text, &["hash-object", "-w", "--stdin"]);
        tree.push_str(&format!("100644 blob {blob}\t{path}\n"));
    }
    let tree = run(&tree, &["mktree"]);
    let commit = run("research", &["commit-tree", &tree]);
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
    Fixture { home, pin: pin.id }
}
fn native_fixture(extra: &[(&str, &str)], execute_caller: bool) -> Fixture {
    let source = format!(
        "{RESEARCH_A}{}",
        if execute_caller { "Caller()\n" } else { "" }
    );
    let mut manifest = String::from("## Interface: 120100\na.lua\nb.lua\nunicode.lua\n");
    for (path, _) in extra {
        if path.ends_with(".lua") {
            manifest.push_str(&format!("{path}\n"));
        }
    }
    let mut originals = extra.to_vec();
    originals.extend([("a.lua", source.as_str()), ("Main.toc", manifest.as_str())]);
    fixture_with(&originals)
}
struct NativeCli {
    executable: PathBuf,
    release: PathBuf,
}
impl NativeCli {
    fn supplied() -> Self {
        Self {
            executable: std::env::var_os("LYCHEEDEV_PROJECT_SELECTION_EXE")
                .expect("sealed current CLI")
                .into(),
            release: std::env::var_os("LYCHEEDEV_PROJECT_SELECTION_RELEASE")
                .expect("matching current release")
                .into(),
        }
    }
    fn refs(&self, f: &Fixture, flags: &[(&str, &str)]) -> Value {
        let options = cli_options(f, flags);
        let envelope = project_selection::cli_envelope(&self.executable, &self.release, f, &f.home, "source refs", &options);
        assert_eq!(envelope["result"]["snapshot"], f.pin);
        assert_eq!(envelope["context"]["snapshot"], f.pin);
        let captures = envelope["captures"].as_array().unwrap();
        assert_eq!(captures.len(), 1);
        for capture in captures {
            assert_eq!(
                evidence::read_capture(&f.home, capture["id"].as_str().unwrap()).unwrap(),
                *capture
            );
            let bytes = evidence::read_blob(&f.home, &capture["blob"], 16 << 20).unwrap();
            assert_eq!(
                serde_json::from_slice::<Value>(&bytes).unwrap(),
                envelope["result"]
            );
            assert_eq!(capture["complete"], envelope["result"]["complete"]);
        }
        envelope["result"].clone()
    }
}
fn assert_native_reading(data: &Value) {
    let native = &data["nativeResearch"];
    assert_eq!(data["semanticRuntime"]["engine"], "native-common-source");
    assert_eq!(native["schema"], "lycheedev.source-native-research.v1");
    assert_eq!(native["lifecycle"]["retired"], true);
    assert_eq!(native["entryEvidence"]["sourceExecution"], false);
    assert_eq!(
        native["semanticPlan"]["plan"]["source_identity"],
        native["inputIdentity"]
    );
    assert_eq!(native["inputIdentity"].as_str().unwrap().len(), 64);
    assert_eq!(data["coverage"]["environment"], "missing-client-pin");
    assert_eq!(data["complete"], false);
}
fn assert_executed_operations(data: &Value, count: usize, batches: &[usize]) {
    assert_native_reading(data);
    let native = &data["nativeResearch"];
    let plan = &native["semanticPlan"]["plan"];
    let operations = plan["operations"].as_array().unwrap();
    assert_eq!(operations.len(), count);
    assert_eq!(plan["batch_sizes"], serde_json::json!(batches));
    assert_eq!(native["semanticPlan"]["state"], "executed");
    let execution = &native["semanticExecution"];
    assert_eq!(
        execution["schema"],
        "lycheedev.native-semantic-execution.v1"
    );
    let receipt = &execution["receipt"];
    assert_eq!(receipt["planned_batches"], batches.len());
    assert_eq!(receipt["completed_batches"], batches.len());
    assert_eq!(receipt["unavailable_scopes"], 0);
    assert_eq!(receipt["unexecuted_operations"], 0);
    assert_eq!(receipt["plan_identity"].as_str().unwrap().len(), 64);
    let scopes = execution["answers"].as_array().unwrap();
    assert!(!scopes.is_empty());
    assert_eq!(receipt["scopes"], scopes.len());
    for scope in scopes {
        let answers = scope["operations"].as_array().unwrap();
        assert_eq!(answers.len(), count);
        for (index, (answer, operation)) in answers.iter().zip(operations).enumerate() {
            assert_eq!(answer["executed"], true);
            assert_eq!(answer["operation"], index);
            assert_eq!(answer["batch"], index / 128);
            assert_eq!(answer["kind"], operation["kind"]);
            assert_eq!(answer["operationIdentity"].as_str().unwrap().len(), 64);
        }
    }
}
#[test]
#[ignore = "requires sealed current CLI and matching Release; 130 executed Native operations across two completed batches"]
fn actual_incoming_hover_plus_128_outgoing_calls_share_one_initialized_workspace() {
    let cli = NativeCli::supplied();
    let body = format!("function Bulk()\n{}end\n", "  Target()\n".repeat(128));
    let f = native_fixture(&[("bulk.lua", &body)], false);
    let flags = [
        ("--symbol", "Bulk"),
        ("--direction", "both"),
        ("--limit", "200"),
    ];
    let reading = cli.refs(&f, &flags);
    assert_executed_operations(&reading, 130, &[128, 2]);
    // Both/outgoing research explicitly enters the selected declaration after
    // loading its TOC. This is authenticated hypothetical input, not a source
    // Bulk() call or evidence that an unselected Caller ever ran.
    let native = &reading["nativeResearch"];
    assert_eq!(
        native["entryEvidence"]["kind"],
        "hypothetical_selected_declaration_entry"
    );
    assert_eq!(native["entryResolution"]["state"], "unique");
    assert_eq!(
        native["entryResolution"]["candidate"]["closure"]["file"],
        "bulk.lua"
    );
    let entries = native["entryEvidence"]["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(
        entries[0]["request"]["candidate"],
        native["entryResolution"]["candidate"]["id"]
    );
    assert!(entries[0]["frame"].is_number());
    assert_eq!(entries[0]["effects_complete"], true);
    assert_eq!(reading["coverage"]["semantic"], "complete", "{reading}");
    assert_eq!(
        reading["nativeResearch"]["semanticExecution"]["receipt"]["uncertain_answers"],
        0
    );
    let operations = reading["nativeResearch"]["semanticPlan"]["plan"]["operations"]
        .as_array()
        .unwrap();
    assert_eq!(operations[0]["kind"], "references");
    assert_eq!(operations[1]["kind"], "type");
    for (index, operation) in operations[2..].iter().enumerate() {
        assert_eq!(operation["kind"], "definition");
        assert_eq!(
            operation["target"]["file_sha256"],
            lycheedev_core::storage::sha256(body.as_bytes())
        );
        assert_eq!(operation["target"]["site"]["file"], "bulk.lua");
        assert_eq!(
            operation["target"]["position"]["coveringRange"]["start"]["line"],
            index + 1
        );
        let site = &operation["target"]["site"];
        assert_eq!(
            &body[site["start"].as_u64().unwrap() as usize..site["end"].as_u64().unwrap() as usize],
            "Target"
        );
    }
    // Count actual definition answers, not merely planned work. Both batches
    // are completed within each authenticated prepared Native source scope.
    for scope in reading["nativeResearch"]["semanticExecution"]["answers"]
        .as_array()
        .unwrap()
    {
        for answer in &scope["operations"].as_array().unwrap()[2..] {
            assert_eq!(answer["state"], "known");
            assert_target_definition(&answer["result"]["selected"]);
        }
    }
    let cursor = reading["nextCursor"]
        .as_str()
        .expect("128 candidates and 128 bindings require continuation");
    let next = cli.refs(
        &f,
        &[
            ("--symbol", "Bulk"),
            ("--direction", "both"),
            ("--limit", "200"),
            ("--cursor", cursor),
        ],
    );
    assert_executed_operations(&next, 130, &[128, 2]);
    assert_eq!(
        next["nativeResearch"]["inputIdentity"],
        reading["nativeResearch"]["inputIdentity"]
    );
    assert_eq!(
        next["nativeResearch"]["semanticPlan"]["plan"],
        reading["nativeResearch"]["semanticPlan"]["plan"]
    );
    let rows = reading["relations"]
        .as_array()
        .unwrap()
        .iter()
        .chain(next["relations"].as_array().unwrap());
    let mut sites = std::collections::BTreeSet::new();
    let mut candidates = 0;
    for row in rows.filter(|row| row["kind"] == "call" && row["source"] == "Bulk") {
        if native_relation(row) {
            assert_eq!(row["state"], "resolved");
            assert_eq!(row["target"], "Target");
            assert_eq!(row["path"], "bulk.lua");
            assert!(sites.insert(row["line"].as_u64().unwrap()));
            assert_observed_target(row);
        } else {
            assert_eq!(row["state"], "static_candidate");
            candidates += 1;
        }
    }
    assert_eq!(
        sites,
        (2..=129).collect::<std::collections::BTreeSet<u64>>()
    );
    assert_eq!(candidates, 128);
    assert!(next["nextCursor"].is_null());
}
fn native_relation(row: &Value) -> bool {
    row["basis"].as_array().is_some_and(|basis| {
        basis
            .iter()
            .any(|value| value == "native_common_source_facts")
    })
}
fn assert_target_definition(selected: &Value) {
    assert_eq!(selected["state"], "known");
    let value = &selected["value"];
    assert!(value["value_id"].is_string() && value["proof"].is_string());
    let declaration = &value["function_declaration"];
    assert_eq!(declaration["file"], "a.lua");
    let start = declaration["start"].as_u64().unwrap() as usize;
    let end = declaration["end"].as_u64().unwrap() as usize;
    assert!(RESEARCH_A.get(start..end).unwrap().contains("Target"));
    assert_eq!(
        RESEARCH_A[..start]
            .bytes()
            .filter(|byte| *byte == b'\n')
            .count()
            + 1,
        5
    );
}
fn assert_observed_target(row: &Value) {
    assert!(row["proof"].is_string());
    let observations = row["observations"].as_array().unwrap();
    assert!(!observations.is_empty());
    for observation in observations {
        assert!(observation["proof"].is_string());
        assert_target_definition(&observation["selected"]);
    }
}
fn source_context(f: &Fixture) -> SourceContext {
    SourceContext {
        home: f.home.clone(),
        cwd: f.home.clone(),
        control: Default::default(),
    }
}
fn cli_options(f: &Fixture, flags: &[(&str, &str)]) -> BTreeMap<String, String> {
    let mut options: BTreeMap<_, _> = flags
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    options.insert("--snapshot".into(), f.pin.clone());
    options
}
fn research_selection(f: &Fixture) -> ResearchSelection {
    ResearchSelection {
        pin: selection::load_pinned(&f.home, &f.pin).unwrap(),
        environment: None,
    }
}
fn relation(
    selector: &str,
    direction: RelationDirection,
    limit: usize,
    cursor: &str,
) -> RelationRequest {
    RelationRequest {
        selector: SymbolSelector::Name(selector.into()),
        direction,
        limit,
        cursor: cursor.into(),
    }
}
fn context_request(
    selector: &str,
    limit: usize,
    max_bytes: usize,
    max_lines: usize,
    depth: usize,
    cursor: &str,
    flow: bool,
) -> ContextRequest {
    ContextRequest {
        selector: SymbolSelector::Name(selector.into()),
        limit,
        max_bytes,
        max_lines,
        depth,
        cursor: cursor.into(),
        flow,
    }
}
#[test]
fn structural_refs_cursor_and_original_context_are_bounded_without_semantic_claims() {
    let f = fixture();
    let ctx = source_context(&f);
    let selected = research_selection(&f);
    let first = refs::dispatch(
        &ctx,
        &selected,
        &relation("Target", RelationDirection::Incoming, 1, ""),
        ResearchMode::Static,
    )
    .unwrap();
    assert_eq!(first.data["relations"].as_array().unwrap().len(), 1);
    assert_eq!(first.data["relations"][0]["state"], "static_candidate");
    assert_eq!(first.data["relations"][0]["target"], "Target");
    assert!(first.data["relations"][0].get("targetId").is_none());
    assert!(!first.complete);
    let cursor = first.data["nextCursor"].as_str().unwrap();
    let next = refs::dispatch(
        &ctx,
        &selected,
        &relation("Target", RelationDirection::Incoming, 1, cursor),
        ResearchMode::Static,
    )
    .unwrap();
    assert_ne!(
        next.data["relations"][0]["line"],
        first.data["relations"][0]["line"]
    );
    assert_eq!(
        refs::dispatch(
            &ctx,
            &selected,
            &relation("Caller", RelationDirection::Incoming, 1, cursor),
            ResearchMode::Static,
        )
        .unwrap_err()
        .exit_code,
        2
    );
    let reading = context::dispatch(
        &ctx,
        &selected,
        &context_request("Target", 50, 16 << 10, 2, 1, "", false),
        ResearchMode::Static,
    )
    .unwrap();
    assert!(!reading.complete);
    assert!(!reading.data["snippets"].as_array().unwrap().is_empty());
    assert_eq!(reading.data["loadEvidence"][0]["path"], "Main.toc");
    assert_eq!(reading.data["loadEvidence"][0]["state"], "structural");
    let captured = evidence::read_blob(&f.home, &reading.provenance[0]["blob"], 1 << 20).unwrap();
    assert_eq!(
        serde_json::from_slice::<Value>(&captured).unwrap(),
        reading.data
    );
}
#[test]
fn context_flow_requires_a_compiler_and_static_depth_remains_available() {
    let f = fixture();
    let ctx = source_context(&f);
    let selected = research_selection(&f);
    assert_eq!(
        context::dispatch(
            &ctx,
            &selected,
            &context_request("Target", 50, 16 << 10, 120, 1, "", true),
            ResearchMode::Static,
        )
        .unwrap_err()
        .exit_code,
        3
    );
    let deep = context::dispatch(
        &ctx,
        &selected,
        &context_request("Target", 50, 16 << 10, 120, 2, "", false),
        ResearchMode::Static,
    )
    .unwrap();
    assert!(!deep.complete);
    assert!(
        deep.data["snippets"]
            .as_array()
            .unwrap()
            .iter()
            .any(|site| site["basis"] == "static-depth:2")
    );
}
#[test]
fn context_byte_continuation_progresses_and_query_changes_reject_the_cursor() {
    let f = fixture();
    let ctx = source_context(&f);
    let selected = research_selection(&f);
    let mut cursor = String::new();
    let mut texts = vec![];
    let mut previous = String::new();
    for _ in 0..32 {
        let reading = context::dispatch(
            &ctx,
            &selected,
            &context_request("Caller", 50, 7, 120, 0, &cursor, false),
            ResearchMode::Static,
        )
        .unwrap();
        let snippets = reading.data["snippets"].as_array().unwrap();
        let bytes = snippets
            .iter()
            .map(|snippet| snippet["text"].as_str().unwrap().len())
            .sum::<usize>();
        assert!(bytes > 0 && bytes <= 7);
        texts.extend(
            snippets
                .iter()
                .map(|snippet| snippet["text"].as_str().unwrap().to_owned()),
        );
        let Some(next) = reading.data["nextCursor"].as_str() else {
            break;
        };
        assert_ne!(next, previous);
        previous = next.into();
        cursor = next.into();
    }
    assert!(texts.concat().contains("Caller"));
    let changed = context::dispatch(
        &ctx,
        &selected,
        &context_request("Target", 50, 7, 120, 0, &cursor, false),
        ResearchMode::Static,
    )
    .unwrap_err();
    assert_eq!(changed.code, "codebase.invalid_search_cursor");
    assert_eq!(changed.exit_code, 2);
}
#[test]
fn unicode_context_continuation_never_splits_codepoints_or_restarts_a_consumed_byte() {
    let f = fixture();
    let ctx = source_context(&f);
    let selected = research_selection(&f);
    let mut cursor = None;
    let mut fragments = String::new();
    let mut pages = 0;
    loop {
        let reading = context::dispatch(
            &ctx,
            &selected,
            &context_request(
                "Unicode",
                50,
                7,
                120,
                0,
                cursor.as_deref().unwrap_or(""),
                false,
            ),
            ResearchMode::Static,
        )
        .unwrap();
        let part = reading.data["snippets"][0]["text"].as_str().unwrap();
        assert!(!part.is_empty() && part.len() <= 7);
        fragments.push_str(part);
        pages += 1;
        let next = reading.data["nextCursor"].as_str().map(str::to_owned);
        if next.is_none() {
            break;
        }
        cursor = next;
        assert!(pages < 32, "continuation failed to make bounded progress");
    }
    assert!(fragments.contains("汉字abcdef🙂"));
    assert!(pages > 4);
}

#[test]
#[ignore = "requires sealed current CLI and matching Release; complete Native relation, source identity and candidate boundaries"]
fn actual_full_source_semantic_relations_preserve_candidates_and_client_gaps() {
    let cli = NativeCli::supplied();
    // Loading function declarations does not prove that Caller selected Target.
    // Preserve this boundary independently from the explicitly invoked fixture.
    let uncalled = native_fixture(&[], false);
    let unknown = cli.refs(
        &uncalled,
        &[("--symbol", "Target"), ("--direction", "incoming")],
    );
    assert_executed_operations(&unknown, 2, &[2]);
    assert_eq!(
        unknown["nativeResearch"]["entryResolution"]["state"],
        "absent"
    );
    assert!(
        unknown["nativeResearch"]["entryEvidence"]["entries"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert_eq!(unknown["coverage"]["semantic"], "partial");
    let unknown_calls = unknown["relations"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| native_relation(row) && row["kind"] == "call")
        .collect::<Vec<_>>();
    assert_eq!(unknown_calls.len(), 2);
    for row in unknown_calls {
        assert_eq!(row["source"], "Caller");
        assert_eq!(row["target"], "Target");
        assert_eq!(row["state"], "static_candidate");
        assert!(row["observations"].as_array().unwrap().is_empty());
        assert_eq!(row["caller"]["kind"], "named");
        assert_eq!(row["caller"]["position"]["path"], "a.lua");
        assert!([2, 3].contains(&row["line"].as_u64().unwrap()));
    }
    let f = native_fixture(&[], true);
    let incoming = cli.refs(&f, &[("--symbol", "Target"), ("--direction", "incoming")]);
    assert_executed_operations(&incoming, 2, &[2]);
    assert_eq!(incoming["coverage"]["semantic"], "complete", "{incoming}");
    let references = incoming["relations"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| native_relation(row) && row["kind"] == "call")
        .collect::<Vec<_>>();
    assert_eq!(references.len(), 2);
    for row in references {
        assert_eq!(row["state"], "resolved");
        assert_eq!(row["source"], "Caller");
        assert_eq!(row["target"], "Target");
        assert_eq!(row["referenceExpression"], "Target");
        assert_eq!(row["caller"]["kind"], "named");
        assert_eq!(row["caller"]["name"], "Caller");
        assert_eq!(row["caller"]["position"]["path"], "a.lua");
        assert_eq!(row["path"], "a.lua");
        assert!([2, 3].contains(&row["line"].as_u64().unwrap()));
        assert_observed_target(row);
    }
    assert_eq!(
        incoming["relations"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|row| !native_relation(row)
                && row["kind"] == "call"
                && row["state"] == "static_candidate")
            .count(),
        2
    );
    let outgoing = cli.refs(&f, &[("--symbol", "Caller"), ("--direction", "outgoing")]);
    assert_executed_operations(&outgoing, 2, &[2]);
    let definitions = outgoing["relations"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| native_relation(row) && row["kind"] == "call")
        .collect::<Vec<_>>();
    assert_eq!(definitions.len(), 2);
    for row in definitions {
        assert_eq!(row["state"], "resolved");
        assert_eq!(row["source"], "Caller");
        assert_eq!(row["target"], "Target");
        assert_eq!(row["path"], "a.lua");
        assert!([2, 3].contains(&row["line"].as_u64().unwrap()));
        assert_observed_target(row);
    }
    assert_eq!(
        outgoing["nativeResearch"]["inputIdentity"],
        incoming["nativeResearch"]["inputIdentity"]
    );
    assert_eq!(outgoing["commit"], incoming["commit"]);
    let static_reading = cli.refs(&f, &[("--symbol", "Target"), ("--static-only", "true")]);
    assert!(static_reading.get("semanticAnalysis").is_none());
    assert_eq!(static_reading["coverage"]["semantic"], "unavailable");
    assert!(
        static_reading["relations"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|row| row["kind"] == "call")
            .all(|row| row["state"] == "static_candidate")
    );
}

#[test]
fn context_after_index_lazily_caches_only_snippet_originals_and_reuses_locked_warm_bytes() {
    let f = fixture();
    let ctx = source_context(&f);
    let selected = research_selection(&f);
    let pin = &selected.pin;
    let index = lycheedev_source::index::build_with_control(
        &f.home,
        pin.source.as_ref().unwrap(),
        &ctx.control,
    )
    .unwrap();
    assert!(!f.home.join("cache/source-originals-rust").exists());
    let request = context_request("Target", 1, 16 << 10, 20, 0, "", false);
    let first = context::dispatch(&ctx, &selected, &request, ResearchMode::Static).unwrap();
    assert_eq!(first.data["snippets"].as_array().unwrap().len(), 1);
    assert_eq!(first.data["snippets"][0]["path"], "a.lua");
    let doc = index
        .documents
        .iter()
        .find(|doc| doc.path == "a.lua")
        .unwrap();
    let original = f.home.join("cache/source-originals-rust").join(&doc.sha256);
    assert!(original.is_file());
    for other in index.documents.iter().filter(|doc| doc.path != "a.lua") {
        assert!(
            !f.home
                .join("cache/source-originals-rust")
                .join(&other.sha256)
                .exists()
        );
    }
    // Deny write/delete while the second actual context command reads the
    // warmed original. Re-publication would fail instead of hiding a rewrite.
    let (held, _) =
        lycheedev_core::storage::locked_sha256(&original, doc.bytes as u64, &ctx.control).unwrap();
    let second = context::dispatch(&ctx, &selected, &request, ResearchMode::Static).unwrap();
    for field in [
        "symbol",
        "relations",
        "snippets",
        "loadEvidence",
        "complete",
        "truncated",
        "nextCursor",
    ] {
        assert_eq!(first.data[field], second.data[field], "{field}");
    }
    lycheedev_core::storage::finish_file(held, Ok(()), &ctx.control).unwrap();
    for result in [&first, &second] {
        let captured =
            evidence::read_blob(&f.home, &result.provenance[0]["blob"], 1 << 20).unwrap();
        assert_eq!(
            serde_json::from_slice::<Value>(&captured).unwrap(),
            result.data
        );
        for load in result.data["loadEvidence"].as_array().unwrap() {
            assert!(
                !evidence::read_blob(&f.home, &load["sourceBlob"], 1 << 20)
                    .unwrap()
                    .is_empty()
            );
        }
    }
}

#[path = "refs_context/project_selection.rs"]
mod project_selection;
