use super::*;
use crate::test_support::{Reply, Server, TestDir};
use std::time::{Duration, Instant};
fn provider(dir: &TestDir, server: &Server, api: bool, upload: bool) -> Curseforge {
    let control = OperationControl::uncancelable(Instant::now() + Duration::from_secs(10));
    Curseforge {
        config: Config {
            path: Config::path(&dir.0),
            author_id: Some(42),
            api_key: api.then(|| "core-secret".into()),
            upload_token: upload.then(|| "upload-secret".into()),
        },
        http: Http::loopback(&control, server.url.clone()),
        control,
        home: dir.0.clone(),
    }
}
fn input(file: &Path) -> Value {
    json!({"schema":"lycheedev.publish.v1.curseforge.plugin.upload","project_id":719,"file":file,"changelog":"Fix login failure","release_type":"release"})
}
fn parse(value: Value, dir: &TestDir) -> Upload {
    Upload::parse(&serde_json::to_vec(&value).unwrap(), &dir.0).unwrap()
}

#[test]
fn profile_precedence_and_doctor_expose_presence_only() {
    let profile=b"\xef\xbb\xbf# local profile\nCURSEFORGE_AUTHOR_ID=42\nCURSEFORGE_API_KEY=file-key\nCURSEFORGE_UPLOAD_TOKEN=upload-secret\n";
    let config = Config::parse(
        "isolated/publish/curseforge.env".into(),
        profile,
        |key| match key {
            "CURSEFORGE_API_KEY" => Some("env-secret".into()),
            "CURSEFORGE_UPLOAD_TOKEN" => Some("   ".into()),
            _ => None,
        },
    )
    .unwrap();
    assert_eq!(config.api_key.as_deref(), Some("env-secret"));
    assert_eq!(config.upload_token.as_deref(), Some("upload-secret"));
    let doctor = config.doctor();
    assert_eq!(doctor["ready"], true);
    assert!(doctor["config_path"].as_str().unwrap().contains("publish"));
    let output = doctor.to_string();
    for secret in ["file-key", "env-secret", "upload-secret"] {
        assert!(!output.contains(secret));
    }
    for raw in [
        "UNKNOWN=private-value",
        "CURSEFORGE_API_KEY=a\nCURSEFORGE_API_KEY=private-value",
        "CURSEFORGE_AUTHOR_ID=0",
        "CURSEFORGE_AUTHOR_ID=private-value",
    ] {
        let error = Config::parse("profile".into(), raw.as_bytes(), |_| None)
            .err()
            .unwrap();
        assert!(
            !serde_json::to_string(&error)
                .unwrap()
                .contains("private-value")
        );
    }
}

#[test]
fn public_project_membership_search_paginates_and_projects_only_public_metadata() {
    let dir = TestDir::new();
    let rows = (1..=50)
        .map(
            |id| json!({"id":id,"name":format!("Project {id}"),"unpublished_token":"never-output"}),
        )
        .collect::<Vec<_>>();
    let server = Server::new(vec![
        Reply::Json(
            json!({"data":rows,"pagination":{"index":0,"resultCount":50,"pageSize":50,"totalCount":52}}),
        ),
        Reply::Json(
            json!({"data":[{"id":51},{"id":52}],"pagination":{"index":50,"resultCount":2,"pageSize":50,"totalCount":52}}),
        ),
    ]);
    let provider = provider(&dir, &server, true, false);
    let output = provider.projects(None).unwrap();
    assert_eq!(output["projects"].as_array().unwrap().len(), 52);
    assert_eq!(output["pages"], 2);
    assert_eq!(output["complete"], true);
    assert_eq!(output["visibility"], "public_only");
    assert!(!output.to_string().contains("never-output"));
    let requests = server.finish();
    assert!(
        requests[0]
            .line
            .starts_with("GET /v1/mods/search?gameId=1&authorId=42&index=0&pageSize=50 ")
    );
    assert!(requests[1].line.contains("index=50&pageSize=50"));
    for request in requests {
        assert!(request.headers.contains("x-api-key: core-secret"));
        assert!(!request.headers.contains("upload-secret"));
    }
}

