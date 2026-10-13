//! Same-logon-only local named pipes. No source framing, leases or business here.
use crate::Result;
// Windows SDK winnt.h, SE_GROUP_LOGON_ID (not an account-SID attribute).
const LOGON_ID_GROUP: u32 = 0xc0000000;
use sha2::{Digest, Sha256};
use std::{
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
use windows::{
    Win32::{
        Foundation::*,
        Security::{Authorization::*, *},
        Storage::FileSystem::*,
        System::{IO::*, Pipes::*, Threading::*},
    },
    core::{PCWSTR, PWSTR},
};
struct Handle(HANDLE);
unsafe impl Send for Handle {}
impl Drop for Handle {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}
struct Descriptor(PSECURITY_DESCRIPTOR);
impl Drop for Descriptor {
    fn drop(&mut self) {
        unsafe {
            let _ = LocalFree(Some(HLOCAL(self.0.0)));
        }
    }
}
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}
fn connection_error(error: windows::core::Error) -> String {
    let kind = if error.code() == ERROR_FILE_NOT_FOUND.to_hresult() {
        "broker.not_found"
    } else if error.code() == ERROR_PIPE_BUSY.to_hresult() {
        "broker.busy"
    } else if error.code() == ERROR_ACCESS_DENIED.to_hresult() {
        "broker.access_denied"
    } else {
        return error.to_string();
    };
    format!("{kind}:0x{:08x}", error.code().0 as u32)
}
fn hex(b: &[u8]) -> String {
    b.iter().map(|b| format!("{b:02x}")).collect()
}
#[derive(Clone)]
pub struct BrokerScope {
    endpoint: String,
    logon_hash: String,
    scope_dir: PathBuf,
    sddl: String,
}
impl BrokerScope {
    pub fn for_home(home: &Path) -> Result<Self> {
        Self::for_kind(home, "source")
    }
    pub fn for_native_source(home: &Path) -> Result<Self> {
        Self::for_kind(home, "native-source")
    }
    fn for_kind(home: &Path, kind: &str) -> Result<Self> {
        let home = std::fs::canonicalize(home).map_err(|e| e.to_string())?;
        let sid = logon_sid()?;
        let identity = home
            .to_string_lossy()
            .trim_start_matches("\\\\?\\")
            .to_lowercase();
        let digest = hex(&Sha256::digest(format!("{identity}\0{sid}").as_bytes()));
        let logon_hash = hex(&Sha256::digest(sid.as_bytes()))[..32].to_owned();
        Ok(Self {
            endpoint: format!("\\\\.\\pipe\\lycheedev-{kind}-{}", &digest[..32]),
            scope_dir: std::env::temp_dir()
                .join(format!("lycheedev-{kind}-broker"))
                .join(&logon_hash),
            logon_hash,
            sddl: format!("D:P(A;;GA;;;{sid})"),
        })
    }
    pub fn endpoint(&self) -> &str {
        &self.endpoint
    }
    pub fn logon_hash(&self) -> &str {
        &self.logon_hash
    }
    pub fn scope_dir(&self) -> &Path {
        &self.scope_dir
    }
}

/// Retire only the already authenticated local CLI owner, holding its exact
/// creation identity throughout termination and waiting.
pub fn retire_native_source_peer(
    pid: u32,
    created: u64,
    executable: &Path,
    deadline: Instant,
) -> Result<()> {
    NativeSourcePeer::open(pid, created, executable)?.retire(deadline)
}

