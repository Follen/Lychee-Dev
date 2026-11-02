use super::*;
use crate::blte::EncodedRead;
use lycheedev_core::control::TrackedCancellation;

fn alive() -> OperationControl {
    OperationControl::uncancelable(Instant::now() + Duration::from_secs(30))
}
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "casc-scope-{}-{}-{}",
            std::process::id(),
            storage::utc_now().replace([':', '.'], "-"),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        let mut raw = vec![0; 30];
        raw.extend_from_slice(b"abcdef");
        fs::write(root.join("data.000"), raw).unwrap();
        Self(root)
    }
    fn reader<'a>(&self, control: &'a OperationControl, budget: &'a Budget) -> LocalReader<'a> {
        let path = self.0.join("data.000");
        LocalReader {
            file: Mutex::new(fs::File::open(&path).unwrap()),
            root: self.0.clone(),
            path,
            archive_bytes: 36,
            index: String::new(),
            span: Span {
                archive: 0,
                offset: 0,
                bytes: 36,
            },
            encoding_key: String::new(),
            header_sha256: String::new(),
            limits: Limits::default(),
            io_bytes: AtomicU64::new(0),
            io_reads: AtomicU64::new(0),
            cancel: None,
            budget: Some(budget),
            control: Some(control),
        }
    }
    fn cleanup(self) {
        fs::remove_dir_all(self.0).unwrap();
    }
}
#[test]
fn controlled_local_reader_keeps_ordered_invocation_and_closes_actual_file() {
    let f = Fixture::new();
    let control = alive();
    let budget = Budget::controlled(Cost::defaults(), control.deadline(), alive());
    let reader = f.reader(&control, &budget);
    let bytes = reader.read_encoded(0, 6).unwrap();
    assert_eq!(&*bytes, b"abcdef");
    drop(bytes);
    assert_eq!(reader.io_stats()["bytes"], 6);
    assert_eq!(budget.snapshot().unwrap().retained_bytes, 0);
    control.ordered_child(control.deadline()).unwrap();
    assert_eq!(control.stop_cause().unwrap(), None);
    reader.close_after(Ok(())).unwrap();
    f.cleanup();
}
#[test]
fn original_cancel_precedes_bad_extent_and_does_not_read_archive() {
    let f = Fixture::new();
    let stop = TrackedCancellation::new();
    let control = OperationControl::tracked(Instant::now() + Duration::from_secs(30), stop.clone());
    let budget = Budget::controlled(Cost::defaults(), control.deadline(), alive());
    let reader = f.reader(&control, &budget);
    stop.cancel();
    let error = reader.read_encoded(usize::MAX, 1).err().unwrap();
    assert_eq!(error.code, "command.cancelled");
    assert_eq!(reader.io_stats()["bytes"], 0);
    assert_eq!(
        reader.close_after(Err::<(), _>(error)).unwrap_err().code,
        "command.cancelled"
    );
    f.cleanup();
}
#[test]
fn independent_budget_deadline_keeps_query_error_with_live_original_control() {
    let f = Fixture::new();
    let control = alive();
    let budget = Budget::controlled(
        Cost::defaults(),
        Instant::now() - Duration::from_millis(1),
        alive(),
    );
    let reader = f.reader(&control, &budget);
    let error = reader.read_encoded(0, 1).err().unwrap();
    assert_eq!(
        (error.code.as_str(), error.exit_code),
        ("records.query_resource_budget", 3)
    );
    control.check().unwrap();
    reader.close_after(Err::<(), _>(error)).unwrap_err();
    f.cleanup();
}
#[test]
fn independent_budget_cancel_keeps_original_ordered_lineage() {
    let f = Fixture::new();
    let control = alive();
    let stop = TrackedCancellation::new();
    let budget_control = OperationControl::tracked(control.deadline(), stop.clone());
    let budget = Budget::controlled(Cost::defaults(), control.deadline(), budget_control);
    let reader = f.reader(&control, &budget);
    stop.cancel();
    assert!(reader.read_encoded(0, 1).is_err());
    control.check().unwrap();
    control.ordered_child(control.deadline()).unwrap();
    reader.close_after(Ok(())).unwrap_err();
    f.cleanup();
}
#[test]
fn equal_real_close_errors_remain_two_events_while_same_stop_is_observed_once() {
    let primary = fault("command.cancelled", 7, "stopped");
    let close = fault("test.close", 5, "same close failure");
    let error = finish::<()>(
        Err(primary.clone()),
        vec![close.clone(), close],
        vec![primary.clone(), primary],
    )
    .unwrap_err();
    assert_eq!(error.details["cleanup"].as_array().unwrap().len(), 2);
    assert_eq!(error.details["stops"].as_array().unwrap().len(), 0);
    assert_eq!(error.code, "command.cancelled");
}
#[test]
fn index_parser_callback_stops_before_raw_index_validation() {
    let error = Index::open_with_check(&[], 0, Limits::default(), None, || {
        Err(fault("test.independent", 3, "stopped"))
    })
    .err()
    .unwrap();
    assert_eq!(error.code, "test.independent");
}
