use super::*;
use std::{
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

fn fixture() -> PreparedSnapshot {
    let control = OperationControl::new(
        Instant::now() + Duration::from_secs(30),
        Arc::new(AtomicBool::new(false)),
    );
    let host = TrustedHostEnvironment::from_host_material(
        "expression-index-test".into(),
        false,
        vec![],
        vec![],
        &control,
    )
    .unwrap();
    let mut config = Emmyrc::default();
    config.runtime.version = emmylua_code_analysis::EmmyrcLuaVersion::Lua51;
    let files = [
        (
            "plain.lua",
            "local value=1; function F(x) return (x+value), Obj:Run(x) end; F(value)",
        ),
        (
            "bom.lua",
            "\u{feff}local text='你好 😀'\r\nObj.field=F(text)\r\nObj:Run(text)",
        ),
        ("empty.lua", ""),
    ]
    .into_iter()
    .map(|(path, text)| SourceFileInput {
        path: path.into(),
        original_utf8: text.into(),
        content_sha256: hash(text.as_bytes()),
        analysis_utf8: text.strip_prefix('\u{feff}').unwrap_or(text).into(),
        map: if text.starts_with('\u{feff}') {
            SourceMap::Utf8BomPrefix
        } else {
            SourceMap::Identity
        },
    })
    .collect();
    let inputs = SourceInputs::admit(
        SourceInputSpec {
            files,
            workspace_root: std::env::current_dir()
                .unwrap()
                .join("expression-index-fixture"),
            source_identity: json!({"test":"expression-index"}),
            phase: "unknown".into(),
            ordered_occurrences: vec![],
            config,
            host,
            luac: LuacIdentity {
                version: "5.1.5".into(),
                platform: "windows-amd64".into(),
                executable_sha256: "a".repeat(64),
                executable_bytes: 1,
            },
            limits: SourceLimits::research(),
        },
        &control,
    )
    .unwrap();
    prepare(inputs, &control).unwrap()
}

#[test]
fn admitted_expression_identities_match_previous_map_for_all_original_byte_ranges() {
    let prepared = fixture();
    let identity = prepared.inputs.identity_sha256().to_owned();
    let steps = prepared.inputs.shared_steps.load(Ordering::Relaxed);
    for source in &prepared.inputs.corpus.files {
        let file = prepared.files[&source.path];
        let model = prepared.analysis.semantic_model(file);
        let root = model.syntax_tree().unwrap().get_red_root();
        // The exact previous production insertion order is the oracle.
        let mut expected = BTreeMap::new();
        for expression in root.descendants().filter_map(LuaExpr::cast) {
            let range = prepared
                .inputs
                .original_range(&source.path, expression.get_range())
                .unwrap();
            expected.insert(
                (u32::from(range.start()), u32::from(range.end())),
                expression.get_syntax_id(),
            );
        }
        for start in 0..=source.original.len() as u32 {
            for end in start..=source.original.len() as u32 {
                let site = Site {
                    file: source.path.clone(),
                    start,
                    end,
                };
                let actual = prepared.binding_site(&site);
                match expected.get(&(start, end)) {
                    Some(syntax) => assert_eq!(
                        actual.unwrap(),
                        BindingSite {
                            file_id: file,
                            syntax: *syntax
                        }
                    ),
                    None => assert!(actual.is_err(), "unexpected expression at {site:?}"),
                }
                assert_eq!(
                    prepared.expression_binding_site(&site),
                    expected.get(&(start, end)).map(|syntax| BindingSite {
                        file_id: file,
                        syntax: *syntax
                    })
                );
            }
        }
    }
    assert_eq!(prepared.inputs.identity_sha256(), identity);
    assert_eq!(
        prepared.inputs.shared_steps.load(Ordering::Relaxed),
        steps,
        "exact lookup must not create or refund charges"
    );
}

#[test]
fn expression_identity_lookup_still_rejects_unknown_file_and_cancelled_request() {
    let prepared = fixture();
    assert!(
        prepared
            .binding_site(&Site {
                file: "missing.lua".into(),
                start: 0,
                end: 0
            })
            .is_err()
    );
    let site = Site {
        file: "plain.lua".into(),
        start: 12,
        end: 13,
    };
    prepared
        .control
        .cancel_handle()
        .store(true, Ordering::Release);
    let expected = prepared.control.check().unwrap_err().to_string();
    assert_eq!(prepared.binding_site(&site).unwrap_err(), expected);
}
