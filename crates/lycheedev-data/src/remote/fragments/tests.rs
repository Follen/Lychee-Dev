use super::*;
use md5::{Digest, Md5};
use std::{
    collections::{BTreeMap, VecDeque},
    path::PathBuf,
    sync::{
        Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
};

#[derive(Default)]
struct Fetch {
    metadata: Mutex<BTreeMap<String, VecDeque<Result<Vec<u8>>>>>,
    ranges: Mutex<BTreeMap<String, Result<(Vec<u8>, i64)>>>,
    calls: Mutex<Vec<String>>,
    cancel_range: AtomicBool,
}
impl MetadataFetcher for Fetch {
    fn fetch(
        &self,
        url: &str,
        _: usize,
        control: &OperationControl,
        _: Option<&Budget>,
    ) -> Result<Vec<u8>> {
        control.check()?;
        self.calls.lock().unwrap().push(format!("metadata:{url}"));
        let mut map = self.metadata.lock().unwrap();
        let values = map
            .get_mut(url)
            .ok_or_else(|| fault("records.remote_http", 5, "metadata fixture absent"))?;
        if values.len() > 1 {
            values.pop_front().unwrap()
        } else {
            values.front().cloned().unwrap()
        }
    }
}
impl RangeFetcher for Fetch {
    fn fetch(
        &self,
        url: &str,
        request: RangeRequest,
        control: &OperationControl,
        _: Option<&Budget>,
    ) -> Result<RangeReply> {
        control.check()?;
        self.calls
            .lock()
            .unwrap()
            .push(format!("range:{url}:{}:{}", request.offset, request.length));
        if self.cancel_range.load(Ordering::Acquire) {
            control.tracked_cancel_handle()?.cancel();
            return Err(fault("records.remote_http", 5, "interrupted fixture body"));
        }
        let (bytes, object_bytes) = self
            .ranges
            .lock()
            .unwrap()
            .get(url)
            .cloned()
            .unwrap_or_else(|| Err(missing()))?;
        Ok(RangeReply {
            bytes,
            object_bytes,
        })
    }
}
impl Fetch {
    fn metadata(&self, url: String, bytes: Vec<u8>) {
        self.metadata
            .lock()
            .unwrap()
            .insert(url, VecDeque::from([Ok(bytes)]));
    }
    fn range(
        &self,
        route: &catalog::Distribution,
        host: usize,
        key: &str,
        result: Result<(Vec<u8>, i64)>,
    ) {
        self.ranges
            .lock()
            .unwrap()
            .insert(route.data_url(&route.hosts[host], key).unwrap(), result);
    }
}
struct Fixture {
    home: PathBuf,
    control: OperationControl,
    fetch: Fetch,
    pin: DataPin,
}
fn cdns(path: &str, hosts: &[String]) -> Vec<u8> {
    format!(
        "Name!STRING:0|Path!STRING:0|Hosts!STRING:0\ncn|{path}|{}\n",
        hosts.join(" ")
    )
    .into_bytes()
}
fn route(path: &str) -> catalog::Distribution {
    catalog::Distribution {
        path: path.into(),
        hosts: vec!["cdn.example.test".into()],
    }
}
impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let home = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../.tmp/remote-fragment-fixtures")
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
        let cdn = b"archives = 44444444444444444444444444444444\n".to_vec();
        let pin = DataPin {
            product: "retail".into(),
            region: "cn".into(),
            language: "zhCN".into(),
            full_build: "12.1.5.70000".into(),
            build_config: crate::casc::hex(&Md5::digest(&build)),
            cdn_config: crate::casc::hex(&Md5::digest(&cdn)),
            definition_commit: "a".repeat(40),
        };
        let fetch = Fetch::default();
        let route = route("tpr/old");
        fetch.metadata(
            cache::catalog_url("cn", "wow", "cdns").unwrap(),
            cdns(&route.path, &route.hosts),
        );
        fetch.metadata(
            route
                .config_url(&route.hosts[0], &pin.build_config)
                .unwrap(),
            build,
        );
        fetch.metadata(
            route.config_url(&route.hosts[0], &pin.cdn_config).unwrap(),
            cdn,
        );
        Self {
            home,
            control: OperationControl::default(),
            fetch,
            pin,
        }
    }
    fn prepare(&self, offline: bool) -> Fragments<'_> {
        Fragments::prepare_with(
            &self.home,
            &self.pin,
            offline,
            &self.control,
            None,
            &self.fetch,
            &self.fetch,
        )
        .unwrap()
        .0
    }
    fn store(&self) -> Store<'_> {
        Store {
            home: &self.home,
            control: &self.control,
            budget: None,
        }
    }
}
fn request() -> RangeRequest {
    RangeRequest {
        offset: 2,
        length: 4,
        total: 10,
    }
}
fn cache_key(path: &str, key: &str) -> String {
    format!(
        "cdn-range/{}",
        storage::sha256(format!("{path}/{key}/2/4").as_bytes())
    )
}

