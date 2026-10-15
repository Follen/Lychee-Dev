//! External output publication. Directory identities and the staged file remain
//! held until an atomic, handle-based rename; existing files are never truncated.
use std::{
    fs::File,
    io::{self, Read, Write},
    mem::{offset_of, size_of},
    os::windows::{
        ffi::OsStrExt,
        fs::MetadataExt,
        io::{AsRawHandle, FromRawHandle},
    },
    path::{Component, Path, PathBuf, Prefix},
};
use windows::{
    Win32::{
        Foundation::HANDLE,
        Storage::FileSystem::{
            CreateFileW, DELETE, FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_REPARSE_POINT,
            FILE_DISPOSITION_INFO, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
            FILE_LIST_DIRECTORY, FILE_READ_ATTRIBUTES, FILE_RENAME_INFO, FILE_RENAME_INFO_0,
            FILE_SHARE_READ, FILE_SHARE_WRITE, FileDispositionInfo, OPEN_EXISTING,
            SetFileInformationByHandle,
        },
    },
    core::PCWSTR,
};

// Documented user-mode Native API. The status/pointer union and ULONG_PTR
// preserve the IO_STATUS_BLOCK ABI on the accepted Windows amd64 target.
#[repr(C)]
union NativeStatus {
    status: i32,
    pointer: *mut std::ffi::c_void,
}
#[repr(C)]
struct IoStatusBlock {
    status: NativeStatus,
    information: usize,
}
#[repr(C)]
struct UnicodeString {
    length: u16,
    maximum_length: u16,
    buffer: *mut u16,
}
#[repr(C)]
struct ObjectAttributes {
    length: u32,
    root_directory: HANDLE,
    object_name: *mut UnicodeString,
    attributes: u32,
    security_descriptor: *mut std::ffi::c_void,
    security_quality_of_service: *mut std::ffi::c_void,
}
const _: () = {
    assert!(size_of::<NativeStatus>() == 8);
    assert!(size_of::<IoStatusBlock>() == 16);
    assert!(offset_of!(FILE_RENAME_INFO, FileName) == 20);
    assert!(size_of::<UnicodeString>() == 16);
    assert!(size_of::<ObjectAttributes>() == 48);
};
#[link(name = "ntdll")]
unsafe extern "system" {
    fn NtCreateFile(
        file: *mut HANDLE,
        access: u32,
        attributes: *const ObjectAttributes,
        status: *mut IoStatusBlock,
        allocation_size: *const i64,
        file_attributes: u32,
        share: u32,
        disposition: u32,
        options: u32,
        ea: *const std::ffi::c_void,
        ea_length: u32,
    ) -> i32;
    fn NtSetInformationFile(
        file: HANDLE,
        status: *mut IoStatusBlock,
        information: *const std::ffi::c_void,
        length: u32,
        class: u32,
    ) -> i32;
    fn RtlNtStatusToDosError(status: i32) -> u32;
}

