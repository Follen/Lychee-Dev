//! The NEP C ABI used by the shipped DD 100130 caller. No HTTP, login, Python,
//! provider state or vendor wrapper DLL runs here. The vendor wrapper binds
//! these four entry points only after checking the exact SDK bytes.
use crate::{Result, sdk_image::Image};
use std::{
    ffi::{CStr, CString, c_char, c_void},
    path::Path,
};

// The original signed 100130 SDK has MD5 61642B7ED07579B9979009849DE633C3,
// which its shipped wrapper checks before binding these RVAs. Use SHA-256 here.
const SDK_SHA256: &str = "e86b40e5b2377ff3d0c51ee251f21bd4a1e5a9d1eb86549bb8878c7e92232319";
type Callback = unsafe extern "C" fn(i32, *const c_char, *mut c_void);
type Get = unsafe extern "C" fn(*const c_char, *mut c_void, Callback, *mut c_void);
type Post =
    unsafe extern "C" fn(*const c_char, *const u8, usize, *mut c_void, Callback, *mut c_void);
type Append = unsafe extern "C" fn(*mut c_void, *const u8, usize, *const u8, usize) -> *mut c_void;
type Free = unsafe extern "C" fn(*mut c_void);

struct Abi {
    get: Get,
    post: Post,
    append: Append,
    free: Free,
}
struct Extra<'a> {
    abi: &'a Abi,
    pointer: *mut c_void,
}
impl Drop for Extra<'_> {
    fn drop(&mut self) {
        if !self.pointer.is_null() {
            // SAFETY: append transfers one current list to its caller. It is
            // freed once after the synchronous request, including failures.
            unsafe { (self.abi.free)(self.pointer) };
        }
    }
}
#[derive(Default)]
struct Reply {
    count: u32,
    value: Option<Result<String>>,
}
unsafe extern "C" fn receive(code: i32, data: *const c_char, context: *mut c_void) {
    // SAFETY: the shipped C caller invokes this callback synchronously, with
    // the same live per-call context. The result is copied before return; the
    // SDK retains ownership of the callback string and the extra-argument list.
    let reply = unsafe { &mut *context.cast::<Reply>() };
    reply.count = reply.count.saturating_add(1);
    reply.value = Some(if code != 0 {
        Err(format!("NEP request signing failed with code {code}"))
    } else if data.is_null() {
        Err("NEP request signing returned no URL".into())
    } else {
        // SAFETY: successful callback data is a NUL-terminated C string valid
        // for this callback, as declared by the fixed vendor ABI.
        let bytes = unsafe { CStr::from_ptr(data) }.to_bytes();
        if bytes.is_empty() || bytes.len() > 65536 {
            Err("NEP request signing returned an invalid URL length".into())
        } else {
            std::str::from_utf8(bytes)
                .map(str::to_owned)
                .map_err(|_| "NEP request signing returned invalid UTF-8".into())
        }
    });
}
impl Abi {
    fn sign(&self, url: &str, body: Option<&[u8]>, values: &[(&str, &str)]) -> Result<String> {
        let url = CString::new(url).map_err(|_| "NEP request URL contains NUL")?;
        if values
            .iter()
            .any(|(key, value)| key.contains('\0') || value.contains('\0'))
        {
            return Err("NEP extra arguments contain NUL".into());
        }
        let mut extra = Extra {
            abi: self,
            pointer: std::ptr::null_mut(),
        };
        for (key, value) in values {
            // SAFETY: UTF-8 byte pointers/lengths stay live through this call;
            // append copies them into the owned list, as the vendor caller does.
            let next = unsafe {
                (self.append)(
                    extra.pointer,
                    key.as_ptr(),
                    key.len(),
                    value.as_ptr(),
                    value.len(),
                )
            };
            if next.is_null() {
                return Err("NEP extra arguments were rejected".into());
            }
            extra.pointer = next;
        }
        let mut reply = Reply::default();
        let context = (&mut reply as *mut Reply).cast();
        // SAFETY: these exact typed entry points are from the pinned SDK ABI;
        // buffers, context, callback and extra list live until the synchronous
        // call returns. POST's size is bytes, not Unicode character count.
        unsafe {
            if let Some(body) = body {
                (self.post)(
                    url.as_ptr(),
                    body.as_ptr(),
                    body.len(),
                    extra.pointer,
                    receive,
                    context,
                );
            } else {
                (self.get)(url.as_ptr(), extra.pointer, receive, context);
            }
        }
        if reply.count != 1 {
            return Err("NEP request signing did not return exactly one result".into());
        }
        reply.value.expect("one callback sets a result")
    }
}

/// One serial native signing capability. SDK policy is limited to the exact
/// external C ABI; sessions, cookies, URLs and author effects belong to Publish.
pub struct NepSigner {
    abi: Abi,
    image: Image,
}
impl NepSigner {
    pub fn open(sdk: &Path) -> Result<Self> {
        let mut image = Image::pin(sdk, SDK_SHA256)?;
        let module = match image.load() {
            Ok(module) => module,
            Err(error) => {
                image.close()?;
                return Err(error);
            }
        };
        let base = module.0 as usize;
        // SAFETY: only the authenticated Windows-amd64 DD 100130 image is
        // admitted. Its shipped wrapper (also statically inspected) binds these
        // exact four RVAs to this C ABI. No caller-supplied address is accepted.
        let abi = unsafe {
            Abi {
                get: std::mem::transmute::<usize, Get>(base + 0x69b5),
                post: std::mem::transmute::<usize, Post>(base + 0x549d),
                append: std::mem::transmute::<usize, Append>(base + 0x37e2),
                // The wrapper's LEA at 0x18000202d uses byte offset 0x5c90.
                // Its decompiler renders typed HMODULE pointer arithmetic;
                // that expression must not be scaled as a pointer-sized unit.
                free: std::mem::transmute::<usize, Free>(base + 0x5c90),
            }
        };
        Ok(Self { abi, image })
    }
    pub fn get(&mut self, url: &str, values: &[(&str, &str)]) -> Result<String> {
        self.abi.sign(url, None, values)
    }
    pub fn post(&mut self, url: &str, body: &[u8], values: &[(&str, &str)]) -> Result<String> {
        self.abi.sign(url, Some(body), values)
    }
    pub fn close(mut self) -> Result<()> {
        self.image.close()
    }
}

#[cfg(test)]
mod tests;
