//! Safe timestamp-only readiness state. No key codes or text are retained.
#![forbid(unsafe_code)]
use std::{
    sync::atomic::{AtomicBool, AtomicU64, Ordering},
    time::{Duration, Instant},
};

pub(super) const QUIET_PERIOD: Duration = Duration::from_millis(500);

pub(super) fn check_keys(mut pressed: impl FnMut(u16) -> bool) -> Result<(), String> {
    // VK_CANCEL is Ctrl+Break. Exclude mouse buttons/reserved 0x04..0x07;
    // 0x01/0x02 are mouse buttons too and 0xff is reserved by Windows.
    for vk in 0x03..0xff {
        if !(0x04..=0x07).contains(&vk) && pressed(vk) {
            return Err("desktop.key_already_held".into());
        }
    }
    Ok(())
}

pub(super) struct KeyboardState {
    origin: Instant,
    last_event_nanos: AtomicU64,
    clock_invalid: AtomicBool,
    stopped: AtomicBool,
}
impl KeyboardState {
    pub(super) fn new(at: Instant) -> Self {
        Self {
            origin: at,
            last_event_nanos: AtomicU64::new(0),
            clock_invalid: AtomicBool::new(false),
            stopped: AtomicBool::new(false),
        }
    }
    fn nanos(&self, at: Instant) -> Option<u64> {
        at.checked_duration_since(self.origin)
            .and_then(|v| u64::try_from(v.as_nanos()).ok())
    }
    pub(super) fn note(&self, at: Instant) {
        match self.nanos(at) {
            Some(nanos) => {
                self.last_event_nanos.fetch_max(nanos, Ordering::AcqRel);
            }
            None => self.clock_invalid.store(true, Ordering::Release),
        }
    }
    pub(super) fn stop(&self) {
        self.stopped.store(true, Ordering::Release);
    }
    pub(super) fn check(
        &self,
        at: Instant,
        pressed: impl FnMut(u16) -> bool,
    ) -> Result<(), String> {
        let held = check_keys(pressed);
        if self.stopped.load(Ordering::Acquire) {
            return Err("desktop.keyboard_monitor_closed".into());
        }
        if self.clock_invalid.load(Ordering::Acquire) {
            return Err("desktop.keyboard_clock_invalid".into());
        }
        held?;
        let elapsed = self
            .nanos(at)
            .and_then(|nanos| nanos.checked_sub(self.last_event_nanos.load(Ordering::Acquire)));
        if elapsed.is_none_or(|nanos| Duration::from_nanos(nanos) < QUIET_PERIOD) {
            return Err("desktop.keyboard_active".into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn every_keyboard_vk_blocks_but_mouse_and_reserved_values_do_not() {
        for held in 0..=0xff {
            let blocked = (0x03..0xff).contains(&held) && !(0x04..=0x07).contains(&held);
            assert_eq!(check_keys(|vk| vk == held).is_err(), blocked, "{held:#x}");
        }
    }
    #[test]
    fn startup_hold_release_repeat_and_short_key_each_restart_half_second_quiet() {
        let start = Instant::now();
        let state = KeyboardState::new(start);
        for (ms, event, held, want) in [
            (499, false, 0, Some("desktop.keyboard_active")),
            (500, false, 0, None),
            (1000, true, 0x57, Some("desktop.key_already_held")),
            (10000, false, 0x57, Some("desktop.key_already_held")),
            (10500, true, 0x57, Some("desktop.key_already_held")),
            (11000, true, 0, Some("desktop.keyboard_active")),
            (11499, false, 0, Some("desktop.keyboard_active")),
            (11500, true, 0x41, Some("desktop.key_already_held")),
            (11510, true, 0, Some("desktop.keyboard_active")),
            (12009, false, 0, Some("desktop.keyboard_active")),
            (12010, false, 0, None),
        ] {
            let at = start + Duration::from_millis(ms);
            if event {
                state.note(at);
            }
            assert_eq!(state.check(at, |vk| vk == held).err().as_deref(), want);
        }
        // Sub-millisecond events must not shorten the required quiet period.
        state.note(start + Duration::from_micros(13000_999));
        assert!(
            state
                .check(start + Duration::from_micros(13500_998), |_| false)
                .is_err()
        );
        assert!(
            state
                .check(start + Duration::from_micros(13500_999), |_| false)
                .is_ok()
        );
    }
    #[test]
    fn stopped_or_invalid_clock_is_fail_closed() {
        let at = Instant::now();
        let state = KeyboardState::new(at);
        state.note(at - Duration::from_millis(1));
        assert_eq!(
            state.check(at + QUIET_PERIOD, |_| false).unwrap_err(),
            "desktop.keyboard_clock_invalid"
        );
        state.stop();
        assert_eq!(
            state.check(at + QUIET_PERIOD, |_| true).unwrap_err(),
            "desktop.keyboard_monitor_closed"
        );
    }
}
