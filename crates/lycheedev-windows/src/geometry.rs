use crate::{Result, WindowIdentity};
use windows::Win32::{
    Foundation::{HWND, POINT, RECT},
    Graphics::{
        Dwm::{DWMWA_EXTENDED_FRAME_BOUNDS, DwmGetWindowAttribute},
        Gdi::ClientToScreen,
    },
    System::Performance::{QueryPerformanceCounter, QueryPerformanceFrequency},
    UI::{HiDpi::*, WindowsAndMessaging::GetClientRect},
};
struct Dpi(DPI_AWARENESS_CONTEXT);
impl Drop for Dpi {
    fn drop(&mut self) {
        unsafe {
            SetThreadDpiAwarenessContext(self.0);
        }
    }
}
pub fn signal_offset(target: &WindowIdentity, width: u32, height: u32) -> Result<(u32, u32)> {
    target.verify()?;
    unsafe {
        let previous = SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        if previous.0.is_null() {
            return Err("capture.dpi_context".into());
        }
        let _dpi = Dpi(previous);
        let hwnd = HWND(target.hwnd as *mut _);
        let mut visible = RECT::default();
        DwmGetWindowAttribute(
            hwnd,
            DWMWA_EXTENDED_FRAME_BOUNDS,
            &mut visible as *mut _ as *mut _,
            std::mem::size_of_val(&visible) as u32,
        )
        .map_err(|e| e.to_string())?;
        let mut client = RECT::default();
        GetClientRect(hwnd, &mut client).map_err(|e| e.to_string())?;
        let mut top = POINT {
            x: client.left,
            y: client.top,
        };
        let mut bottom = POINT {
            x: client.right,
            y: client.bottom,
        };
        if !ClientToScreen(hwnd, &mut top).as_bool() || !ClientToScreen(hwnd, &mut bottom).as_bool()
        {
            return Err("capture.client_origin".into());
        }
        if visible.right - visible.left != width as i32
            || visible.bottom - visible.top != height as i32
            || bottom.x - top.x < 6
            || bottom.y - top.y < 2
            || top.x < visible.left
            || top.y < visible.top
            || bottom.x > visible.right
            || bottom.y > visible.bottom
        {
            return Err("capture.client_geometry_mismatch".into());
        }
        target.verify()?;
        Ok(((top.x - visible.left) as u32, (top.y - visible.top) as u32))
    }
}
pub fn system_ticks() -> Result<i64> {
    unsafe {
        let (mut count, mut freq) = (0, 0);
        QueryPerformanceCounter(&mut count).map_err(|e| e.to_string())?;
        QueryPerformanceFrequency(&mut freq).map_err(|e| e.to_string())?;
        if count < 0 || freq <= 0 {
            return Err("capture.qpc_clock".into());
        }
        let ticks = (count as i128 * 10_000_000) / freq as i128;
        i64::try_from(ticks).map_err(|_| "capture.qpc_overflow".into())
    }
}
