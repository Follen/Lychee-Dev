//! Regressions exercise the production collector and authenticated index/fact
//! inputs used by context_flow, without launching a compiler or native worker.
//! The cold/warm case also uses the real fingerprinted persistent index gate.
use super::*;
use lycheedev_core::{selection::SourcePin, storage, workspace};
use std::{
    fs,
    io::Write,
    path::PathBuf,
    process::{Command, Stdio},
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

struct Fixture {
    home: PathBuf,
    index: index::Index,
    facts: Vec<index::LoadFact>,
    source: Value,
}
impl Fixture {
    fn new(repository: &str, product: &str, documents: &[(&str, &str)]) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let home = std::env::temp_dir().join(format!(
            "lycheedev-flow-conditions-{}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT.fetch_add(1, Ordering::Relaxed),
        ));
        workspace::initialize(&home).unwrap();
        let index = index::Index {
            documents: documents
                .iter()
                .map(|(path, raw)| index::Document {
                    path: (*path).into(),
                    text: Some(raw.as_bytes().to_vec()),
                    sha256: storage::sha256(raw.as_bytes()),
                    bytes: raw.len(),
                    facts: Default::default(),
                    facts_locator: None,
                })
                .collect(),
            assets: vec![],
            symbols: documents
                .iter()
                .flat_map(|(path, raw)| {
                    if !path.ends_with(".toc") {
                        return Vec::new();
                    }
                    crate::facts::toc(raw.as_bytes())
                        .headers
                        .into_iter()
                        .map(|header| index::Symbol {
                            id: String::new(),
                            kind: "header".into(),
                            name: header.key,
                            target: header.value,
                            category: "toc-field".into(),
                            confidence: "exact".into(),
                            path: (*path).into(),
                            line: header.line,
                            end_line: header.line,
                            signature: String::new(),
                            scope: String::new(),
                            ordinal: 0,
                        })
                        .collect()
                })
                .collect(),
            compatibility_gaps: BTreeSet::new(),
            home: home.clone(),
            summary: crate::index::model::test_summary(
                json!({"schema":"lycheedev.source-map.v2","documents":documents.len(),"repository":repository,"product":product,"commit":"a".repeat(40),"parser":"source-v1"}),
            ),
        };
        let raw = documents
            .iter()
            .map(|(path, raw)| ((*path).into(), (*raw).into()))
            .collect();
        let facts = index::load_facts_from_documents_with_control(
            &home,
            &index,
            &raw,
            &OperationControl::default(),
        )
        .unwrap();
        Self {
            home,
            index,
            facts,
            source: json!({"repository":repository,"product":product,"commit":"a".repeat(40)}),
        }
    }
    fn authenticated_index(&mut self) -> SourcePin {
        let mirror = crate::git::mirror(&self.home, self.source["repository"].as_str().unwrap());
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
        let run = |raw: &[u8], args: &[&str]| {
            let mut child = Command::new("git")
                .arg(format!("--git-dir={}", mirror.display()))
                .args(args)
                .env("GIT_AUTHOR_NAME", "Flow Fixture")
                .env("GIT_AUTHOR_EMAIL", "fixture@example.invalid")
                .env("GIT_COMMITTER_NAME", "Flow Fixture")
                .env("GIT_COMMITTER_EMAIL", "fixture@example.invalid")
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap();
            child.stdin.take().unwrap().write_all(raw).unwrap();
            let result = child.wait_with_output().unwrap();
            assert!(
                result.status.success(),
                "{}",
                String::from_utf8_lossy(&result.stderr)
            );
            String::from_utf8(result.stdout).unwrap().trim().to_owned()
        };
        let mut tree = String::new();
        for document in &self.index.documents {
            assert!(
                !document.path.contains('/'),
                "this fixture has only top-level Git entries"
            );
            let blob = run(
                document.text.as_deref().unwrap(),
                &["hash-object", "-w", "--stdin"],
            );
            tree.push_str(&format!("100644 blob {blob}\t{}\n", document.path));
        }
        let tree = run(tree.as_bytes(), &["mktree"]);
        let commit = run(b"flow header projection", &["commit-tree", &tree]);
        let pin = SourcePin {
            repository: self.source["repository"].as_str().unwrap().into(),
            product: self.source["product"].as_str().unwrap().into(),
            requested_ref: "fixture".into(),
            exact_commit: commit.clone(),
            parser_revision: "source-v1".into(),
        };
        self.source["commit"] = json!(commit);
        self.index =
            index::build_with_control(&self.home, &pin, &OperationControl::default()).unwrap();
        self.facts = index::load_facts_with_control(
            &self.home,
            &pin,
            &self.index,
            &OperationControl::default(),
        )
        .unwrap();
        pin
    }
    fn collect(&self, selected: &str) -> Selection {
        collect(
            selected,
            &self.index,
            &self.facts,
            &self.source,
            &OperationControl::default(),
        )
        .unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        if !std::thread::panicking()
            && std::env::var_os("LYCHEEDEV_RESEARCH_KEEP_FIXTURE").is_none()
        {
            let _ = fs::remove_dir_all(&self.home);
        }
    }
}
fn assert_condition_gap(result: &Selection, code: &str, target: &str) {
    assert!(result.ordered.is_none());
    assert!(
        result
            .input_gaps
            .iter()
            .any(|gap| gap["code"] == "load_order_unknown")
    );
    assert!(
        result.load_evidence["incompletePlans"]
            .as_array()
            .unwrap()
            .iter()
            .any(|plan| plan["issues"]
                .as_array()
                .unwrap()
                .iter()
                .any(|issue| issue["code"] == code
                    && issue["detail"].as_str().unwrap().contains(target)))
    );
}

