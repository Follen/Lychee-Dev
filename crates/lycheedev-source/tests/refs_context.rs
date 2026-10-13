use lycheedev_core::{
    Invocation, evidence,
    selection::{self, SelectionSpec, SourcePin},
    workspace,
};
use lycheedev_source::{SourceTools, context, git, luals, refs};
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
    let home = std::env::temp_dir().join(format!(
        "lycheedev-research-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
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
        (
            "a.lua",
            "function Caller()\n  Target()\n  Target()\nend\nfunction Target() end\n-- Target() fake comment\nlocal label='Target() fake string'\n",
        ),
        ("Main.toc", "## Interface: 120100\na.lua\n"),
        ("b.lua", "function Neighbor()\n  Caller()\nend\n"),
        (
            "unicode.lua",
            "function Unicode()\r\n  return '汉字abcdef🙂'\r\nend\r\n",
        ),
    ];
    let mut tree = String::new();
    for (path, text) in files.into_iter().chain(extra.iter().copied()) {
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
#[test]
#[ignore = "requires exact pinned LuaLS ZIP; 130 real semantic queries across two batches"]
fn actual_incoming_hover_plus_128_outgoing_calls_share_one_initialized_workspace() {
    let zip = PathBuf::from(std::env::var_os("LYCHEEDEV_LUALS_ZIP").expect("fixed ZIP"));
    let body = format!("function Bulk()\n{}end\n", "  Target()\n".repeat(128));
    let f = fixture_with(&[("bulk.lua", &body)]);
    let runtime_root = f.home.join("fixture-runtime");
    let runtime = luals::Runtime::unpack_pinned(&zip, &runtime_root).unwrap();
    let reading = refs::dispatch_with_tools(
        &invoke(
            &f,
            "source refs",
            &[
                ("--symbol", "Bulk"),
                ("--direction", "both"),
                ("--limit", "200"),
            ],
        ),
        SourceTools {
            luac: None,
            luals: Some(&runtime),
        },
    )
    .unwrap();
    assert_eq!(
        reading.data["semanticAnalysis"]["queryCount"], 130,
        "{}",
        reading.data
    );
    assert_eq!(reading.data["semanticAnalysis"]["queryBatches"], 2);
    assert_eq!(reading.data["coverage"]["semantic"], "complete");
    let mut resolved = reading.data["relations"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| row["kind"] == "call" && row["state"] == "resolved")
        .count();
    // The public 200-row page also contains structural candidates; continuation
    // must contain the remainder without reducing the admitted semantic budget.
    let cursor = reading.data["nextCursor"].as_str().unwrap();
    let next = refs::dispatch_with_tools(
        &invoke(
            &f,
            "source refs",
            &[
                ("--symbol", "Bulk"),
                ("--direction", "both"),
                ("--limit", "200"),
                ("--cursor", cursor),
            ],
        ),
        SourceTools {
            luac: None,
            luals: Some(&runtime),
        },
    )
    .unwrap();
    resolved += next.data["relations"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| row["kind"] == "call" && row["state"] == "resolved")
        .count();
    assert_eq!(
        resolved, 128,
        "all outgoing bindings must be returned across legal pages"
    );
    assert!(
        !reading.complete,
        "no selected client environment must stay partial"
    );
}
fn invoke(f: &Fixture, command: &str, flags: &[(&str, &str)]) -> Invocation {
    let mut options: BTreeMap<_, _> = flags
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    options.insert("--snapshot".into(), f.pin.clone());
    Invocation {
        command: command.into(),
        flags: options,
        positional: None,
        home: f.home.clone(),
        cwd: f.home.clone(),
        control: Default::default(),
    }
}
#[test]
fn structural_refs_cursor_and_original_context_are_bounded_without_semantic_claims() {
    let f = fixture();
    let first = refs::dispatch(&invoke(
        &f,
        "source refs",
        &[
            ("--symbol", "Target"),
            ("--direction", "incoming"),
            ("--limit", "1"),
            ("--static-only", "true"),
        ],
    ))
    .unwrap();
    assert_eq!(first.data["relations"].as_array().unwrap().len(), 1);
    assert_eq!(first.data["relations"][0]["state"], "static_candidate");
    assert_eq!(first.data["relations"][0]["target"], "Target");
    assert!(first.data["relations"][0].get("targetId").is_none());
    assert!(!first.complete);
    let cursor = first.data["nextCursor"].as_str().unwrap();
    let next = refs::dispatch(&invoke(
        &f,
        "source refs",
        &[
            ("--symbol", "Target"),
            ("--direction", "incoming"),
            ("--limit", "1"),
            ("--cursor", cursor),
            ("--static-only", "true"),
        ],
    ))
    .unwrap();
    assert_ne!(
        next.data["relations"][0]["line"],
        first.data["relations"][0]["line"]
    );
    assert_eq!(
        refs::dispatch(&invoke(
            &f,
            "source refs",
            &[
                ("--symbol", "Caller"),
                ("--direction", "incoming"),
                ("--limit", "1"),
                ("--cursor", cursor)
            ]
        ))
        .unwrap_err()
        .exit_code,
        2
    );
    let reading = context::dispatch(&invoke(
        &f,
        "source context",
        &[
            ("--symbol", "Target"),
            ("--static-only", "true"),
            ("--max-lines", "2"),
        ],
    ))
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
fn research_rejects_unconnected_profiles_and_selector_conflicts_explicitly() {
    let f = fixture();
    assert_eq!(
        refs::dispatch(&invoke(&f, "source refs", &[]))
            .unwrap_err()
            .exit_code,
        2
    );
    assert_eq!(
        refs::dispatch(&invoke(
            &f,
            "source refs",
            &[("--symbol", "Target"), ("--symbol-id", "different")]
        ))
        .unwrap_err()
        .exit_code,
        2
    );
    assert_eq!(
        refs::dispatch(&invoke(
            &f,
            "source refs",
            &[("--symbol", "Target"), ("--release", "explicit")]
        ))
        .unwrap_err()
        .exit_code,
        3
    );
    assert_eq!(
        context::dispatch(&invoke(
            &f,
            "source context",
            &[("--symbol", "Target"), ("--flow", "true")]
        ))
        .unwrap_err()
        .exit_code,
        3
    );
    let deep = context::dispatch(&invoke(
        &f,
        "source context",
        &[
            ("--symbol", "Target"),
            ("--depth", "2"),
            ("--static-only", "true"),
        ],
    ))
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
    let mut cursor = String::new();
    let mut texts = vec![];
    let mut previous = String::new();
    for _ in 0..32 {
        let mut invocation = invoke(
            &f,
            "source context",
            &[
                ("--symbol", "Caller"),
                ("--static-only", "true"),
                ("--depth", "0"),
                ("--max-bytes", "7"),
            ],
        );
        if !cursor.is_empty() {
            invocation.flags.insert("--cursor".into(), cursor.clone());
        }
        let reading = context::dispatch(&invocation).unwrap();
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
    let changed = context::dispatch(&invoke(
        &f,
        "source context",
        &[
            ("--symbol", "Target"),
            ("--static-only", "true"),
            ("--depth", "0"),
            ("--max-bytes", "7"),
            ("--cursor", &cursor),
        ],
    ))
    .unwrap_err();
    assert_eq!(changed.code, "codebase.invalid_search_cursor");
    assert_eq!(changed.exit_code, 2);
}
#[test]
fn unicode_context_continuation_never_splits_codepoints_or_restarts_a_consumed_byte() {
    let f = fixture();
    let mut cursor = None;
    let mut fragments = String::new();
    let mut pages = 0;
    loop {
        let mut inv = invoke(
            &f,
            "source context",
            &[
                ("--symbol", "Unicode"),
                ("--static-only", "true"),
                ("--depth", "0"),
                ("--max-bytes", "7"),
            ],
        );
        if let Some(value) = cursor.take() {
            inv.flags.insert("--cursor".into(), value);
        }
        let reading = context::dispatch(&inv).unwrap();
        let part = reading.data["snippets"][0]["text"].as_str().unwrap();
        assert!(!part.is_empty() && part.len() <= 7);
        fragments.push_str(part);
        pages += 1;
        cursor = reading.data["nextCursor"].as_str().map(str::to_owned);
        if cursor.is_none() {
            break;
        }
        assert!(pages < 32, "continuation failed to make bounded progress");
    }
    assert!(fragments.contains("汉字abcdef🙂"));
    assert!(pages > 4);
}

#[test]
#[ignore = "requires exact pinned LuaLS ZIP; real full-source research regression"]
fn actual_full_source_semantic_relations_preserve_candidates_and_client_gaps() {
    let zip = PathBuf::from(std::env::var_os("LYCHEEDEV_LUALS_ZIP").expect("set fixed LuaLS ZIP"));
    let f = fixture();
    let stage = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../.tmp/rust-source-research-semantic-real")
        .join(format!("run-{}", std::process::id()));
    fs::create_dir_all(&stage).unwrap();
    let runtime_root = stage.join("runtime");
    let runtime = luals::Runtime::unpack_pinned(&zip, &runtime_root).unwrap();
    let tools = || SourceTools {
        luac: None,
        luals: Some(&runtime),
    };
    let incoming = refs::dispatch_with_tools(
        &invoke(
            &f,
            "source refs",
            &[
                ("--symbol", "Target"),
                ("--direction", "incoming"),
                ("--release", "verified-by-caller"),
            ],
        ),
        tools(),
    )
    .unwrap();
    assert_eq!(incoming.data["semanticRuntime"], runtime.identity());
    assert_eq!(
        incoming.data["coverage"]["semantic"], "complete",
        "{}",
        incoming.data
    );
    assert_eq!(
        incoming.data["coverage"]["environment"],
        "missing-client-pin"
    );
    assert!(!incoming.complete);
    let references = incoming.data["relations"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| row["kind"] == "reference" && row["state"] == "resolved")
        .collect::<Vec<_>>();
    assert_eq!(references.len(), 2, "{}", incoming.data);
    assert!(references.iter().all(|row| row["state"] == "resolved"
        && row["targetId"].is_string()
        && [2, 3].contains(&row["line"].as_u64().unwrap())));
    let outgoing = refs::dispatch_with_tools(
        &invoke(
            &f,
            "source refs",
            &[("--symbol", "Caller"), ("--direction", "outgoing")],
        ),
        tools(),
    )
    .unwrap();
    let definitions = outgoing.data["relations"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| row["kind"] == "call" && row["state"] == "resolved")
        .collect::<Vec<_>>();
    assert_eq!(definitions.len(), 2, "{}", outgoing.data);
    assert!(definitions.iter().all(|row| row["targetPath"] == "a.lua"
        && row["targetLine"] == 5
        && row["targetId"].is_string()));
    let static_reading = refs::dispatch_with_tools(
        &invoke(
            &f,
            "source refs",
            &[("--symbol", "Target"), ("--static-only", "true")],
        ),
        tools(),
    )
    .unwrap();
    assert!(static_reading.data.get("semanticAnalysis").is_none());
    assert_eq!(static_reading.data["coverage"]["semantic"], "unavailable");
    let captured = evidence::read_blob(&f.home, &incoming.provenance[0]["blob"], 1 << 20).unwrap();
    assert_eq!(
        serde_json::from_slice::<Value>(&captured).unwrap(),
        incoming.data
    );
    fs::write(
        stage.join("report.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
        "state":"passed", "home":f.home,"pin":f.pin, "scope":"full fixed fixture source, one-shot semantic relations",
            "incoming":incoming.data,"outgoing":outgoing.data,"static":static_reading.data,
            "wholeSourceAcceptance":"not_run","broker":"not_run","flow":"not_run"
        }))
        .unwrap(),
    )
    .unwrap();
    drop(runtime);
    fs::remove_dir_all(&runtime_root).unwrap();
}