/// Hold retirement access before submitting a request. Reopening a dying
/// process after EOF can fail and obscure the original worker failure.
pub struct NativeSourcePeer {
    process: Handle,
}
impl NativeSourcePeer {
    pub fn open(pid: u32, created: u64, executable: &Path) -> Result<Self> {
        unsafe {
            let process = Handle(
                OpenProcess(
                    PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_TERMINATE | PROCESS_SYNCHRONIZE,
                    false,
                    pid,
                )
                .map_err(|e| e.to_string())?,
            );
            let (mut birth, mut exit, mut kernel, mut user) = (
                FILETIME::default(),
                FILETIME::default(),
                FILETIME::default(),
                FILETIME::default(),
            );
            GetProcessTimes(process.0, &mut birth, &mut exit, &mut kernel, &mut user)
                .map_err(|e| e.to_string())?;
            if ((birth.dwHighDateTime as u64) << 32 | birth.dwLowDateTime as u64) != created {
                return Err("source.owner_identity_changed".into());
            }
            if WaitForSingleObject(process.0, 0) == WAIT_OBJECT_0 {
                return Ok(Self { process });
            }
            let mut path = vec![0u16; 32768];
            let mut count = path.len() as u32;
            QueryFullProcessImageNameW(
                process.0,
                PROCESS_NAME_WIN32,
                PWSTR(path.as_mut_ptr()),
                &mut count,
            )
            .map_err(|e| e.to_string())?;
            let image = PathBuf::from(String::from_utf16_lossy(&path[..count as usize]));
            let image = std::fs::canonicalize(image).map_err(|e| e.to_string())?;
            let expected = std::fs::canonicalize(executable).map_err(|e| e.to_string())?;
            if !image
                .to_string_lossy()
                .eq_ignore_ascii_case(&expected.to_string_lossy())
            {
                return Err("source.owner_executable_changed".into());
            }
            Ok(Self { process })
        }
    }
    pub fn retire(&self, deadline: Instant) -> Result<()> {
        unsafe {
            let process = &self.process;
            if WaitForSingleObject(process.0, 0) == WAIT_OBJECT_0 {
                return Ok(());
            }
            if let Err(error) = TerminateProcess(process.0, 5)
                && WaitForSingleObject(process.0, 0) != WAIT_OBJECT_0
            {
                return Err(error.to_string());
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if WaitForSingleObject(
                process.0,
                remaining.as_millis().min(u32::MAX as u128) as u32,
            ) != WAIT_OBJECT_0
            {
                return Err("source.owner_retirement_deadline".into());
            }
        }
        Ok(())
    }
}
fn logon_sid() -> Result<String> {
    unsafe {
        let mut token = HANDLE::default();
        OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token)
            .map_err(|e| e.to_string())?;
        let token = Handle(token);
        let mut bytes = 0;
        let _ = GetTokenInformation(token.0, TokenGroups, None, 0, &mut bytes);
        if bytes == 0 || bytes > 128 * 1024 {
            return Err("broker.token_budget".into());
        }
        let mut storage = vec![0u64; (bytes as usize).div_ceil(8)];
        GetTokenInformation(
            token.0,
            TokenGroups,
            Some(storage.as_mut_ptr() as *mut _),
            bytes,
            &mut bytes,
        )
        .map_err(|e| e.to_string())?;
        let groups = &*(storage.as_ptr() as *const TOKEN_GROUPS);
        let count = groups.GroupCount as usize;
        let offset = std::mem::offset_of!(TOKEN_GROUPS, Groups);
        if count > 4096
            || offset + count * std::mem::size_of::<SID_AND_ATTRIBUTES>() > bytes as usize
        {
            return Err("broker.token_groups_bounds".into());
        }
        let entries = std::slice::from_raw_parts(groups.Groups.as_ptr(), count);
        for group in entries {
            if group.Attributes & LOGON_ID_GROUP == LOGON_ID_GROUP {
                let mut text = PWSTR::null();
                ConvertSidToStringSidW(group.Sid, &mut text).map_err(|e| e.to_string())?;
                let result = text.to_string().map_err(|e| e.to_string());
                let _ = LocalFree(Some(HLOCAL(text.0 as *mut _)));
                return result;
            }
        }
        Err("broker.logon_sid_unavailable".into())
    }
}
pub struct BrokerListener {
    scope: BrokerScope,
    pending: Handle,
}
impl BrokerListener {
    pub fn bind(scope: &BrokerScope) -> Result<Self> {
        Ok(Self {
            scope: scope.clone(),
            pending: create(scope, true)?,
        })
    }
    pub fn accept(&mut self, deadline: Instant, cancel: &AtomicBool) -> Result<BrokerStream> {
        check(deadline, cancel)?;
        let event = event()?;
        let mut overlapped = Box::new(OVERLAPPED::default());
        overlapped.hEvent = event.0;
        unsafe {
            match ConnectNamedPipe(self.pending.0, Some(&mut *overlapped)) {
                Ok(()) => {}
                Err(error) if error.code() == ERROR_PIPE_CONNECTED.to_hresult() => {}
                Err(error) if error.code() == ERROR_IO_PENDING.to_hresult() => {
                    complete(self.pending.0, &overlapped, deadline, cancel)?;
                }
                Err(error) => return Err(error.to_string()),
            }
        }
        // Keep one server instance present while handing the connected one out.
        // This prevents another process becoming the first server in a gap.
        let replacement = create(&self.scope, false)?;
        let connected = std::mem::replace(&mut self.pending, replacement);
        Ok(BrokerStream {
            handle: connected,
            server: true,
            deadline,
            cancel: Arc::new(AtomicBool::new(false)),
        })
    }
}
fn create(scope: &BrokerScope, first: bool) -> Result<Handle> {
    unsafe {
        let sddl = wide(&scope.sddl);
        let mut descriptor = PSECURITY_DESCRIPTOR::default();
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            PCWSTR(sddl.as_ptr()),
            SDDL_REVISION_1,
            &mut descriptor,
            None,
        )
        .map_err(|e| e.to_string())?;
        let descriptor = Descriptor(descriptor);
        let attributes = SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: descriptor.0.0,
            bInheritHandle: windows::core::BOOL(0),
        };
        let name = wide(&scope.endpoint);
        let flags = PIPE_ACCESS_DUPLEX
            | FILE_FLAG_OVERLAPPED
            | if first {
                FILE_FLAG_FIRST_PIPE_INSTANCE
            } else {
                FILE_FLAGS_AND_ATTRIBUTES(0)
            };
        let handle = CreateNamedPipeW(
            PCWSTR(name.as_ptr()),
            flags,
            PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
            64,
            65536,
            65536,
            0,
            Some(&attributes),
        );
        if handle == INVALID_HANDLE_VALUE {
            return Err(format!("broker.pipe_create:{}", GetLastError().0));
        }
        Ok(Handle(handle))
    }
}
fn event() -> Result<Handle> {
    unsafe {
        CreateEventW(None, true, false, None)
            .map(Handle)
            .map_err(|e| e.to_string())
    }
}
fn check(deadline: Instant, cancel: &AtomicBool) -> Result<()> {
    if cancel.load(Ordering::Acquire) {
        Err("broker.cancelled".into())
    } else if Instant::now() >= deadline {
        Err("broker.deadline".into())
    } else {
        Ok(())
    }
}
fn complete(
    handle: HANDLE,
    overlapped: &OVERLAPPED,
    deadline: Instant,
    cancel: &AtomicBool,
) -> Result<u32> {
    loop {
        if let Err(reason) = check(deadline, cancel) {
            unsafe {
                let _ = CancelIoEx(handle, Some(overlapped));
                let mut bytes = 0;
                // The buffer/OVERLAPPED remain alive until terminal completion.
                let _ = GetOverlappedResult(handle, overlapped, &mut bytes, true);
            }
            return Err(reason);
        }
        let wait = deadline
            .saturating_duration_since(Instant::now())
            .min(Duration::from_millis(50));
        let state =
            unsafe { WaitForSingleObject(overlapped.hEvent, wait.as_millis().max(1) as u32) };
        if state == WAIT_TIMEOUT {
            continue;
        }
        if state != WAIT_OBJECT_0 {
            unsafe {
                let _ = CancelIoEx(handle, Some(overlapped));
                let mut bytes = 0;
                let _ = GetOverlappedResult(handle, overlapped, &mut bytes, true);
            }
            return Err("broker.wait_failed".into());
        }
        let mut bytes = 0;
        unsafe {
            GetOverlappedResult(handle, overlapped, &mut bytes, false).map_err(|e| {
                if e.code() == ERROR_BROKEN_PIPE.to_hresult()
                    || e.code() == ERROR_PIPE_NOT_CONNECTED.to_hresult()
                {
                    "broker.eof".into()
                } else {
                    e.to_string()
                }
            })?;
        }
        return Ok(bytes);
    }
}
pub struct BrokerStream {
    handle: Handle,
    server: bool,
    deadline: Instant,
    cancel: Arc<AtomicBool>,
}
impl BrokerStream {
    pub fn try_clone(&self) -> Result<Self> {
        let mut duplicate = HANDLE::default();
        unsafe {
            DuplicateHandle(
                GetCurrentProcess(),
                self.handle.0,
                GetCurrentProcess(),
                &mut duplicate,
                0,
                false,
                DUPLICATE_SAME_ACCESS,
            )
            .map_err(|e| e.to_string())?;
        }
        Ok(Self {
            handle: Handle(duplicate),
            server: self.server,
            deadline: self.deadline,
            cancel: self.cancel.clone(),
        })
    }
    pub fn connect(
        scope: &BrokerScope,
        deadline: Instant,
        cancel: Arc<AtomicBool>,
    ) -> Result<Self> {
        check(deadline, &cancel)?;
        let name = wide(&scope.endpoint);
        let handle = unsafe {
            CreateFileW(
                PCWSTR(name.as_ptr()),
                GENERIC_READ.0 | GENERIC_WRITE.0,
                FILE_SHARE_MODE(0),
                None,
                OPEN_EXISTING,
                FILE_FLAG_OVERLAPPED,
                None,
            )
            .map_err(connection_error)?
        };
        Ok(Self {
            handle: Handle(handle),
            server: false,
            deadline,
            cancel,
        })
    }
    pub fn set_deadline(&mut self, deadline: Instant, cancel: Arc<AtomicBool>) {
        self.deadline = deadline;
        self.cancel = cancel
    }
    pub fn peer_pid(&self) -> Result<u32> {
        let mut pid = 0;
        unsafe {
            if self.server {
                GetNamedPipeClientProcessId(self.handle.0, &mut pid)
            } else {
                GetNamedPipeServerProcessId(self.handle.0, &mut pid)
            }
            .map_err(|e| e.to_string())?;
        }
        if pid == 0 {
            Err("broker.peer_pid_missing".into())
        } else {
            Ok(pid)
        }
    }
    fn transfer(&mut self, buffer: &mut [u8], write: bool) -> Result<usize> {
        check(self.deadline, &self.cancel)?;
        if buffer.is_empty() {
            return Ok(0);
        }
        let event = event()?;
        let mut overlapped = Box::new(OVERLAPPED::default());
        overlapped.hEvent = event.0;
        let mut bytes = 0;
        let result = unsafe {
            if write {
                WriteFile(
                    self.handle.0,
                    Some(buffer),
                    Some(&mut bytes),
                    Some(&mut *overlapped),
                )
            } else {
                ReadFile(
                    self.handle.0,
                    Some(buffer),
                    Some(&mut bytes),
                    Some(&mut *overlapped),
                )
            }
        };
        match result {
            Ok(()) => Ok(bytes as usize),
            Err(e) if e.code() == ERROR_IO_PENDING.to_hresult() => {
                match complete(self.handle.0, &overlapped, self.deadline, &self.cancel) {
                    Err(reason) if !write && reason == "broker.eof" => Ok(0),
                    result => result.map(|n| n as usize),
                }
            }
            Err(e)
                if !write
                    && (e.code() == ERROR_BROKEN_PIPE.to_hresult()
                        || e.code() == ERROR_PIPE_NOT_CONNECTED.to_hresult()) =>
            {
                Ok(0)
            }
            Err(e) => Err(e.to_string()),
        }
    }
}
impl Read for BrokerStream {
    fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
        let length = out.len().min(65536);
        self.transfer(&mut out[..length], false)
            .map_err(std::io::Error::other)
    }
}
impl Write for BrokerStream {
    fn write(&mut self, input: &[u8]) -> std::io::Result<usize> {
        let mut owned = input[..input.len().min(65536)].to_vec();
        self.transfer(&mut owned, true)
            .map_err(std::io::Error::other)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
// Every operation drains before returning, so dropping a duplicate has no
// outstanding IO. CloseHandle on the last server handle disconnects naturally;
// DisconnectNamedPipe on each clone would destroy another clone's writer.
#[cfg(test)]
mod tests {
    use super::*;
    fn scope() -> BrokerScope {
        let path = std::env::temp_dir().join(format!(
            "lycheedev-pipe-{}",
            crate::random::token().unwrap()
        ));
        std::fs::create_dir_all(&path).unwrap();
        BrokerScope::for_home(&path).unwrap()
    }
    #[test]
    fn actual_local_pipe_peer_and_first_instance_guard() {
        let scope = scope();
        let mut listener = BrokerListener::bind(&scope).unwrap();
        assert!(BrokerListener::bind(&scope).is_err());
        let client_scope = scope.clone();
        let client = std::thread::spawn(move || {
            let mut stream = BrokerStream::connect(
                &client_scope,
                Instant::now() + Duration::from_secs(3),
                Arc::new(AtomicBool::new(false)),
            )
            .unwrap();
            assert_eq!(stream.peer_pid().unwrap(), std::process::id());
            stream.write_all(b"ping").unwrap();
            let mut response = [0; 4];
            stream.read_exact(&mut response).unwrap();
            assert_eq!(&response, b"pong");
        });
        let mut server = listener
            .accept(
                Instant::now() + Duration::from_secs(3),
                &AtomicBool::new(false),
            )
            .unwrap();
        assert_eq!(server.peer_pid().unwrap(), std::process::id());
        let mut request = [0; 4];
        server.read_exact(&mut request).unwrap();
        assert_eq!(&request, b"ping");
        server.write_all(b"pong").unwrap();
        client.join().unwrap();
        assert!(BrokerListener::bind(&scope).is_err());
        drop(server);
        drop(listener);
        assert!(BrokerListener::bind(&scope).is_ok());
    }
    #[test]
    fn sequential_accept_eof_and_clone_drop_leave_owner_listener_live() {
        let scope = scope();
        let mut listener = BrokerListener::bind(&scope).unwrap();
        for round in 0..4u8 {
            let client_scope = scope.clone();
            let client = std::thread::spawn(move || {
                let mut stream = BrokerStream::connect(
                    &client_scope,
                    Instant::now() + Duration::from_secs(3),
                    Arc::new(AtomicBool::new(false)),
                )
                .unwrap();
                stream.write_all(&[round]).unwrap();
                let mut reply = [0];
                stream.read_exact(&mut reply).unwrap();
                assert_eq!(reply[0], round + 1);
            });
            let mut server = listener
                .accept(
                    Instant::now() + Duration::from_secs(3),
                    &AtomicBool::new(false),
                )
                .unwrap();
            let observer = server.try_clone().unwrap();
            drop(observer);
            let mut value = [0];
            server.read_exact(&mut value).unwrap();
            assert_eq!(value[0], round);
            server.write_all(&[round + 1]).unwrap();
            client.join().unwrap();
            assert_eq!(server.read(&mut value).unwrap(), 0);
            drop(server);
            assert!(
                BrokerListener::bind(&scope).is_err(),
                "continuous first-instance ownership round {round}"
            );
        }
        drop(listener);
        assert!(BrokerListener::bind(&scope).is_ok());
    }
    #[test]
    fn pending_accept_timeout_cancel_drains_and_can_accept_later() {
        let scope = scope();
        let mut listener = BrokerListener::bind(&scope).unwrap();
        let start = Instant::now();
        assert!(
            listener
                .accept(start + Duration::from_millis(20), &AtomicBool::new(false))
                .is_err()
        );
        assert!(start.elapsed() < Duration::from_secs(1));
        assert!(
            listener
                .accept(
                    Instant::now() + Duration::from_secs(1),
                    &AtomicBool::new(true)
                )
                .is_err()
        );
        let client_scope = scope.clone();
        let client = std::thread::spawn(move || {
            BrokerStream::connect(
                &client_scope,
                Instant::now() + Duration::from_secs(2),
                Arc::new(AtomicBool::new(false)),
            )
            .unwrap()
        });
        let server = listener
            .accept(
                Instant::now() + Duration::from_secs(2),
                &AtomicBool::new(false),
            )
            .unwrap();
        let mut client = client.join().unwrap();
        client.set_deadline(
            Instant::now() + Duration::from_millis(20),
            Arc::new(AtomicBool::new(false)),
        );
        assert!(client.read(&mut [0; 1]).is_err());
        drop(server);
        client.set_deadline(
            Instant::now() + Duration::from_secs(1),
            Arc::new(AtomicBool::new(false)),
        );
        assert_eq!(client.read(&mut [0; 1]).unwrap(), 0);
    }
    #[test]
    fn duplicate_observer_cancel_and_drop_does_not_disconnect_writer() {
        let scope = scope();
        let mut listener = BrokerListener::bind(&scope).unwrap();
        let mut client = BrokerStream::connect(
            &scope,
            Instant::now() + Duration::from_secs(2),
            Arc::new(AtomicBool::new(false)),
        )
        .unwrap();
        let mut server = listener
            .accept(
                Instant::now() + Duration::from_secs(2),
                &AtomicBool::new(false),
            )
            .unwrap();
        let mut observer = server.try_clone().unwrap();
        observer.set_deadline(
            Instant::now() + Duration::from_millis(20),
            Arc::new(AtomicBool::new(false)),
        );
        let observed = std::thread::spawn(move || {
            assert!(observer.read(&mut [0; 1]).is_err());
            drop(observer)
        });
        observed.join().unwrap();
        server.write_all(b"ok").unwrap();
        let mut message = [0; 2];
        client.read_exact(&mut message).unwrap();
        assert_eq!(&message, b"ok");
        drop(server);
        assert_eq!(client.read(&mut [0; 1]).unwrap(), 0);
    }
}