#[test]
fn flow_conditional_production_collector_keeps_matching_defs_and_authenticated_warm_header_symbols()
{
    for (manifest, raw) in [
        (
            "Main_Mainline.toc",
            "Defs.lua [AllowLoadGameType mainline]\nSelected.lua\n",
        ),
        (
            "Main.toc",
            "## AllowLoadGameType: mainline\n## AllowLoadGameType: mainline\nDefs.lua [AllowLoadGameType mainline]\nSelected.lua\n",
        ),
    ] {
        let mut fixture = Fixture::new(
            "elvui",
            "main",
            &[
                (manifest, raw),
                ("Defs.lua", "function Definition() return 1 end\n"),
                (
                    "Selected.lua",
                    "function Selected() return Definition() end\n",
                ),
            ],
        );
        let pin = fixture.authenticated_index();
        assert_eq!(
            fixture.facts[0].target,
            "Defs.lua [AllowLoadGameType mainline]"
        );
        let original_facts = serde_json::to_vec(&fixture.facts).unwrap();
        let original_headers = serde_json::to_vec(
            &fixture
                .index
                .symbols
                .iter()
                .filter(|symbol| symbol.kind == "header")
                .collect::<Vec<_>>(),
        )
        .unwrap();
        index::take_read_observations();
        let cold = fixture.collect("Selected.lua");
        assert_eq!(
            cold.ordered.as_ref().unwrap(),
            &["Defs.lua", "Selected.lua"]
        );
        assert!(cold.input_gaps.is_empty());
        assert!(index::take_read_observations().is_empty());
        fixture.index =
            index::build_with_control(&fixture.home, &pin, &OperationControl::default()).unwrap();
        assert_eq!(fixture.index.summary.io["cachedIndex"], true);
        assert!(
            fixture
                .index
                .documents
                .iter()
                .all(|document| document.text.is_none() && document.facts.headers.is_empty())
        );
        assert_eq!(
            serde_json::to_vec(
                &fixture
                    .index
                    .symbols
                    .iter()
                    .filter(|symbol| symbol.kind == "header")
                    .collect::<Vec<_>>()
            )
            .unwrap(),
            original_headers
        );
        let warm = fixture.collect("Selected.lua");
        assert_eq!(warm.ordered, cold.ordered);
        assert_eq!(warm.load_evidence, cold.load_evidence);
        assert!(warm.input_gaps.is_empty());
        assert!(index::take_read_observations().is_empty());
        assert_eq!(serde_json::to_vec(&fixture.facts).unwrap(), original_facts);
    }
}

