//! The single appId capability from DD 100130's Unifiedlogin SDK. This adapter
//! does not expose password, SMS, account login or arbitrary SDK methods.
use crate::{
    Result,
    sdk_image::{Image, PinnedFile},
};
use std::{
    ffi::{CStr, CString, c_char, c_void},
    path::Path,
    ptr::NonNull,
};
use windows::{Win32::System::LibraryLoader::GetProcAddress, core::PCSTR};

const SDK_SHA256: &str = "cbb16ca38f679e4d004e099e52e0374d19961baa854bde551797f30c22b926f9";
const RESOURCES: [(&str, &str); 3] = [
    (
        "ursk1.dat",
        "30a4e4f8ea3c7aec3f9c2394abebf709d196ea99e5a3c0e75f45d9908a01da7b",
    ),
    (
        "ursk2.dat",
        "00b6b0925372f956904a40aa503639dd499b40b26d29b0ac6abe9dc138ef0299",
    ),
    (
        "ursk3.dat",
        "899f232085ba6cba233c4d6da5e211d22f6a1c0a84092a90340a2f5cb9458455",
    ),
];

type Factory = unsafe extern "system" fn() -> *mut Sdk;
#[repr(C)]
struct InitParams {
    values: [*const c_char; 6],
}
#[repr(C)]
struct Sdk {
    vtable: *const SdkVtable,
}
#[repr(C)]
struct SdkVtable {
    _destructor: usize,
    _last_error: usize,
    release: unsafe extern "system" fn(*mut Sdk),
    free_buffer: unsafe extern "system" fn(*mut Sdk, *mut c_void),
    create_method: unsafe extern "system" fn(*mut Sdk, i32) -> *mut Method,
    init: unsafe extern "system" fn(*mut Sdk, *const InitParams) -> u32,
}
#[repr(C)]
struct Method {
    vtable: *const MethodVtable,
}
#[repr(C)]
struct MethodVtable {
    _destructor: usize,
    release: unsafe extern "system" fn(*mut Method),
    _arg: usize,
    perform: unsafe extern "system" fn(*mut Method, *mut *mut c_char, *mut *mut c_char) -> u32,
}
struct SdkObject(NonNull<Sdk>);
impl SdkObject {
    fn table(&self) -> &SdkVtable {
        // SAFETY: only the fixed, pinned factory (or the synthetic ABI fixture)
        // creates this object. Its vtable lives until SDK release, before unload.
        unsafe { &*self.0.as_ref().vtable }
    }
}
impl Drop for SdkObject {
    fn drop(&mut self) {
        // SAFETY: this consumes the unique factory result, after its method and
        // buffers have been released. Slot 2 is release, not COM AddRef/Release.
        unsafe { (self.table().release)(self.0.as_ptr()) };
    }
}
struct MethodObject<'a> {
    pointer: NonNull<Method>,
    _sdk: &'a SdkObject,
}
impl MethodObject<'_> {
    fn table(&self) -> &MethodVtable {
        // SAFETY: method 2 is created by the pinned SDK and remains alive until
        // this owner's Drop, while its parent SDK and module are still live.
        unsafe { &*self.pointer.as_ref().vtable }
    }
}
impl Drop for MethodObject<'_> {
    fn drop(&mut self) {
        // SAFETY: slot 1 consumes this unique method; never call its destructor
        // directly or release the SDK before its child method.
        unsafe { (self.table().release)(self.pointer.as_ptr()) };
    }
}
struct Buffers<'a> {
    sdk: &'a SdkObject,
    json: *mut c_char,
    message: *mut c_char,
}
impl Drop for Buffers<'_> {
    fn drop(&mut self) {
        for pointer in [self.json, self.message] {
            if !pointer.is_null() {
                // SAFETY: perform transfers each non-null output, on success
                // and error. Free once with its originating SDK allocator.
                unsafe { (self.sdk.table().free_buffer)(self.sdk.0.as_ptr(), pointer.cast()) };
            }
        }
    }
}
fn parameters(paths: [&Path; 3]) -> Result<[CString; 6]> {
    let mut values = Vec::with_capacity(6);
    // Public application parameters from the fixed shipped caller. These are
    // wire/SDK identity, independent of Lychee's release version and channel.
    for value in ["cc_team", "1.1.30", "3822d0cbc13db710cb04d9e4025eb3d2"] {
        values.push(CString::new(value).expect("fixed SDK parameter"));
    }
    for path in paths {
        let value = path
            .to_str()
            .ok_or("URS SDK resource path requires UTF-8")?;
        values.push(CString::new(value).map_err(|_| "URS SDK resource path contains NUL")?);
    }
    Ok(values.try_into().expect("six SDK parameters"))
}

