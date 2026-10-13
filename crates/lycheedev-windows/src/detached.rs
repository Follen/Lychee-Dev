//! Narrow daemon startup: exact executable, no shell, no inheritable handles, explicit custody.
use crate::{ProcessIdentity, Result};
use std::{
    ffi::{OsStr, OsString},
    os::windows::ffi::OsStrExt,
    path::Path,
    time::{Duration, Instant},
};
use windows::{
    Win32::{
        Foundation::{CloseHandle, FILETIME, HANDLE, WAIT_OBJECT_0, WAIT_TIMEOUT},
        Globalization::CompareStringOrdinal,
        System::Threading::{
            CREATE_NO_WINDOW, CREATE_UNICODE_ENVIRONMENT, CreateProcessW, GetExitCodeProcess,
            GetProcessTimes, PROCESS_INFORMATION, STARTF_USESHOWWINDOW, STARTUPINFOW,
            TerminateProcess, WaitForSingleObject,
        },
        UI::WindowsAndMessaging::SW_HIDE,
    },
    core::{PCWSTR, PWSTR},
};
struct Handle(HANDLE);
impl Drop for Handle {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}
fn wide(value: &OsStr) -> Result<Vec<u16>> {
    let mut value = value.encode_wide().collect::<Vec<_>>();
    if value.contains(&0) {
        return Err("process.nul_argument".into());
    }
    value.push(0);
    Ok(value)
}
fn quoted(value: &OsStr) -> Result<Vec<u16>> {
    let value = wide(value)?;
    let mut result = vec![34];
    let mut slashes = 0;
    for &ch in &value[..value.len() - 1] {
        if ch == 92 {
            slashes += 1;
            continue;
        }
        result.extend(std::iter::repeat_n(
            92,
            slashes * if ch == 34 { 2 } else { 1 },
        ));
        if ch == 34 {
            result.push(92);
        }
        result.push(ch);
        slashes = 0;
    }
    result.extend(std::iter::repeat_n(92, slashes * 2));
    result.push(34);
    Ok(result)
}
fn environment(overrides: &[(String, OsString)]) -> Result<Vec<u16>> {
    if overrides.len() > 32 {
        return Err("process.environment_budget".into());
    }
    let mut pairs = std::env::vars_os().collect::<Vec<_>>();
    for (key, value) in overrides {
        if key.is_empty()
            || key.len() > 128
            || !key.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_')
        {
            return Err("process.environment_key".into());
        }
        wide(value)?;
        pairs.retain(|(name, _)| !name.to_string_lossy().eq_ignore_ascii_case(key));
        pairs.push((OsString::from(key), value.clone()));
    }
    let mut pairs = pairs
        .into_iter()
        .map(|(key, value)| Ok((wide(&key)?, wide(&value)?)))
        .collect::<Result<Vec<_>>>()?;
    pairs.sort_by(|(a, _), (b, _)| {
        unsafe { CompareStringOrdinal(&a[..a.len() - 1], &b[..b.len() - 1], true) }
            .0
            .cmp(&2)
    });
    let mut block = Vec::new();
    for (key, value) in pairs {
        block.extend_from_slice(&key[..key.len() - 1]);
        block.push(61);
        block.extend_from_slice(&value);
    }
    block.push(0);
    if block.len() == 1 {
        block.push(0);
    }
    if block.len() > 32767 {
        return Err("process.environment_budget".into());
    }
    Ok(block)
}
pub struct DetachedChild {
    process: Handle,
    identity: ProcessIdentity,
    owned: bool,
}
/// Fails before spawn for relative/oversized/nul inputs. Neither parent flags nor global env change.
pub fn spawn_hidden_no_inherit(
    executable: &Path,
    args: &[OsString],
    cwd: &Path,
    env_overrides: &[(String, OsString)],
) -> Result<DetachedChild> {
    if !executable.is_absolute() || !cwd.is_absolute() || args.len() > 128 {
        return Err("process.absolute_launch_required".into());
    }
    if !executable.is_file() || !cwd.is_dir() {
        return Err("process.launch_path_missing".into());
    }
    let executable = executable.canonicalize().map_err(|e| e.to_string())?;
    let cwd = cwd.canonicalize().map_err(|e| e.to_string())?;
    let application = wide(executable.as_os_str())?;
    let directory = wide(cwd.as_os_str())?;
    let mut command = quoted(executable.as_os_str())?;
    for arg in args {
        command.push(32);
        command.extend(quoted(arg)?);
    }
    command.push(0);
    if command.len() > 32767 {
        return Err("process.command_line_budget".into());
    }
    let env = environment(env_overrides)?;
    let startup = STARTUPINFOW {
        cb: std::mem::size_of::<STARTUPINFOW>() as u32,
        dwFlags: STARTF_USESHOWWINDOW,
        wShowWindow: SW_HIDE.0 as u16,
        ..Default::default()
    };
    let mut info = PROCESS_INFORMATION::default();
    unsafe {
        CreateProcessW(
            PCWSTR(application.as_ptr()),
            Some(PWSTR(command.as_mut_ptr())),
            None,
            None,
            false,
            CREATE_NO_WINDOW | CREATE_UNICODE_ENVIRONMENT,
            Some(env.as_ptr().cast()),
            PCWSTR(directory.as_ptr()),
            &startup,
            &mut info,
        )
        .map_err(|e| format!("process.create:{e}"))?;
    }
    let process = Handle(info.hProcess);
    let thread = Handle(info.hThread);
    drop(thread);
    let mut child = DetachedChild {
        process,
        identity: ProcessIdentity {
            pid: info.dwProcessId,
            created: 0,
        },
        owned: true,
    };
    let (mut created, mut exit, mut kernel, mut user) = (
        FILETIME::default(),
        FILETIME::default(),
        FILETIME::default(),
        FILETIME::default(),
    );
    unsafe {
        GetProcessTimes(
            child.process.0,
            &mut created,
            &mut exit,
            &mut kernel,
            &mut user,
        )
        .map_err(|e| e.to_string())?;
    }
    child.identity.created = (created.dwHighDateTime as u64) << 32 | created.dwLowDateTime as u64;
    Ok(child)
}
impl DetachedChild {
    pub fn id(&self) -> u32 {
        self.identity.pid
    }
    pub fn identity(&self) -> &ProcessIdentity {
        &self.identity
    }
    pub fn try_wait(&self) -> Result<Option<u32>> {
        let wait = unsafe { WaitForSingleObject(self.process.0, 0) };
        if wait == WAIT_TIMEOUT {
            return Ok(None);
        }
        if wait != WAIT_OBJECT_0 {
            return Err("process.wait_failed".into());
        }
        let mut exit = 0;
        unsafe {
            GetExitCodeProcess(self.process.0, &mut exit).map_err(|e| e.to_string())?;
        }
        Ok(Some(exit))
    }
    pub fn terminate_and_wait(&mut self, deadline: Instant) -> Result<u32> {
        if let Some(code) = self.try_wait()? {
            self.owned = false;
            return Ok(code);
        }
        let terminate = unsafe { TerminateProcess(self.process.0, 7) };
        if let Err(error) = terminate
            && self.try_wait()?.is_none()
        {
            return Err(error.to_string());
        }
        loop {
            if let Some(code) = self.try_wait()? {
                self.owned = false;
                return Ok(code);
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err("process.reap_deadline".into());
            }
            let wait = unsafe {
                WaitForSingleObject(self.process.0, remaining.as_millis().clamp(1, 30000) as u32)
            };
            if wait != WAIT_OBJECT_0 && wait != WAIT_TIMEOUT {
                return Err("process.wait_failed".into());
            }
        }
    }
    /// Call only after the authenticated owner has taken custody of the live daemon.
    pub fn handoff(mut self) -> Result<ProcessIdentity> {
        if self.try_wait()?.is_some() {
            return Err("process.detached_exited".into());
        }
        self.owned = false;
        Ok(self.identity.clone())
    }
}
impl Drop for DetachedChild {
    fn drop(&mut self) {
        if self.owned {
            let _ = self.terminate_and_wait(Instant::now() + Duration::from_secs(2));
        }
    }
}
