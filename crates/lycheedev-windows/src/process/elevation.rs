//! Same-user, exact-lifetime peers and the fixed DD channel-only UAC launch.
//! No arbitrary executable, command, shell script, output path or URL is admitted.
use super::{
    Handle, Process, ProcessIdentity,
    bound_memory::{image, same_path, user_sid},
    identity,
};
use crate::Result;
use std::{
    os::windows::ffi::OsStrExt,
    path::Path,
    time::{Duration, Instant},
};
use windows::{
    Win32::{
        Foundation::*,
        Security::{Authorization::ConvertSidToStringSidW, PSID},
        System::Threading::*,
        UI::{Shell::*, WindowsAndMessaging::SW_HIDE},
    },
    core::{PCWSTR, w},
};

fn fault() -> String {
    "process.channel_helper_unavailable".into()
}
pub const CANCELLED: &str = "process.channel_elevation_cancelled";

pub(crate) fn current_user_sid() -> Result<String> {
    let bytes = user_sid(unsafe { GetCurrentProcess() })?;
    let mut aligned = vec![0u64; bytes.len().div_ceil(8)];
    for (index, byte) in bytes.into_iter().enumerate() {
        aligned[index / 8] |= u64::from(byte) << (index % 8 * 8);
    }
    let mut text = windows::core::PWSTR::null();
    // SAFETY: this is an aligned owned copy of the complete validated TokenUser
    // SID. Windows allocates the string, which is copied before LocalFree.
    unsafe { ConvertSidToStringSidW(PSID(aligned.as_mut_ptr().cast()), &mut text) }
        .map_err(|_| fault())?;
    let result = unsafe { text.to_string() }.map_err(|_| fault());
    let cleanup = if unsafe { LocalFree(Some(HLOCAL(text.0.cast()))) }
        .0
        .is_null()
    {
        Ok(())
    } else {
        Err(fault())
    };
    super::finish_query(result, cleanup)
}
fn bound(process: &Process, expected: &ProcessIdentity, executable: &Path) -> Result<()> {
    process.verify()?;
    if process.identity() != expected
        || !same_path(
            &image(process)?,
            &executable.canonicalize().map_err(|_| fault())?,
        )
        || user_sid(process.handle.0)? != user_sid(unsafe { GetCurrentProcess() })?
    {
        return Err(fault());
    }
    process.verify()
}
/// Limited-query peer. It cannot read memory, send input or terminate a process.
pub struct SameUserPeer {
    process: Process,
}
impl SameUserPeer {
    pub fn open(expected: &ProcessIdentity, executable: &Path) -> Result<Self> {
        if expected.pid == 0 || expected.created == 0 || !executable.is_absolute() {
            return Err(fault());
        }
        let handle = Handle(
            unsafe {
                OpenProcess(
                    PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
                    false,
                    expected.pid,
                )
            }
            .map_err(|_| fault())?,
        );
        let observed = match unsafe { identity(handle.0, expected.pid) } {
            Ok(id) => id,
            Err(error) => return handle.finish(Err(error)),
        };
        let process = Process {
            handle,
            identity: observed,
        };
        match bound(&process, expected, executable) {
            Ok(()) => Ok(Self { process }),
            Err(error) => process.finish(Err(error)),
        }
    }
    pub fn verify(&self) -> Result<()> {
        self.process.verify()
    }
    pub fn close(self) -> Result<()> {
        self.process.close()
    }
}
/// Original process handle returned by ShellExecuteExW. Retirement never reopens
/// a PID and can affect only this call's auxiliary process.
pub struct ChannelChild {
    process: Process,
}
impl ChannelChild {
    pub fn launch(
        home: &Path,
        parent: &ProcessIdentity,
        nonce: &str,
        deadline_ms: u64,
    ) -> Result<Self> {
        let executable = std::env::current_exe()
            .map_err(|_| fault())?
            .canonicalize()
            .map_err(|_| fault())?;
        let home = home.canonicalize().map_err(|_| fault())?;
        if nonce.len() != 32
            || !nonce
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            || parent.pid != std::process::id()
            || parent.created == 0
            || deadline_ms == 0
        {
            return Err(fault());
        }
        let peer = SameUserPeer::open(parent, &executable)?;
        peer.close()?;
        // Windows quoting for this sole path operand. Numeric and nonce operands
        // contain no quoting characters; no shell or executable lookup is used.
        let path = home.as_os_str().encode_wide().collect::<Vec<_>>();
        if path.contains(&0) || path.contains(&34) {
            return Err(fault());
        }
        let mut parameters = "--publish-dd-channel-helper \""
            .encode_utf16()
            .collect::<Vec<_>>();
        parameters.extend(&path);
        // A trailing slash must be doubled before the closing quote.
        for _ in path.iter().rev().take_while(|&&v| v == 92) {
            parameters.push(92);
        }
        parameters.extend(
            format!(
                "\" {} {} {} {}",
                parent.pid, parent.created, nonce, deadline_ms
            )
            .encode_utf16(),
        );
        parameters.push(0);
        let file = executable
            .as_os_str()
            .encode_wide()
            .chain(Some(0))
            .collect::<Vec<_>>();
        let directory = home
            .as_os_str()
            .encode_wide()
            .chain(Some(0))
            .collect::<Vec<_>>();
        let mut info = SHELLEXECUTEINFOW {
            cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
            fMask: SEE_MASK_NOCLOSEPROCESS | SEE_MASK_NOASYNC | SEE_MASK_FLAG_NO_UI,
            lpVerb: w!("runas"),
            lpFile: PCWSTR(file.as_ptr()),
            lpParameters: PCWSTR(parameters.as_ptr()),
            lpDirectory: PCWSTR(directory.as_ptr()),
            nShow: SW_HIDE.0,
            ..Default::default()
        };
        // SAFETY: typed SDK structure and terminated strings live throughout the
        // synchronous shell call. Only this same executable's fixed helper entry
        // is requested. NOASYNC makes hProcess available before these buffers die.
        unsafe { ShellExecuteExW(&mut info) }.map_err(|error| {
            if error.code() == ERROR_CANCELLED.to_hresult() {
                CANCELLED.into()
            } else {
                fault()
            }
        })?;
        if info.hProcess.is_invalid() {
            return Err(fault());
        }
        let handle = Handle(info.hProcess);
        let pid = unsafe { GetProcessId(handle.0) };
        let observed = match unsafe { identity(handle.0, pid) } {
            Ok(id) => id,
            Err(error) => return handle.finish(Err(error)),
        };
        Ok(Self {
            process: Process {
                handle,
                identity: observed,
            },
        })
    }
    pub fn identity(&self) -> &ProcessIdentity {
        self.process.identity()
    }
    pub fn verify(&self, executable: &Path) -> Result<()> {
        bound(&self.process, self.identity(), executable)
    }
    pub fn wait_exit(&self, deadline: Instant, check: impl FnMut() -> Result<()>) -> Result<u32> {
        self.process.wait_exit_with_control(deadline, check)
    }
    pub fn retire(&self, deadline: Instant) -> Result<()> {
        if unsafe { WaitForSingleObject(self.process.handle.0, 0) } == WAIT_OBJECT_0 {
            return Ok(());
        }
        let termination =
            unsafe { TerminateProcess(self.process.handle.0, 124) }.map_err(|_| fault());
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if unsafe {
                WaitForSingleObject(
                    self.process.handle.0,
                    remaining.min(Duration::from_millis(50)).as_millis() as u32,
                )
            } == WAIT_OBJECT_0
            {
                return Ok(());
            }
            if Instant::now() >= deadline {
                return termination.and(Err("process.channel_helper_cleanup_pending".into()));
            }
        }
    }
    pub fn close(self) -> Result<()> {
        self.process.close()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn limited_peer_and_account_sid_bind_the_original_user_image_and_lifetime_without_uac() {
        let process = Process::open(std::process::id()).unwrap();
        let id = process.identity().clone();
        process.close().unwrap();
        let executable = std::env::current_exe().unwrap();
        let peer = SameUserPeer::open(&id, &executable).unwrap();
        peer.verify().unwrap();
        peer.close().unwrap();
        assert!(current_user_sid().unwrap().starts_with("S-1-"));
        let wrong = ProcessIdentity {
            pid: id.pid,
            created: id.created + 1,
        };
        assert!(SameUserPeer::open(&wrong, &executable).is_err());
        assert!(SameUserPeer::open(&id, &executable.with_file_name("wrong.exe")).is_err());
        assert!(ChannelChild::launch(&std::env::temp_dir(), &id, "invalid", 1).is_err());
    }
}
