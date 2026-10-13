use crate::Result;
use std::sync::{
    Arc, Mutex, Weak,
    atomic::{AtomicBool, Ordering},
};
use windows::{
    Win32::System::Console::{CTRL_BREAK_EVENT, CTRL_C_EVENT, SetConsoleCtrlHandler},
    core::BOOL,
};
static ACTIVE: Mutex<Option<Weak<AtomicBool>>> = Mutex::new(None);
unsafe extern "system" fn handler(kind: u32) -> BOOL {
    if ![CTRL_C_EVENT, CTRL_BREAK_EVENT].contains(&kind) {
        return BOOL(0);
    }
    if let Ok(active) = ACTIVE.try_lock()
        && let Some(token) = active.as_ref().and_then(Weak::upgrade)
    {
        token.store(true, Ordering::Release);
        return BOOL(1);
    }
    BOOL(0)
}
pub struct ConsoleCancellation {
    token: Arc<AtomicBool>,
}
impl ConsoleCancellation {
    pub fn install() -> Result<Self> {
        let mut active = ACTIVE.lock().map_err(|_| "console.handler_poisoned")?;
        if active.as_ref().and_then(Weak::upgrade).is_some() {
            return Err("console.handler_busy".into());
        }
        let token = Arc::new(AtomicBool::new(false));
        unsafe {
            SetConsoleCtrlHandler(Some(handler), true).map_err(|e| e.to_string())?;
        }
        *active = Some(Arc::downgrade(&token));
        Ok(Self { token })
    }
    pub fn cancelled(&self) -> &AtomicBool {
        &self.token
    }
    pub fn token(&self) -> Arc<AtomicBool> {
        self.token.clone()
    }
}
impl Drop for ConsoleCancellation {
    fn drop(&mut self) {
        if let Ok(mut active) = ACTIVE.lock() {
            *active = None;
            unsafe {
                let _ = SetConsoleCtrlHandler(Some(handler), false);
            }
        }
    }
}
