use crate::{NamedLock, Result, WindowIdentity};
use serde::{Deserialize, Serialize};
use windows::Win32::{
    Foundation::{HWND, LPARAM, WPARAM},
    UI::{Input::KeyboardAndMouse::*, WindowsAndMessaging::*},
};
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct InputReceipt {
    pub messages_queued: u32,
    pub submission_complete: bool,
    pub release_complete: bool,
}
/// An input error retains exact zero/partial queue evidence. Queueing does not prove dispatch.
#[derive(Debug)]
pub struct InputFailure {
    pub reason: String,
    pub receipt: InputReceipt,
}
pub fn chord(
    target: &WindowIdentity,
    keys: &[u16],
    mut guard: impl FnMut() -> Result<()>,
) -> std::result::Result<InputReceipt, InputFailure> {
    let mut receipt = InputReceipt::default();
    let mut down = Vec::new();
    let action = (|| -> Result<()> {
        target.verify()?;
        let _lock = NamedLock::acquire(&format!(
            "Local\\LycheeDev.Input.{}.{}.{}",
            target.process.pid, target.process.created, target.hwnd
        ))?;
        if keys.is_empty() || keys.len() > 4 {
            return Err("input.chord_bounds".into());
        }
        unsafe {
            if IsIconic(HWND(target.hwnd as *mut _)).as_bool() {
                return Err("input.minimized".into());
            }
        }
        for &key in keys {
            guard()?;
            target.verify()?;
            unsafe {
                if GetAsyncKeyState(key as i32) < 0 {
                    return Err("input.physical_key_held".into());
                }
            }
        }
        let send = (|| -> Result<()> {
            for &key in keys {
                guard()?;
                target.verify()?;
                post(target, key, false, down.contains(&0x12))?;
                receipt.messages_queued += 1;
                down.push(key);
            }
            Ok(())
        })();
        receipt.submission_complete = send.is_ok();
        let mut release_error = None;
        let mut alt = down.contains(&0x12);
        for &key in down.iter().rev() {
            if let Err(e) = target.verify().and_then(|_| post(target, key, true, alt)) {
                release_error = Some(e)
            } else {
                receipt.messages_queued += 1;
            }
            if key == 0x12 {
                alt = false;
            }
        }
        receipt.release_complete = release_error.is_none();
        send?;
        if let Some(e) = release_error {
            return Err(e);
        }
        Ok(())
    })();
    match action {
        Ok(()) => Ok(receipt),
        Err(reason) => Err(InputFailure { reason, receipt }),
    }
}
fn post(target: &WindowIdentity, vk: u16, up: bool, alt: bool) -> Result<()> {
    unsafe {
        let scan = MapVirtualKeyW(vk as u32, MAPVK_VK_TO_VSC);
        let mut bits = 1u32 | (scan << 16);
        let mut message = if alt { WM_SYSKEYDOWN } else { WM_KEYDOWN };
        if alt {
            bits |= 1 << 29
        }
        if up {
            message += 1;
            bits |= 1 << 30 | 1 << 31
        }
        PostMessageW(
            Some(HWND(target.hwnd as *mut _)),
            message,
            WPARAM(vk as usize),
            LPARAM(bits as isize),
        )
        .map_err(|e| e.to_string())
    }
}
/// The sole literal chat bootstrap: Esc x3, Enter, /reload, Enter.
/// No arbitrary text parameter exists. Whole intent is persisted by the caller
/// before entry; failure reports zero/partial queued messages and never retries.
pub fn fixed_reload(
    target: &WindowIdentity,
    mut guard: impl FnMut(u8) -> Result<()>,
    mut wait: impl FnMut(std::time::Duration) -> Result<()>,
) -> std::result::Result<InputReceipt, InputFailure> {
    let mut receipt = InputReceipt::default();
    let action = (|| -> Result<()> {
        target.verify()?;
        let _lock = NamedLock::acquire(&format!(
            "Local\\LycheeDev.Input.{}.{}.{}",
            target.process.pid, target.process.created, target.hwnd
        ))?;
        unsafe {
            if IsIconic(HWND(target.hwnd as *mut _)).as_bool() {
                return Err("input.minimized".into());
            }
            for key in [0x11, 0x12, 0x10, 0x1b, 0x0d] {
                if GetAsyncKeyState(key) < 0 {
                    return Err("input.physical_key_held".into());
                }
            }
        }
        for step in 1..=6u8 {
            guard(step)?;
            target.verify()?;
            if step == 5 {
                for byte in b"/reload" {
                    guard(step)?;
                    target.verify()?;
                    unsafe {
                        PostMessageW(
                            Some(HWND(target.hwnd as *mut _)),
                            WM_CHAR,
                            WPARAM(*byte as usize),
                            LPARAM(1),
                        )
                        .map_err(|e| e.to_string())?
                    }
                    receipt.messages_queued += 1;
                    wait(std::time::Duration::from_millis(50))?;
                }
            } else {
                let key = if step <= 3 { 0x1b } else { 0x0d };
                post(target, key, false, false)?;
                receipt.messages_queued += 1;
                // Only our exact down is released, even if a guard would now
                // refuse further submission. Do not leave owned Enter/Esc down.
                if let Err(error) = target.verify().and_then(|_| post(target, key, true, false)) {
                    receipt.release_complete = false;
                    return Err(error);
                }
                receipt.messages_queued += 1;
                receipt.release_complete = true;
            }
            if step <= 4 {
                wait(std::time::Duration::from_millis(150))?
            }
        }
        receipt.submission_complete = true;
        receipt.release_complete = true;
        Ok(())
    })();
    match action {
        Ok(()) => Ok(receipt),
        Err(reason) => Err(InputFailure { reason, receipt }),
    }
}