#[test]
fn flow_conditional_production_collector_excludes_supported_conditions_without_losing_order_or_repeats()
 {
    let fixture = Fixture::new(
        "wow-ui-source",
        "retail",
        &[
            (
                "Main.toc",
                "First.lua\nSkip.lua [AllowLoadGameType classic]\nShared.xml [AllowLoadGameType standard]\nFirst.lua [ExcludeLoadGameType classic]\nSkip.lua [ExcludeLoadGameType mainline]\nSkip.lua [AllowLoad glue]\nSkip.lua [UnknownCondition future] [AllowLoadGameType classic]\nLast.lua [AllowLoad game]\n",
            ),
            (
                "Shared.xml",
                "<Ui><Include file=\"Nested.xml\"/><Script file=\"Shared.lua\"/></Ui>",
            ),
            (
                "Nested.xml",
                "<Ui><Script file=\"Selected.lua\"/><Script file=\"Selected.lua\"/></Ui>",
            ),
            ("First.lua", ""),
            ("Selected.lua", ""),
            ("Shared.lua", ""),
            ("Last.lua", ""),
        ],
    );
    let result = fixture.collect("Selected.lua");
    assert_eq!(
        result.ordered.unwrap(),
        [
            "First.lua",
            "Selected.lua",
            "Selected.lua",
            "Shared.lua",
            "First.lua",
            "Last.lua"
        ]
    );
    assert!(result.input_gaps.is_empty());
    assert_eq!(result.load_evidence["incompletePlans"], json!([]));
}

#[test]
fn flow_conditional_production_collector_uses_pinned_family_and_manifest_game_inside_nested_xml() {
    for (product, family, game) in [
        ("retail", "Mainline", "Standard"),
        ("forever", "Mainline", "Standard"),
        ("classic", "Classic", "Wrath"),
        ("titan", "Classic", "Wrath"),
    ] {
        let manifest = if family == "Classic" {
            "Main_Wrath.toc"
        } else {
            "Main.toc"
        };
        let definitions = format!("{family}/Defs.lua");
        let nested = format!("{family}/Nested.xml");
        let selected = format!("{family}/{game}/Selected.lua");
        let fixture = Fixture::new(
            "wow-ui-source",
            product,
            &[
                (
                    manifest,
                    "[Family]\\Defs.lua\nShared.xml\n[Family]/Defs.lua\n",
                ),
                (
                    "Shared.xml",
                    "<Ui><Include file=\"[Family]/Nested.xml\"/></Ui>",
                ),
                (&nested, "<Ui><Script file=\"[Game]/Selected.lua\"/></Ui>"),
                (&definitions, ""),
                (&selected, ""),
            ],
        );
        let result = fixture.collect(&selected);
        assert_eq!(
            result.ordered.unwrap(),
            [definitions.clone(), selected, definitions]
        );
        assert!(result.input_gaps.is_empty());
    }
}

#[test]
fn flow_conditional_production_collector_keeps_unknown_and_malformed_nested_conditions_partial() {
    for target in [
        "Maybe.lua [AllowLoadGameType future]",
        "Maybe.lua [UnknownCondition mainline]",
        "Maybe.lua [AllowLoadGameType]",
        "Maybe.lua [AllowLoadGameType mainline",
        "Maybe.lua AllowLoadGameType mainline]",
        "Maybe.lua [AllowLoadGameType mainline] trailing",
    ] {
        let nested = format!("<Ui><Script file=\"{target}\"/><Script file=\"Selected.lua\"/></Ui>");
        let fixture = Fixture::new(
            "wow-ui-source",
            "retail",
            &[
                ("Main_Mainline.toc", "Shared.xml\n"),
                ("Shared.xml", "<Ui><Include file=\"Nested.xml\"/></Ui>"),
                ("Nested.xml", &nested),
                ("Selected.lua", ""),
                ("Maybe.lua", ""),
            ],
        );
        let result = fixture.collect("Selected.lua");
        assert_condition_gap(&result, "load_condition_unavailable", target);
        assert_eq!(
            result.load_evidence["candidates"][0]["ordered"],
            json!(["Selected.lua"])
        );
        assert!(fixture.facts.iter().any(|fact| fact.target == target));
    }
}

