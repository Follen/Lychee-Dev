//! Synthetic disk bytes and this test executable/window only. No game or input.
use super::*;
use crate::{Process, WindowIdentity};
use std::{
    cell::{Cell, RefCell},
    fs::OpenOptions,
    marker::PhantomData,
    path::PathBuf,
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use windows::{
    Win32::{Foundation::HWND, UI::WindowsAndMessaging::*},
    core::w,
};

fn budget() -> (RuntimeBudget, Arc<AtomicBool>) {
    let cancel = Arc::new(AtomicBool::new(false));
    let budget = RuntimeBudget::new(
        RuntimeCost::defaults(),
        Instant::now() + Duration::from_secs(30),
        cancel.clone(),
    )
    .unwrap();
    (budget, cancel)
}
struct Bytes {
    bytes: Vec<u8>,
    calls: Cell<usize>,
    read_ranges: RefCell<Vec<(u64, usize)>>,
    maximum: usize,
    failure: Option<&'static str>,
    cancel: Option<Arc<AtomicBool>>,
}
impl Bytes {
    fn new(bytes: Vec<u8>) -> Self {
        Self {
            bytes,
            calls: Cell::new(0),
            read_ranges: RefCell::new(Vec::new()),
            maximum: usize::MAX,
            failure: None,
            cancel: None,
        }
    }
}
impl ImageRead for Bytes {
    fn read_at(&self, into: &mut [u8], offset: u64) -> Result<usize> {
        self.calls.set(self.calls.get() + 1);
        self.read_ranges.borrow_mut().push((offset, into.len()));
        if let Some(cancel) = &self.cancel {
            cancel.store(true, Ordering::Release);
        }
        if let Some(error) = self.failure {
            return Err(error.into());
        }
        let offset = usize::try_from(offset).unwrap();
        let got = self
            .bytes
            .len()
            .saturating_sub(offset)
            .min(into.len())
            .min(self.maximum);
        if got > 0 {
            into[..got].copy_from_slice(&self.bytes[offset..offset + got]);
        }
        Ok(got)
    }
}
fn put16(bytes: &mut [u8], at: usize, value: u16) {
    bytes[at..at + 2].copy_from_slice(&value.to_le_bytes());
}
fn put32(bytes: &mut [u8], at: usize, value: u32) {
    bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
}
const NT: usize = 0x80;
const OPT: usize = NT + 24;
const TABLE: usize = OPT + OPTIONAL64_BYTES;
const MODULE_SIZE: u64 = 0x8000;
fn pe() -> Vec<u8> {
    let mut bytes = vec![0; 0x400];
    bytes[..2].copy_from_slice(b"MZ");
    put32(&mut bytes, 0x3c, NT as u32);
    bytes[NT..NT + 4].copy_from_slice(b"PE\0\0");
    put16(&mut bytes, NT + 4, 0x8664);
    put16(&mut bytes, NT + 6, 2);
    put16(&mut bytes, NT + 20, OPTIONAL64_BYTES as u16);
    put16(&mut bytes, OPT, 0x20b);
    put32(&mut bytes, OPT + 56, MODULE_SIZE as u32);
    put32(&mut bytes, OPT + 60, 0x400);
    bytes[TABLE..TABLE + 8].copy_from_slice(b".text\0\0\0");
    put32(&mut bytes, TABLE + 8, 0x2000);
    put32(&mut bytes, TABLE + 12, 0x2000);
    put32(&mut bytes, TABLE + 36, READ | EXECUTE);
    bytes[TABLE + 40..TABLE + 48].copy_from_slice(b".data\0\0\0");
    put32(&mut bytes, TABLE + 48, 0x1000);
    put32(&mut bytes, TABLE + 52, 0x5000);
    put32(&mut bytes, TABLE + 76, READ | 0x8000_0000);
    bytes
}
fn check_runtime(parsed: &ParsedImage, runtime: &[u8], budget: &mut RuntimeBudget) -> Result<()> {
    verify_guards(&parsed.guards, budget, |rva, into, budget| {
        budget.charge(RuntimeCost::read(into.len()))?;
        let at = rva as usize;
        into.copy_from_slice(
            runtime
                .get(at..at + into.len())
                .ok_or("fixture.runtime_short_read")?,
        );
        budget.check()
    })
}

#[test]
fn fixed_pe_layout_and_exact_seven_locating_guards() {
    let reader = Bytes::new(pe());
    let (mut budget, _) = budget();
    let parsed = parse_image(&reader, MODULE_SIZE, &mut budget).unwrap();
    assert_eq!(parsed.sections.len(), 2);
    assert_eq!(parsed.text_index, 0);
    assert_eq!(parsed.sections[0].start(), 0x2000);
    assert_eq!(parsed.sections[0].end(), 0x4000);
    assert_eq!(parsed.sections[0].flags(), READ | EXECUTE);
    assert!(parsed.sections[0].is_text());
    assert_eq!(
        parsed
            .guards
            .iter()
            .map(|g| (g.rva(), g.bytes().len()))
            .collect::<Vec<_>>(),
        vec![
            (0, 2),
            (0x3c, 4),
            (NT as u64, 4),
            ((NT + 6) as u64, 2),
            ((NT + 20) as u64, 2),
            (OPT as u64, 2),
            (TABLE as u64, 80)
        ]
    );
    assert_eq!(reader.calls.get(), 5);
    assert!(budget.usage().allocation_bytes > 0 && budget.usage().work_units > 0);
}

#[test]
fn unrelated_disk_coff_symbols_raw_offsets_and_directories_are_never_followed() {
    let mut bytes = pe();
    put32(&mut bytes, NT + 12, u32::MAX);
    put32(&mut bytes, NT + 16, 100_000);
    put32(&mut bytes, TABLE + 20, u32::MAX);
    put32(&mut bytes, OPT + 112, u32::MAX);
    let reader = Bytes::new(bytes.clone());
    let (mut budget, _) = budget();
    let parsed = parse_image(&reader, MODULE_SIZE, &mut budget).unwrap();
    assert_eq!(parsed.sections[0].start(), 0x2000);
    assert!(
        reader
            .read_ranges
            .borrow()
            .iter()
            .all(|(offset, n)| offset + *n as u64 <= 0x400)
    );
    assert_eq!(reader.bytes, bytes);
}

#[test]
fn transformed_unrelated_runtime_machine_and_optional_fields_are_not_authority() {
    let reader = Bytes::new(pe());
    let (mut budget, _) = budget();
    let parsed = parse_image(&reader, MODULE_SIZE, &mut budget).unwrap();
    let mut runtime = pe();
    put16(&mut runtime, NT + 4, 0x200);
    put32(&mut runtime, OPT + 4, u32::MAX);
    put32(&mut runtime, OPT + 56, 0x12345678);
    let unchanged = runtime.clone();
    check_runtime(&parsed, &runtime, &mut budget).unwrap();
    assert_eq!(runtime, unchanged);
}

#[test]
fn every_runtime_locating_group_must_match_issued_disk_bytes() {
    let disk = Bytes::new(pe());
    let (mut budget, _) = budget();
    let parsed = parse_image(&disk, MODULE_SIZE, &mut budget).unwrap();
    for offset in [0, 0x3c, NT, NT + 6, NT + 20, OPT, TABLE + 36] {
        let mut runtime = pe();
        runtime[offset] ^= 1;
        assert_eq!(
            check_runtime(&parsed, &runtime, &mut budget).unwrap_err(),
            "memory.lua_mailbox_root_unsupported: runtime_image_metadata",
            "offset {offset}"
        );
    }
}

#[test]
fn malformed_fixed_disk_headers_never_issue_a_layout() {
    for mode in [
        "dos_truncated",
        "coff_truncated",
        "optional_truncated",
        "sections_truncated",
        "wrong_machine",
        "wrong_magic",
        "zero_sections",
        "too_many_sections",
        "optional_size",
        "header_budget",
        "bad_text",
        "duplicate_text",
        "missing_text",
        "overlap",
        "wrong_image_size",
        "zero_headers",
        "headers_outside_image",
        "table_outside_headers",
        "zero_section",
        "section_outside_image",
    ] {
        let mut bytes = pe();
        match mode {
            "dos_truncated" => bytes.truncate(63),
            "coff_truncated" => bytes.truncate(NT + 12),
            "optional_truncated" => bytes.truncate(OPT + 100),
            "sections_truncated" => bytes.truncate(TABLE + 79),
            "wrong_machine" => put16(&mut bytes, NT + 4, 0x200),
            "wrong_magic" => put16(&mut bytes, OPT, 0x10b),
            "zero_sections" => put16(&mut bytes, NT + 6, 0),
            "too_many_sections" => put16(&mut bytes, NT + 6, 97),
            "optional_size" => put16(&mut bytes, NT + 20, 112),
            "header_budget" => put32(&mut bytes, 0x3c, (HEADER_LIMIT + 1) as u32),
            "bad_text" => put32(&mut bytes, TABLE + 36, READ),
            "duplicate_text" => bytes[TABLE + 40..TABLE + 48].copy_from_slice(b".text\0\0\0"),
            "missing_text" => bytes[TABLE] = b'x',
            "overlap" => put32(&mut bytes, TABLE + 52, 0x3000),
            "wrong_image_size" => put32(&mut bytes, OPT + 56, 0x7000),
            "zero_headers" => put32(&mut bytes, OPT + 60, 0),
            "headers_outside_image" => put32(&mut bytes, OPT + 60, 0x9000),
            "table_outside_headers" => put32(&mut bytes, OPT + 60, (TABLE + 79) as u32),
            "zero_section" => put32(&mut bytes, TABLE + 8, 0),
            "section_outside_image" => put32(&mut bytes, TABLE + 8, 0x7000),
            _ => unreachable!(),
        }
        let reader = Bytes::new(bytes);
        let (mut budget, _) = budget();
        assert!(
            parse_image(&reader, MODULE_SIZE, &mut budget).is_err(),
            "{mode}"
        );
        assert!(budget.usage().requested_bytes <= HEADER_LIMIT, "{mode}");
        assert!(reader.calls.get() <= 6, "{mode}");
    }
}

#[test]
fn disk_pe_requires_exact_os_image_size_and_bounded_text() {
    for size in [0, MODULE_SIZE + 4096, EXECUTABLE_LIMIT + 1, u64::MAX] {
        let (mut budget, _) = budget();
        assert!(parse_image(&Bytes::new(pe()), size, &mut budget).is_err());
    }
    let mut bytes = pe();
    let size = TEXT_LIMIT + 0x3000;
    put32(&mut bytes, OPT + 56, size as u32);
    put16(&mut bytes, NT + 6, 1);
    put32(&mut bytes, TABLE + 8, (TEXT_LIMIT + 1) as u32);
    let (mut budget, _) = budget();
    assert!(parse_image(&Bytes::new(bytes), size, &mut budget).is_err());
}

#[test]
fn complete_hash_accounts_overlay_and_short_reads_without_hashing_disk_code_as_runtime() {
    let mut bytes = pe();
    bytes.resize(CHUNK + 273, 0x5a);
    let expected = format!("{:x}", Sha256::digest(&bytes));
    let mut reader = Bytes::new(bytes);
    reader.maximum = 4093;
    let (mut budget, _) = budget();
    assert_eq!(
        hash_image(&reader, reader.bytes.len() as u64, &mut budget).unwrap(),
        expected
    );
    assert!(reader.calls.get() > 2);
    assert_eq!(
        reader.read_ranges.borrow().last().unwrap(),
        &(reader.bytes.len() as u64, 1)
    );
    assert!(budget.usage().requested_bytes >= reader.bytes.len() as u64 + 1);
}

#[test]
fn hash_rejects_empty_truncated_growing_and_no_progress_sources() {
    for (data, announced, maximum) in [
        (vec![], 0, usize::MAX),
        (vec![1], 2, usize::MAX),
        (vec![1, 2], 1, usize::MAX),
        (vec![1], 1, 0),
    ] {
        let mut reader = Bytes::new(data);
        reader.maximum = maximum;
        let (mut budget, _) = budget();
        assert!(hash_image(&reader, announced, &mut budget).is_err());
    }
}

#[test]
fn cancellation_and_exhausted_limits_prevent_io_and_do_not_replace_control() {
    for kind in ["cancel", "bytes", "reads", "allocation", "work", "os"] {
        let cancel = Arc::new(AtomicBool::new(kind == "cancel"));
        let mut limits = RuntimeCost::defaults();
        match kind {
            "bytes" => limits.requested_bytes = 0,
            "reads" => limits.read_calls = 0,
            "allocation" => limits.allocation_bytes = 0,
            "work" => limits.work_units = 0,
            "os" => limits.os_call_units = 0,
            _ => (),
        }
        let mut budget =
            RuntimeBudget::new(limits, Instant::now() + Duration::from_secs(10), cancel).unwrap();
        let reader = Bytes::new(vec![1]);
        assert!(hash_image(&reader, 1, &mut budget).is_err(), "{kind}");
        assert_eq!(reader.calls.get(), 0, "{kind}");
    }
}

#[test]
fn io_and_control_errors_survive_together_and_success_after_cancel_is_rejected() {
    for fail in [false, true] {
        let (mut budget, cancel) = budget();
        let mut reader = Bytes::new(vec![1]);
        reader.cancel = Some(cancel);
        if fail {
            reader.failure = Some("fixture.read_failure");
        }
        let error = hash_image(&reader, 1, &mut budget).unwrap_err();
        assert!(error.contains("memory.runtime_cancelled"));
        assert_eq!(error.contains("fixture.read_failure"), fail);
        assert_eq!(reader.calls.get(), 1);
    }
}

#[test]
fn primary_close_and_control_failures_are_all_retained() {
    assert_eq!(
        finish_with_close::<()>(
            Err("primary".into()),
            Err("close".into()),
            Err("cancel".into())
        )
        .unwrap_err(),
        "primary; close; cancel"
    );
    assert_eq!(
        finish_with_close(Ok(7), Err("close".into()), Ok(())).unwrap_err(),
        "close"
    );
    assert_eq!(
        finish_with_close(Ok(7), Ok(()), Err("cancel".into())).unwrap_err(),
        "cancel"
    );
    assert_eq!(finish_with_close(Ok(7), Ok(()), Ok(())).unwrap(), 7);
}

struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "lycheedev-runtime-image-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn path(&self) -> PathBuf {
        self.0.join("owned.exe")
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn wide(path: &std::path::Path) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    path.as_os_str().encode_wide().collect()
}
#[test]
fn owned_regular_file_is_locked_against_write_delete_and_released_on_all_result_paths() {
    for fail in [false, true] {
        let temp = Temp::new();
        let path = temp.path();
        std::fs::write(&path, pe()).unwrap();
        let (mut budget, cancel) = budget();
        let file = LockedImage::open(&wide(&path), &mut budget).unwrap();
        assert!(OpenOptions::new().write(true).open(&path).is_err());
        assert!(std::fs::remove_file(&path).is_err());
        assert_eq!(file.stamp(&mut budget).unwrap().length, 0x400);
        let result = if fail {
            cancel.store(true, Ordering::Release);
            Err("fixture.business_error".into())
        } else {
            Ok(())
        };
        let outcome = file.finish(result, &budget);
        if fail {
            let error = outcome.unwrap_err();
            assert!(error.contains("fixture.business_error"));
            assert!(error.contains("memory.runtime_cancelled"));
        } else {
            outcome.unwrap();
        }
        OpenOptions::new().write(true).open(&path).unwrap();
        std::fs::remove_file(&path).unwrap();
    }
}
#[test]
fn empty_and_non_pe_owned_files_cannot_supply_layout_proof() {
    let temp = Temp::new();
    let path = temp.path();
    for bytes in [vec![], vec![1; 100]] {
        std::fs::write(&path, &bytes).unwrap();
        let (mut budget, _) = budget();
        let file = LockedImage::open(&wide(&path), &mut budget).unwrap();
        let result = (|| {
            let stamp = file.stamp(&mut budget)?;
            hash_image(file.file(), stamp.length, &mut budget)?;
            parse_image(file.file(), MODULE_SIZE, &mut budget).map(|_| ())
        })();
        assert!(file.finish(result, &budget).is_err());
        std::fs::remove_file(&path).unwrap();
    }
}