enum RelativeOpen {
    Directory { deny_write: bool },
    Stage,
    Inspect,
    Reader,
}
#[derive(Clone, Copy, Debug, Default)]
pub struct FileCost {
    pub io_operations: u64,
    pub allocation_bytes: u64,
    pub work_units: u64,
}
/// Controlled filesystem operations share the caller's error and ledger type.
/// Open attempts reserve their eventual close before acquiring a handle.
pub trait FileControl {
    type Error: From<io::Error>;
    fn charge(&mut self, cost: FileCost) -> Result<(), Self::Error>;
    fn check(&mut self) -> Result<(), Self::Error>;
}
struct Uncontrolled;
impl FileControl for Uncontrolled {
    type Error = io::Error;
    fn charge(&mut self, _: FileCost) -> io::Result<()> {
        Ok(())
    }
    fn check(&mut self) -> io::Result<()> {
        Ok(())
    }
}
fn controlled_io<C: FileControl, T>(
    control: &mut C,
    operation: impl FnOnce() -> io::Result<T>,
) -> Result<T, C::Error> {
    control.charge(FileCost {
        io_operations: 1,
        ..FileCost::default()
    })?;
    let value = operation().map_err(C::Error::from)?;
    control.check()?;
    Ok(value)
}
fn relative_file(parent: &File, name: &std::ffi::OsStr, kind: RelativeOpen) -> io::Result<File> {
    relative_file_controlled(parent, name, kind, &mut Uncontrolled)
}
fn relative_file_controlled<C: FileControl>(
    parent: &File,
    name: &std::ffi::OsStr,
    kind: RelativeOpen,
    control: &mut C,
) -> Result<File, C::Error> {
    relative_file_optional(parent, name, kind, false, control)
        .map(|file| file.expect("required relative file"))
}
fn relative_file_optional<C: FileControl>(
    parent: &File,
    name: &std::ffi::OsStr,
    kind: RelativeOpen,
    optional: bool,
    control: &mut C,
) -> Result<Option<File>, C::Error> {
    if name.len() > 32768 {
        return Err(invalid("relative name exceeds admission").into());
    }
    control.charge(FileCost {
        allocation_bytes: name.len() as u64 * 16 + 1024,
        work_units: name.len() as u64 * 16 + 1024,
        ..FileCost::default()
    })?;
    if !valid_name(name) || name.encode_wide().any(|unit| [47, 92].contains(&unit)) {
        return Err(invalid("relative file operation requires one simple name").into());
    }
    let mut wide: Vec<u16> = name.encode_wide().collect();
    let byte_length = wide
        .len()
        .checked_mul(2)
        .filter(|&length| length <= u16::MAX as usize)
        .ok_or_else(|| C::Error::from(invalid("relative name exceeds Unicode string admission")))?;
    let mut unicode = UnicodeString {
        length: byte_length as u16,
        maximum_length: byte_length as u16,
        buffer: wide.as_mut_ptr(),
    };
    let attributes = ObjectAttributes {
        length: size_of::<ObjectAttributes>() as u32,
        root_directory: handle(parent),
        object_name: &mut unicode,
        attributes: 0x40, // OBJ_CASE_INSENSITIVE
        security_descriptor: std::ptr::null_mut(),
        security_quality_of_service: std::ptr::null_mut(),
    };
    // All handles are synchronous, every named component opens the reparse
    // object itself, and stages cannot replace a pre-existing directory entry.
    let (access, share, disposition, options) = match kind {
        RelativeOpen::Directory { deny_write } => (
            FILE_LIST_DIRECTORY.0 | FILE_READ_ATTRIBUTES.0 | 0x20 | 0x100000, // TRAVERSE | SYNCHRONIZE
            if deny_write { 1 } else { 1 | 2 },
            1,
            0x00200000 | 0x20 | 1, // OPEN, REPARSE | SYNCHRONOUS | DIRECTORY
        ),
        RelativeOpen::Stage => (
            1 | 2 | FILE_READ_ATTRIBUTES.0 | DELETE.0 | 0x100000,
            1,
            2,
            0x00200000 | 0x20 | 0x40, // CREATE, REPARSE | SYNCHRONOUS | NON_DIRECTORY
        ),
        RelativeOpen::Inspect => (
            FILE_READ_ATTRIBUTES.0 | 0x100000,
            1 | 2 | 4,
            1,
            0x00200000 | 0x20,
        ),
        RelativeOpen::Reader => (
            1 | FILE_READ_ATTRIBUTES.0 | 0x100000,
            1,
            1,
            0x00200000 | 0x20,
        ),
    };
    let mut raw = HANDLE::default();
    let mut status_block = IoStatusBlock {
        status: NativeStatus { status: 0 },
        information: 0,
    };
    // NtCreateFile, possible status translation and reserved CloseHandle.
    control.charge(FileCost {
        io_operations: 3,
        ..FileCost::default()
    })?;
    let status = unsafe {
        NtCreateFile(
            &mut raw,
            access,
            &attributes,
            &mut status_block,
            std::ptr::null(),
            0x80,
            share,
            disposition,
            options,
            std::ptr::null(),
            0,
        )
    };
    if status < 0 {
        let error = io::Error::from_raw_os_error(unsafe { RtlNtStatusToDosError(status) } as i32);
        if optional && error.kind() == io::ErrorKind::NotFound {
            control.check()?;
            return Ok(None);
        }
        return Err(error.into());
    }
    let file = unsafe { File::from_raw_handle(raw.0) };
    control.check()?;
    Ok(Some(file))
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}
fn win_error(error: windows::core::Error) -> io::Error {
    // Preserve the Win32 error kind (including existing-file and sharing errors).
    io::Error::from_raw_os_error((error.code().0 as u32 & 0xffff) as i32)
}
fn handle(file: &File) -> HANDLE {
    HANDLE(file.as_raw_handle())
}
fn valid_name(name: &std::ffi::OsStr) -> bool {
    let units: Vec<u16> = name.encode_wide().collect();
    if units.is_empty()
        || matches!(units.last(), Some(32 | 46))
        || units
            .iter()
            .any(|&u| u < 32 || [34, 42, 58, 60, 62, 63, 124].contains(&u))
    {
        return false;
    }
    let base = name
        .to_string_lossy()
        .split('.')
        .next()
        .unwrap_or("")
        .to_ascii_uppercase();
    if ["CON", "PRN", "AUX", "NUL"].contains(&base.as_str()) {
        return false;
    }
    !["COM", "LPT"].iter().any(|prefix| {
        base.strip_prefix(prefix).is_some_and(|suffix| {
            matches!(
                suffix,
                "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
            )
        })
    })
}

