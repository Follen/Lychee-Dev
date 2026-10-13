//! Hidden current-test-process fixtures. This is not game/client acceptance.
use lycheedev_live::{
    runtime_discovery::{
        Admission, Format, Hints, Limits, Mode, Request, Scope, Source, SourceError, Verified,
        Verifier,
    },
    runtime_windows::WindowsSource,
};
use lycheedev_windows::{Process, process::PrivateReadLimits};
use std::{
    marker::PhantomData,
    rc::Rc,
    sync::atomic::AtomicBool,
    time::{Duration, Instant},
};
use windows::{
    Win32::{Foundation::HWND, UI::WindowsAndMessaging::*},
    core::w,
};

struct HiddenWindow {
    hwnd: Option<HWND>,
    _thread: PhantomData<Rc<()>>,
}
impl HiddenWindow {
    fn new() -> Self {
        let hwnd = unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                w!("STATIC"),
                w!("Lychee read-only runtime fixture"),
                WINDOW_STYLE::default(),
                0,
                0,
                0,
                0,
                Some(HWND_MESSAGE),
                None,
                None,
                None,
            )
        }
        .unwrap();
        Self {
            hwnd: Some(hwnd),
            _thread: PhantomData,
        }
    }
    fn scope(&self) -> Scope {
        let process = Process::open(std::process::id()).unwrap();
        Scope {
            pid: process.identity().pid,
            created: process.identity().created,
            window: self.hwnd.unwrap().0 as u64,
            installation: [1; 32],
            client_channel: [2; 32],
            protocol: 1,
            runtime: Some([3; 16]),
        }
    }
    fn close(&mut self) {
        if let Some(hwnd) = self.hwnd.take() {
            unsafe {
                DestroyWindow(hwnd).unwrap();
            }
        }
    }
}
impl Drop for HiddenWindow {
    fn drop(&mut self) {
        if let Some(hwnd) = self.hwnd.take() {
            unsafe {
                let _ = DestroyWindow(hwnd);
            }
        }
    }
}

#[test]
fn held_source_reads_only_current_private_bytes_without_module_or_disk_gate() {
    let window = HiddenWindow::new();
    let scope = window.scope();
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut source =
        WindowsSource::open(scope.clone(), PrivateReadLimits::default(), deadline).unwrap();
    source.verify_identity(&scope, deadline).unwrap();
    let expected = b"private runtime fixture without EXE hash".to_vec();
    let address = expected.as_ptr() as u64;
    let region = source.query_region(address, deadline).unwrap().unwrap();
    assert!(region.private && region.committed && region.readable);
    assert!(region.span.start <= address && region.span.end > address);
    let before = source.usage();
    let mut bytes = vec![0; expected.len()];
    assert_eq!(
        source.read_private(address, &mut bytes, deadline).unwrap(),
        expected.len()
    );
    assert_eq!(bytes, expected);
    assert_eq!(source.usage().read_calls - before.read_calls, 1);
    assert_eq!(source.scope(), &scope);
}

#[test]
fn scope_comparison_includes_process_window_installation_channel_protocol_and_runtime() {
    let first = HiddenWindow::new();
    let second = HiddenWindow::new();
    let scope = first.scope();
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut source =
        WindowsSource::open(scope.clone(), PrivateReadLimits::default(), deadline).unwrap();
    for field in 0..7 {
        let mut changed = scope.clone();
        match field {
            0 => changed.pid += 1,
            1 => changed.created += 1,
            2 => changed.window = second.scope().window,
            3 => changed.installation = [8; 32],
            4 => changed.client_channel = [8; 32],
            5 => changed.protocol += 1,
            _ => changed.runtime = Some([8; 16]),
        }
        let before = source.usage();
        assert_eq!(
            source.verify_identity(&changed, deadline).unwrap_err(),
            SourceError::Fatal("memory.runtime_scope_changed".into())
        );
        assert_eq!(source.usage(), before);
    }
    source.verify_identity(&scope, deadline).unwrap();
}