struct Window {
    hwnd: Option<HWND>,
    thread_bound: PhantomData<Rc<()>>,
}
impl Window {
    fn new() -> Self {
        // SAFETY: STATIC is a predefined system class; both strings are static
        // NUL-terminated UTF-16. The message-only window has no callback/user
        // pointer, is owned by this fixture thread and is destroyed before exit.
        let hwnd = unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                w!("STATIC"),
                w!("Lychee trusted image fixture"),
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
            thread_bound: PhantomData,
        }
    }
    fn identity(&self) -> WindowIdentity {
        WindowIdentity {
            process: Process::open(std::process::id())
                .unwrap()
                .identity()
                .clone(),
            hwnd: self.hwnd.unwrap().0 as usize,
        }
    }
    fn close(mut self) {
        let hwnd = self.hwnd.take().unwrap();
        // SAFETY: this fixture owns the live window on its creating thread;
        // taking it prevents its destructor from attempting a second destroy.
        unsafe { DestroyWindow(hwnd) }.unwrap();
    }
}
impl Drop for Window {
    fn drop(&mut self) {
        if let Some(hwnd) = self.hwnd.take() {
            // SAFETY: panic-unwind fallback for this fixture's same-thread owned
            // window only; no borrowed buffers or callbacks survive destruction.
            let _ = unsafe { DestroyWindow(hwnd) };
        }
    }
}
#[test]
fn os_issued_self_executable_layout_is_bound_to_exact_module_issuance_and_request() {
    let window = Window::new();
    let (mut budget, _) = budget();
    let module = RuntimeModule::open(window.identity(), &mut budget).unwrap();
    let before = budget.usage();
    let layout = module.verify_image_layout(&mut budget).unwrap();
    assert_eq!(layout.identity(), module.identity());
    assert_eq!(layout.executable_sha256().len(), 64);
    assert!(
        layout
            .executable_sha256()
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    );
    assert!(layout.retained_bytes() > 64);
    assert!(budget.usage().requested_bytes > before.requested_bytes);
    layout.verify(&module, &mut budget).unwrap();
    let second = RuntimeModule::open(window.identity(), &mut budget).unwrap();
    let before_wrong = budget.usage();
    assert_eq!(
        layout.verify(&second, &mut budget).unwrap_err(),
        "memory.runtime_image_issuance_changed"
    );
    assert_eq!(budget.usage(), before_wrong);
    let cancel = Arc::new(AtomicBool::new(false));
    let mut replacement = RuntimeBudget::new(
        RuntimeCost::defaults(),
        Instant::now() + Duration::from_secs(30),
        cancel,
    )
    .unwrap();
    assert_eq!(
        layout.verify(&module, &mut replacement).unwrap_err(),
        "memory.runtime_budget_replaced"
    );
    window.close();
}

#[test]
fn trusted_image_path_uses_shared_fixed_go_fold_without_ordinal_os_comparison() {
    let (mut b, _) = budget();
    for (left, right) in [
        ("C:\\K\\wow.exe", "c:\\K\\WOW.EXE"),
        ("S", "ſ"),
        ("ß", "ẞ"),
        ("Σ", "ς"),
    ] {
        let left: Vec<_> = left.encode_utf16().collect();
        let right: Vec<_> = right.encode_utf16().collect();
        let used = b.usage();
        verify_path_values(&left, &right, &mut b).unwrap();
        assert_eq!(b.usage().os_call_units, used.os_call_units);
    }
    verify_path_values(&[0xd800], &[0xfffd], &mut b).unwrap();
    assert_eq!(
        verify_path_values(&[b'K' as u16], &[b'S' as u16], &mut b).unwrap_err(),
        "memory.runtime_image_path_changed"
    );
    let (mut b, cancel) = budget();
    cancel.store(true, Ordering::Release);
    assert_eq!(
        verify_path_values(&[1], &[1], &mut b).unwrap_err(),
        "memory.runtime_cancelled"
    );
}