/// A fixed directory with no observed reparse components. Names are walked
/// relative to held directory handles. Ancestors deny delete; the selected
/// directory also denies reparse writers without blocking sibling workspaces.
pub struct Directory {
    path: PathBuf,
    _directories: Vec<File>,
}
impl Directory {
    pub fn open(directory: &Path) -> io::Result<Self> {
        Self::open_mode(directory, true)
    }
    /// Hold the home identity without blocking its own atomic metadata updates.
    /// Body destinations separately hold their exact publication directory.
    pub fn open_shared(directory: &Path) -> io::Result<Self> {
        Self::open_mode(directory, false)
    }
    /// The same shared identity policy under the caller's original control.
    pub fn open_shared_controlled<C: FileControl>(
        directory: &Path,
        control: &mut C,
    ) -> Result<Self, C::Error> {
        Self::open_mode_controlled(directory, false, control)
    }
    /// Consume every held ancestor in reverse order. Equal errors from separate
    /// close operations remain separate events; no consumed handle is retried.
    pub fn close(self) -> Result<(), Vec<io::Error>> {
        self.close_with(super::close_file)
    }
    fn close_with(
        mut self,
        mut close: impl FnMut(File) -> io::Result<()>,
    ) -> Result<(), Vec<io::Error>> {
        let mut errors = Vec::new();
        while let Some(file) = self._directories.pop() {
            if let Err(error) = close(file) {
                errors.push(error);
            }
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }
    fn open_mode(directory: &Path, deny_write: bool) -> io::Result<Self> {
        Self::open_mode_controlled(directory, deny_write, &mut Uncontrolled)
    }
    pub fn open_controlled<C: FileControl>(
        directory: &Path,
        control: &mut C,
    ) -> Result<Self, C::Error> {
        Self::open_mode_controlled(directory, true, control)
    }
    fn open_mode_controlled<C: FileControl>(
        directory: &Path,
        deny_write: bool,
        control: &mut C,
    ) -> Result<Self, C::Error> {
        if directory.as_os_str().len() > 32768 {
            return Err(invalid("directory exceeds path admission").into());
        }
        control.charge(FileCost {
            allocation_bytes: directory.as_os_str().len() as u64 * 128 + 4096,
            work_units: directory.as_os_str().len() as u64 * 128 + 4096,
            ..FileCost::default()
        })?;
        if directory
            .components()
            .any(|component| matches!(component, Component::Normal(name) if !valid_name(name)))
        {
            return Err(invalid("output directory has an ambiguous component").into());
        }
        // absolute() may incorporate a long current directory even when the
        // supplied relative path is short. Reserve its bounded Windows path
        // buffers before that filesystem API, then charge the resulting walk.
        control.charge(FileCost {
            allocation_bytes: 1 << 20,
            work_units: 32768,
            ..FileCost::default()
        })?;
        let path = controlled_io(control, || std::path::absolute(directory))?;
        if path.as_os_str().len() > 32768 || path.components().count() > 1024 {
            return Err(invalid("directory exceeds component admission").into());
        }
        control.charge(FileCost {
            allocation_bytes: path.as_os_str().len() as u64 * 128 + 4096,
            work_units: path.as_os_str().len() as u64 * 128 + 4096,
            ..FileCost::default()
        })?;
        let mut current = PathBuf::new();
        let mut directories = Vec::new();
        let components: Vec<_> = path.components().collect();
        for (index, component) in components.iter().copied().enumerate() {
            let final_directory = index + 1 == components.len();
            match component {
                Component::Prefix(prefix) => {
                    if !matches!(
                        prefix.kind(),
                        Prefix::Disk(_)
                            | Prefix::UNC(_, _)
                            | Prefix::VerbatimDisk(_)
                            | Prefix::VerbatimUNC(_, _)
                    ) {
                        return Err(invalid("device namespace is not an output directory").into());
                    }
                    current.push(component.as_os_str());
                    continue;
                }
                Component::RootDir => current.push(component.as_os_str()),
                Component::Normal(name) if valid_name(name) => current.push(name),
                _ => return Err(invalid("output directory has an ambiguous component").into()),
            }
            let file = if let Some(parent) = directories.last() {
                relative_file_controlled(
                    parent,
                    component.as_os_str(),
                    RelativeOpen::Directory {
                        deny_write: deny_write && final_directory,
                    },
                    control,
                )?
            } else {
                // Only the volume/share root uses a full path; every subsequent
                // component is anchored to the preceding original directory.
                let wide: Vec<u16> = current.as_os_str().encode_wide().chain(Some(0)).collect();
                control.charge(FileCost {
                    io_operations: 3,
                    ..FileCost::default()
                })?;
                let raw = unsafe {
                    CreateFileW(
                        PCWSTR(wide.as_ptr()),
                        FILE_LIST_DIRECTORY.0 | FILE_READ_ATTRIBUTES.0,
                        if deny_write && final_directory {
                            FILE_SHARE_READ
                        } else {
                            FILE_SHARE_READ | FILE_SHARE_WRITE
                        },
                        None,
                        OPEN_EXISTING,
                        FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
                        None,
                    )
                }
                .map_err(|e| C::Error::from(win_error(e)))?;
                let file = unsafe { File::from_raw_handle(raw.0) };
                control.check()?;
                file
            };
            let attrs = controlled_io(control, || file.metadata())?.file_attributes();
            if attrs & FILE_ATTRIBUTE_DIRECTORY.0 == 0
                || attrs & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0
            {
                return Err(invalid("output directory is not a regular directory").into());
            }
            directories.push(file);
        }
        if directories.is_empty() {
            return Err(invalid("output requires an existing parent").into());
        }
        Ok(Self {
            path,
            _directories: directories,
        })
    }
    pub fn path(&self) -> &Path {
        &self.path
    }
    pub fn verify(&self) -> io::Result<()> {
        self.verify_controlled(&mut Uncontrolled)
    }
    pub fn verify_controlled<C: FileControl>(&self, control: &mut C) -> Result<(), C::Error> {
        for file in &self._directories {
            let attrs = controlled_io(control, || file.metadata())?.file_attributes();
            if attrs & FILE_ATTRIBUTE_DIRECTORY.0 == 0
                || attrs & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0
            {
                return Err(invalid("held output ancestry became a reparse point").into());
            }
        }
        Ok(())
    }
    fn parent_handle(&self) -> &File {
        self._directories.last().expect("held selected directory")
    }
    /// Open this exact directory entry without following a reparse point.
    /// The consumer must validate its regular-file kind and admitted length.
    pub fn open_file(&self, name: &std::ffi::OsStr) -> io::Result<File> {
        self.open_file_controlled(name, &mut Uncontrolled)
    }
    pub fn open_file_controlled<C: FileControl>(
        &self,
        name: &std::ffi::OsStr,
        control: &mut C,
    ) -> Result<File, C::Error> {
        self.verify_controlled(control)?;
        relative_file_controlled(self.parent_handle(), name, RelativeOpen::Reader, control)
    }
    pub fn open_optional_file_controlled<C: FileControl>(
        &self,
        name: &std::ffi::OsStr,
        control: &mut C,
    ) -> Result<Option<File>, C::Error> {
        self.verify_controlled(control)?;
        relative_file_optional(
            self.parent_handle(),
            name,
            RelativeOpen::Reader,
            true,
            control,
        )
    }
    /// Inspect size/kind without data-read permission or an allocation. This
    /// remains possible while another writer exists; opening admitted data for
    /// reading separately applies deny-write/delete ownership.
    pub fn metadata(&self, name: &std::ffi::OsStr) -> io::Result<std::fs::Metadata> {
        self.metadata_controlled(name, &mut Uncontrolled)
    }
    pub fn metadata_controlled<C: FileControl>(
        &self,
        name: &std::ffi::OsStr,
        control: &mut C,
    ) -> Result<std::fs::Metadata, C::Error> {
        self.verify_controlled(control)?;
        let file =
            relative_file_controlled(self.parent_handle(), name, RelativeOpen::Inspect, control)?;
        controlled_io(control, || file.metadata())
    }
    pub fn optional_metadata_controlled<C: FileControl>(
        &self,
        name: &std::ffi::OsStr,
        control: &mut C,
    ) -> Result<Option<std::fs::Metadata>, C::Error> {
        self.verify_controlled(control)?;
        let Some(file) = relative_file_optional(
            self.parent_handle(),
            name,
            RelativeOpen::Inspect,
            true,
            control,
        )?
        else {
            return Ok(None);
        };
        controlled_io(control, || file.metadata()).map(Some)
    }
}

pub struct Destination {
    path: PathBuf,
    _directory: Directory,
    overwrite: bool,
}
impl Destination {
    pub fn open(output: &Path, overwrite: bool) -> io::Result<Self> {
        if output.as_os_str().is_empty()
            || matches!(output.as_os_str().encode_wide().last(), Some(47 | 92))
        {
            return Err(invalid("output must name a file"));
        }
        // GetFullPathName (used by absolute) normalizes trailing dots/spaces.
        // Validate original components before that normalization can hide them.
        if output
            .components()
            .any(|component| matches!(component, Component::Normal(name) if !valid_name(name)))
        {
            return Err(invalid(
                "output contains a device, stream or ambiguous name",
            ));
        }
        let path = std::path::absolute(output)?;
        let parent = path
            .parent()
            .ok_or_else(|| invalid("output requires an existing parent"))?;
        let name = path
            .file_name()
            .ok_or_else(|| invalid("output must name a file"))?;
        if !valid_name(name) {
            return Err(invalid(
                "output contains a device, stream or ambiguous name",
            ));
        }
        let directory = Directory::open(parent)?;
        let destination = Self {
            path,
            _directory: directory,
            overwrite,
        };
        destination.check()?;
        Ok(destination)
    }
    pub fn path(&self) -> &Path {
        &self.path
    }
    fn check(&self) -> io::Result<()> {
        // Refuse a currently observed reparse target. If the directory entry
        // changes after this check, the handle-based rename replaces that entry
        // (or fails); it never opens or writes through the replacement target.
        self._directory.verify()?;
        match relative_file(
            self._directory.parent_handle(),
            self.path.file_name().expect("validated simple name"),
            RelativeOpen::Inspect,
        ) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error),
            Ok(file) => {
                let metadata = file.metadata()?;
                if !self.overwrite
                    || !metadata.is_file()
                    || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0
                {
                    Err(io::Error::new(
                        io::ErrorKind::AlreadyExists,
                        "output already exists or is not a regular file",
                    ))
                } else {
                    Ok(())
                }
            }
        }
    }
    /// The caller's control is checked before reading, at each 64 KiB block and
    /// before publication. The source must have exactly the declared length.
    pub fn publish_reader(
        &self,
        reader: &mut impl Read,
        length: u64,
        maximum: u64,
        mut check: impl FnMut() -> io::Result<()>,
    ) -> io::Result<()> {
        check()?;
        if length > maximum {
            return Err(invalid("output exceeds publication budget"));
        }
        self.check()?;
        let mut stage = Stage::create(&self._directory)?;
        let result = (|| {
            let mut buffer = [0u8; 65536];
            let mut remaining = length;
            while remaining != 0 {
                check()?;
                let admitted = buffer.len().min(remaining as usize);
                let n = reader.read(&mut buffer[..admitted])?;
                if n == 0 {
                    return Err(io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        "output source ended before its declared length",
                    ));
                }
                check()?;
                stage.file.write_all(&buffer[..n])?;
                remaining -= n as u64;
            }
            check()?;
            if reader.read(&mut buffer[..1])? != 0 {
                return Err(invalid("output source exceeds its declared length"));
            }
            stage.file.sync_all()?;
            check()?;
            self.check()?;
            stage.rename(&self.path, self.overwrite)?;
            Ok(())
        })();
        if result.is_err() {
            // Delete exactly the opened stage through its handle, never by a
            // potentially substituted pathname. Drop retries if this fails.
            stage.discard()?;
        }
        result
    }
    pub fn publish(
        &self,
        bytes: &[u8],
        maximum: u64,
        check: impl FnMut() -> io::Result<()>,
    ) -> io::Result<()> {
        self.publish_reader(
            &mut io::Cursor::new(bytes),
            bytes.len() as u64,
            maximum,
            check,
        )
    }
}