#[test]
fn flow_conditional_production_collector_uses_manifest_headers_and_explicit_admission() {
    let fixture = Fixture::new(
        "elvui",
        "main",
        &[
            (
                "Main.toc",
                "## AllowLoadGameType: mainline\n[Family]/[Game]/Selected.lua\n",
            ),
            ("Mainline/Standard/Selected.lua", ""),
        ],
    );
    let result = fixture.collect("Mainline/Standard/Selected.lua");
    assert_eq!(result.ordered.unwrap(), ["Mainline/Standard/Selected.lua"]);
    assert!(result.input_gaps.is_empty());
    for mode in ["plunderstorm", "wowhack"] {
        let toc = format!(
            "## AllowLoadGameType: {mode}\nSelected.lua [AllowLoadGameType {mode}]\nSkip.lua [AllowLoadGameType standard]\n"
        );
        let fixture = Fixture::new(
            "wow-ui-source",
            "retail",
            &[("Main.toc", &toc), ("Selected.lua", "")],
        );
        let result = fixture.collect("Selected.lua");
        assert_eq!(result.ordered.unwrap(), ["Selected.lua"]);
        assert!(result.input_gaps.is_empty());
    }
    for (manifest, header) in [
        ("Main.toc", "## AllowLoadEnvironment: future"),
        ("Main_Mainline.toc", "## AllowLoadGameType: classic"),
    ] {
        let toc = format!("{header}\nSelected.lua\n");
        let fixture = Fixture::new("elvui", "main", &[(manifest, &toc), ("Selected.lua", "")]);
        let result = fixture.collect("Selected.lua");
        assert_condition_gap(&result, "toc_manifest_condition_unavailable", manifest);
        assert_eq!(result.load_evidence["candidates"], json!([]));
    }
    let fixture = Fixture::new(
        "wow-ui-source",
        "retail",
        &[
            (
                "Excluded.toc",
                "## ExcludeLoadGameType: mainline\nSelected.lua\n",
            ),
            ("Main.toc", "Selected.lua\n"),
            ("Selected.lua", ""),
        ],
    );
    let result = fixture.collect("Selected.lua");
    assert_eq!(result.ordered.unwrap(), ["Selected.lua"]);
    assert!(result.input_gaps.is_empty());
    assert_eq!(
        result.load_evidence["candidates"].as_array().unwrap().len(),
        1
    );
}

#[test]
fn flow_conditional_production_collector_keeps_alternative_manifest_contexts_independent() {
    let fixture = Fixture::new(
        "wow-ui-source",
        "retail",
        &[
            ("Main_Mainline.toc", "[Family]/Selected.lua\n"),
            ("Main_Classic.toc", "[Family]/Selected.lua\n"),
            ("Mainline/Selected.lua", ""),
            ("Classic/Selected.lua", ""),
        ],
    );
    for family in ["Mainline", "Classic"] {
        let selected = format!("{family}/Selected.lua");
        let result = fixture.collect(&selected);
        assert_eq!(result.ordered.unwrap(), [selected]);
        assert!(result.input_gaps.is_empty());
        assert_eq!(
            result.load_evidence["candidates"][0]["manifest"],
            format!("Main_{family}.toc")
        );
    }
    let fixture = Fixture::new(
        "wow-ui-source",
        "retail",
        &[
            ("Main_Mainline.toc", "Selected.lua\n"),
            ("Main_Classic.toc", "Selected.lua\n"),
            ("Selected.lua", ""),
        ],
    );
    let result = fixture.collect("Selected.lua");
    assert!(result.ordered.is_none());
    assert_eq!(
        result.load_evidence["candidates"].as_array().unwrap().len(),
        2
    );
    assert_eq!(result.load_evidence["incompletePlans"], json!([]));
}

#[test]
fn flow_conditional_production_collector_keeps_unestablished_family_and_game_partial() {
    for (repository, product, manifest, target) in [
        ("elvui", "main", "Main.toc", "[Family]/Maybe.lua"),
        ("wow-ui-source", "classic", "Main.toc", "[Game]/Maybe.lua"),
        (
            "wow-ui-source",
            "retail",
            "Main_Plunderstorm.toc",
            "[Game]/Maybe.lua",
        ),
    ] {
        let toc = format!("{target}\nSelected.lua\n");
        let fixture = Fixture::new(
            repository,
            product,
            &[(manifest, &toc), ("Selected.lua", "")],
        );
        let result = fixture.collect("Selected.lua");
        assert_condition_gap(&result, "load_condition_unavailable", target);
    }
}

#[test]
fn flow_conditional_production_collector_uses_headers_without_an_original_byte_size_gate() {
    let oversized = format!("#{}", "x".repeat(super::super::FILE_BYTES));
    let fixture = Fixture::new(
        "wow-ui-source",
        "retail",
        &[
            ("First.toc", "Selected.lua\n"),
            ("Oversized.toc", &oversized),
            ("Selected.lua", ""),
        ],
    );
    index::take_read_observations();
    let result = fixture.collect("Selected.lua");
    assert_eq!(result.ordered.unwrap(), ["Selected.lua"]);
    assert!(result.input_gaps.is_empty());
    assert_eq!(result.input_bytes, 0);
    assert!(index::take_read_observations().is_empty());
}