// SAFETY: factory must be the authenticated Windows-amd64 SDK export (or our
// synthetic C ABI fixture); its module must outlive this entire invocation.
unsafe fn invoke(factory: Factory, strings: &[CString; 6]) -> Result<String> {
    // SAFETY: the caller supplies the pinned factory with its exact ABI.
    let pointer = unsafe { factory() };
    let sdk = SdkObject(NonNull::new(pointer).ok_or("URS SDK did not create an interface")?);
    let params = InitParams {
        values: strings.each_ref().map(|s| s.as_ptr()),
    };
    // SAFETY: the six NUL-terminated UTF-8 strings and parameter array live for
    // the complete call; the SDK copies them, as in its shipped C++ caller.
    let code = unsafe { (sdk.table().init)(sdk.0.as_ptr(), &params) };
    if code != 0 {
        return Err(format!("URS SDK initialization failed with code {code}"));
    }
    // SAFETY: method 2 alone is the statically verified RequestAppId interface.
    let pointer = unsafe { (sdk.table().create_method)(sdk.0.as_ptr(), 2) };
    let method = MethodObject {
        pointer: NonNull::new(pointer).ok_or("URS SDK appId method is unavailable")?,
        _sdk: &sdk,
    };
    let mut buffers = Buffers {
        sdk: &sdk,
        json: std::ptr::null_mut(),
        message: std::ptr::null_mut(),
    };
    // SAFETY: writable out-pointers start null. The synchronous call returns
    // SDK-allocated NUL-terminated strings, copied before all native releases.
    let code = unsafe {
        (method.table().perform)(
            method.pointer.as_ptr(),
            &mut buffers.json,
            &mut buffers.message,
        )
    };
    if code != 0 {
        return Err(format!("URS appId request failed with code {code}"));
    }
    if buffers.json.is_null() {
        return Err("URS appId request returned no data".into());
    }
    // SAFETY: a successful non-null result is a NUL-terminated string from the
    // fixed ABI. Its allocator and module remain live through this copy.
    let bytes = unsafe { CStr::from_ptr(buffers.json) }.to_bytes();
    if bytes.is_empty() || bytes.len() > 65536 {
        return Err("URS appId request returned an invalid data length".into());
    }
    std::str::from_utf8(bytes)
        .map(str::to_owned)
        .map_err(|_| "URS appId request returned invalid UTF-8".into())
}

/// Synchronous native SDK operation. The Publish task owner supplies a fixed
/// verified program directory and owns the process deadline. Returned JSON is
/// an in-memory capability; Publish validates appId without exposing this text.
pub fn request_app_id(program_dir: &Path) -> Result<String> {
    let mut image = Image::pin(&program_dir.join("Unifiedlogin.dll"), SDK_SHA256)?;
    let mut resources = Vec::with_capacity(3);
    let result = (|| {
        for (name, digest) in RESOURCES {
            resources.push(PinnedFile::open(
                &program_dir.join("resource").join(name),
                digest,
            )?);
        }
        let strings = parameters([
            resources[0].path(),
            resources[1].path(),
            resources[2].path(),
        ])?;
        let module = image.load()?;
        // SAFETY: the authenticated module stays loaded until invoke releases
        // all SDK objects and outputs. Only this documented factory is resolved.
        let entry = unsafe { GetProcAddress(module, PCSTR(c"CreateSDKInterface".as_ptr().cast())) }
            .ok_or("URS SDK factory export is unavailable")?;
        // SAFETY: this fixed image's export is the inspected Win64 C factory.
        let factory: Factory = unsafe { std::mem::transmute(entry) };
        // SAFETY: the pinned image/resources outlive all SDK calls and releases.
        unsafe { invoke(factory, &strings) }
    })();
    // Native objects have already been released by invoke, even on error.
    let mut failed = image.close().is_err();
    for resource in &mut resources {
        failed |= resource.close().is_err();
    }
    if failed {
        Err("URS SDK resource release failed".into())
    } else {
        result
    }
}

#[cfg(test)]
mod tests;
