use super::super::super::tests::{api as fixture_api, ok};
use super::*;
use crate::test_support::{Reply, Server, TestDir};
use std::{
    fs,
    time::{Duration, Instant},
};
fn control() -> OperationControl {
    OperationControl::uncancelable(Instant::now() + Duration::from_secs(30))
}
fn bundle(dir: &Path, known_only: bool, c: &OperationControl) -> PathBuf {
    let game = dir.join("game");
    fs::create_dir_all(game.join("Interface/AddOns/Custom")).unwrap();
    fs::create_dir_all(game.join("Fonts")).unwrap();
    fs::write(game.join("Interface/AddOns/Custom/Custom.toc"), "toc").unwrap();
    fs::write(game.join("Fonts/Font.ttf"), "font bytes").unwrap();
    let doc = json!({"schema":"lycheedev.publish.v1.blackbox.backup.generate","game_dir":game,"output_dir":dir.join("bundle"),"strappid":"wowretail","known_addons":[{"ID":719,"Name":"Known","Strategy":1,"VersionID":17}],"unknown_addons":if known_only{vec![]}else{vec!["Custom"]},"materials":[],"fonts":if known_only{vec![]}else{vec!["Font.ttf"]},"wtf_accounts":[]});
    let request = BackupGenerate::parse(&serde_json::to_vec(&doc).unwrap(), dir).unwrap();
    PathBuf::from(
        request.generate(c).unwrap()["bundle_file"]
            .as_str()
            .unwrap(),
    )
}
fn allocation(path: &str) -> Value {
    json!({"cos":"https://fixture-123.cos.ap-shanghai.myqcloud.com/","Credentials":{"Credentials":{"TmpSecretID":"fixtureKey","TmpSecretKey":"temporary-secret","Token":"temporary-token"}},"host":"wow.max-c.com","path":path})
}
#[test]
fn backup_unknown_put_and_lost_save_use_original_objects_allocation_and_bundle() {
    let dir = TestDir::new();
    let c = control();
    let path = bundle(&dir.0, false, &c);
    let mut b = Validated::open(&path, &c).unwrap();
    let first = Server::new(vec![
        ok(json!({"backupList":[]})),
        ok(json!({"ID":"allocated-original","MaxSize":5000000000u64})),
        ok(allocation("wow/backups/original-addons.zip")),
        Reply::Headers(vec![(
            "ETag".into(),
            format!("\"{}\"", b.spec.archives[0].md5),
        )]),
        ok(allocation("wow/backups/original-fonts.zip")),
        Reply::Drop,
    ]);
    let (api, mut session) = fixture_api(&first);
    let error = run(&dir.0, &api, &mut session, &mut b, None, &c).unwrap_err();
    assert_eq!(error.details["backup_id"], "allocated-original");
    let sent = first.finish();
    assert_eq!(
        sent.iter().filter(|r| r.line.starts_with("PUT ")).count(),
        2
    );
    assert_eq!(
        sent[3].body,
        fs::read(dir.0.join("bundle/unknown.zip")).unwrap()
    );
    assert_eq!(
        sent[5].body,
        fs::read(dir.0.join("bundle/fonts.zip")).unwrap()
    );
    assert!(sent[3].headers.contains("temporary-token"));
    assert!(
        !serde_json::to_string(&error)
            .unwrap()
            .contains("temporary-token")
    );
    let second = Server::new(vec![
        Reply::Bytes(fs::read(dir.0.join("bundle/fonts.zip")).unwrap()),
        Reply::Drop,
    ]);
    let (api, mut session) = fixture_api(&second);
    assert!(run(&dir.0, &api, &mut session, &mut b, None, &c).is_err());
    let sent = second.finish();
    assert!(
        sent[0]
            .line
            .starts_with("GET /wow/backups/original-fonts.zip")
    );
    assert!(sent[1].line.contains("save_user_configuration"));
    let url = reqwest::Url::parse(&format!(
        "https://local.invalid/?{}",
        String::from_utf8_lossy(&sent[1].body)
    ))
    .unwrap();
    let encoded = url
        .query_pairs()
        .find(|(k, _)| k == "configuration")
        .unwrap()
        .1
        .into_owned();
    let raw = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .unwrap();
    let configuration: Value = serde_json::from_slice(&raw).unwrap();
    assert_eq!(configuration["ID"], "allocated-original");
    assert_eq!(
        configuration["UnknownAddons"]["Path"],
        "wow/backups/original-addons.zip"
    );
    assert_eq!(
        configuration["Fonts"]["Path"],
        "wow/backups/original-fonts.zip"
    );
    let third = Server::new(vec![ok(json!({"backupList":[configuration]}))]);
    let (api, mut session) = fixture_api(&third);
    let result = run(&dir.0, &api, &mut session, &mut b, None, &c).unwrap();
    assert_eq!(result["backup_id"], "allocated-original");
    assert_eq!(result["metadata_verified"], true);
    assert_eq!(result["archive_bytes_verified"], false);
    assert_eq!(third.finish().len(), 1);
    assert_eq!(
        run(&dir.0, &api, &mut session, &mut b, None, &c).unwrap()["reused"],
        true
    );
    b.finish(Ok(()), &c).unwrap();
    assert_eq!(
        fs::read_dir(dir.0.join("publish/blackbox/checks"))
            .unwrap()
            .count(),
        0
    );
}
#[test]
fn unknown_allocation_needs_explicit_original_id_and_never_allocates_again() {
    let dir = TestDir::new();
    let c = control();
    let path = bundle(&dir.0, true, &c);
    let mut b = Validated::open(&path, &c).unwrap();
    let first = Server::new(vec![ok(json!({"backupList":[{"ID":"old"}]})), Reply::Drop]);
    let (api, mut session) = fixture_api(&first);
    assert!(run(&dir.0, &api, &mut session, &mut b, None, &c).is_err());
    assert_eq!(first.finish().len(), 2);
    assert_eq!(
        run(&dir.0, &api, &mut session, &mut b, None, &c)
            .unwrap_err()
            .exit_code,
        6
    );
    assert!(
        run(&dir.0, &api, &mut session, &mut b, Some("old"), &c)
            .unwrap_err()
            .message
            .contains("already present")
    );
    let mut original = b.spec.configuration.clone();
    original["ID"] = json!("original-new");
    let second = Server::new(vec![ok(json!({"backupList":[original]}))]);
    let (api, mut session) = fixture_api(&second);
    let result = run(&dir.0, &api, &mut session, &mut b, Some("original-new"), &c).unwrap();
    assert_eq!(
        result["verification_scope"],
        "explicit_original_backup_metadata"
    );
    assert_eq!(second.finish().len(), 1);
    b.finish(Ok(()), &c).unwrap();
}
#[test]
fn allocation_size_limit_keeps_original_id_and_sends_no_objects() {
    let dir = TestDir::new();
    let c = control();
    let path = bundle(&dir.0, false, &c);
    let mut b = Validated::open(&path, &c).unwrap();
    let server = Server::new(vec![
        ok(json!({"backupList":[]})),
        ok(json!({"ID":"too-small","MaxSize":1})),
    ]);
    let (api, mut session) = fixture_api(&server);
    let error = run(&dir.0, &api, &mut session, &mut b, None, &c).unwrap_err();
    assert_eq!(error.details["backup_id"], "too-small");
    assert_eq!(server.finish().len(), 2);
    assert_eq!(
        run(&dir.0, &api, &mut session, &mut b, None, &c)
            .unwrap_err()
            .details["backup_id"],
        "too-small"
    );
    b.finish(Ok(()), &c).unwrap();
}