#[test]
fn public_search_rejects_incomplete_repeated_and_changing_pages() {
    for second in [
        json!({"data":[],"pagination":{"totalCount":2}}),
        json!({"data":[{"id":1}],"pagination":{"totalCount":2}}),
        json!({"data":[{"id":2}],"pagination":{"totalCount":3}}),
        json!({"data":[{"id":true}],"pagination":{"totalCount":2}}),
        json!({"data":[{"id":2}],"pagination":{"index":0,"totalCount":2}}),
    ] {
        let dir = TestDir::new();
        let server = Server::new(vec![
            Reply::Json(json!({"data":[{"id":1}],"pagination":{"totalCount":2}})),
            Reply::Json(second),
        ]);
        let provider = provider(&dir, &server, true, false);
        assert_eq!(
            provider.projects(Some(7)).unwrap_err().code,
            "publish.curseforge_data"
        );
        assert_eq!(server.finish().len(), 2);
    }
}

#[test]
fn child_upload_uses_upload_token_and_exact_multipart_without_public_listing() {
    let dir = TestDir::new();
    let archive = dir.zip();
    let archive_bytes = fs::read(&archive).unwrap();
    let server = Server::new(vec![Reply::Json(json!({"id":991}))]);
    let provider = provider(&dir, &server, false, true);
    let mut value = input(&archive);
    value["parent_file_id"] = json!(881);
    value["is_marked_for_manual_release"] = json!(false);
    value["relations"] = json!({"projects":[{"slug":"required-addon","type":"requiredDependency","project_id":333}]});
    let upload = parse(value, &dir);
    let result = provider.upload(&upload).unwrap();
    assert_eq!(result["file_id"], 991);
    assert_eq!(result["project_id"], 719);
    assert_eq!(result["verification"], "upload_response");
    assert_eq!(provider.upload(&upload).unwrap()["reused"], true);
    let requests = server.finish();
    assert_eq!(requests.len(), 1);
    let request = &requests[0];
    assert_eq!(request.line, "POST /api/projects/719/upload-file HTTP/1.1");
    assert!(request.headers.contains("x-api-token: upload-secret"));
    assert!(!request.headers.contains("x-api-key"));
    assert!(
        request
            .body
            .windows(archive_bytes.len())
            .any(|part| part == archive_bytes)
    );
    let body = String::from_utf8_lossy(&request.body);
    let start = body.find("\r\n\r\n").unwrap() + 4;
    let end = body[start..].find("\r\n--").unwrap() + start;
    let metadata: Value = serde_json::from_str(&body[start..end]).unwrap();
    assert_eq!(
        metadata,
        json!({"changelog":"Fix login failure","releaseType":"release","parentFileID":881,"isMarkedForManualRelease":false,
        "relations":{"projects":[{"slug":"required-addon","type":"requiredDependency","projectID":333}]}})
    );
    let receipt =
        fs::read_to_string(dir.0.join("publish/curseforge/719/active-upload.json")).unwrap();
    assert!(!receipt.contains("upload-secret"));
    assert!(!receipt.contains("Fix login failure"));
}

#[test]
fn game_version_selection_is_live_upload_options_and_failure_sends_no_mutation() {
    let dir = TestDir::new();
    let archive = dir.zip();
    let server = Server::new(vec![Reply::Json(
        json!([{"id":777,"name":"12.0.1","gameVersionTypeID":517,"slug":"12-0-1","token":"never-output"}]),
    )]);
    let provider = provider(&dir, &server, false, true);
    let mut value = input(&archive);
    value["game_versions"] = json!([778]);
    let upload = parse(value, &dir);
    assert_eq!(provider.upload(&upload).unwrap_err().code, "publish.input");
    assert!(
        !dir.0
            .join("publish/curseforge/719/active-upload.json")
            .exists()
    );
    let requests = server.finish();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].line, "GET /api/game/versions HTTP/1.1");
    assert!(requests[0].headers.contains("x-api-token: upload-secret"));
}

