use crate::{Process, Result, WindowIdentity};
use serde::{Deserialize, Serialize};
use windows::{
    Win32::{
        Foundation::{HWND, LPARAM},
        UI::WindowsAndMessaging::*,
    },
    core::BOOL,
};
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Window {
    pub identity: WindowIdentity,
    pub executable: String,
    pub class: String,
    pub title: String,
    pub visible: bool,
}
unsafe extern "system" fn collect(hwnd: HWND, parameter: LPARAM) -> BOOL {
    unsafe {
        let out = &mut *(parameter.0 as *mut Vec<Window>);
        if out.len() >= 4096 {
            return BOOL(0);
        }
        let mut pid = 0;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid == 0 {
            return BOOL(1);
        }
        let Ok(process) = Process::open(pid) else {
            return BOOL(1);
        };
        let Ok(path) = process.image_path() else {
            return BOOL(1);
        };
        let mut class = [0u16; 256];
        let mut title = [0u16; 1024];
        let count = GetClassNameW(hwnd, &mut class);
        if count <= 0 {
            return BOOL(1);
        }
        let len = GetWindowTextW(hwnd, &mut title).max(0) as usize;
        let identity = WindowIdentity {
            process: process.identity().clone(),
            hwnd: hwnd.0 as usize,
        };
        if identity.verify().is_ok() {
            out.push(Window {
                identity,
                executable: path.to_string_lossy().into_owned(),
                class: String::from_utf16_lossy(&class[..count as usize]),
                title: String::from_utf16_lossy(&title[..len]),
                visible: IsWindowVisible(hwnd).as_bool(),
            });
        }
        BOOL(1)
    }
}
pub fn windows() -> Result<Vec<Window>> {
    let mut out = Vec::new();
    unsafe {
        EnumWindows(Some(collect), LPARAM(&mut out as *mut _ as isize))
            .map_err(|e| e.to_string())?;
    }
    if out.len() >= 4096 {
        return Err("window.inventory_truncated".into());
    }
    Ok(out)
}

/// Visible-window metadata only. The held process handle has limited-query
/// access; this path cannot read process memory or open an executable file.
pub fn passive_windows(mut check: impl FnMut() -> Result<()>) -> Result<Vec<Window>> {
    use windows::Win32::{Foundation::*, System::Threading::*};
    struct QueryHandle(HANDLE);
    impl Drop for QueryHandle {
        fn drop(&mut self) {
            unsafe {
                let _ = CloseHandle(self.0);
            }
        }
    }
    struct Enumeration<'a> {
        windows: Vec<Window>,
        check: &'a mut dyn FnMut() -> Result<()>,
        error: Option<String>,
        visited: usize,
    }
    unsafe extern "system" fn collect_passive(hwnd: HWND, parameter: LPARAM) -> BOOL {
        unsafe {
            let state = &mut *(parameter.0 as *mut Enumeration<'_>);
            if let Err(error) = (state.check)() {
                state.error = Some(error);
                return BOOL(0);
            }
            state.visited += 1;
            if state.visited > 4096 {
                state.error = Some("window.inventory_truncated".into());
                return BOOL(0);
            }
            if !IsWindowVisible(hwnd).as_bool() {
                return BOOL(1);
            }
            let mut pid = 0;
            GetWindowThreadProcessId(hwnd, Some(&mut pid));
            if pid == 0 {
                return BOOL(1);
            }
            let Ok(handle) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else {
                return BOOL(1);
            };
            let handle = QueryHandle(handle);
            let (mut created, mut exited, mut kernel, mut user) = (
                FILETIME::default(),
                FILETIME::default(),
                FILETIME::default(),
                FILETIME::default(),
            );
            if GetProcessTimes(handle.0, &mut created, &mut exited, &mut kernel, &mut user).is_err()
            {
                return BOOL(1);
            }
            let mut path = [0u16; 32768];
            let mut length = path.len() as u32;
            if QueryFullProcessImageNameW(
                handle.0,
                PROCESS_NAME_WIN32,
                windows::core::PWSTR(path.as_mut_ptr()),
                &mut length,
            )
            .is_err()
            {
                return BOOL(1);
            }
            let mut class = [0u16; 256];
            let mut title = [0u16; 1024];
            let count = GetClassNameW(hwnd, &mut class);
            if count <= 0 {
                return BOOL(1);
            }
            let title_length = GetWindowTextW(hwnd, &mut title).max(0) as usize;
            let mut final_pid = 0;
            GetWindowThreadProcessId(hwnd, Some(&mut final_pid));
            if final_pid != pid || !IsWindow(Some(hwnd)).as_bool() {
                return BOOL(1);
            }
            state.windows.push(Window {
                identity: WindowIdentity {
                    process: crate::ProcessIdentity {
                        pid,
                        created: (u64::from(created.dwHighDateTime) << 32)
                            | u64::from(created.dwLowDateTime),
                    },
                    hwnd: hwnd.0 as usize,
                },
                executable: String::from_utf16_lossy(&path[..length as usize]),
                class: String::from_utf16_lossy(&class[..count as usize]),
                title: String::from_utf16_lossy(&title[..title_length]),
                visible: true,
            });
            BOOL(1)
        }
    }
    check()?;
    let mut state = Enumeration {
        windows: Vec::new(),
        check: &mut check,
        error: None,
        visited: 0,
    };
    let result =
        unsafe { EnumWindows(Some(collect_passive), LPARAM(&mut state as *mut _ as isize)) };
    if let Some(error) = state.error {
        return Err(error);
    }
    result.map_err(|error| error.to_string())?;
    (state.check)()?;
    Ok(state.windows)
}
