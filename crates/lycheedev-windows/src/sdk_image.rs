//! Private file/loader ownership shared by the two fixed DD SDK adapters.
use crate::Result;
use sha2::{Digest, Sha256};
use std::{
    fs::{File, OpenOptions},
    io::Read,
    os::windows::{ffi::OsStrExt, fs::OpenOptionsExt},
    path::{Path, PathBuf},
};
use windows::{
    Win32::{
        Foundation::{FreeLibrary, HMODULE},
        System::LibraryLoader::{
            GetModuleFileNameW, LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR, LOAD_LIBRARY_SEARCH_SYSTEM32,
            LoadLibraryExW,
        },
    },
    core::PCWSTR,
};

pub(super) struct PinnedFile {
    path: PathBuf,
    file: Option<File>,
}
impl PinnedFile {
    pub(super) fn open(path: &Path, expected: &str) -> Result<Self> {
        if !path.is_absolute() {
            return Err("native SDK requires an absolute installation path".into());
        }
        let path = path
            .canonicalize()
            .map_err(|_| "native SDK file is unavailable")?;
        let file = OpenOptions::new()
            .read(true)
            .share_mode(1)
            .open(&path)
            .map_err(|_| "native SDK file cannot be held against replacement")?;
        let mut pin = Self {
            path,
            file: Some(file),
        };
        let result = (|| {
            let file = pin.file.as_mut().expect("new pin owns its file");
            let size = file
                .metadata()
                .map_err(|_| "native SDK metadata is unavailable")?
                .len();
            if size == 0 || size > 16 * 1024 * 1024 {
                return Err("unsupported native SDK file".into());
            }
            let mut hash = Sha256::new();
            let mut buffer = [0; 65536];
            loop {
                let count = file
                    .read(&mut buffer)
                    .map_err(|_| "native SDK file cannot be authenticated")?;
                if count == 0 {
                    break;
                }
                hash.update(&buffer[..count]);
            }
            if format!("{:x}", hash.finalize()) != expected {
                return Err("unsupported native SDK bytes; the DD 100130 SDK is required".into());
            }
            Ok(())
        })();
        if let Err(error) = result {
            pin.close()?;
            return Err(error);
        }
        Ok(pin)
    }
    pub(super) fn path(&self) -> &Path {
        &self.path
    }
    pub(super) fn close(&mut self) -> Result<()> {
        if let Some(file) = self.file.take() {
            crate::filesystem::close_file(file).map_err(|_| "native SDK file release failed")?;
        }
        Ok(())
    }
}
impl Drop for PinnedFile {
    fn drop(&mut self) {
        let _ = self.close();
    }
}

pub(super) struct Image {
    pin: PinnedFile,
    module: Option<HMODULE>,
}
impl Image {
    pub(super) fn pin(path: &Path, expected: &str) -> Result<Self> {
        Ok(Self {
            pin: PinnedFile::open(path, expected)?,
            module: None,
        })
    }
    pub(super) fn load(&mut self) -> Result<HMODULE> {
        let mut path: Vec<u16> = self.pin.path().as_os_str().encode_wide().collect();
        if path.contains(&0) {
            return Err("native SDK path contains NUL".into());
        }
        path.push(0);
        // SAFETY: the authenticated image is held deny-write/delete throughout
        // loading and all calls. Only its own directory and System32 can supply
        // dependencies; CWD/PATH are excluded. The path lives through the call.
        let module = unsafe {
            LoadLibraryExW(
                PCWSTR(path.as_ptr()),
                None,
                LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_SYSTEM32,
            )
        }
        .map_err(|_| "native SDK cannot be loaded")?;
        self.module = Some(module);
        let mut name = vec![0; 32768];
        // SAFETY: the owned loader reference is live, with the actual buffer length.
        let size = unsafe { GetModuleFileNameW(Some(module), &mut name) } as usize;
        if size == 0 || size >= name.len() {
            return Err("native SDK loaded identity is unavailable".into());
        }
        use std::os::windows::ffi::OsStringExt;
        let loaded = PathBuf::from(std::ffi::OsString::from_wide(&name[..size]));
        if loaded.canonicalize().ok().as_deref() != Some(self.pin.path()) {
            return Err("native SDK loaded from a different installation".into());
        }
        Ok(module)
    }
    pub(super) fn close(&mut self) -> Result<()> {
        let mut failed = false;
        if let Some(module) = self.module.take() {
            // SAFETY: consumes exactly the acquired loader reference; adapters
            // release their objects before closing and never call it afterward.
            failed |= unsafe { FreeLibrary(module) }.is_err();
        }
        failed |= self.pin.close().is_err();
        if failed {
            Err("native SDK resource release failed".into())
        } else {
            Ok(())
        }
    }
}
impl Drop for Image {
    fn drop(&mut self) {
        let _ = self.close();
    }
}