#[test]
fn lost_response_is_durable_and_resolved_only_by_original_remote_file_identity() {
    let dir = TestDir::new();
    let archive = dir.zip();
    let bytes = fs::read(&archive).unwrap();
    let sha1 = hex::encode(Sha1::digest(&bytes));
    let server = Server::new(vec![
        Reply::Drop,
        Reply::Json(
            json!({"data":{"id":991,"modId":719,"fileLength":bytes.len(),"hashes":[{"algo":1,"value":"0000000000000000000000000000000000000000"}]}}),
        ),
        Reply::Json(
            json!({"data":{"id":991,"modId":719,"fileLength":bytes.len(),"hashes":[{"algo":1,"value":sha1}]}}),
        ),
    ]);
    let provider = provider(&dir, &server, true, true);
    let upload = parse(input(&archive), &dir);
    let error = provider.upload(&upload).unwrap_err();
    assert_eq!(error.details["submission"], "uncertain");
    assert_eq!(error.details["replay_allowed"], false);
    let operation = error.details["operation_id"].as_str().unwrap().to_owned();
    assert_eq!(provider.upload(&upload).unwrap_err().exit_code, 6);
    let mut changed = input(&archive);
    changed["changelog"] = json!("changed after interrupted upload");
    assert_eq!(
        provider
            .upload(&parse(changed, &dir))
            .unwrap_err()
            .exit_code,
        6
    );
    assert_eq!(
        provider
            .verify_upload(719, &operation, 991)
            .unwrap_err()
            .code,
        "publish.upload_mismatch"
    );
    assert_eq!(provider.upload(&upload).unwrap_err().exit_code, 6);
    assert_eq!(
        provider.verify_upload(719, &operation, 991).unwrap()["verification"],
        "remote_file_identity"
    );
    assert_eq!(provider.upload(&upload).unwrap()["reused"], true);
    let reused = provider.upload(&upload).unwrap();
    assert_eq!(reused["verification"], "remote_file_identity");
    assert_eq!(reused["metadata_verification"], "not_checked");
    assert_eq!(
        Curseforge::upload_status(&dir.0, 719, &operation, &provider.control).unwrap()["submission"],
        "acknowledged"
    );
    assert_eq!(server.finish().len(), 3);
}

#[test]
fn remote_errors_do_not_echo_credentials_urls_or_author_payload() {
    let dir = TestDir::new();
    let archive = dir.zip();
    let server = Server::new(vec![Reply::Raw(
        "HTTP/1.1 403 Forbidden\r\nContent-Length: 71\r\nConnection: close\r\n\r\n{\"token\":\"upload-secret\",\"url\":\"https://oss.invalid/?signature=secret\"}",
    )]);
    let provider = provider(&dir, &server, false, true);
    let error = provider.upload(&parse(input(&archive), &dir)).unwrap_err();
    assert_eq!(error.code, "publish.http_status");
    assert_eq!(error.details["cause"]["http_status"], 403);
    assert_eq!(error.details["submission"], "rejected");
    let wire = serde_json::to_string(&error).unwrap();
    for value in [
        "upload-secret",
        "oss.invalid",
        "signature=secret",
        "Fix login failure",
    ] {
        assert!(!wire.contains(value));
    }
    assert_eq!(server.finish().len(), 1);
}

#[test]
fn connection_refusal_is_not_sent_and_an_explicit_later_submission_is_allowed() {
    let dir = TestDir::new();
    let archive = dir.zip();
    let closed = Server::new(vec![]);
    let mut provider = provider(&dir, &closed, false, true);
    assert!(closed.finish().is_empty());
    let upload = parse(input(&archive), &dir);
    let error = provider.upload(&upload).unwrap_err();
    assert_eq!(error.details["submission"], "not_sent");
    let operation = error.details["operation_id"].as_str().unwrap();
    assert_eq!(
        Curseforge::upload_status(&dir.0, 719, operation, &provider.control).unwrap()["submission"],
        "not_sent"
    );
    let connected = Server::new(vec![Reply::Json(json!({"id":991}))]);
    provider.http = Http::loopback(&provider.control, connected.url.clone());
    assert_eq!(provider.upload(&upload).unwrap()["file_id"], 991);
    assert_eq!(connected.finish().len(), 1);
}