#[test]
fn wrong_creation_identity_and_closed_window_are_fatal() {
    let mut window = HiddenWindow::new();
    let scope = window.scope();
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut wrong = scope.clone();
    wrong.created += 1;
    let result = WindowsSource::open(wrong, PrivateReadLimits::default(), deadline);
    assert_eq!(result.err().unwrap(), "memory.runtime_process_reused");
    let mut source =
        WindowsSource::open(scope.clone(), PrivateReadLimits::default(), deadline).unwrap();
    window.close();
    assert_eq!(
        source.verify_identity(&scope, deadline).unwrap_err(),
        SourceError::Fatal("window.closed".into())
    );
    let expected = [1u8; 32];
    let before = source.usage();
    let mut bytes = [0xa5; 32];
    assert_eq!(
        source
            .read_private(expected.as_ptr() as u64, &mut bytes, deadline)
            .unwrap_err(),
        SourceError::Fatal("window.closed".into())
    );
    assert_eq!(source.usage().read_calls, before.read_calls);
    assert_eq!(bytes, [0xa5; 32]);
}

#[test]
fn expired_received_deadline_performs_no_io() {
    let window = HiddenWindow::new();
    let scope = window.scope();
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut source =
        WindowsSource::open(scope.clone(), PrivateReadLimits::default(), deadline).unwrap();
    let before = source.usage();
    assert_eq!(
        source.verify_identity(&scope, Instant::now()).unwrap_err(),
        SourceError::Deadline
    );
    assert_eq!(source.usage(), before);
    let expected = [1u8; 32];
    let mut bytes = [0xa5; 32];
    assert_eq!(
        source
            .read_private(expected.as_ptr() as u64, &mut bytes, Instant::now())
            .unwrap_err(),
        SourceError::Deadline
    );
    assert_eq!(source.usage(), before);
    assert_eq!(bytes, [0xa5; 32]);
    assert_eq!(
        source
            .query_region(expected.as_ptr() as u64, Instant::now())
            .unwrap_err(),
        SourceError::Deadline
    );
    assert_eq!(source.usage(), before);
    // A later caller deadline is intersected with the source's frozen deadline.
    source
        .verify_identity(&scope, deadline + Duration::from_secs(10))
        .unwrap();
}

#[test]
fn os_query_budget_is_cumulative_across_source_operations() {
    let window = HiddenWindow::new();
    let scope = window.scope();
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut source = WindowsSource::open(
        scope,
        PrivateReadLimits {
            query_calls: 1,
            ..PrivateReadLimits::default()
        },
        deadline,
    )
    .unwrap();
    let expected = [1u8; 32];
    let at = expected.as_ptr() as u64;
    source.query_region(at, deadline).unwrap().unwrap();
    assert_eq!(
        source.query_region(at, deadline).unwrap_err(),
        SourceError::Fatal("memory.private_query_budget".into())
    );
    assert_eq!(source.usage().query_calls, 1);
    let mut bytes = [0; 32];
    assert_eq!(
        source.read_private(at, &mut bytes, deadline).unwrap_err(),
        SourceError::Fatal("memory.private_query_budget".into())
    );
    assert_eq!(source.usage().read_calls, 0);
}

#[test]
fn window_and_channel_domains_stay_distinct_in_same_process() {
    let first = HiddenWindow::new();
    let second = HiddenWindow::new();
    let scope = first.scope();
    let mut another = second.scope();
    another.client_channel = [7; 32];
    assert_eq!(scope.pid, another.pid);
    assert_eq!(scope.created, another.created);
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut a = WindowsSource::open(scope.clone(), PrivateReadLimits::default(), deadline).unwrap();
    let mut b =
        WindowsSource::open(another.clone(), PrivateReadLimits::default(), deadline).unwrap();
    assert!(a.verify_identity(&another, deadline).is_err());
    assert!(b.verify_identity(&scope, deadline).is_err());
    a.verify_identity(&scope, deadline).unwrap();
    b.verify_identity(&another, deadline).unwrap();
}

#[test]
fn later_received_deadline_cannot_extend_expired_source_lifetime() {
    let window = HiddenWindow::new();
    let scope = window.scope();
    let deadline = Instant::now() + Duration::from_millis(250);
    let mut source =
        WindowsSource::open(scope.clone(), PrivateReadLimits::default(), deadline).unwrap();
    std::thread::sleep(
        deadline.saturating_duration_since(Instant::now()) + Duration::from_millis(1),
    );
    let before = source.usage();
    let later = Instant::now() + Duration::from_secs(10);
    assert_eq!(
        source.verify_identity(&scope, later).unwrap_err(),
        SourceError::Deadline
    );
    assert_eq!(source.usage(), before);
}

