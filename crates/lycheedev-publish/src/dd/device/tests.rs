use super::*;
use crate::test_support::TestDir;

fn facts(serials: Option<Vec<String>>) -> Facts {
    Facts {
        node: [0, 0xab, 0x0c, 0x21, 0xfe, 0x99],
        version: [10, 0, 26100],
        serials,
    }
}
#[test]
fn device_state_uses_original_serial_identity_and_reuses_it_without_querying_hardware_again() {
    let dir = TestDir::new();
    let control = OperationControl::default();
    let first = load_with(&dir.0, &control, |needed| {
        assert!(needed);
        Ok(facts(Some(vec![
            "  DISK-A".into(),
            "DISK-B  ".into(),
            "".into(),
        ])))
    })
    .unwrap();
    assert_eq!(first.client_no, "b8a6c1f18b0f6c48431045904e192340");
    assert_eq!(first.mac, "00-ab-0c-21-fe-99");
    assert_eq!(first.os_version, "10.0.26100");
    assert_eq!(first.bi_desc, json!({}));
    let path = dir.0.join("publish/dd/device.json");
    let original = fs::read(&path).unwrap();
    let second = load_with(&dir.0, &control, |needed| {
        assert!(!needed);
        Ok(Facts {
            node: [1, 2, 3, 4, 5, 6],
            version: [10, 0, 26200],
            serials: Some(vec!["different-hardware".into()]),
        })
    })
    .unwrap();
    assert_eq!(second.client_no, first.client_no);
    assert_eq!(fs::read(path).unwrap(), original);
    let text = String::from_utf8(original).unwrap();
    for secret in [
        "DISK-A",
        "DISK-B",
        "different-hardware",
        "00-ab-0c-21-fe-99",
    ] {
        assert!(!text.contains(secret));
    }
}
#[test]
fn unavailable_hardware_gets_a_saved_identity_and_never_a_shared_empty_serial_hash() {
    for serials in [None, Some(vec![]), Some(vec![" ".into()])] {
        let dir = TestDir::new();
        let control = OperationControl::default();
        let first = load_with(&dir.0, &control, |_| Ok(facts(serials))).unwrap();
        assert_eq!(first.client_no.len(), 32);
        assert_ne!(first.client_no, "d41d8cd98f00b204e9800998ecf8427e");
        let next = load_with(&dir.0, &control, |needed| {
            assert!(!needed);
            Ok(facts(None))
        })
        .unwrap();
        assert_eq!(next.client_no, first.client_no);
    }
}
#[test]
fn malformed_device_state_is_not_replaced_or_used_to_authorize_hardware_or_login_work() {
    let dir = TestDir::new();
    let directory = dir.0.join("publish/dd");
    fs::create_dir_all(&directory).unwrap();
    let path = directory.join("device.json");
    for text in [
        r#"{"schema":"lycheedev.publish.dd.device.v1","client_no":"private-corrupt-value"}"#,
        r#"{"schema":"lycheedev.publish.dd.device.v1","client_no":"0123456789ABCDEF0123456789ABCDEF"}"#,
        r#"{"schema":"lycheedev.publish.dd.device.v1","client_no":"0123456789abcdef0123456789abcdef","client_no":"private-corrupt-value"}"#,
        r#"{"schema":"wrong","client_no":"0123456789abcdef0123456789abcdef"}"#,
        r#"{"schema":"lycheedev.publish.dd.device.v1","client_no":"0123456789abcdef0123456789abcdef","extra":"private-corrupt-value"}"#,
    ] {
        fs::write(&path, text).unwrap();
        let error = load_with(&dir.0, &OperationControl::default(), |_| {
            panic!("invalid state must stop before OS/device work")
        })
        .err()
        .unwrap();
        assert_eq!(error.exit_code, 3);
        assert!(!error.to_string().contains("private-corrupt-value"));
        assert_eq!(fs::read(&path).unwrap(), text.as_bytes());
    }
}
#[test]
fn cancelled_device_start_leaves_home_untouched() {
    let dir = TestDir::new();
    let control = OperationControl::default();
    control.tracked_cancel_handle().unwrap().cancel();
    assert_eq!(
        load_with(&dir.0, &control, |_| panic!("cancelled"))
            .err()
            .unwrap()
            .exit_code,
        7
    );
    assert!(!dir.0.join("publish").exists());
}

#[test]
fn native_device_collection_uses_real_os_build_and_only_saves_the_derived_identity() {
    let dir = TestDir::new();
    let control = OperationControl::default();
    // Current-machine metadata and a task-owned temporary device record only.
    // No DD install, account database or vendor SDK is opened.
    let device = load(&dir.0, &control).unwrap();
    let system = lycheedev_windows::filesystem::system_directory().unwrap();
    let mut command = Command::new(system.join("WindowsPowerShell/v1.0/powershell.exe"));
    command.args(["-NoProfile", "-NonInteractive", "-Command", r#"$ErrorActionPreference='Stop'
$key=[Microsoft.Win32.Registry]::LocalMachine.OpenSubKey('SOFTWARE\Microsoft\Windows NT\CurrentVersion')
try { [Console]::Out.Write(('{0}.{1}.{2}' -f $key.GetValue('CurrentMajorVersionNumber'),$key.GetValue('CurrentMinorVersionNumber'),$key.GetValue('CurrentBuildNumber'))) } finally { $key.Dispose() }"#]).creation_flags(0x08000000);
    let independent = process::run(command, None, 1024, 4096, &control).unwrap();
    assert_eq!(independent.status, 0);
    assert_eq!(device.os_version.as_bytes(), independent.stdout);
    let saved: Value =
        serde_json::from_slice(&fs::read(dir.0.join("publish/dd/device.json")).unwrap()).unwrap();
    assert_eq!(saved.as_object().unwrap().len(), 2);
    assert_eq!(saved["schema"], SCHEMA);
    assert_eq!(saved["client_no"], device.client_no);
}