#[test]
fn fragment_cache_rehashes_exact_original_bytes_and_offline_does_no_network() {
    let f = Fixture::new();
    let key = "a".repeat(32);
    let mut reader = f.prepare(false);
    f.fetch
        .range(&reader.route.current, 0, &key, Ok((b"body".to_vec(), 10)));
    assert_eq!(reader.range(&key, request()).unwrap().bytes(), b"body");
    assert_eq!(
        (reader.requests, reader.transferred, reader.fragment_loads),
        (1, 4, 1)
    );
    let calls = f.fetch.calls.lock().unwrap().len();
    assert_eq!(reader.range(&key, request()).unwrap().bytes(), b"body");
    let mut offline = f.prepare(true);
    assert_eq!(offline.range(&key, request()).unwrap().bytes(), b"body");
    assert_eq!(f.fetch.calls.lock().unwrap().len(), calls);
    assert_eq!(offline.requests, 0);
    assert_eq!(
        offline.range(&"b".repeat(32), request()).unwrap_err().code,
        "records.remote_unavailable_offline"
    );
}

#[test]
fn corrupted_cached_fragment_never_falls_back_to_a_download() {
    let f = Fixture::new();
    let key = "a".repeat(32);
    let mut reader = f.prepare(false);
    f.fetch
        .range(&reader.route.current, 0, &key, Ok((b"body".to_vec(), 10)));
    reader.range(&key, request()).unwrap();
    let (saved, _) = f
        .store()
        .read::<SavedRange>(&cache_key("tpr/old", &key))
        .unwrap()
        .unwrap();
    let path = lycheedev_core::evidence::blob_path(&f.home, &saved.blob.sha256).unwrap();
    std::fs::write(path, b"bad!").unwrap();
    let calls = f.fetch.calls.lock().unwrap().len();
    assert_eq!(
        reader.range(&key, request()).unwrap_err().code,
        "vault.blob_integrity"
    );
    assert_eq!(f.fetch.calls.lock().unwrap().len(), calls);
}

#[test]
fn malformed_saved_range_is_terminal_before_network() {
    for payload in [
        serde_json::json!({"objectBytes":9,"blob":{"bytes":4}}),
        serde_json::json!({"objectBytes":10,"blob":{"bytes":-4}}),
        serde_json::json!({"objectBytes":1e2,"blob":null}),
    ] {
        let f = Fixture::new();
        let key = "a".repeat(32);
        let mut reader = f.prepare(false);
        f.store()
            .put(&cache_key("tpr/old", &key), &payload, None)
            .unwrap();
        let calls = f.fetch.calls.lock().unwrap().len();
        assert_eq!(
            reader.range(&key, request()).unwrap_err().code,
            "records.remote_range"
        );
        assert_eq!(f.fetch.calls.lock().unwrap().len(), calls);
    }
}