#[test]
fn flow_conditional_production_collector_keeps_missing_or_incomplete_projection_explicit() {
    let mut fixture = Fixture::new(
        "wow-ui-source",
        "retail",
        &[
            ("Main.toc", "## AllowLoadGameType: mainline\nSelected.lua\n"),
            ("Selected.lua", ""),
        ],
    );
    let original_summary = fixture.index.summary.clone();
    let mut invalid_summary = serde_json::to_value(&fixture.index.summary).unwrap();
    invalid_summary["schema"] = Value::Null;
    assert!(serde_json::from_value::<index::SourceSummary>(invalid_summary).is_err());
    for replacement in ["", "legacy-map-v1"] {
        fixture.index.summary.schema = replacement.into();
        let result = fixture.collect("Selected.lua");
        assert!(result.ordered.is_none());
        assert!(
            result.load_evidence["unavailableFacts"]
                .as_array()
                .unwrap()
                .iter()
                .any(|gap| gap["code"] == "load_header_evidence_unavailable")
        );
    }
    fixture.index.summary = original_summary;
    fixture.index.summary.documents = 99;
    assert!(fixture.collect("Selected.lua").ordered.is_none());
    let mut missing_symbols = serde_json::to_value(&fixture.index).unwrap();
    missing_symbols.as_object_mut().unwrap().remove("symbols");
    assert!(serde_json::from_value::<index::Index>(missing_symbols).is_err());
    fixture.index.summary.documents = fixture.index.documents.len();
    fixture.index.symbols[0].category = "incomplete".into();
    let result = fixture.collect("Selected.lua");
    assert!(result.ordered.is_none());
    assert!(
        result.load_evidence["unavailableFacts"]
            .as_array()
            .unwrap()
            .iter()
            .any(|gap| gap["code"] == "load_header_evidence_unavailable")
    );
}

#[test]
fn flow_conditional_production_collector_preserves_original_control_before_header_selection() {
    let fixture = Fixture::new(
        "wow-ui-source",
        "retail",
        &[
            ("Main.toc", "## AllowLoadGameType: mainline\nSelected.lua\n"),
            ("Selected.lua", ""),
        ],
    );
    let cancelled = crate::native::stopped_control(
        false,
        std::time::Instant::now() + std::time::Duration::from_secs(60),
    );
    for (control, code) in [
        (cancelled, "command.cancelled"),
        (
            OperationControl::uncancelable(std::time::Instant::now()),
            "command.deadline",
        ),
    ] {
        let error = collect(
            "Selected.lua",
            &fixture.index,
            &fixture.facts,
            &fixture.source,
            &control,
        )
        .err()
        .unwrap();
        assert_eq!(error.code, code);
    }
}

#[test]
fn flow_conditional_header_bytes_consume_the_same_public_input_allowance() {
    let mut fixture = Fixture::new(
        "wow-ui-source",
        "retail",
        &[
            ("Main.toc", "## Title: x\nSelected.lua\n"),
            ("Selected.lua", ""),
        ],
    );
    fixture.index.symbols[0].target = "x".repeat(super::super::INPUT_BYTES - "Title".len());
    let full = fixture.collect("Selected.lua");
    assert_eq!(full.ordered.unwrap(), ["Selected.lua"]);
    assert_eq!(full.input_bytes, super::super::INPUT_BYTES);
    assert_eq!(
        full.load_evidence["budget"]["metadataBytes"],
        super::super::INPUT_BYTES
    );
    fixture.index.symbols[0].target.push('x');
    let exceeded = fixture.collect("Selected.lua");
    assert!(exceeded.ordered.is_none());
    assert!(
        exceeded
            .input_gaps
            .iter()
            .any(|gap| gap["code"] == "load_budget")
    );
    assert_eq!(exceeded.load_evidence["budget"]["exhausted"], true);
}

#[test]
fn flow_conditional_header_references_share_the_existing_work_budget() {
    let fixture = Fixture::new(
        "wow-ui-source",
        "retail",
        &[
            (
                "Main.toc",
                "## AllowLoadGameType: mainline\n## AllowLoadGameType: mainline\nSelected.lua\n",
            ),
            ("Selected.lua", ""),
        ],
    );
    let result = collect_with(
        "Selected.lua",
        &fixture.index.documents,
        &fixture.facts,
        &fixture.index.symbols,
        true,
        Some(&fixture.source),
        &OperationControl::default(),
        Limits {
            work: 2,
            ..Limits::default()
        },
        |_| {},
    )
    .unwrap();
    assert!(result.ordered.is_none());
    assert_eq!(result.load_evidence["budget"]["work"], 2);
    assert_eq!(result.load_evidence["budget"]["exhausted"], true);
    let token = lycheedev_core::control::TrackedCancellation::new();
    let control = OperationControl::tracked(
        std::time::Instant::now() + std::time::Duration::from_secs(60),
        token.clone(),
    );
    let error = collect_with(
        "Selected.lua",
        &fixture.index.documents,
        &fixture.facts,
        &fixture.index.symbols,
        true,
        Some(&fixture.source),
        &control,
        Limits::default(),
        |work| {
            if work > fixture.facts.len() {
                token.cancel();
            }
        },
    )
    .err()
    .unwrap();
    assert_eq!(error.code, "command.cancelled");
}

