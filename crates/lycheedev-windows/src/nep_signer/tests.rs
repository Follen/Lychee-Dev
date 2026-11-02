use super::*;
use sha2::{Digest, Sha256};
use std::{cell::RefCell, fs};
#[derive(Default)]
struct Fixture {
    args: Vec<(Vec<u8>, Vec<u8>)>,
    url: String,
    body: Option<Vec<u8>>,
    frees: usize,
    appends: usize,
    mode: u8,
}
thread_local! { static FIXTURE: RefCell<Fixture> = RefCell::default(); }
unsafe extern "C" fn append(
    pointer: *mut c_void,
    key: *const u8,
    size: usize,
    value: *const u8,
    length: usize,
) -> *mut c_void {
    let key = unsafe { std::slice::from_raw_parts(key, size) }.to_vec();
    let value = unsafe { std::slice::from_raw_parts(value, length) }.to_vec();
    FIXTURE.with_borrow_mut(|f| {
        f.appends += 1;
        if f.mode == 4 && f.appends == 2 {
            return std::ptr::null_mut();
        }
        f.args.push((key, value));
        if pointer.is_null() {
            Box::into_raw(Box::new(0u8)).cast()
        } else {
            pointer
        }
    })
}
unsafe extern "C" fn free(pointer: *mut c_void) {
    drop(unsafe { Box::from_raw(pointer.cast::<u8>()) });
    FIXTURE.with_borrow_mut(|f| f.frees += 1);
}
unsafe extern "C" fn get(
    url: *const c_char,
    _: *mut c_void,
    callback: Callback,
    context: *mut c_void,
) {
    let url = unsafe { CStr::from_ptr(url) }.to_str().unwrap().to_owned();
    let mode = FIXTURE.with_borrow_mut(|f| {
        f.url = url;
        f.mode
    });
    let bytes = if mode == 3 {
        vec![255, 0]
    } else {
        b"https://uiapi.w.163.com/path?signature=synthetic\0".to_vec()
    };
    if mode != 1 {
        unsafe {
            callback(
                if mode == 5 { 19 } else { 0 },
                bytes.as_ptr().cast(),
                context,
            )
        };
        if mode == 2 {
            unsafe { callback(0, bytes.as_ptr().cast(), context) };
        }
    }
    // The returned Rust string must outlive native callback storage.
    drop(bytes);
}
unsafe extern "C" fn post(
    url: *const c_char,
    body: *const u8,
    length: usize,
    extra: *mut c_void,
    callback: Callback,
    context: *mut c_void,
) {
    FIXTURE.with_borrow_mut(|f| {
        f.body = Some(unsafe { std::slice::from_raw_parts(body, length) }.to_vec())
    });
    unsafe { get(url, extra, callback, context) };
}
fn abi(mode: u8) -> Abi {
    FIXTURE.with_borrow_mut(|f| {
        *f = Fixture {
            mode,
            ..Default::default()
        }
    });
    Abi {
        get,
        post,
        append,
        free,
    }
}
#[test]
fn native_abi_preserves_utf8_body_and_sampled_headers_and_copies_callback_storage() {
    let abi = abi(0);
    let values = [("x-w163-token", "合成token"), ("x-timestamp", "1793500000")];
    let body = br#"{"name":"\u4e2d\u6587","amount":0}"#;
    assert!(
        abi.sign(
            "https://uiapi.w.163.com/path?q=%E4%B8%AD",
            Some(body),
            &values
        )
        .unwrap()
        .contains("synthetic")
    );
    FIXTURE.with_borrow(|f| {
        assert_eq!(f.body.as_deref(), Some(body.as_slice()));
        assert_eq!(
            f.args,
            values
                .iter()
                .map(|(k, v)| (k.as_bytes().to_vec(), v.as_bytes().to_vec()))
                .collect::<Vec<_>>()
        );
        assert!(f.url.ends_with("q=%E4%B8%AD"));
        assert_eq!(f.frees, 1);
    });
    let utf8 = "{\"name\":\"中文😀\"}".as_bytes();
    abi.sign("https://uiapi.w.163.com/path", Some(utf8), &[])
        .unwrap();
    FIXTURE.with_borrow(|f| assert_eq!(f.body.as_deref(), Some(utf8)));
}
#[test]
fn all_failed_native_signatures_release_the_owned_extra_list_without_returning_secrets() {
    for mode in 1..=5 {
        let abi = abi(mode);
        let error = abi
            .sign(
                "https://uiapi.w.163.com/path",
                None,
                &[("token", "secret"), ("ts", "1")],
            )
            .unwrap_err();
        assert!(!error.contains("secret"));
        assert!(!error.contains("synthetic"));
        FIXTURE.with_borrow(|f| assert_eq!(f.frees, 1));
    }
    let abi = abi(0);
    assert!(
        abi.sign("https://example.invalid/\0secret", None, &[])
            .is_err()
    );
    assert!(
        abi.sign("https://example.invalid/", None, &[("token", "a\0secret")])
            .is_err()
    );
    FIXTURE.with_borrow(|f| {
        assert_eq!(f.appends, 0);
        assert_eq!(f.frees, 0);
    });
}
#[test]
fn image_authentication_precedes_loading_and_pins_bytes_against_replacement() {
    let dir = std::env::temp_dir().join(format!("lychee-nep-synthetic-{}", std::process::id()));
    fs::create_dir(&dir).unwrap();
    let path = dir.join("synthetic-not-a-dll.bin");
    let bytes = b"synthetic image; no SDK or user credentials";
    fs::write(&path, bytes).unwrap();
    assert!(
        NepSigner::open(&path)
            .err()
            .unwrap()
            .contains("unsupported")
    );
    let mut pin = Image::pin(&path, &format!("{:x}", Sha256::digest(bytes))).unwrap();
    assert!(fs::write(&path, b"changed").is_err());
    assert!(fs::remove_file(&path).is_err());
    pin.close().unwrap();
    fs::remove_file(path).unwrap();
    fs::remove_dir(dir).unwrap();
    assert!(NepSigner::open(Path::new("relative/libnep.dll")).is_err());
}