#[test]
fn failed_mirror_and_refresh_attempts_are_precharged_and_preserve_uncertainty() {
    let f = Fixture::new();
    let key = "a".repeat(32);
    let mut reader = f.prepare(false);
    Arc::make_mut(&mut reader.route.current)
        .hosts
        .push("second.example.test".into());
    f.fetch.metadata(
        cache::catalog_url("cn", "wow", "cdns").unwrap(),
        cdns("tpr/old", &reader.route.current.hosts),
    );
    f.fetch.range(
        &reader.route.current,
        0,
        &key,
        Err(fault("records.remote_http", 5, "failed mirror")),
    );
    f.fetch
        .range(&reader.route.current, 1, &key, Err(missing()));
    let error = reader.range(&key, request()).unwrap_err();
    assert_eq!(error.code, "records.remote_http");
    assert!(error.message.contains(&key));
    assert_eq!((reader.requests, reader.transferred), (4, 16));
    assert!(
        f.store()
            .read::<SavedRange>(&cache_key("tpr/old", &key))
            .unwrap()
            .is_none()
    );
}

#[test]
fn route_refresh_cannot_publish_new_path_bytes_beneath_old_path_lease() {
    let f = Fixture::new();
    let key = "a".repeat(32);
    let mut reader = f.prepare(false);
    let next = route("tpr/new");
    f.fetch.metadata(
        cache::catalog_url("cn", "wow", "cdns").unwrap(),
        cdns(&next.path, &next.hosts),
    );
    f.fetch.range(&next, 0, &key, Ok((b"body".to_vec(), 10)));
    assert_eq!(reader.range(&key, request()).unwrap().bytes(), b"body");
    for path in ["tpr/old", "tpr/new"] {
        assert!(
            f.store()
                .read::<SavedRange>(&cache_key(path, &key))
                .unwrap()
                .is_none()
        );
    }
    reader.range(&key, request()).unwrap();
    assert!(
        f.store()
            .read::<SavedRange>(&cache_key("tpr/new", &key))
            .unwrap()
            .is_some()
    );
    assert_eq!((reader.requests, reader.transferred), (3, 12));
    assert!(
        f.fetch
            .calls
            .lock()
            .unwrap()
            .iter()
            .all(|url| !url.contains("/versions"))
    );
}

#[test]
fn request_and_transfer_caps_stop_before_the_next_attempt_without_saturation() {
    let f = Fixture::new();
    let mut reader = f.prepare(false);
    let key = "a".repeat(32);
    let calls = f.fetch.calls.lock().unwrap().len();
    for (requests, transferred) in [(4096, 0), (0, (1 << 30) - 3), (0, (1 << 30) + 1)] {
        reader.requests = requests;
        reader.transferred = transferred;
        assert_eq!(
            reader.range(&key, request()).unwrap_err().code,
            "records.metadata_limit"
        );
        assert_eq!(
            (reader.requests, reader.transferred),
            (requests, transferred)
        );
    }
    assert_eq!(f.fetch.calls.lock().unwrap().len(), calls);
}

#[test]
fn invalid_range_reply_is_not_cached_or_retried_at_another_host() {
    for (bytes, total) in [(b"bad".to_vec(), 10), (b"body".to_vec(), 11)] {
        let f = Fixture::new();
        let key = "a".repeat(32);
        let mut reader = f.prepare(false);
        Arc::make_mut(&mut reader.route.current)
            .hosts
            .push("second.example.test".into());
        f.fetch
            .range(&reader.route.current, 0, &key, Ok((bytes, total)));
        assert_eq!(
            reader.range(&key, request()).unwrap_err().code,
            "records.remote_range"
        );
        assert_eq!(reader.requests, 1);
        assert!(
            f.store()
                .read::<SavedRange>(&cache_key("tpr/old", &key))
                .unwrap()
                .is_none()
        );
    }
}

#[test]
fn cancellation_keeps_attempt_error_and_releases_the_owned_fragment_lease() {
    let f = Fixture::new();
    let key = "a".repeat(32);
    let mut reader = f.prepare(false);
    f.fetch.cancel_range.store(true, Ordering::Release);
    let error = reader.range(&key, request()).unwrap_err();
    assert_eq!(error.code, "command.cancelled");
    assert!(
        serde_json::to_string(&error)
            .unwrap()
            .contains("interrupted fixture body")
    );
    assert_eq!((reader.requests, reader.transferred), (1, 4));
    let control = OperationControl::default();
    let store = Store {
        home: &f.home,
        control: &control,
        budget: None,
    };
    store
        .lease(&cache_key("tpr/old", &key))
        .unwrap()
        .close()
        .unwrap();
}

