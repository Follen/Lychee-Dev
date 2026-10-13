use lycheedev_core::{control::OperationControl, storage};
use lycheedev_delivery::release;
use serde_json::json;
use std::{
    fs,
    path::PathBuf,
    sync::{Arc, atomic::AtomicBool},
    time::Instant,
};
struct Temp(PathBuf);
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
#[test]
fn manifest_and_inventory_admission_are_bounded_before_reads_and_share_caller_control() {
    let temp = Temp(
        std::env::temp_dir().join(format!(
            "lycheedev-release-control-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        )),
    );
    fs::create_dir_all(&temp.0).unwrap();
    let manifest = temp.0.join("release.json");
    fs::File::create(&manifest)
        .unwrap()
        .set_len(128 << 20)
        .unwrap();
    assert_eq!(
        release::inspect_with_control(&temp.0, "3.1.2", &OperationControl::default())
            .unwrap_err()
            .exit_code,
        4
    );
    let bytes = b"fixed payload";
    let resources = ["addon/Lychee Dev.toc", "skill/SKILL.md"].map(|path| {
        let dest = temp.0.join("payload").join(path);
        fs::create_dir_all(dest.parent().unwrap()).unwrap();
        fs::write(dest, bytes).unwrap();
        json!({"path":path,"bytes":bytes.len(),"sha256":storage::sha256(bytes)})
    });
    storage::atomic_json(&manifest,&json!({"schema":"lycheedev.release.v1","version":"3.1.2","commit":"1".repeat(40),"binaries":{"windows-amd64":{"path":"native/windows-amd64/lycheedev.exe","bytes":1,"sha256":"2".repeat(64)}},"resources":resources})).unwrap();
    assert!(release::inspect_with_control(&temp.0, "3.1.2", &OperationControl::default()).is_ok());
    fs::write(temp.0.join("payload/skill/SKILL.md"), b"mutated bytes").unwrap();
    assert_eq!(
        release::inspect_with_control(&temp.0, "3.1.2", &OperationControl::default())
            .unwrap_err()
            .code,
        "delivery.invalid_payload"
    );
    let cancelled = OperationControl::new(Instant::now(), Arc::new(AtomicBool::new(true)));
    assert_eq!(
        release::inspect_with_control(&temp.0, "3.1.2", &cancelled)
            .unwrap_err()
            .exit_code,
        7
    );
    assert_eq!(
        release::inspect_with_control(
            &temp.0,
            "3.1.2",
            &OperationControl::new(Instant::now(), Arc::new(AtomicBool::new(false)))
        )
        .unwrap_err()
        .exit_code,
        5
    );
}