#[test]
fn explicit_authentication_rejection_allows_a_corrected_submission_without_an_uncertain_replay() {
    let dir = TestDir::new();
    let archive = dir.zip();
    let server = Server::new(vec![
        Reply::Raw("HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"),
        Reply::Json(json!({"id":991})),
    ]);
    let mut provider = provider(&dir, &server, false, true);
    let upload = parse(input(&archive), &dir);
    let rejected = provider.upload(&upload).unwrap_err();
    let operation = rejected.details["operation_id"].as_str().unwrap();
    assert_eq!(rejected.details["submission"], "rejected");
    assert_eq!(
        Curseforge::upload_status(&dir.0, 719, operation, &provider.control).unwrap()["rejection_status"],
        403
    );
    provider.config.upload_token = Some("corrected-upload-token".into());
    assert_eq!(provider.upload(&upload).unwrap()["file_id"], 991);
    assert_eq!(provider.upload(&upload).unwrap()["reused"], true);
    let requests = server.finish();
    assert_eq!(requests.len(), 2);
    assert!(
        requests[1]
            .headers
            .contains("x-api-token: corrected-upload-token")
    );
}

#[test]
fn invalid_nested_and_parent_inputs_fail_before_opening_a_profile_or_network() {
    let dir = TestDir::new();
    let original = input(Path::new("plugin.zip"));
    let mut invalids = Vec::new();
    for field in ["project_id", "parent_file_id"] {
        let mut value = original.clone();
        value[field] = json!(0);
        invalids.push(value);
    }
    let mut child = original.clone();
    child["parent_file_id"] = json!(42);
    child["game_version_names"] = json!([]);
    invalids.push(child);
    let mut relation = original.clone();
    relation["relations"] =
        json!({"projects":[{"slug":"addon","type":"requiredDependency","token":"secret"}]});
    invalids.push(relation);
    let mut boolean = original.clone();
    boolean["game_versions"] = json!([true]);
    invalids.push(boolean);
    let mut null_relation = original.clone();
    null_relation["relations"] =
        json!({"projects":[{"slug":"addon","type":"requiredDependency","project_id":null}]});
    invalids.push(null_relation);
    let mut clear = original;
    clear["is_marked_for_manual_release"] = Value::Null;
    invalids.push(clear);
    for value in invalids {
        assert!(Upload::parse(&serde_json::to_vec(&value).unwrap(), &dir.0).is_err());
    }
}

#[test]
fn selected_version_ids_and_names_are_checked_before_the_exact_upload() {
    let dir = TestDir::new();
    let archive = dir.zip();
    let server = Server::new(vec![
        Reply::Json(
            json!([{"id":777,"name":"12.0.1","gameVersionTypeID":517,"token":"never-output"}]),
        ),
        Reply::Json(json!({"id":991})),
    ]);
    let provider = provider(&dir, &server, false, true);
    let mut value = input(&archive);
    value["game_versions"] = json!([777]);
    value["game_version_names"] = json!(["12.0.1"]);
    let output = provider.upload(&parse(value, &dir)).unwrap();
    assert_eq!(output["file_id"], 991);
    assert!(!output.to_string().contains("never-output"));
    let requests = server.finish();
    assert_eq!(requests[0].line, "GET /api/game/versions HTTP/1.1");
    assert_eq!(
        requests[1].line,
        "POST /api/projects/719/upload-file HTTP/1.1"
    );
    assert!(String::from_utf8_lossy(&requests[1].body).contains("\"gameVersions\":[777]"));
    assert!(
        String::from_utf8_lossy(&requests[1].body).contains("\"gameVersionNames\":[\"12.0.1\"]")
    );
}

#[test]
fn plan_and_local_status_leave_an_unused_profile_absent() {
    let dir = TestDir::new();
    let archive = dir.zip();
    let absent_home = dir.0.join("unused-home");
    let upload = parse(input(&archive), &dir);
    let control = OperationControl::default();
    let plan = upload.plan(&control).unwrap();
    assert_eq!(plan["planned"], true);
    assert_eq!(plan["remote_writes"], 0);
    assert_eq!(plan["archive"], "AuthorAddon.zip");
    assert_eq!(plan["metadata"]["releaseType"], "release");
    assert!(
        !plan["metadata"]
            .as_object()
            .unwrap()
            .contains_key("parentFileID")
    );
    assert!(Curseforge::upload_status(&absent_home, 719, &"a".repeat(64), &control).is_err());
    assert!(!absent_home.exists());
}