#[test]
fn prepare_keeps_exact_config_keys_when_a_stale_route_refreshes() {
    let f = Fixture::new();
    let old = route("tpr/old");
    let next = route("tpr/new");
    let locator = cache::catalog_url("cn", "wow", "cdns").unwrap();
    f.fetch.metadata.lock().unwrap().insert(
        locator,
        VecDeque::from([
            Ok(cdns(&old.path, &old.hosts)),
            Ok(cdns(&next.path, &next.hosts)),
        ]),
    );
    for key in [&f.pin.build_config, &f.pin.cdn_config] {
        let bytes = f
            .fetch
            .metadata
            .lock()
            .unwrap()
            .get(&old.config_url(&old.hosts[0], key).unwrap())
            .unwrap()
            .front()
            .cloned()
            .unwrap()
            .unwrap();
        f.fetch
            .metadata(next.config_url(&next.hosts[0], key).unwrap(), bytes);
    }
    f.fetch
        .metadata
        .lock()
        .unwrap()
        .remove(&old.config_url(&old.hosts[0], &f.pin.build_config).unwrap());
    let (reader, metadata) =
        Fragments::prepare_with(&f.home, &f.pin, false, &f.control, None, &f.fetch, &f.fetch)
            .unwrap();
    assert_eq!(reader.route.current.path, "tpr/new");
    assert_eq!(metadata.installed.build_config, f.pin.build_config);
    assert_eq!(metadata.installed.cdn_config, f.pin.cdn_config);
    assert!(
        f.fetch
            .calls
            .lock()
            .unwrap()
            .iter()
            .all(|url| !url.contains("/versions"))
    );
}

#[test]
fn after_metadata_open_exact_pin_validation_precedes_catalog_access() {
    let f = Fixture::new();
    let mut pin = f.pin.clone();
    pin.build_config.clear();
    pin.product = "unsupported".into();
    assert_eq!(
        Fragments::prepare_with(&f.home, &pin, false, &f.control, None, &f.fetch, &f.fetch)
            .err()
            .unwrap()
            .code,
        "selection.unresolved_data"
    );
    pin.build_config = f.pin.build_config.clone();
    assert_eq!(
        Fragments::prepare_with(&f.home, &pin, false, &f.control, None, &f.fetch, &f.fetch)
            .err()
            .unwrap()
            .code,
        "selection.unsupported_data_product"
    );
    assert!(f.fetch.calls.lock().unwrap().is_empty());
}

#[test]
fn availability_primary_with_cleanup_failure_cannot_authorize_retry() {
    let mut error = fault("records.remote_http", 5, "body failed");
    assert!(retryable(&error));
    error.details = serde_json::json!({"schema":"lycheedev.remote-cache-cleanup.v1","primary":error.clone(),"cleanup":fault("external.io",9,"lease close failed"),"control":null,"budget":null});
    assert!(!retryable(&error));
}

