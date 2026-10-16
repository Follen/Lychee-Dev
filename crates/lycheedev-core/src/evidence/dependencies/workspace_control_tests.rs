use super::*;
#[test]
fn workspace_parser_keeps_raw_time_and_go_unknown_depth_with_control() {
    let prefix=br#"{"schema":"lycheedev.workspace.v1","workspaceId":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","createdAt":"2026-10-15T00:00:00Z","ignored":"#;
    let mut raw = prefix.to_vec();
    raw.extend_from_slice("[".repeat(9999).as_bytes());
    raw.push(b'0');
    raw.extend_from_slice("]".repeat(9999).as_bytes());
    raw.push(b'}');
    assert!(decode_workspace_identity_with_check(&raw, || Ok(())).is_ok());
    let bad=br#"{"schema":"lycheedev.workspace.v1","workspaceId":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","createdAt":"2026-10-15T00:00:00\u005a"}"#;
    assert!(decode_workspace_identity_with_check(bad, || Ok(())).is_err());
}
#[test]
fn normalization_checks_original_scope_inside_long_strings_and_utf8_boundaries() {
    let raw = format!("\"{}\\u0061\"", "a".repeat(200000));
    let mut calls = 0;
    let error = normalize_go_strings_checked(raw.as_bytes(), "createdat", &mut || {
        calls += 1;
        if calls == 4 {
            Err(ToolkitError::new(
                "records.query_resource_budget",
                "normalizer original budget",
                3,
            ))
        } else {
            Ok(())
        }
    })
    .unwrap_err();
    assert_eq!(error.code, "records.query_resource_budget");
    let raw = format!("\"{}é{}\"", "a".repeat(65534), "b".repeat(65536));
    let out = normalize_go_strings_checked(raw.as_bytes(), "createdat", &mut || Ok(())).unwrap();
    assert!(matches!(out, Cow::Borrowed(_)));
    assert_eq!(out.as_ref(), raw.as_bytes());
}
#[test]
fn large_workspace_typed_parse_cannot_escape_the_same_check() {
    let raw = format!(
        r#"{{"schema":"lycheedev.workspace.v1","workspaceId":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","createdAt":"2026-10-15T00:00:00Z","ignored":"{}"}}"#,
        "x".repeat(200000)
    );
    let mut calls = 0;
    decode_workspace_identity_with_check(raw.as_bytes(), || {
        calls += 1;
        Ok(())
    })
    .unwrap();
    assert!(calls >= 15);
    let mut seen = 0;
    let error = decode_workspace_identity_with_check(raw.as_bytes(), || {
        seen += 1;
        if seen == calls - 1 {
            Err(ToolkitError::new(
                "records.query_resource_budget",
                "typed decoder stopped",
                3,
            ))
        } else {
            Ok(())
        }
    })
    .unwrap_err();
    assert_eq!(error.code, "records.query_resource_budget");
}
