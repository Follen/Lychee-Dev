use super::*;
use std::{
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

fn fixture() -> PreparedSnapshot {
    let control = OperationControl::uncancelable(Instant::now() + Duration::from_secs(30));
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
    prepared.control.tracked_cancel_handle().unwrap().cancel();
    let expected = prepared.control.check().unwrap_err().to_string();
    assert_eq!(prepared.binding_site(&site).unwrap_err(), expected);
}