#[test]
fn real_loopback_strict_range_reaches_fragment_cache_and_rejects_full_object_200() {
    use std::{
        io::{Read, Write},
        net::TcpListener,
        time::{Duration, Instant},
    };
    struct LocalRange(String);
    impl RangeFetcher for LocalRange {
        fn fetch(
            &self,
            _: &str,
            request: RangeRequest,
            control: &OperationControl,
            budget: Option<&Budget>,
        ) -> Result<RangeReply> {
            transport::fetch_range(&self.0, request, control, budget)
        }
    }
    for status in [206, 200] {
        let f = Fixture::new();
        drop(f.prepare(false));
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let fetch = LocalRange(format!("http://{}/fixture", listener.local_addr().unwrap()));
        let budget = Budget::controlled(
            crate::resource::Cost::defaults(),
            f.control.deadline(),
            f.control.clone(),
        );
        std::thread::scope(|scope| {
            let server = scope.spawn(move || {
                let until = Instant::now() + Duration::from_secs(3);
                let mut stream = loop {
                    match listener.accept() {
                        Ok((stream, _)) => break stream,
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            assert!(Instant::now() < until, "bounded loopback accept expired");
                            std::thread::sleep(Duration::from_millis(2));
                        }
                        Err(error) => panic!("loopback accept: {error}"),
                    }
                };
                stream.set_read_timeout(Some(Duration::from_millis(100))).unwrap();
                stream.set_write_timeout(Some(Duration::from_millis(100))).unwrap();
                let mut request = Vec::new();
                let mut chunk = [0; 512];
                while !request.ends_with(b"\r\n\r\n") {
                    assert!(Instant::now() < until && request.len() < 8192);
                    match stream.read(&mut chunk) {
                        Ok(0) => panic!("request ended before headers"),
                        Ok(n) => request.extend_from_slice(&chunk[..n]),
                        Err(error) if matches!(error.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => (),
                        Err(error) => panic!("loopback request: {error}"),
                    }
                }
                let request = String::from_utf8(request).unwrap().to_ascii_lowercase();
                assert!(request.contains("\r\nrange: bytes=2-5\r\n"));
                assert!(request.contains("\r\naccept-encoding: identity\r\n"));
                stream.write_all(format!("HTTP/1.1 {status} Fixture\r\nContent-Range: bytes 2-5/10\r\nContent-Length: 4\r\nConnection: close\r\n\r\nbody").as_bytes()).unwrap();
            });
            let (mut reader, _) = Fragments::prepare_with(
                &f.home,
                &f.pin,
                false,
                &f.control,
                Some(&budget),
                &f.fetch,
                &fetch,
            )
            .unwrap();
            let key = "a".repeat(32);
            let result = reader.range(&key, request());
            server.join().unwrap();
            assert_eq!(budget.snapshot().unwrap().network_requests, 1);
            if status == 206 {
                assert_eq!(result.unwrap().bytes(), b"body");
                assert_eq!(budget.snapshot().unwrap().network_bytes, 4);
                reader.offline = true;
                assert_eq!(reader.range(&key, request()).unwrap().bytes(), b"body");
                assert_eq!(budget.snapshot().unwrap().network_requests, 1);
            } else {
                assert_eq!(result.unwrap_err().code, "records.remote_range");
                assert_eq!(budget.snapshot().unwrap().network_bytes, 0);
                assert!(
                    f.store()
                        .read::<SavedRange>(&cache_key("tpr/old", &key))
                        .unwrap()
                        .is_none()
                );
            }
        });
    }
}

#[test]
fn discarded_return_buffer_refunds_allocation_after_close_failure_or_post_close_stop() {
    for stop in [false, true] {
        let f = Fixture::new();
        let budget = Budget::controlled(
            crate::resource::Cost::defaults(),
            f.control.deadline(),
            f.control.clone(),
        );
        let (reader, _) = Fragments::prepare_with(
            &f.home,
            &f.pin,
            false,
            &f.control,
            Some(&budget),
            &f.fetch,
            &f.fetch,
        )
        .unwrap();
        let before = budget.snapshot().unwrap();
        budget
            .charge(crate::resource::Cost {
                network_requests: 1,
                ..Default::default()
            })
            .unwrap();
        budget.charge_observed_network_bytes(4).unwrap();
        let allocation = reader.admit(4).unwrap();
        assert_eq!(
            budget.snapshot().unwrap().retained_bytes,
            before.retained_bytes + 4
        );
        let cleanup = if stop {
            f.control.tracked_cancel_handle().unwrap().cancel();
            Ok(())
        } else {
            Err(fault("external.io", 5, "injected consuming close failure"))
        };
        let error = reader
            .finish_range(
                Ok((
                    RangeReply {
                        bytes: b"body".to_vec(),
                        object_bytes: 10,
                    },
                    allocation,
                )),
                cleanup,
            )
            .unwrap_err();
        assert_eq!(
            error.code,
            if stop {
                "command.cancelled"
            } else {
                "external.io"
            }
        );
        let after = budget.snapshot().unwrap();
        assert_eq!(after.retained_bytes, before.retained_bytes);
        assert_eq!(after.network_bytes, before.network_bytes + 4);
        assert_eq!(after.network_requests, before.network_requests + 1);
    }
}

