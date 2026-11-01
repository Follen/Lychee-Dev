//! Local named pipes scoped to one logon, or explicitly to one account for UAC.
use crate::Result;
// Windows SDK winnt.h, SE_GROUP_LOGON_ID (not an account-SID attribute).
const LOGON_ID_GROUP: u32 = 0xc0000000;
/// Caller-owned complete cancellation/deadline check. Never exposes a writable token.
pub type BrokerCheck = Arc<dyn Fn() -> Result<()> + Send + Sync>;
use sha2::{Digest, Sha256};
use std::{
    io::{Read, Write},
    os::windows::io::{IntoRawHandle, RawHandle},
    path::{Path, PathBuf},
    sync::Arc,
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
impl IntoRawHandle for Handle {
    fn into_raw_handle(self) -> RawHandle {
        let handle = self.0.0;
        std::mem::forget(self);
        handle
    }
}
impl Handle {
    fn close(self) -> Result<()> {
        let result = crate::guarded_spawn::close_owned_handle(self);
        #[cfg(test)]
        let result = crate::process::checked_query_tests::observe_close(result);
        result.map_err(|error| format!("broker.handle_close:{error}"))
    }
    fn finish<T>(self, result: Result<T>) -> Result<T> {
        crate::process::finish_query(result, self.close())
    }
}
impl Drop for Handle {
    fn drop(&mut self) {
        let result = unsafe { CloseHandle(self.0) }.map_err(|error| error.to_string());
        #[cfg(test)]
        let result = crate::process::checked_query_tests::observe_close(result);
        let _ = result;
    }
}
fn free_local(allocation: HLOCAL) -> Result<()> {
    let result = if unsafe { LocalFree(Some(allocation)) }.0.is_null() {
        Ok(())
    } else {
        Err(format!("broker.local_free:{}", unsafe { GetLastError().0 }))
    };
    #[cfg(test)]
    let result = crate::process::checked_query_tests::observe_close(result);
    result
}
struct Descriptor(PSECURITY_DESCRIPTOR);
impl Descriptor {
    fn close(self) -> Result<()> {
        let allocation = HLOCAL(self.0.0);
        std::mem::forget(self);
        free_local(allocation)
    }
}
impl Drop for Descriptor {
    fn drop(&mut self) {
        let _ = free_local(HLOCAL(self.0.0));
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
    pub fn for_dd_task(home: &Path) -> Result<Self> {
        Self::for_kind(home, "publish-dd-task")
    }
    pub fn for_dd_worker(home: &Path, nonce: &str) -> Result<Self> {
        if nonce.len() != 32
            || !nonce
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err("broker.invalid_worker_identity".into());
        }
        Self::for_kind(home, &format!("publish-dd-{nonce}"))
    }
    /// One-shot same-account scope permits filtered/elevated tokens. Existing
    /// Source and DD task/worker scopes retain their original logon isolation.
    pub fn for_dd_channel(home: &Path, nonce: &str) -> Result<Self> {
        if nonce.len() != 32
            || !nonce
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err("broker.invalid_channel_identity".into());
        }
        Self::with_sid(
            home,
            &format!("publish-dd-channel-{nonce}"),
            crate::process::elevation::current_user_sid()?,
        )
    }
    pub fn for_native_source(home: &Path) -> Result<Self> {
        Self::for_kind(home, "native-source")
    }
    fn for_kind(home: &Path, kind: &str) -> Result<Self> {
        Self::with_sid(home, kind, logon_sid()?)
    }
    fn with_sid(home: &Path, kind: &str, sid: String) -> Result<Self> {
        let home = std::fs::canonicalize(home).map_err(|e| e.to_string())?;
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

/// Hold retirement access before submitting a request. Reopening a dying
/// process after EOF can fail and obscure the original worker failure.
pub struct NativeSourcePeer {
    process: Handle,
}
impl NativeSourcePeer {
    /// Close the original retirement handle after its last wait or retirement.
    pub fn close(self) -> Result<()> {
        self.process.close()
    }
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
            let result = (|| {
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
                    return Ok(());
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
                Ok(())
            })();
            match result {
                Ok(()) => Ok(Self { process }),
                Err(error) => process.finish(Err(error)),
            }
        }
    }
    pub fn retire(&self, deadline: Instant) -> Result<()> {
        unsafe {
            let process = &self.process;
            if WaitForSingleObject(process.0, 0) == WAIT_OBJECT_0 {
                return Ok(());
            }
            // Termination can already be in progress after an IPC EOF. Even
            // with our pre-authenticated termination handle, Windows may report
            // ACCESS_DENIED for an exiting process before it becomes signaled.
            // Observe this same process until the caller's original deadline;
            // never reopen its PID, retry termination, or infer successful exit
            // from that error alone.
            let termination = TerminateProcess(process.0, 5);
            let remaining = deadline.saturating_duration_since(Instant::now());
            if WaitForSingleObject(
                process.0,
                remaining.as_millis().min(u32::MAX as u128) as u32,
            ) != WAIT_OBJECT_0
            {
                if let Err(error) = termination {
                    return Err(error.to_string());
                }
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
        let result = (|| {
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
                    return crate::process::finish_query(
                        result,
                        free_local(HLOCAL(text.0 as *mut _)),
                    );
                }
            }
            Err("broker.logon_sid_unavailable".into())
        })();
        token.finish(result)
    }
}
pub struct BrokerListener {
    scope: BrokerScope,
    pending: Handle,
}
impl BrokerListener {
    pub fn close(self) -> Result<()> {
        self.pending.close()
    }
    pub fn bind(scope: &BrokerScope) -> Result<Self> {
        Ok(Self {
            scope: scope.clone(),
            pending: create(scope, true)?,
        })
    }
    pub fn accept_with_control(
        &mut self,
        deadline: Instant,
        control_check: BrokerCheck,
    ) -> Result<BrokerStream> {
        check(deadline, control_check.as_ref())?;
        let event = event()?;
        let mut overlapped = Box::new(OVERLAPPED::default());
        overlapped.hEvent = event.0;
        let result = (|| {
            unsafe {
                match ConnectNamedPipe(self.pending.0, Some(&mut *overlapped)) {
                    Ok(()) => {}
                    Err(error) if error.code() == ERROR_PIPE_CONNECTED.to_hresult() => {}
                    Err(error) if error.code() == ERROR_IO_PENDING.to_hresult() => {
                        complete(
                            self.pending.0,
                            &overlapped,
                            deadline,
                            control_check.as_ref(),
                        )?;
                    }
                    Err(error) => return Err(error.to_string()),
                }
            }
            check(deadline, control_check.as_ref())
        })();
        // Connect completion has drained its OVERLAPPED before consuming the
        // event. A failed close cannot publish a connected stream.
        event.finish(result)?;
        // Keep one server instance present while handing the connected one out.
        // This prevents another process becoming the first server in a gap.
        let replacement = create(&self.scope, false)?;
        let connected = std::mem::replace(&mut self.pending, replacement);
        Ok(BrokerStream {
            handle: connected,
            server: true,
            deadline,
            control_check,
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
        let created = if handle == INVALID_HANDLE_VALUE {
            Err(format!("broker.pipe_create:{}", GetLastError().0))
        } else {
            Ok(Handle(handle))
        };
        match (created, descriptor.close()) {
            (Ok(handle), Ok(())) => Ok(handle),
            (Ok(handle), Err(error)) => handle.finish(Err(error)),
            (Err(error), closed) => crate::process::finish_query(Err(error), closed),
        }
    }
}
fn event() -> Result<Handle> {
    unsafe {
        CreateEventW(None, true, false, None)
            .map(Handle)
            .map_err(|e| e.to_string())
    }
}
fn check(deadline: Instant, control_check: &(dyn Fn() -> Result<()> + Send + Sync)) -> Result<()> {
    control_check()?;
    if Instant::now() >= deadline {
        control_check()?;
        Err("broker.deadline".into())
    } else {
        Ok(())
    }
}
fn complete(
    handle: HANDLE,
    overlapped: &OVERLAPPED,
    deadline: Instant,
    control_check: &(dyn Fn() -> Result<()> + Send + Sync),
) -> Result<u32> {
    loop {
        if let Err(reason) = check(deadline, control_check) {
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
        check(deadline, control_check)?;
        return Ok(bytes);
    }
}
pub struct BrokerStream {
    handle: Handle,
    server: bool,
    deadline: Instant,
    control_check: BrokerCheck,
}
impl BrokerStream {
    /// Each operation has drained its OVERLAPPED before this owner is consumed.
    /// Closing a duplicate must not disconnect the other stream owners.
    pub fn close(self) -> Result<()> {
        self.handle.close()
    }
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
            control_check: self.control_check.clone(),
        })
    }
    pub fn connect_with_control(
        scope: &BrokerScope,
        deadline: Instant,
        control_check: BrokerCheck,
    ) -> Result<Self> {
        check(deadline, control_check.as_ref())?;
        let name = wide(&scope.endpoint);
        let handle = unsafe {
            CreateFileW(
                PCWSTR(name.as_ptr()),
                GENERIC_READ.0 | GENERIC_WRITE.0,
                FILE_SHARE_MODE(0),
                None,
                OPEN_EXISTING,
                FILE_FLAG_OVERLAPPED | SECURITY_SQOS_PRESENT | SECURITY_IDENTIFICATION,
                None,
            )
            .map_err(connection_error)?
        };
        let handle = Handle(handle);
        if let Err(error) = check(deadline, control_check.as_ref()) {
            return handle.finish(Err(error));
        }
        Ok(Self {
            handle,
            server: false,
            deadline,
            control_check,
        })
    }
    pub fn set_deadline_with_control(&mut self, deadline: Instant, control_check: BrokerCheck) {
        self.deadline = deadline;
        self.control_check = control_check
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
        check(self.deadline, self.control_check.as_ref())?;
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
        let result = match result {
            Ok(()) => check(self.deadline, self.control_check.as_ref()).map(|()| bytes as usize),
            Err(e) if e.code() == ERROR_IO_PENDING.to_hresult() => {
                match complete(
                    self.handle.0,
                    &overlapped,
                    self.deadline,
                    self.control_check.as_ref(),
                ) {
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
        };
        event.finish(result)
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
    use crate::process::checked_query_tests::CloseObservation;
    fn fail_close() -> CloseObservation {
        CloseObservation::install(|result| {
            result?;
            Err("fixture.broker_close".into())
        })
    }
    #[test]
    fn checked_authentication_cannot_succeed_after_resource_close_failure() {
        let path = std::env::temp_dir().join(format!(
            "lycheedev-broker-auth-{}",
            crate::random::token().unwrap()
        ));
        std::fs::create_dir(&path).unwrap();
        BrokerScope::for_native_source(&path).unwrap();
        let observer = fail_close();
        let result = BrokerScope::for_native_source(&path);
        drop(observer);
        std::fs::remove_dir(&path).unwrap();
        assert!(
            result
                .err()
                .expect("authentication close must fail")
                .contains("fixture.broker_close")
        );
    }
    #[test]
    fn checked_peer_rejection_retains_primary_and_process_close_failure() {
        let own = crate::Process::open(std::process::id()).unwrap();
        let created = own.identity().created;
        own.close().unwrap();
        let observer = fail_close();
        let result = NativeSourcePeer::open(
            std::process::id(),
            created + 1,
            &std::env::current_exe().unwrap(),
        );
        drop(observer);
        let error = result.err().expect("wrong creation identity must fail");
        assert!(error.contains("source.owner_identity_changed"));
        assert!(error.contains("fixture.broker_close"));
    }
    #[test]
    fn checked_connect_keeps_post_open_stop_and_its_close_failure() {
        let scope = scope();
        let listener = BrokerListener::bind(&scope).unwrap();
        let observer = fail_close();
        let calls = std::sync::atomic::AtomicUsize::new(0);
        let result = BrokerStream::connect_with_control(
            &scope,
            Instant::now() + Duration::from_secs(3),
            Arc::new(move || {
                if calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 0 {
                    Ok(())
                } else {
                    Err("fixture.post_open_stop".into())
                }
            }),
        );
        drop(observer);
        listener.close().unwrap();
        let error = match result {
            Ok(stream) => {
                stream.close().unwrap();
                panic!("a stopped connect cannot return a stream")
            }
            Err(error) => error,
        };
        assert!(error.contains("fixture.post_open_stop"));
        assert!(error.contains("fixture.broker_close"));
    }
    #[test]
    fn checked_listener_creation_rejects_descriptor_cleanup_failure() {
        let scope = scope();
        BrokerListener::bind(&scope).unwrap().close().unwrap();
        let observer = fail_close();
        let result = BrokerListener::bind(&scope);
        drop(observer);
        let error = match result {
            Ok(listener) => {
                listener.close().unwrap();
                None
            }
            Err(error) => Some(error),
        };
        BrokerListener::bind(&scope).unwrap().close().unwrap();
        assert!(
            error
                .expect("descriptor close failure must reject a listener")
                .contains("fixture.broker_close")
        );
    }
    #[test]
    fn checked_transfer_retains_real_write_but_rejects_event_close_failure() {
        let scope = scope();
        let mut listener = BrokerListener::bind(&scope).unwrap();
        let (accepted, accepted_rx) = std::sync::mpsc::channel();
        let (observed, received) = std::sync::mpsc::channel();
        let server = std::thread::spawn(move || {
            let mut stream = listener
                .accept_with_control(Instant::now() + Duration::from_secs(3), ready())
                .unwrap();
            accepted.send(()).unwrap();
            let mut bytes = [0; 4];
            stream.read_exact(&mut bytes).unwrap();
            assert_eq!(&bytes, b"ping");
            stream.close().unwrap();
            listener.close().unwrap();
            observed.send(()).unwrap();
        });
        let mut client = BrokerStream::connect_with_control(
            &scope,
            Instant::now() + Duration::from_secs(3),
            ready(),
        )
        .unwrap();
        accepted_rx.recv_timeout(Duration::from_secs(3)).unwrap();
        let observer = fail_close();
        let result = client.write_all(b"ping");
        drop(observer);
        received.recv_timeout(Duration::from_secs(3)).unwrap();
        client.close().unwrap();
        server.join().unwrap();
        assert!(
            result
                .err()
                .expect("event close failure must reject an otherwise completed write")
                .to_string()
                .contains("fixture.broker_close")
        );
    }
    #[test]
    fn checked_read_reports_event_close_failure_after_receiving_the_real_marker() {
        let scope = scope();
        let mut listener = BrokerListener::bind(&scope).unwrap();
        let (accepted_tx, accepted_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let server = std::thread::spawn(move || {
            let mut stream = listener
                .accept_with_control(Instant::now() + Duration::from_secs(3), ready())
                .unwrap();
            stream.write_all(&[1]).unwrap();
            accepted_tx.send(()).unwrap();
            release_rx.recv_timeout(Duration::from_secs(3)).unwrap();
            stream.close().unwrap();
            listener.close().unwrap();
        });
        let mut client = BrokerStream::connect_with_control(
            &scope,
            Instant::now() + Duration::from_secs(3),
            ready(),
        )
        .unwrap();
        accepted_rx.recv_timeout(Duration::from_secs(3)).unwrap();
        let observer = fail_close();
        let mut marker = [0];
        let result = client.read(&mut marker);
        drop(observer);
        release_tx.send(()).unwrap();
        client.close().unwrap();
        server.join().unwrap();
        assert_eq!(marker, [1], "the cancellation marker was actually received");
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("fixture.broker_close")
        );
    }
    fn ready() -> BrokerCheck {
        Arc::new(|| Ok(()))
    }
    fn cancelled() -> BrokerCheck {
        Arc::new(|| Err("broker.cancelled".into()))
    }
    #[test]
    fn caller_stop_cause_precedes_pipe_deadline_at_both_checks() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let expired = Instant::now();
        assert_eq!(
            check(expired, &|| Err("fixture.ancestor_cancelled".into())),
            Err("fixture.ancestor_cancelled".into())
        );
        let checks = AtomicUsize::new(0);
        let result = check(expired, &|| {
            if checks.fetch_add(1, Ordering::SeqCst) == 0 {
                Ok(())
            } else {
                Err("fixture.original_deadline".into())
            }
        });
        assert_eq!(result, Err("fixture.original_deadline".into()));
        assert_eq!(checks.load(Ordering::SeqCst), 2);
    }

    fn scope() -> BrokerScope {
        let path = std::env::temp_dir().join(format!(
            "lycheedev-pipe-{}",
            crate::random::token().unwrap()
        ));
        std::fs::create_dir_all(&path).unwrap();
        let scope = BrokerScope::for_native_source(&path).unwrap();
        std::fs::remove_dir(&path).unwrap();
        scope
    }
    #[test]
    fn actual_local_pipe_peer_and_first_instance_guard() {
        let scope = scope();
        let mut listener = BrokerListener::bind(&scope).unwrap();
        assert!(BrokerListener::bind(&scope).is_err());
        let client_scope = scope.clone();
        let client = std::thread::spawn(move || {
            let mut stream = BrokerStream::connect_with_control(
                &client_scope,
                Instant::now() + Duration::from_secs(3),
                ready(),
            )
            .unwrap();
            assert_eq!(stream.peer_pid().unwrap(), std::process::id());
            stream.write_all(b"ping").unwrap();
            let mut response = [0; 4];
            stream.read_exact(&mut response).unwrap();
            assert_eq!(&response, b"pong");
        });
        let mut server = listener
            .accept_with_control(Instant::now() + Duration::from_secs(3), ready())
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
    fn account_scoped_channel_pipe_limits_the_client_token_to_identification() {
        let home = std::env::temp_dir();
        let nonce = crate::random::token().unwrap();
        let scope = BrokerScope::for_dd_channel(&home, &nonce).unwrap();
        let user_sid = crate::process::elevation::current_user_sid().unwrap();
        assert_eq!(scope.sddl, format!("D:P(A;;GA;;;{user_sid})"));
        let mut listener = BrokerListener::bind(&scope).unwrap();
        let client_scope = scope.clone();
        let client = std::thread::spawn(move || {
            let mut stream = BrokerStream::connect_with_control(
                &client_scope,
                Instant::now() + Duration::from_secs(3),
                ready(),
            )
            .unwrap();
            stream.write_all(b"x").unwrap();
            let mut reply = [0];
            stream.read_exact(&mut reply).unwrap();
            assert_eq!(reply, [b'y']);
            stream.close().unwrap();
        });
        let mut server = listener
            .accept_with_control(Instant::now() + Duration::from_secs(3), ready())
            .unwrap();
        let mut request = [0];
        server.read_exact(&mut request).unwrap();
        struct Revert;
        impl Drop for Revert {
            fn drop(&mut self) {
                let _ = unsafe { RevertToSelf() };
            }
        }
        // SAFETY: only this owned, authenticated local test pipe is impersonated;
        // the scope guard always restores the test thread, including on panic.
        unsafe { ImpersonateNamedPipeClient(server.handle.0) }.unwrap();
        let restore = Revert;
        let mut raw = HANDLE::default();
        unsafe { OpenThreadToken(GetCurrentThread(), TOKEN_QUERY, true, &mut raw) }.unwrap();
        let token = Handle(raw);
        let mut level = SECURITY_IMPERSONATION_LEVEL::default();
        let mut needed = 0;
        unsafe {
            GetTokenInformation(
                token.0,
                TokenImpersonationLevel,
                Some((&mut level as *mut SECURITY_IMPERSONATION_LEVEL).cast()),
                std::mem::size_of_val(&level) as u32,
                &mut needed,
            )
        }
        .unwrap();
        token.close().unwrap();
        unsafe { RevertToSelf() }.unwrap();
        std::mem::forget(restore);
        assert_eq!(level, SecurityIdentification);
        server.write_all(b"y").unwrap();
        server.close().unwrap();
        listener.close().unwrap();
        client.join().unwrap();
    }
    #[test]
    fn sequential_accept_eof_and_clone_drop_leave_owner_listener_live() {
        let scope = scope();
        let mut listener = BrokerListener::bind(&scope).unwrap();
        for round in 0..4u8 {
            let client_scope = scope.clone();
            let client = std::thread::spawn(move || {
                let mut stream = BrokerStream::connect_with_control(
                    &client_scope,
                    Instant::now() + Duration::from_secs(3),
                    ready(),
                )
                .unwrap();
                stream.write_all(&[round]).unwrap();
                let mut reply = [0];
                stream.read_exact(&mut reply).unwrap();
                assert_eq!(reply[0], round + 1);
            });
            let mut server = listener
                .accept_with_control(Instant::now() + Duration::from_secs(3), ready())
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
                .accept_with_control(start + Duration::from_millis(20), ready())
                .is_err()
        );
        assert!(start.elapsed() < Duration::from_secs(1));
        assert!(
            listener
                .accept_with_control(Instant::now() + Duration::from_secs(1), cancelled())
                .is_err()
        );
        let client_scope = scope.clone();
        let client = std::thread::spawn(move || {
            BrokerStream::connect_with_control(
                &client_scope,
                Instant::now() + Duration::from_secs(2),
                ready(),
            )
            .unwrap()
        });
        let server = listener
            .accept_with_control(Instant::now() + Duration::from_secs(2), ready())
            .unwrap();
        let mut client = client.join().unwrap();
        client.set_deadline_with_control(Instant::now() + Duration::from_millis(20), ready());
        assert!(client.read(&mut [0; 1]).is_err());
        drop(server);
        client.set_deadline_with_control(Instant::now() + Duration::from_secs(1), ready());
        assert_eq!(client.read(&mut [0; 1]).unwrap(), 0);
    }
    #[test]
    fn duplicate_observer_cancel_and_drop_does_not_disconnect_writer() {
        let scope = scope();
        let mut listener = BrokerListener::bind(&scope).unwrap();
        let mut client = BrokerStream::connect_with_control(
            &scope,
            Instant::now() + Duration::from_secs(2),
            ready(),
        )
        .unwrap();
        let mut server = listener
            .accept_with_control(Instant::now() + Duration::from_secs(2), ready())
            .unwrap();
        let mut observer = server.try_clone().unwrap();
        observer.set_deadline_with_control(Instant::now() + Duration::from_millis(20), ready());
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
