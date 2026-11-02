use super::*;
use std::{cell::RefCell, collections::BTreeMap, fs};

#[derive(Clone, Copy, Default, PartialEq)]
enum Mode {
    #[default]
    Happy,
    NoFactoryObject,
    InitReject,
    NoMethod,
    PerformReject,
    NoJson,
    EmptyJson,
    InvalidUtf8,
    Oversized,
}
#[derive(Default)]
struct Fixture {
    mode: Mode,
    calls: Vec<&'static str>,
    strings: Vec<String>,
    method_ids: Vec<i32>,
    allocations: BTreeMap<usize, &'static str>,
}
thread_local! { static FIXTURE: RefCell<Fixture> = RefCell::default(); }
static SDK_TABLE: SdkVtable = SdkVtable {
    _destructor: 0,
    _last_error: 0,
    release: release_sdk,
    free_buffer,
    create_method,
    init,
};
static METHOD_TABLE: MethodVtable = MethodVtable {
    _destructor: 0,
    release: release_method,
    _arg: 0,
    perform,
};
unsafe extern "system" fn factory() -> *mut Sdk {
    FIXTURE.with_borrow_mut(|f| {
        f.calls.push("factory");
        if f.mode == Mode::NoFactoryObject {
            std::ptr::null_mut()
        } else {
            Box::into_raw(Box::new(Sdk { vtable: &SDK_TABLE }))
        }
    })
}
unsafe extern "system" fn init(_: *mut Sdk, params: *const InitParams) -> u32 {
    let strings = unsafe { &*params }
        .values
        .iter()
        .map(|pointer| {
            unsafe { CStr::from_ptr(*pointer) }
                .to_str()
                .unwrap()
                .to_owned()
        })
        .collect();
    FIXTURE.with_borrow_mut(|f| {
        f.calls.push("init");
        f.strings = strings;
        if f.mode == Mode::InitReject { 27 } else { 0 }
    })
}
unsafe extern "system" fn release_sdk(pointer: *mut Sdk) {
    drop(unsafe { Box::from_raw(pointer) });
    FIXTURE.with_borrow_mut(|f| {
        assert!(
            f.allocations.is_empty(),
            "all SDK buffers must be freed before SDK release"
        );
        f.calls.push("sdk.release");
    });
}
unsafe extern "system" fn create_method(_: *mut Sdk, id: i32) -> *mut Method {
    FIXTURE.with_borrow_mut(|f| {
        f.calls.push("create_method");
        f.method_ids.push(id);
        if f.mode == Mode::NoMethod {
            std::ptr::null_mut()
        } else {
            Box::into_raw(Box::new(Method {
                vtable: &METHOD_TABLE,
            }))
        }
    })
}
unsafe extern "system" fn release_method(pointer: *mut Method) {
    drop(unsafe { Box::from_raw(pointer) });
    FIXTURE.with_borrow_mut(|f| {
        assert!(
            f.allocations.is_empty(),
            "copy and free outputs before releasing the method"
        );
        f.calls.push("method.release");
    });
}
fn allocate(f: &mut Fixture, bytes: Vec<u8>, name: &'static str) -> *mut c_char {
    let pointer = CString::new(bytes).unwrap().into_raw();
    assert!(f.allocations.insert(pointer as usize, name).is_none());
    pointer
}
unsafe extern "system" fn free_buffer(_: *mut Sdk, pointer: *mut c_void) {
    FIXTURE.with_borrow_mut(|f| {
        let name = f
            .allocations
            .remove(&(pointer as usize))
            .expect("one release per SDK output");
        f.calls.push(name);
    });
    drop(unsafe { CString::from_raw(pointer.cast()) });
}
unsafe extern "system" fn perform(
    _: *mut Method,
    json: *mut *mut c_char,
    message: *mut *mut c_char,
) -> u32 {
    assert!(unsafe { *json }.is_null());
    assert!(unsafe { *message }.is_null());
    FIXTURE.with_borrow_mut(|f| {
        f.calls.push("perform");
        let bytes = match f.mode {
            Mode::InvalidUtf8 => vec![255],
            Mode::EmptyJson => vec![],
            Mode::Oversized => vec![b'x'; 65537],
            _ => "{\"appId\":\"合成-identity\",\"opaque\":\"private-sdk-value\"}\n"
                .as_bytes()
                .to_vec(),
        };
        if f.mode != Mode::NoJson {
            unsafe { *json = allocate(f, bytes, "free.json") };
        }
        unsafe { *message = allocate(f, b"private-sdk-message".to_vec(), "free.message") };
        if f.mode == Mode::PerformReject {
            401
        } else {
            0
        }
    })
}
fn run(mode: Mode) -> Result<String> {
    FIXTURE.with_borrow_mut(|f| {
        assert!(f.allocations.is_empty());
        *f = Fixture {
            mode,
            ..Default::default()
        };
    });
    let strings = parameters([
        Path::new("C:\\合成测试\\resource\\ursk1.dat"),
        Path::new("C:\\合成测试\\resource\\ursk2.dat"),
        Path::new("C:\\合成测试\\resource\\ursk3.dat"),
    ])?;
    // SAFETY: this fixture implements the exact C ABI with independent heap
    // outputs and records all ownership transfers; no vendor code is loaded.
    unsafe { invoke(factory, &strings) }
}