struct Stage {
    file: File,
    published: bool,
}
impl Stage {
    fn create(parent: &Directory) -> io::Result<Self> {
        parent.verify()?;
        for _ in 0..64 {
            let token = crate::random::token().map_err(io::Error::other)?;
            let name = format!(".lycheedev-export-{token}");
            let opened = relative_file(
                parent.parent_handle(),
                std::ffi::OsStr::new(&name),
                RelativeOpen::Stage,
            );
            match opened {
                Ok(file) => {
                    return Ok(Self {
                        file,
                        published: false,
                    });
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error),
            }
        }
        Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "stage name collision budget exhausted",
        ))
    }
    fn rename(&mut self, destination: &Path, overwrite: bool) -> io::Result<()> {
        // A simple name with NULL RootDirectory is the documented same-directory
        // rename form. It uses the stage's original parent, without reopening our
        // write-protected parent directory by pathname.
        let name = destination
            .file_name()
            .ok_or_else(|| invalid("rename requires a simple name"))?;
        let wide: Vec<u16> = name.encode_wide().collect();
        let prefix = offset_of!(FILE_RENAME_INFO, FileName);
        let bytes = prefix + (wide.len() + 1) * 2;
        // Vec<u64> provides the native structure's required eight-byte alignment.
        let mut buffer = vec![0u64; bytes.div_ceil(8)];
        let info = buffer.as_mut_ptr().cast::<FILE_RENAME_INFO>();
        unsafe {
            (*info).Anonymous = if overwrite {
                // Documented FileRenameInformationEx: replace the directory
                // entry while retaining old open readers on the original file.
                FILE_RENAME_INFO_0 { Flags: 1 | 2 } // REPLACE_IF_EXISTS | POSIX_SEMANTICS
            } else {
                FILE_RENAME_INFO_0 {
                    ReplaceIfExists: false,
                }
            };
            (*info).RootDirectory = HANDLE::default();
            (*info).FileNameLength = (wide.len() * 2) as u32;
            std::ptr::copy_nonoverlapping(wide.as_ptr(), (*info).FileName.as_mut_ptr(), wide.len());
            let mut status_block = IoStatusBlock {
                status: NativeStatus { status: 0 },
                information: 0,
            };
            // Stage has synchronous I/O (no FILE_FLAG_OVERLAPPED), so the call
            // returns the final status before these buffers expire.
            let status = NtSetInformationFile(
                handle(&self.file),
                &mut status_block,
                info.cast(),
                bytes as u32,
                if overwrite { 65 } else { 10 },
            ); // FileRenameInformationEx / FileRenameInformation
            if status < 0 {
                return Err(io::Error::from_raw_os_error(
                    RtlNtStatusToDosError(status) as i32
                ));
            }
        }
        self.published = true;
        Ok(())
    }
    fn discard(&mut self) -> io::Result<()> {
        if !self.published {
            let info = FILE_DISPOSITION_INFO { DeleteFile: true };
            unsafe {
                SetFileInformationByHandle(
                    handle(&self.file),
                    FileDispositionInfo,
                    (&info as *const FILE_DISPOSITION_INFO).cast(),
                    size_of::<FILE_DISPOSITION_INFO>() as u32,
                )
            }
            .map_err(win_error)?;
            self.published = true;
        }
        Ok(())
    }
}
impl Drop for Stage {
    fn drop(&mut self) {
        let _ = self.discard();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    mod reparse {
        include!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/support/reparse.rs"
        ));
    }
    #[derive(Default)]
    struct Meter {
        cost: FileCost,
        fail_io: bool,
    }
    impl FileControl for Meter {
        type Error = io::Error;
        fn charge(&mut self, cost: FileCost) -> io::Result<()> {
            if self.fail_io && cost.io_operations != 0 {
                return Err(io::Error::other("fixture.io_budget"));
            }
            self.cost.io_operations += cost.io_operations;
            self.cost.allocation_bytes += cost.allocation_bytes;
            self.cost.work_units += cost.work_units;
            Ok(())
        }
        fn check(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    #[test]
    fn controlled_directory_owns_ancestry_and_debits_open_query_and_close() {
        let root = std::env::temp_dir().join(format!(
            "lychee-controlled-directory-{}",
            crate::random::token().unwrap()
        ));
        let selected = root.join("parent").join("selected");
        fs::create_dir_all(&selected).unwrap();
        fs::write(selected.join("identity"), b"original").unwrap();
        let mut meter = Meter::default();
        let directory = Directory::open_controlled(&selected, &mut meter).unwrap();
        assert!(meter.cost.io_operations >= directory._directories.len() as u64 * 4);
        assert!(meter.cost.allocation_bytes > 0 && meter.cost.work_units > 0);
        assert!(fs::rename(selected.parent().unwrap(), root.join("moved")).is_err());
        assert!(reparse::reparse_writer(&selected).is_err());
        let before = meter.cost.io_operations;
        let mut file = directory
            .open_file_controlled("identity".as_ref(), &mut meter)
            .unwrap();
        assert_eq!(
            meter.cost.io_operations - before,
            directory._directories.len() as u64 + 3
        );
        let mut content = String::new();
        file.read_to_string(&mut content).unwrap();
        assert_eq!(content, "original");
        drop(file);
        assert!(
            directory
                .optional_metadata_controlled("missing".as_ref(), &mut meter)
                .unwrap()
                .is_none()
        );
        meter.fail_io = true;
        assert_eq!(
            directory
                .open_file_controlled("identity".as_ref(), &mut meter)
                .unwrap_err()
                .to_string(),
            "fixture.io_budget"
        );
        drop(directory);
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn controlled_relative_reader_refuses_junction_inserted_after_verify() {
        let root = std::env::temp_dir().join(format!(
            "lychee-controlled-reader-race-{}",
            crate::random::token().unwrap()
        ));
        let selected = root.join("selected");
        let outside = root.join("outside");
        fs::create_dir_all(&selected).unwrap();
        fs::create_dir(&outside).unwrap();
        fs::write(outside.join("identity"), b"outside").unwrap();
        let directory = Directory::open_shared(&selected).unwrap();
        struct Mutate {
            writer: Option<File>,
            outside: PathBuf,
        }
        impl FileControl for Mutate {
            type Error = io::Error;
            fn charge(&mut self, cost: FileCost) -> io::Result<()> {
                if cost.io_operations == 3 {
                    if let Some(writer) = self.writer.take() {
                        reparse::set_junction(&writer, &self.outside)?;
                    }
                }
                Ok(())
            }
            fn check(&mut self) -> io::Result<()> {
                Ok(())
            }
        }
        let mut control = Mutate {
            writer: Some(reparse::reparse_writer(&selected).unwrap()),
            outside: outside.clone(),
        };
        assert!(
            directory
                .open_file_controlled("identity".as_ref(), &mut control)
                .is_err()
        );
        assert!(
            control.writer.is_none(),
            "mutation happened at the actual open boundary"
        );
        assert_eq!(fs::read(outside.join("identity")).unwrap(), b"outside");
        drop(directory);
        fs::remove_dir(&selected).unwrap();
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn controlled_filesystem_keeps_os_error_before_later_cancellation() {
        let root = std::env::temp_dir().join(format!(
            "lychee-controlled-error-{}",
            crate::random::token().unwrap()
        ));
        fs::create_dir(&root).unwrap();
        fs::write(root.join("present"), b"x").unwrap();
        let directory = Directory::open(&root).unwrap();
        struct CancelAfterOpen(bool);
        impl FileControl for CancelAfterOpen {
            type Error = io::Error;
            fn charge(&mut self, cost: FileCost) -> io::Result<()> {
                if cost.io_operations == 3 {
                    self.0 = true;
                }
                Ok(())
            }
            fn check(&mut self) -> io::Result<()> {
                if self.0 {
                    Err(io::Error::other("fixture.cancelled"))
                } else {
                    Ok(())
                }
            }
        }
        let error = relative_file_controlled(
            directory.parent_handle(),
            "missing".as_ref(),
            RelativeOpen::Reader,
            &mut CancelAfterOpen(false),
        )
        .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::NotFound);
        let error = relative_file_controlled(
            directory.parent_handle(),
            "present".as_ref(),
            RelativeOpen::Reader,
            &mut CancelAfterOpen(false),
        )
        .unwrap_err();
        assert_eq!(error.to_string(), "fixture.cancelled");
        // Cancellation after open must drop the handle whose close was reserved.
        fs::write(root.join("present"), b"after cancellation").unwrap();
        drop(directory);
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn changed_ancestor_cannot_redirect_relative_walk_or_stage_creation() {
        let root = std::env::temp_dir().join(format!(
            "lycheedev-relative-reparse-{}-{}",
            std::process::id(),
            crate::random::token().unwrap()
        ));
        let ancestor = root.join("ancestor");
        let outside = root.join("outside");
        fs::create_dir_all(&ancestor).unwrap();
        fs::create_dir(&outside).unwrap();
        fs::create_dir(outside.join("next")).unwrap();
        fs::write(outside.join("sentinel"), b"outside original").unwrap();
        let held = Directory::open_shared(&ancestor).unwrap();
        held.verify().unwrap();
        // Exact walk/create boundary: convert the original opened directory
        // after its attributes were checked. Its new junction has a plausible
        // next component outside the intended directory.
        reparse::set_junction(&reparse::reparse_writer(&ancestor).unwrap(), &outside).unwrap();
        assert!(held.verify().is_err());
        assert!(
            relative_file(
                held.parent_handle(),
                std::ffi::OsStr::new("next"),
                RelativeOpen::Directory { deny_write: true }
            )
            .is_err()
        );
        let error = relative_file(
            held.parent_handle(),
            std::ffi::OsStr::new("owned-stage"),
            RelativeOpen::Stage,
        )
        .unwrap_err();
        assert_eq!(error.raw_os_error(), Some(1921)); // ERROR_CANT_RESOLVE_FILENAME
        assert!(!outside.join("owned-stage").exists());
        assert_eq!(
            fs::read(outside.join("sentinel")).unwrap(),
            b"outside original"
        );
        assert_eq!(fs::read_dir(&outside).unwrap().count(), 2);
        drop(held);
        fs::remove_dir(&ancestor).unwrap();
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn reparse_entry_inserted_after_final_check_is_never_followed_by_handle_rename() {
        let root = std::env::temp_dir().join(format!(
            "lycheedev-rename-race-{}-{}",
            std::process::id(),
            crate::random::token().unwrap()
        ));
        let parent = root.join("parent");
        let outside = root.join("outside");
        fs::create_dir_all(&parent).unwrap();
        fs::create_dir(&outside).unwrap();
        fs::write(outside.join("sentinel"), b"original outside bytes").unwrap();
        let output = parent.join("body");
        let destination = Destination::open(&output, true).unwrap();
        let mut stage = Stage::create(&destination._directory).unwrap();
        stage.file.write_all(b"complete staged body").unwrap();
        stage.file.sync_all().unwrap();
        destination.check().unwrap();
        // Exact production boundary: a reparse target is inserted after check
        // but before NtSetInformationFile.
        fs::create_dir(&output).unwrap();
        reparse::set_junction(&reparse::reparse_writer(&output).unwrap(), &outside).unwrap();
        stage.rename(&output, true).unwrap();
        assert_eq!(
            fs::read(outside.join("sentinel")).unwrap(),
            b"original outside bytes"
        );
        assert_eq!(fs::read(&output).unwrap(), b"complete staged body");
        assert_eq!(fs::read_dir(&outside).unwrap().count(), 1);
        stage.discard().unwrap();
        drop(stage);
        drop(destination);
        fs::remove_file(&output).unwrap();
        assert_eq!(fs::read_dir(&parent).unwrap().count(), 0);
        fs::remove_dir_all(root).unwrap();
    }
}

#[cfg(test)]
#[path = "destination/admission_tests.rs"]
mod admission_tests;