// An exact-byte fixture oracle, deliberately not a production protocol/MAC.
struct FixtureVerifier {
    scope: Scope,
    bytes: Vec<u8>,
}
impl Verifier for FixtureVerifier {
    fn admit_header(
        &mut self,
        scope: &Scope,
        header: &[u8],
        _: Instant,
    ) -> Result<Option<Admission>, String> {
        Ok(
            (scope == &self.scope && header == &self.bytes[..16]).then_some(Admission::Small {
                total_bytes: self.bytes.len(),
            }),
        )
    }
    fn verify_record(
        &mut self,
        scope: &Scope,
        _: Admission,
        bytes: &[u8],
        _: Instant,
    ) -> Result<Option<Verified>, String> {
        Ok(
            (scope == &self.scope && bytes == self.bytes).then_some(Verified {
                identity: [42; 32],
                matches_query: true,
            }),
        )
    }
}
#[test]
fn actual_windows_hint_reader_calls_full_fixture_verification() {
    let window = HiddenWindow::new();
    let scope = window.scope();
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut source =
        WindowsSource::open(scope.clone(), PrivateReadLimits::default(), deadline).unwrap();
    let expected = b"TSTMEM01header00:body verified through the real read-only source".to_vec();
    let hints = Hints {
        scope: scope.clone(),
        addresses: vec![expected.as_ptr() as u64],
    };
    let mut verifier = FixtureVerifier {
        scope: scope.clone(),
        bytes: expected.clone(),
    };
    let cancel = AtomicBool::new(false);
    let mut request = Request::new(
        &mut source,
        &mut verifier,
        scope,
        Format {
            marker: b"TSTMEM01".to_vec(),
            marker_offset: 0,
            header_bytes: 16,
        },
        Limits::default(),
        deadline,
        &cancel,
    )
    .unwrap();
    let out = request.discover(Mode::First, Some(&hints));
    assert_eq!(out.stop, None);
    assert_eq!(out.records.len(), 1);
    assert_eq!(out.records[0].bytes, expected);
    assert!(out.coverage.hint_hit && !out.coverage.complete);
    assert_eq!(out.usage.read_calls, 2);
    drop(request);
    assert_eq!(source.usage().read_calls, 2);
}

#[test]
fn final_window_guard_budget_failure_does_not_publish_staged_private_bytes() {
    let window = HiddenWindow::new();
    let scope = window.scope();
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut source = WindowsSource::open(
        scope,
        PrivateReadLimits {
            os_call_units: 49,
            ..PrivateReadLimits::default()
        },
        deadline,
    )
    .unwrap();
    let expected = [0x57u8; 16];
    let mut actual = [0xa5u8; 16];
    // 23 units open + 12 entry window check + 14 private read = 49.
    // The RPM and mapping guards pass, then the exit window check is denied.
    assert_eq!(source.usage().os_call_units, 23);
    assert_eq!(
        source
            .read_private(expected.as_ptr() as u64, &mut actual, deadline)
            .unwrap_err(),
        SourceError::Fatal("memory.private_os_call_budget".into())
    );
    assert_eq!(source.usage().os_call_units, 49);
    assert_eq!(source.usage().read_calls, 1);
    assert_eq!(source.usage().query_calls, 2);
    assert_eq!(actual, [0xa5; 16]);
}

#[test]
fn byte_budget_preflight_rejects_before_staging_or_os_work() {
    let window = HiddenWindow::new();
    let scope = window.scope();
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut source = WindowsSource::open(
        scope,
        PrivateReadLimits {
            requested_bytes: 8,
            ..PrivateReadLimits::default()
        },
        deadline,
    )
    .unwrap();
    let before = source.usage();
    let expected = [0x57u8; 16];
    let mut actual = [0xa5u8; 16];
    assert_eq!(
        source
            .read_private(expected.as_ptr() as u64, &mut actual, deadline)
            .unwrap_err(),
        SourceError::Fatal("memory.private_byte_budget".into())
    );
    assert_eq!(source.usage(), before);
    assert_eq!(actual, [0xa5; 16]);
}