#[test]
fn public_prepare_checks_stop_and_metadata_open_before_invalid_pin() {
    for stopped in [false, true] {
        let f = Fixture::new();
        let mut pin = f.pin.clone();
        pin.build_config.clear();
        if stopped {
            f.control.tracked_cancel_handle().unwrap().cancel();
        } else {
            let path = f.home.join("state/toolkit.sqlite");
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, b"not a SQLite database").unwrap();
        }
        let error =
            Fragments::prepare_with(&f.home, &pin, true, &f.control, None, &f.fetch, &f.fetch)
                .err()
                .unwrap();
        assert_eq!(
            error.code,
            if stopped {
                "command.cancelled"
            } else {
                "vault.metadata"
            }
        );
        assert!(f.fetch.calls.lock().unwrap().is_empty());
    }
}
#[test]
fn successful_fragment_owns_current_allocation_and_drop_refunds_only_retained() {
    let f = Fixture::new();
    let budget = Budget::controlled(
        crate::resource::Cost::defaults(),
        f.control.deadline(),
        f.control.clone(),
    );
    let (reader, _) = Fragments::prepare_with(
        &f.home,
        &f.pin,
        false,
        &f.control,
        Some(&budget),
        &f.fetch,
        &f.fetch,
    )
    .unwrap();
    let before = budget.snapshot().unwrap();
    budget.charge_observed_network_bytes(4).unwrap();
    let allocation = reader.admit(4).unwrap();
    let fragment = reader
        .finish_range(
            Ok((
                RangeReply {
                    bytes: b"body".to_vec(),
                    object_bytes: 10,
                },
                allocation,
            )),
            Ok(()),
        )
        .unwrap();
    assert_eq!(fragment.bytes(), b"body");
    assert_eq!(fragment.object_bytes(), 10);
    assert_eq!(
        budget.snapshot().unwrap().retained_bytes,
        before.retained_bytes + 4
    );
    drop(fragment);
    assert_eq!(
        budget.snapshot().unwrap().retained_bytes,
        before.retained_bytes
    );
    assert_eq!(
        budget.snapshot().unwrap().network_bytes,
        before.network_bytes + 4
    );
}

#[test]
fn prepared_build_and_cdn_configuration_share_one_owner_under_tight_remaining_budget() {
    fn setup() -> Fixture {
        let mut f = Fixture::new();
        let raw = format!(
            "archives = {}\nextra = {}\n",
            "4".repeat(32),
            "x ".repeat(10000)
        )
        .into_bytes();
        f.pin.cdn_config = crate::casc::hex(&Md5::digest(&raw));
        let route = route("tpr/old");
        f.fetch.metadata(
            route
                .config_url(&route.hosts[0], &f.pin.cdn_config)
                .unwrap(),
            raw,
        );
        f
    }
    let first = setup();
    let budget = Budget::controlled(
        crate::resource::Cost::defaults(),
        first.control.deadline(),
        first.control.clone(),
    );
    let (_, loaded) = Fragments::prepare_with(
        &first.home,
        &first.pin,
        false,
        &first.control,
        Some(&budget),
        &first.fetch,
        &first.fetch,
    )
    .unwrap();
    let needed = budget.snapshot().unwrap().retained_bytes;
    assert_eq!(loaded.cdn_document.fields["extra"].len(), 10000);
    let f = setup();
    let cap = needed + 4096;
    let budget = Budget::controlled(
        crate::resource::Cost {
            retained_bytes: cap,
            ..crate::resource::Cost::defaults()
        },
        f.control.deadline(),
        f.control.clone(),
    );
    let (reader, loaded) = Fragments::prepare_with(
        &f.home,
        &f.pin,
        false,
        &f.control,
        Some(&budget),
        &f.fetch,
        &f.fetch,
    )
    .unwrap();
    assert!(std::sync::Arc::ptr_eq(&reader.build, &loaded));
    assert_eq!(
        reader.build.cdn_document.raw.as_ptr(),
        loaded.cdn_document.raw.as_ptr()
    );
    assert!(
        cap - budget.snapshot().unwrap().retained_bytes
            < 10000 * std::mem::size_of::<String>() as u64
    );
}