#[test]
fn app_id_abi_keeps_utf8_parameters_and_copies_outputs_before_native_release() {
    let result = run(Mode::Happy).unwrap();
    assert_eq!(
        result,
        "{\"appId\":\"合成-identity\",\"opaque\":\"private-sdk-value\"}\n"
    );
    FIXTURE.with_borrow(|f| {
        assert_eq!(&f.strings[..2], ["cc_team", "1.1.30"]);
        assert_eq!(f.strings[2].len(), 32);
        assert_eq!(
            &f.strings[3..],
            [
                "C:\\合成测试\\resource\\ursk1.dat",
                "C:\\合成测试\\resource\\ursk2.dat",
                "C:\\合成测试\\resource\\ursk3.dat",
            ]
        );
        assert_eq!(f.method_ids, [2]);
        assert_eq!(
            f.calls,
            [
                "factory",
                "init",
                "create_method",
                "perform",
                "free.json",
                "free.message",
                "method.release",
                "sdk.release"
            ]
        );
        assert!(f.allocations.is_empty());
    });
}
#[test]
fn factory_init_and_method_failure_release_only_created_native_objects() {
    for (mode, expected) in [
        (Mode::NoFactoryObject, vec!["factory"]),
        (Mode::InitReject, vec!["factory", "init", "sdk.release"]),
        (
            Mode::NoMethod,
            vec!["factory", "init", "create_method", "sdk.release"],
        ),
    ] {
        let error = run(mode).unwrap_err();
        assert!(!error.contains("private"));
        FIXTURE.with_borrow(|f| {
            assert_eq!(f.calls, expected);
            assert!(f.allocations.is_empty());
        });
    }
}
#[test]
fn every_perform_failure_frees_both_outputs_before_method_and_sdk_without_echoing_them() {
    for mode in [
        Mode::PerformReject,
        Mode::NoJson,
        Mode::EmptyJson,
        Mode::InvalidUtf8,
        Mode::Oversized,
    ] {
        let error = run(mode).unwrap_err();
        assert!(!error.contains("private-sdk"));
        assert!(!error.contains("合成"));
        FIXTURE.with_borrow(|f| {
            let mut expected = vec!["factory", "init", "create_method", "perform"];
            if mode != Mode::NoJson {
                expected.push("free.json");
            }
            expected.extend(["free.message", "method.release", "sdk.release"]);
            assert_eq!(f.calls, expected);
            assert!(f.allocations.is_empty());
        });
    }
}
#[test]
fn unrecognized_sdk_and_invalid_resource_paths_stop_before_native_calls() {
    let dir = std::env::temp_dir().join(format!(
        "lychee-urs-synthetic-{}",
        crate::random::token().unwrap()
    ));
    fs::create_dir(&dir).unwrap();
    let path = dir.join("Unifiedlogin.dll");
    fs::write(&path, b"synthetic, never loaded").unwrap();
    assert!(request_app_id(&dir).unwrap_err().contains("unsupported"));
    // Rejection releases the file lock as well as avoiding the factory.
    fs::write(&path, b"changed after rejection").unwrap();
    fs::remove_file(path).unwrap();
    fs::remove_dir(dir).unwrap();
    assert!(request_app_id(Path::new("relative")).is_err());
    assert!(parameters([Path::new("C:\\bad\0path"); 3]).is_err());
    use std::os::windows::ffi::OsStringExt;
    let invalid = std::ffi::OsString::from_wide(&[0xd800]);
    assert!(parameters([Path::new(&invalid); 3]).is_err());
}
