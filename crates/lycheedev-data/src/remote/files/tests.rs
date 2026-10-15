use super::super::{cache::MetadataFetcher, fragments::RangeFetcher, transport::RangeReply};
use super::*;
use crate::{
    blte,
    resource::{Budget, Cost},
};
use lycheedev_core::{control::OperationControl, selection::DataPin, storage};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{Condvar, atomic::AtomicUsize},
    time::{Duration, Instant},
};

#[derive(Default)]
struct Backend {
    metadata: Mutex<BTreeMap<String, Vec<u8>>>,
    objects: Mutex<BTreeMap<String, Vec<u8>>>,
    errors: Mutex<BTreeMap<String, ToolkitError>>,
    calls: Mutex<Vec<(String, i64, i64)>>,
    active: AtomicUsize,
    peak: AtomicUsize,
    gate: Option<(Mutex<usize>, Condvar)>,
    cancel_after_gate: Option<lycheedev_core::control::TrackedCancellation>,
    panic_after_attempt: Option<String>,
}
struct Active<'a>(&'a AtomicUsize);
impl Drop for Active<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}
impl MetadataFetcher for Backend {
    fn fetch(
        &self,
        url: &str,
        _: usize,
        control: &OperationControl,
        budget: Option<&Budget>,
    ) -> Result<Vec<u8>> {
        control.check()?;
        if let Some(b) = budget {
            b.check()?;
        }
        self.metadata
            .lock()
            .unwrap()
            .get(url)
            .cloned()
            .ok_or_else(missing)
    }
}
impl RangeFetcher for Backend {
    fn fetch(
        &self,
        url: &str,
        request: RangeRequest,
        control: &OperationControl,
        budget: Option<&Budget>,
    ) -> Result<RangeReply> {
        control.check()?;
        if let Some(b) = budget {
            b.charge(Cost {
                network_requests: 1,
                ..Cost::default()
            })?;
        }
        let object = url.rsplit('/').next().unwrap().to_string();
        self.calls
            .lock()
            .unwrap()
            .push((object.clone(), request.offset, request.length));
        let active = self.active.fetch_add(1, Ordering::SeqCst) + 1;
        self.peak.fetch_max(active, Ordering::SeqCst);
        let _active = Active(&self.active);
        if let Some((gate, ready)) = &self.gate {
            if object.ends_with(".index") && request.offset == 0 && request.length == 1 {
                let mut count = gate.lock().unwrap();
                *count += 1;
                ready.notify_all();
                let until = Instant::now() + Duration::from_secs(2);
                while *count < 2 {
                    let wait = until
                        .checked_duration_since(Instant::now())
                        .ok_or_else(internal)?;
                    let (next, result) = ready.wait_timeout(count, wait).unwrap();
                    count = next;
                    if result.timed_out() && *count < 2 {
                        return Err(internal());
                    }
                }
                if let Some(cancel) = &self.cancel_after_gate {
                    cancel.cancel();
                }
                control.check()?;
                if let Some(b) = budget {
                    b.check()?;
                }
            }
        }
        if self.panic_after_attempt.as_ref() == Some(&object) {
            panic!("injected worker failure after recorded attempt");
        }
        if let Some(error) = self.errors.lock().unwrap().get(&object).cloned() {
            return Err(error);
        }
        let objects = self.objects.lock().unwrap();
        let raw = objects.get(&object).ok_or_else(missing)?;
        let start = request.offset as usize;
        let end = start + request.length as usize;
        let bytes = raw.get(start..end).ok_or_else(range)?.to_vec();
        if let Some(b) = budget {
            b.charge_observed_network_bytes(bytes.len() as u64)?;
        }
        Ok(RangeReply {
            bytes,
            object_bytes: raw.len() as i64,
        })
    }
}
struct Fixture {
    home: PathBuf,
    control: OperationControl,
    backend: Backend,
    pin: DataPin,
}
impl Fixture {
    fn new(fields: &str) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let home = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../.tmp/remote-reader-fixtures")
            .join(format!(
                "{}-{}-{}",
                std::process::id(),
                storage::utc_now().replace([':', '.'], "-"),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
        lycheedev_core::workspace::initialize(&home).unwrap();
        let build = format!(
            "root = {}\nencoding = {} {}\nencoding-size = 123 456\nbuild-uid = wow\n",
            "1".repeat(32),
            "2".repeat(32),
            "3".repeat(32)
        )
        .into_bytes();
        let cdn = fields.as_bytes().to_vec();
        let backend = Backend::default();
        let pin = DataPin {
            product: "retail".into(),
            region: "cn".into(),
            language: "zhCN".into(),
            full_build: "12.1.5.70000".into(),
            build_config: hash(&build),
            cdn_config: hash(&cdn),
            definition_commit: "a".repeat(40),
        };
        let route = catalog::Distribution {
            path: "tpr/test".into(),
            hosts: vec!["cdn.example.test".into()],
        };
        backend.metadata.lock().unwrap().insert(
            super::super::cache::catalog_url("cn", "wow", "cdns").unwrap(),
            b"Name!STRING:0|Path!STRING:0|Hosts!STRING:0\ncn|tpr/test|cdn.example.test\n".to_vec(),
        );
        for (key, raw) in [(&pin.build_config, build), (&pin.cdn_config, cdn)] {
            backend
                .metadata
                .lock()
                .unwrap()
                .insert(route.config_url(&route.hosts[0], key).unwrap(), raw);
        }
        Self {
            home,
            control: OperationControl::default(),
            backend,
            pin,
        }
    }
    fn files<'a>(&'a self, offline: bool, budget: Option<&'a Budget>, workers: usize) -> Files<'a> {
        let (fragments, _) = Fragments::prepare_with(
            &self.home,
            &self.pin,
            offline,
            &self.control,
            budget,
            &self.backend,
            &self.backend,
        )
        .unwrap();
        Files::new(fragments, workers).unwrap()
    }
    fn object(&self, key: &str, raw: Vec<u8>) {
        self.backend.objects.lock().unwrap().insert(key.into(), raw);
    }
}
fn hash(raw: &[u8]) -> String {
    crate::casc::hex(&Md5::digest(raw))
}
fn bare(payload: &[u8]) -> (String, Vec<u8>) {
    let mut raw = b"BLTE\0\0\0\0N".to_vec();
    raw.extend_from_slice(payload);
    (hash(&raw), raw)
}
fn framed(payload: &[u8]) -> (String, Vec<u8>) {
    let chunk = [b"N".as_slice(), payload].concat();
    let mut raw = b"BLTE".to_vec();
    raw.extend_from_slice(&36u32.to_be_bytes());
    raw.extend_from_slice(&[15, 0, 0, 1]);
    raw.extend_from_slice(&(chunk.len() as u32).to_be_bytes());
    raw.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    raw.extend_from_slice(&Md5::digest(&chunk));
    let key = hash(&raw);
    raw.extend(chunk);
    (key, raw)
}
fn index(key: &str, size: usize, offset: u32) -> (String, Vec<u8>) {
    let mut raw = vec![0; 1024 + 24 + 28];
    raw[..16].copy_from_slice(&crate::casc::key(key).unwrap());
    raw[16..20].copy_from_slice(&(size as u32).to_be_bytes());
    raw[20..24].copy_from_slice(&offset.to_be_bytes());
    raw[1024..1040].copy_from_slice(&crate::casc::key(key).unwrap());
    let digest = Md5::digest(&raw[..1024]);
    raw[1040..1048].copy_from_slice(&digest[..8]);
    let at = 1048;
    let digest = Md5::digest(&raw[1024..1048]);
    raw[at..at + 8].copy_from_slice(&digest[..8]);
    raw[at + 8] = 1;
    raw[at + 11] = 1;
    raw[at + 12] = 4;
    raw[at + 13] = 4;
    raw[at + 14] = 16;
    raw[at + 15] = 8;
    raw[at + 16..at + 20].copy_from_slice(&1u32.to_le_bytes());
    let mut check = [0; 20];
    check[..12].copy_from_slice(&raw[at + 8..at + 20]);
    raw[at + 20..].copy_from_slice(&Md5::digest(check)[..8]);
    (hash(&raw[at..]), raw)
}
#[test]
fn loose_headerless_ekey_location_streaming_ckey_and_owned_block_drop() {
    let f = Fixture::new("archives =\n");
    let payload = b"authenticated body";
    let (key, raw) = bare(payload);
    let size = raw.len();
    f.object(&key, raw.clone());
    let budget = Budget::controlled(Cost::defaults(), f.control.deadline(), f.control.clone());
    let files = f.files(false, Some(&budget), 2);
    let before = budget.snapshot().unwrap().retained_bytes;
    let mut object = files.open(&key, size as i64).unwrap();
    assert_eq!(object.encoded_size(), size);
    assert_eq!(
        budget.snapshot().unwrap().retained_bytes,
        before + size as u64
    );
    let mut out = Vec::new();
    object.read_to_end(&mut out).unwrap();
    assert_eq!(out, raw);
    object.rewind().unwrap();
    let keys = BTreeMap::new();
    let mut decoded = blte::StreamingReader::new(
        &mut object,
        size,
        payload.len(),
        Md5::digest(payload).into(),
        &keys,
        &budget,
    )
    .unwrap();
    let mut out = Vec::new();
    decoded.read_to_end(&mut out).unwrap();
    assert_eq!(out, payload);
    drop(decoded);
    let retained_with_block = budget.snapshot().unwrap().retained_bytes;
    drop(object);
    assert_eq!(
        budget.snapshot().unwrap().retained_bytes,
        retained_with_block - size as u64
    );
    assert!(
        files
            .store
            .read::<Location>(&format!("cdn-location/{}/{key}", f.pin.cdn_config))
            .unwrap()
            .is_some()
    );
    assert!(files.counters().unwrap().block_cache_hits > 0);
}
#[test]
fn framed_ekey_checks_header_and_leaves_chunk_and_ckey_to_decoder() {
    let f = Fixture::new("archives =\n");
    let (key, mut raw) = framed(b"body");
    let size = raw.len();
    *raw.last_mut().unwrap() ^= 1;
    f.object(&key, raw);
    let budget = Budget::controlled(Cost::defaults(), f.control.deadline(), f.control.clone());
    let files = f.files(false, Some(&budget), 1);
    let mut object = files.open(&key, size as i64).unwrap();
    let keys = BTreeMap::new();
    let mut decoded = blte::StreamingReader::new(
        &mut object,
        size,
        4,
        Md5::digest(b"body").into(),
        &keys,
        &budget,
    )
    .unwrap();
    let error = decoded.read_to_end(&mut Vec::new()).unwrap_err();
    assert_eq!(
        blte::stream_io_error(error).code,
        "container.integrity_mismatch"
    );
}
#[test]
fn corrupted_headerless_ekey_never_publishes_location() {
    let f = Fixture::new("archives =\n");
    let (key, mut raw) = bare(b"body");
    let size = raw.len();
    *raw.last_mut().unwrap() ^= 1;
    f.object(&key, raw);
    let files = f.files(false, None, 1);
    assert_eq!(
        files.open(&key, size as i64).err().unwrap().code,
        "container.integrity_mismatch"
    );
    assert!(
        files
            .store
            .read::<Location>(&format!("cdn-location/{}/{key}", f.pin.cdn_config))
            .unwrap()
            .is_none()
    );
}
#[test]
fn archive_index_footer_toc_page_span_and_ekey_produce_one_verified_section() {
    let (key, raw) = bare(b"archive object");
    let size = raw.len();
    let (archive, idx) = index(&key, size, 42);
    let f = Fixture::new(&format!("archives = {archive}\n"));
    f.object(&format!("{archive}.index"), idx);
    let mut body = vec![0; 42];
    body.extend_from_slice(&raw);
    f.object(&archive, body);
    let files = f.files(false, None, 1);
    let mut object = files.open(&key, size as i64).unwrap();
    let mut out = Vec::new();
    object.read_to_end(&mut out).unwrap();
    assert_eq!(out, raw);
    let (location, _) = files
        .store
        .read::<Location>(&format!("cdn-location/{}/{key}", f.pin.cdn_config))
        .unwrap()
        .unwrap();
    assert_eq!(location.object, archive);
    assert_eq!(location.offset, 42);
    assert_eq!(location.bytes, size as i64);
}
#[test]
fn parallel_archive_join_preserves_order_and_all_actual_attempts() {
    for bad_first in [true, false] {
        let (key, raw) = bare(b"body");
        let size = raw.len();
        let (a, mut ia) = index(&key, size, 0);
        let (b, mut ib) = index(&key, size, 1);
        if bad_first {
            *ia.last_mut().unwrap() ^= 1;
        } else {
            *ib.last_mut().unwrap() ^= 1;
        }
        let mut f = Fixture::new(&format!("archives = {a} {b}\n"));
        f.backend.gate = Some((Mutex::new(0), Condvar::new()));
        f.object(&format!("{a}.index"), ia);
        f.object(&format!("{b}.index"), ib);
        let budget = Budget::controlled(Cost::defaults(), f.control.deadline(), f.control.clone());
        let files = f.files(false, Some(&budget), 2);
        let result = files.locate(&key, size as i64);
        if bad_first {
            assert_eq!(result.unwrap_err().code, "records.configuration_integrity");
        } else {
            assert_eq!(result.unwrap().object, a);
        }
        assert_eq!(f.backend.active.load(Ordering::SeqCst), 0);
        assert_eq!(f.backend.peak.load(Ordering::SeqCst), 2);
        let counters = files.counters().unwrap();
        assert_eq!(
            counters.requests,
            budget.snapshot().unwrap().network_requests
        );
        assert_eq!(
            counters.requests,
            f.backend.calls.lock().unwrap().len() as u64
        );
        assert_eq!(counters.fragment_loads, 5);
    }
}
#[test]
fn offline_old_index_footer_toc_page_fragments_do_not_require_whole_prefetch() {
    let (key, raw) = bare(b"body");
    let (archive, idx) = index(&key, raw.len(), 0);
    let f = Fixture::new(&format!("archives = {archive}\n"));
    f.object(&format!("{archive}.index"), idx.clone());
    let files = f.files(false, None, 1);
    let object = format!("{archive}.index");
    for (offset, length) in [(0, 1), (1048, 28), (1024, 24), (0, 1024)] {
        drop(
            files
                .range(&object, offset, length, idx.len() as i64)
                .unwrap(),
        );
    }
    let calls = f.backend.calls.lock().unwrap().len();
    let offline = f.files(true, None, 8);
    let span = offline
        .lookup_index(
            &key,
            raw.len() as i64,
            &[archive.clone()],
            &archive,
            &archive,
            0,
        )
        .unwrap();
    assert_eq!(span.object, archive);
    assert_eq!(f.backend.calls.lock().unwrap().len(), calls);
}
#[test]
fn failed_block_replacement_keeps_old_owner_and_reuses_it_without_new_fragment() {
    let f = Fixture::new("archives =\n");
    let key = "a".repeat(32);
    f.object(&key, vec![7; 2 * BLOCK]);
    let budget = Budget::controlled(
        Cost {
            retained_bytes: BLOCK as u64 + 8192,
            ..Cost::defaults()
        },
        f.control.deadline(),
        f.control.clone(),
    );
    let files = f.files(false, Some(&budget), 1);
    let before = budget.snapshot().unwrap().retained_bytes;
    let source = files.source(key, (2 * BLOCK) as i64, false).unwrap();
    let mut byte = [0];
    source.read_at(&mut byte, 0).unwrap();
    let loads = files.counters().unwrap().fragment_loads;
    assert_eq!(
        source.read_at(&mut byte, BLOCK as i64).unwrap_err().code,
        "records.query_resource_budget"
    );
    assert_eq!(
        budget.snapshot().unwrap().retained_bytes,
        before + BLOCK as u64
    );
    source.read_at(&mut byte, 0).unwrap();
    assert_eq!(files.counters().unwrap().fragment_loads, loads + 1);
    drop(source);
    assert_eq!(budget.snapshot().unwrap().retained_bytes, before);
    assert_eq!(budget.snapshot().unwrap().network_bytes, BLOCK as u64);
}
#[test]
fn invalid_cached_span_is_terminal_before_any_object_lookup() {
    let f = Fixture::new("archives =\n");
    let (key, raw) = bare(b"body");
    let files = f.files(false, None, 1);
    files
        .store
        .put(
            &format!("cdn-location/{}/{key}", f.pin.cdn_config),
            &Location {
                object: key.clone(),
                offset: -1,
                bytes: raw.len() as i64,
            },
            None,
        )
        .unwrap();
    assert_eq!(
        files.open(&key, raw.len() as i64).err().unwrap().code,
        "records.remote_identity"
    );
    assert!(f.backend.calls.lock().unwrap().is_empty());
}
#[test]
fn loose_http_uncertainty_is_not_rewritten_as_missing_by_later_indexes() {
    let f = Fixture::new(&format!("archives = {}\n", "b".repeat(32)));
    let key = "a".repeat(32);
    f.backend.errors.lock().unwrap().insert(
        key.clone(),
        fault("records.remote_http", 5, "uncertain loose copy"),
    );
    let files = f.files(false, None, 1);
    assert_eq!(
        files.locate(&key, 20).unwrap_err().code,
        "records.remote_http"
    );
}
#[test]
fn location_go_duplicate_null_and_signed_shape_is_retained_until_consumption() {
    let location:Location=serde_json::from_str(r#"{"Object":"first","object":"last","OBJECT":null,"offset":-1,"Offset":2,"bytes":20,"Bytes":null,"extra":[]}"#).unwrap();
    assert_eq!(location.object, "last");
    assert_eq!(location.offset, 2);
    assert_eq!(location.bytes, 20);
    assert!(serde_json::from_str::<Location>(r#"{"bytes":1e0}"#).is_err());
}

#[test]
fn joined_archive_workers_keep_original_cancellation_and_spent_attempts() {
    for cancel_budget_only in [false, true] {
        let (key, raw) = bare(b"body");
        let (a, ia) = index(&key, raw.len(), 0);
        let (b, ib) = index(&key, raw.len(), 1);
        let mut f = Fixture::new(&format!("archives = {a} {b}\n"));
        let budget_control = if cancel_budget_only {
            OperationControl::default()
        } else {
            f.control.clone()
        };
        f.backend.gate = Some((Mutex::new(0), Condvar::new()));
        f.backend.cancel_after_gate = Some(budget_control.tracked_cancel_handle().unwrap());
        f.object(&format!("{a}.index"), ia);
        f.object(&format!("{b}.index"), ib);
        let budget =
            Budget::controlled(Cost::defaults(), budget_control.deadline(), budget_control);
        let files = f.files(false, Some(&budget), 2);
        let before = budget.snapshot().unwrap().retained_bytes;
        assert_eq!(
            files.locate(&key, raw.len() as i64).unwrap_err().code,
            "command.cancelled"
        );
        assert_eq!(f.backend.active.load(Ordering::SeqCst), 0);
        assert_eq!(f.backend.peak.load(Ordering::SeqCst), 2);
        let counts = files.counters().unwrap();
        assert_eq!(counts.requests, 4);
        assert_eq!(counts.requests, budget.snapshot().unwrap().network_requests);
        assert_eq!(
            counts.requests,
            f.backend.calls.lock().unwrap().len() as u64
        );
        assert_eq!(budget.snapshot().unwrap().retained_bytes, before);
        assert_eq!(f.control.check().is_ok(), cancel_budget_only);
    }
}
#[test]
fn worker_forks_share_build_and_route_values_but_refresh_is_local() {
    let f = Fixture::new("archives =\n");
    let files = f.files(false, None, 2);
    let parent = files.locked().unwrap();
    let mut worker = parent.fork_for_lookup();
    assert!(Arc::ptr_eq(&worker.build, &parent.build));
    assert!(Arc::ptr_eq(&worker.route.current, &parent.route.current));
    assert!(worker.route.refresh(&worker.store, &f.backend).unwrap());
    assert!(!Arc::ptr_eq(&worker.route.current, &parent.route.current));
    assert_eq!(worker.route.current.path, parent.route.current.path);
}
#[test]
fn sequential_read_returns_prefix_before_a_later_block_failure() {
    let f = Fixture::new("archives =\n");
    let key = "a".repeat(32);
    f.object(&key, vec![7; BLOCK + 1]);
    let budget = Budget::controlled(
        Cost {
            retained_bytes: BLOCK as u64 + 8192,
            ..Cost::defaults()
        },
        f.control.deadline(),
        f.control.clone(),
    );
    let files = f.files(false, Some(&budget), 1);
    let source = files
        .source(key.clone(), (BLOCK + 1) as i64, false)
        .unwrap();
    let mut encoded = Encoded {
        source,
        offset: 0,
        size: BLOCK + 1,
        position: 0,
    };
    let mut output = vec![0; BLOCK + 1];
    assert_eq!(encoded.read(&mut output).unwrap(), BLOCK);
    assert_eq!(&output[..BLOCK], vec![7; BLOCK]);
    f.backend.errors.lock().unwrap().insert(
        key,
        fault("records.remote_http", 5, "second block unavailable"),
    );
    assert_eq!(
        blte::stream_io_error(encoded.read(&mut output).unwrap_err()).code,
        "records.remote_http"
    );
    assert_eq!(encoded.position, BLOCK);
}

#[test]
fn joined_panicked_worker_preserves_its_and_later_workers_recorded_counters() {
    let (key, raw) = bare(b"body");
    let (a, ia) = index(&key, raw.len(), 0);
    let (b, ib) = index(&key, raw.len(), 1);
    let mut f = Fixture::new(&format!("archives = {a} {b}\n"));
    f.backend.gate = Some((Mutex::new(0), Condvar::new()));
    f.backend.panic_after_attempt = Some(format!("{a}.index"));
    f.object(&format!("{a}.index"), ia);
    f.object(&format!("{b}.index"), ib);
    let budget = Budget::controlled(Cost::defaults(), f.control.deadline(), f.control.clone());
    let files = f.files(false, Some(&budget), 2);
    let retained = budget.snapshot().unwrap().retained_bytes;
    assert_eq!(
        files.locate(&key, raw.len() as i64).unwrap_err().code,
        "records.internal"
    );
    assert_eq!(f.backend.active.load(Ordering::SeqCst), 0);
    assert_eq!(f.backend.peak.load(Ordering::SeqCst), 2);
    let counts = files.counters().unwrap();
    assert_eq!(counts.requests, 5);
    assert_eq!(counts.fragment_loads, 4);
    assert_eq!(counts.requests, budget.snapshot().unwrap().network_requests);
    let calls = f.backend.calls.lock().unwrap();
    assert_eq!(calls.len() as u64, counts.requests);
    assert_eq!(
        calls.iter().map(|(_, _, bytes)| bytes).sum::<i64>(),
        counts.transferred
    );
    assert_eq!(budget.snapshot().unwrap().retained_bytes, retained);
}
