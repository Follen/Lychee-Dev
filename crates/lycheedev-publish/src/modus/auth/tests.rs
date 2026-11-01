use super::*;
use crate::{
    modus::leveldb::tests::{control, fixture},
    test_support::TestDir,
};
#[test]
fn creator_dpapi_and_main_current_store_keep_distinct_auth_headers_and_logout() {
    let d = TestDir::new();
    let creator = d.0.join("creator");
    fs::create_dir_all(&creator).unwrap();
    fs::write(
        creator.join("token.dat"),
        credentials::protect_current_user(b"creator-private", Some(b"ModUs.Creator.TokenStore.v1"))
            .unwrap(),
    )
    .unwrap();
    let s = Session::creator(&creator, &control()).unwrap();
    assert_eq!(s.authorization(), "Bearer creator-private");
    assert!(s.device.is_none());
    fs::write(creator.join("token.json"), br#"{"token":"old-fallback"}"#).unwrap();
    fs::write(creator.join("token.dat"), b"broken cipher").unwrap();
    assert!(Session::creator(&creator, &control()).is_err());
    fs::remove_file(creator.join("token.dat")).unwrap();
    fs::write(
        creator.join("token.json"),
        br#"{"accessToken":"bEaReR official-json"}"#,
    )
    .unwrap();
    assert_eq!(
        Session::creator(&creator, &control())
            .unwrap()
            .authorization(),
        "bEaReR official-json"
    );
    let main = d.0.join("main");
    fixture(
        &main,
        Some(r#"{"token":"main-private","nickname":"作者"}"#),
        Some(r#"{"deviceId":"device-private"}"#),
        Some(4),
    );
    let s = Session::main(&main, &control()).unwrap();
    assert_eq!(s.authorization(), "main-private");
    assert_eq!(s.device.as_deref(), Some("device-private"));
    assert_eq!(s.build(None).unwrap(), 4);
    assert_eq!(s.build(Some(0)).unwrap(), 0);
    fixture(&main, None, None, None);
    assert!(Session::main(&main, &control()).is_err());
    fixture(
        &main,
        Some(r#"{"wallet":{"token":"nested-other"}}"#),
        None,
        None,
    );
    assert!(Session::main(&main, &control()).is_err());
}