#[test]
fn flow_conditional_borrowed_headers_match_raw_context_admission_and_targets_byte_for_byte() {
    let targets = [
        "Defs.lua [AllowLoadGameType mainline]",
        "Skip.lua [ExcludeLoadGameType classic]",
        "Maybe.lua [AllowLoadGameType future]",
        "[Family]/[Game]/Selected.lua",
    ];
    for (manifest, raw) in [
        ("Main.toc", ""),
        (
            "Main.toc",
            "\u{feff}## Title: 标题\r\n## AllowLoadGameType: mainline\r\n## AllowLoadGameType: mainline\r\n",
        ),
        (
            "Main.toc",
            "## AllowLoadGameType: classic, standard\n## AllowLoad: game\n## Unknown: ignored\n",
        ),
        (
            "Main.toc",
            "## AllowLoadGameType: \n## AllowLoadEnvironment: future\n",
        ),
        (
            "Main_Mainline.toc",
            "## AllowLoadGameType: classic\n## AllowLoadGameType: mainline\n",
        ),
        (
            "Main.toc",
            "## AllowLoadGameType: plunderstorm\n## AllowLoadGameType: standard\n",
        ),
        (
            "Main.toc",
            "## AllowLoadGameType: standard\n## AllowLoadGameType: plunderstorm\n",
        ),
        (
            "Main.toc",
            "## AllowLoadGameType: mainline\rclassic\n## ExcludeLoadGameType: future\n## AllowLoad: glue\n",
        ),
    ] {
        for product in ["retail", "classic", "titan", "forever", "unknown"] {
            let source = json!({"repository":"wow-ui-source","product":product});
            let parsed = crate::facts::toc(raw.as_bytes());
            let fields = || {
                parsed
                    .headers
                    .iter()
                    .map(|header| (header.key.as_str(), header.value.as_str()))
            };
            let raw_context = LoadContext::for_manifest(manifest, raw.as_bytes(), Some(&source));
            let borrowed = LoadContext::for_manifest_headers(manifest, fields(), Some(&source));
            assert_eq!(
                serde_json::to_vec(&borrowed).unwrap(),
                serde_json::to_vec(&raw_context).unwrap()
            );
            assert_eq!(
                borrowed.manifest_admission_headers(fields()),
                raw_context.manifest_admission(raw.as_bytes())
            );
            for target in targets {
                assert_eq!(borrowed.target(target), raw_context.target(target));
            }
        }
    }
    let source = json!({"repository":"wow-ui-source","product":"retail"});
    let a = LoadContext::for_manifest_headers(
        "Main.toc",
        [
            ("AllowLoadGameType", "plunderstorm"),
            ("AllowLoadGameType", "standard"),
        ],
        Some(&source),
    );
    let b = LoadContext::for_manifest_headers(
        "Main.toc",
        [
            ("AllowLoadGameType", "standard"),
            ("AllowLoadGameType", "plunderstorm"),
        ],
        Some(&source),
    );
    assert_ne!(
        serde_json::to_vec(&a).unwrap(),
        serde_json::to_vec(&b).unwrap(),
        "header order must not be sorted or deduplicated"
    );
    assert_eq!(a.mode, crate::xml_load::GameMode::Standard);
    assert_eq!(b.mode, crate::xml_load::GameMode::Plunderstorm);
    let repeated = LoadContext::for_manifest_headers(
        "Main.toc",
        [
            ("AllowLoadGameType", "mainline"),
            ("AllowLoadGameType", "mainline"),
        ],
        Some(&source),
    );
    assert_eq!(
        repeated
            .basis
            .iter()
            .filter(|basis| basis.as_str() == "manifest_AllowLoadGameType:mainline")
            .count(),
        2
    );
    assert!(matches!(
        repeated.manifest_admission_headers([("AllowLoadGameType", "")]),
        LoadDecision::Unavailable(_)
    ));
}