#[test]
fn fragment_prepare_schema_wait_keeps_independent_budget_as_primary() {
    use std::time::{Duration, Instant};
    let f = Fixture::new();
    let guard = f.store().lease("metadata-schema").unwrap();
    let budget = Budget::new(
        crate::resource::Cost::defaults(),
        Instant::now() + Duration::from_millis(30),
        None,
    );
    let started = Instant::now();
    let error = Fragments::prepare_with(
        &f.home,
        &f.pin,
        false,
        &f.control,
        Some(&budget),
        &f.fetch,
        &f.fetch,
    )
    .err()
    .unwrap();
    assert_eq!(
        (error.code.as_str(), error.exit_code),
        ("records.query_resource_budget", 3)
    );
    assert!(started.elapsed() < Duration::from_millis(500));
    f.control.check().unwrap();
    assert!(f.fetch.calls.lock().unwrap().is_empty());
    assert!(!f.home.join("state/toolkit.sqlite").exists());
    guard.close().unwrap();
    f.store().lease("metadata-schema").unwrap().close().unwrap();
}

#[test]
fn completed_fragment_read_keeps_bytes_charged_after_independent_deadline_without_retry() {
    use std::time::{Duration, Instant};
    struct ExpiresAfterRead(AtomicU64);
    impl RangeFetcher for ExpiresAfterRead {
        fn fetch(
            &self,
            _: &str,
            request: RangeRequest,
            control: &OperationControl,
            budget: Option<&Budget>,
        ) -> Result<RangeReply> {
            control.check()?;
            let budget = budget.unwrap();
            budget.check()?;
            self.0.fetch_add(1, Ordering::Relaxed);
            budget.charge(crate::resource::Cost {
                network_requests: 1,
                ..Default::default()
            })?;
            let bytes = b"body".to_vec();
            assert_eq!(bytes.len() as i64, request.length);
            budget.charge_observed_network_bytes(bytes.len() as u64)?;
            std::thread::sleep(budget.deadline.saturating_duration_since(Instant::now()));
            Ok(RangeReply {
                bytes,
                object_bytes: 10,
            })
        }
    }
    let f = Fixture::new();
    let ranges = ExpiresAfterRead(AtomicU64::new(0));
    let budget;
    let (mut reader, _) =
        Fragments::prepare_with(&f.home, &f.pin, false, &f.control, None, &f.fetch, &ranges)
            .unwrap();
    budget = Budget::new(
        crate::resource::Cost::defaults(),
        Instant::now() + Duration::from_millis(30),
        None,
    );
    reader.store.budget = Some(&budget);
    let object = "a".repeat(32);
    let error = reader.range(&object, request()).unwrap_err();
    assert_eq!(
        (error.code.as_str(), error.exit_code),
        ("records.query_resource_budget", 3)
    );
    assert_eq!(ranges.0.load(Ordering::Relaxed), 1);
    assert_eq!((reader.requests, reader.transferred), (1, 4));
    assert_eq!(f.fetch.calls.lock().unwrap().len(), 3);
    let used = budget.snapshot().unwrap();
    assert_eq!(
        (
            used.network_requests,
            used.network_bytes,
            used.retained_bytes
        ),
        (1, 4, 0)
    );
    f.control.check().unwrap();
    let key = cache_key("tpr/old", &object);
    assert!(f.store().read::<SavedRange>(&key).unwrap().is_none());
    f.store().lease(&key).unwrap().close().unwrap();
}
