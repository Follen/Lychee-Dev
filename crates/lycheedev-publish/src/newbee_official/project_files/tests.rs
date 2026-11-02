use super::super::tests::{client, reply};
use super::*;
use crate::test_support::{Reply, Server, TestDir};
fn request(dir: &TestDir, resource: Resource, init: bool, fields: Value) -> TextProject {
    let mut value = fields;
    value["schema"] = json!(format!(
        "lycheedev.publish.v1.newbee.official.{}.{}",
        resource.key(),
        if init { "init" } else { "pull" }
    ));
    TextProject::parse(resource, init, &serde_json::to_vec(&value).unwrap(), &dir.0).unwrap()
}
#[test]
fn pulls_export_original_opaque_wa_and_html_text_without_echoing_them() {
    let dir = TestDir::new();
    let wa = request(&dir, Resource::Wa, false, json!({"id":92}));
    let cfg = request(&dir, Resource::Config, false, json!({"id":99}));
    let server = Server::new(vec![
        reply(
            json!({"wa_id":92,"version":"legacy/4","wa_str":"!WA:2!PRIVATE_OPAQUE","is_multi_string":true}),
        ),
        reply(json!({"config_id":99,"content_format":1,"content":"<h1>PRIVATE_HTML 中文</h1>"})),
    ]);
    let mut c = client(&dir, &server);
    let result = c.export_text_project(&wa).unwrap();
    assert_eq!(result["is_multi_string"], true);
    assert_eq!(result["size"], 20);
    assert!(!result.to_string().contains("PRIVATE_OPAQUE"));
    assert_eq!(
        fs::read_to_string(dir.0.join("wa_92_vlegacy_4.txt")).unwrap(),
        "!WA:2!PRIVATE_OPAQUE"
    );
    let result = c.export_text_project(&cfg).unwrap();
    assert_eq!(result["content_format"], 1);
    assert!(!result.to_string().contains("PRIVATE_HTML"));
    assert_eq!(
        fs::read_to_string(dir.0.join("uipack_99.html")).unwrap(),
        "<h1>PRIVATE_HTML 中文</h1>"
    );
    let requests = server.finish();
    assert_eq!(
        requests[0].line,
        "POST /creator/cli/v1/wa/pull_str HTTP/1.1"
    );
    assert_eq!(
        requests[1].line,
        "POST /creator/cli/v1/share_config/pull_text HTTP/1.1"
    );
}
#[test]
fn wa_initialization_exports_collection_content_description_and_exact_price_snapshot() {
    let dir = TestDir::new();
    let request = request(
        &dir,
        Resource::Wa,
        true,
        json!({"id":92,"dir":"new/project"}),
    );
    assert_eq!(request.plan().unwrap()["local_writes"], 0);
    assert!(!dir.0.join("new").exists());
    let server = Server::new(vec![
        Reply::Raw(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n{\"code\":1,\"data\":{\"wa_id\":92,\"name\":\"Collection\",\"intro\":\"Summary\",\"description\":\"PRIVATE_DESCRIPTION\",\"game_versions\":[\"12.0.0\"],\"visibility_text\":\"private\",\"price\":90071992547409.93,\"is_multi_string\":true}}",
        ),
        reply(
            json!({"wa_id":92,"version":9007199254740993u64,"wa_str":"PRIVATE_COLLECTION_STRINGS"}),
        ),
    ]);
    let mut c = client(&dir, &server);
    let result = c.export_text_project(&request).unwrap();
    assert_eq!(result["initialized"], true);
    assert_eq!(result["files"].as_array().unwrap().len(), 3);
    assert!(!result.to_string().contains("PRIVATE_"));
    let root = dir.0.join("new/project");
    assert_eq!(
        fs::read_to_string(root.join("wa_92_v9007199254740993.txt")).unwrap(),
        "PRIVATE_COLLECTION_STRINGS"
    );
    assert_eq!(
        fs::read_to_string(root.join("description.md")).unwrap(),
        "PRIVATE_DESCRIPTION"
    );
    let snapshot = fs::read_to_string(root.join("project.json")).unwrap();
    assert!(snapshot.contains("90071992547409.93"));
    let value: Value = serde_json::from_str(&snapshot).unwrap();
    assert_eq!(value["version"], "9007199254740993");
    assert_eq!(value["is_multi_string"], true);
    assert_eq!(c.export_text_project(&request).unwrap_err().exit_code, 4);
    assert_eq!(server.finish().len(), 2);
}
#[test]
fn configuration_initialization_keeps_original_format_backup_and_exact_role_identity() {
    let dir = TestDir::new();
    let request = request(&dir, Resource::Config, true, json!({"id":99,"dir":"ui"}));
    let server = Server::new(vec![
        reply(
            json!({"config_id":99,"title":"UI","intro":"Summary","visibility_text":"private","cloud_backup":{"backup_id":"9007199254740993","name":"Original","backup_time":123,"token":"PRIVATE_BACKUP_TOKEN"},"shared_role":{"role_id":"9007199254740995","account":"Account","server":"Realm","role":"Character","token":"PRIVATE_ROLE_TOKEN"}}),
        ),
        reply(json!({"config_id":99,"content_format":1,"content":"<b>PRIVATE_CONTENT</b>"})),
    ]);
    let result = client(&dir, &server).export_text_project(&request).unwrap();
    assert_eq!(result["content_format"], 1);
    let snapshot = fs::read_to_string(dir.0.join("ui/project.json")).unwrap();
    assert!(!snapshot.contains("PRIVATE_"));
    let snapshot: Value = serde_json::from_str(&snapshot).unwrap();
    assert_eq!(snapshot["content_format"], "html");
    assert_eq!(snapshot["cloud_backup"]["backup_id"], "9007199254740993");
    assert_eq!(snapshot["shared_role"]["role_id"], "9007199254740995");
    assert_eq!(
        fs::read_to_string(dir.0.join("ui/uipack_99.html")).unwrap(),
        "<b>PRIVATE_CONTENT</b>"
    );
    server.finish();
}
#[test]
fn local_selection_and_existing_files_require_explicit_overwrite_before_authentication() {
    let dir = TestDir::new();
    fs::write(dir.0.join("existing.md"), b"preserve original").unwrap();
    assert!(TextProject::parse(Resource::Config,false,br#"{"schema":"lycheedev.publish.v1.newbee.official.config.pull","id":99,"out":"existing.md"}"#,&dir.0).is_err());
    let pull = request(
        &dir,
        Resource::Config,
        false,
        json!({"id":99,"out":"existing.md","overwrite":true}),
    );
    assert_eq!(pull.plan().unwrap()["author_writes"], 0);
    let server = Server::new(vec![reply(
        json!({"config_id":99,"content_format":2,"content":null}),
    )]);
    client(&dir, &server).export_text_project(&pull).unwrap();
    assert_eq!(fs::read(dir.0.join("existing.md")).unwrap(), b"");
    server.finish();
    fs::create_dir(dir.0.join("not-empty")).unwrap();
    fs::write(dir.0.join("not-empty/user-data"), b"preserve").unwrap();
    assert!(TextProject::parse(Resource::Wa,true,br#"{"schema":"lycheedev.publish.v1.newbee.official.wa.init","id":92,"dir":"not-empty"}"#,&dir.0).is_err());
    assert_eq!(
        fs::read(dir.0.join("not-empty/user-data")).unwrap(),
        b"preserve"
    );
}
#[test]
fn wrong_resource_identity_and_unknown_formats_leave_no_local_export() {
    for (resource, init, fields, replies) in [
        (
            Resource::Config,
            false,
            json!({"id":99,"out":"out.md"}),
            vec![reply(
                json!({"config_id":98,"content_format":2,"content":"wrong"}),
            )],
        ),
        (
            Resource::Config,
            true,
            json!({"id":99,"dir":"out"}),
            vec![reply(json!({"config_id":98}))],
        ),
        (
            Resource::Config,
            false,
            json!({"id":99,"out":"out.md"}),
            vec![reply(
                json!({"config_id":99,"content_format":3,"content":"unknown"}),
            )],
        ),
    ] {
        let dir = TestDir::new();
        let request = request(&dir, resource, init, fields);
        let server = Server::new(replies);
        assert_eq!(
            client(&dir, &server)
                .export_text_project(&request)
                .unwrap_err()
                .exit_code,
            5
        );
        assert!(!dir.0.join("out.md").exists());
        assert!(!dir.0.join("out").exists());
        server.finish();
    }
}
