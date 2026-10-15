use super::*;
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::Duration,
};
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "lychee-publish-wait-{}-{}-{}",
            std::process::id(),
            storage::utc_now().replace(':', "_"),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        workspace::initialize(&path).unwrap();
        Self(path)
    }
    fn hold(&self, body: &[u8]) -> ExclusiveLock {
        ExclusiveLock::resource(
            &self.0.join("locks"),
            &format!("blob:{}", storage::sha256(body)),
        )
        .unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let result = fs::remove_dir_all(&self.0);
        if !std::thread::panicking() {
            result.unwrap();
        }
    }
}
#[test]
fn concurrent_same_digest_byte_publishers_wait_and_verify_one_winner() {
    let f = Fixture::new();
    let body = b"same-probe";
    let held = f.hold(body);
    let (started_tx, started_rx) = mpsc::channel();
    let (done_tx, done_rx) = mpsc::channel();
    std::thread::scope(|scope| {
        for _ in 0..2 {
            let tx = started_tx.clone();
            let done = done_tx.clone();
            let home = &f.0;
            scope.spawn(move || {
                ExclusiveLock::set_blob_wait_observer(move || tx.send(()).unwrap());
                done.send(publish_blob_bounded(
                    home,
                    body,
                    body.len(),
                    &OperationControl::default(),
                ))
                .unwrap();
            });
        }
        for _ in 0..2 {
            started_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        }
        assert!(matches!(done_rx.try_recv(), Err(mpsc::TryRecvError::Empty)));
        held.close().unwrap();
        let first = done_rx
            .recv_timeout(Duration::from_secs(5))
            .unwrap()
            .unwrap();
        let second = done_rx
            .recv_timeout(Duration::from_secs(5))
            .unwrap()
            .unwrap();
        assert_eq!(first, second);
        assert_eq!(first["sha256"], storage::sha256(body));
        assert_eq!(
            read_blob_bounded(&f.0, &first, body.len(), &OperationControl::default()).unwrap(),
            body
        );
        let path = blob_path(&f.0, first["sha256"].as_str().unwrap()).unwrap();
        assert_eq!(fs::read_dir(path.parent().unwrap()).unwrap().count(), 1);
    });
}
#[test]
fn waiting_byte_publisher_validates_corrupt_winner_without_replacing_it() {
    let f = Fixture::new();
    let body = b"wanted";
    let held = f.hold(body);
    let path = blob_path(&f.0, &storage::sha256(body)).unwrap();
    fs::create_dir(path.parent().unwrap()).unwrap();
    fs::write(&path, b"broken").unwrap();
    let (tx, rx) = mpsc::channel();
    std::thread::scope(|scope| {
        let home = &f.0;
        let worker = scope.spawn(move || {
            ExclusiveLock::set_blob_wait_observer(move || tx.send(()).unwrap());
            publish_blob_bounded(home, body, body.len(), &OperationControl::default())
        });
        rx.recv_timeout(Duration::from_secs(5)).unwrap();
        held.close().unwrap();
        assert_eq!(
            worker.join().unwrap().unwrap_err().code,
            "vault.blob_integrity"
        );
    });
    assert_eq!(fs::read(path).unwrap(), b"broken");
}
#[test]
fn waiting_byte_publisher_retains_original_invocation_or_budget_stop() {
    for original in [true, false] {
        let f = Fixture::new();
        let body = b"probe";
        let held = f.hold(body);
        let inv = OperationControl::default();
        let token = inv.tracked_cancel_handle().unwrap();
        let budget_stopped = Arc::new(AtomicBool::new(false));
        let worker_budget = budget_stopped.clone();
        let (tx, rx) = mpsc::channel();
        std::thread::scope(|scope| {
            let home = &f.0;
            let worker = scope.spawn(move || {
                ExclusiveLock::set_blob_wait_observer(move || tx.send(()).unwrap());
                publish_blob_bounded_with_check(home, body, body.len(), &inv, move || {
                    if worker_budget.load(Ordering::Acquire) {
                        Err(
                            ToolkitError::new("fixture.budget", "original budget stopped", 7)
                                .with_details(serde_json::json!({"owner":"same"})),
                        )
                    } else {
                        Ok(())
                    }
                })
            });
            rx.recv_timeout(Duration::from_secs(5)).unwrap();
            if original {
                token.cancel();
            } else {
                budget_stopped.store(true, Ordering::Release);
            }
            let error = worker.join().unwrap().unwrap_err();
            assert_eq!(
                error.code,
                if original {
                    "command.cancelled"
                } else {
                    "fixture.budget"
                }
            );
            if !original {
                assert_eq!(error.details["owner"], "same");
            }
            assert!(matches!(f.hold_result(body),Err(e) if e.code=="workspace.busy"));
            assert!(!blob_path(&f.0, &storage::sha256(body)).unwrap().exists());
            held.close().unwrap();
            f.hold(body).close().unwrap();
        });
    }
}
impl Fixture {
    fn hold_result(&self, body: &[u8]) -> Result<ExclusiveLock> {
        ExclusiveLock::resource(
            &self.0.join("locks"),
            &format!("blob:{}", storage::sha256(body)),
        )
    }
}
