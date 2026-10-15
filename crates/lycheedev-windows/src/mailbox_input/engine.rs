//! Safe, independently fault-injectable orchestration. No Windows handles here.
#![forbid(unsafe_code)]

use std::{
    fmt,
    panic::{AssertUnwindSafe, catch_unwind},
    time::{Duration, Instant},
};

pub const INPUT_LIMIT: Duration = Duration::from_secs(60);
pub const KEY_CADENCE: Duration = Duration::from_millis(40);
pub const CLEANUP_LIMIT: Duration = Duration::from_secs(1);
const WAIT_SLICE: Duration = Duration::from_millis(5);
const CTRL: u16 = 0x11;
const ALT: u16 = 0x12;

/// A caller-owned ordered cancellation scope. An adapter must retain the actual
/// parent's deadline and cancellation timestamps, never export a raw token or
/// substitute an independent timer. `check` returns its original first cause.
/// All methods and the fresh-input guard must be synchronous and bounded.
/// The Core adapter belongs to the caller (Core already depends on Windows).
pub trait MailboxControl: Sized {
    type Error;
    fn deadline(&self) -> Instant;
    fn ordered_child(&self, deadline: Instant) -> Result<Self, Self::Error>;
    fn check(&self) -> Result<(), Self::Error>;
}

/// Validated observed bindings are data, not permission to send input.
/// There is deliberately no default, deserializer or arbitrary-key sender.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MailboxBindings {
    invoke: u16,
    reload: u16,
}
impl MailboxBindings {
    pub fn new(invoke: &str, reload: &str) -> Result<Self, String> {
        let invoke = binding_key(invoke)?;
        let reload = binding_key(reload)?;
        if invoke == reload {
            return Err("desktop.mailbox_binding_duplicate".into());
        }
        Ok(Self { invoke, reload })
    }
}
fn binding_key(chord: &str) -> Result<u16, String> {
    // Exact canonical spelling, including no SHIFT or leading zeroes.
    for n in 1..=12 {
        if chord == format!("ALT-CTRL-F{n}") {
            return Ok(0x70 + n - 1);
        }
    }
    Err("desktop.mailbox_binding_invalid".into())
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MailboxAction {
    Invoke,
    Reload,
}
/// Queue submission only. Even a complete receipt does not prove execution.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct InputReceipt {
    pub messages_queued: u32,
    pub submission_complete: bool,
    pub release_complete: bool,
}
#[derive(Debug)]
pub enum InputError<E> {
    Control(E),
    Guard(E),
    Platform(String),
    CallbackPanicked,
}
/// The original control/guard error is retained without string conversion.
/// Cleanup failures are all retained in attempt order, even when input failed.
#[derive(Debug)]
pub struct InputFailure<E> {
    pub cause: Option<InputError<E>>,
    pub cleanup_errors: Vec<String>,
    pub receipt: InputReceipt,
}
impl<E: fmt::Display> fmt::Display for InputFailure<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.cause {
            Some(InputError::Control(e) | InputError::Guard(e)) => write!(f, "{e}")?,
            Some(InputError::Platform(e)) => write!(f, "{e}")?,
            Some(InputError::CallbackPanicked) => write!(f, "desktop.input_callback_panicked")?,
            None => {}
        }
        for (index, error) in self.cleanup_errors.iter().enumerate() {
            if self.cause.is_some() || index > 0 {
                writeln!(f)?;
            }
            write!(f, "{error}")?;
        }
        Ok(())
    }
}
impl<E: std::error::Error + 'static> std::error::Error for InputFailure<E> {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match &self.cause {
            Some(InputError::Control(error) | InputError::Guard(error)) => Some(error),
            _ => None,
        }
    }
}

pub(super) trait Backend {
    fn now(&self) -> Instant;
    fn sleep(&mut self, duration: Duration);
    fn verify(&mut self) -> Result<(), String>;
    fn minimized(&mut self) -> bool;
    fn keyboard_ready(&mut self) -> Result<(), String>;
    fn scan(&mut self, vk: u16) -> u32;
    fn post(&mut self, message: u32, vk: u16, bits: u32) -> Result<(), String>;
    fn acquire(&mut self) -> Result<(), String>;
    /// Includes release AND close failures; an acquisition failure can still
    /// leave a handle requiring close or an abandoned mutex requiring release.
    fn finish_lock(&mut self) -> Vec<String>;
}

pub(super) fn send<C: MailboxControl, B: Backend>(
    backend: &mut B,
    parent: &C,
    bindings: MailboxBindings,
    action: MailboxAction,
    mut guard: impl FnMut(&C) -> Result<(), C::Error>,
) -> Result<InputReceipt, InputFailure<C::Error>> {
    let limit = (backend.now() + INPUT_LIMIT).min(parent.deadline());
    let control = parent.ordered_child(limit).map_err(|error| InputFailure {
        cause: Some(InputError::Control(error)),
        cleanup_errors: Vec::new(),
        receipt: InputReceipt {
            release_complete: true,
            ..InputReceipt::default()
        },
    })?;
    if control.deadline() > limit {
        return Err(InputFailure {
            cause: Some(InputError::Platform(
                "desktop.input_control_extended".into(),
            )),
            cleanup_errors: Vec::new(),
            receipt: InputReceipt {
                release_complete: true,
                ..InputReceipt::default()
            },
        });
    }
    let mut input = Attempt {
        backend,
        control: &control,
        guard: &mut guard,
        receipt: InputReceipt::default(),
        down: Vec::with_capacity(3),
    };
    // A panicking caller must not strand our keys or OS-thread-owned mutex.
    // This contains callbacks only; no OS callback/ABI crosses this boundary.
    let result = catch_unwind(AssertUnwindSafe(|| input.run(bindings, action)));
    let cause = match result {
        Ok(result) => result.err(),
        Err(_) => Some(InputError::CallbackPanicked),
    };
    input.receipt.submission_complete = cause.is_none();
    let mut cleanup_errors = input.release();
    input.receipt.release_complete = cleanup_errors.is_empty();
    cleanup_errors.extend(input.backend.finish_lock());
    if cause.is_some() || !cleanup_errors.is_empty() {
        Err(InputFailure {
            cause,
            cleanup_errors,
            receipt: input.receipt,
        })
    } else {
        Ok(input.receipt)
    }
}
struct Attempt<'a, C: MailboxControl, B, G> {
    backend: &'a mut B,
    control: &'a C,
    guard: &'a mut G,
    receipt: InputReceipt,
    down: Vec<u16>,
}
impl<C: MailboxControl, B: Backend, G: FnMut(&C) -> Result<(), C::Error>> Attempt<'_, C, B, G> {
    fn control(&self) -> Result<(), InputError<C::Error>> {
        self.control.check().map_err(InputError::Control)?;
        // Fail closed if a broken adapter fails to enforce its declared limit.
        if self.backend.now() >= self.control.deadline() {
            // The clock may cross the deadline just after check(). Let the
            // ordered control resolve its original cause before our fallback.
            self.control.check().map_err(InputError::Control)?;
            return Err(InputError::Platform("desktop.input_deadline".into()));
        }
        Ok(())
    }
    fn identity(&mut self) -> Result<(), InputError<C::Error>> {
        self.control()?;
        self.backend.verify().map_err(InputError::Platform)?;
        self.control()
    }
    fn check(&mut self) -> Result<(), InputError<C::Error>> {
        self.control()?;
        (self.guard)(self.control).map_err(InputError::Guard)?;
        self.identity()?;
        if self.backend.minimized() {
            return Err(InputError::Platform("desktop.window_minimized".into()));
        }
        self.control()
    }
    fn run(
        &mut self,
        bindings: MailboxBindings,
        action: MailboxAction,
    ) -> Result<(), InputError<C::Error>> {
        self.identity()?;
        self.backend.acquire().map_err(InputError::Platform)?;
        self.identity()?;
        // Known-zero readiness is try-only. Return with the mutex released;
        // the driver retains the original exchange/deadline and gets fresh INPUT.
        self.backend
            .keyboard_ready()
            .map_err(InputError::Platform)?;
        self.check()?;
        let function = match action {
            MailboxAction::Invoke => bindings.invoke,
            MailboxAction::Reload => bindings.reload,
        };
        let keys = [CTRL, ALT, function];
        let mut alt = false;
        for vk in keys {
            self.key(vk, false, alt || vk == ALT)?;
            self.down.push(vk);
            alt |= vk == ALT;
            self.wait(KEY_CADENCE)?;
        }
        while let Some(vk) = self.down.last().copied() {
            self.key(vk, true, alt)?;
            self.down.pop();
            if vk == ALT {
                alt = false;
            }
            self.wait(KEY_CADENCE)?;
        }
        Ok(())
    }
    fn key(&mut self, vk: u16, up: bool, alt: bool) -> Result<(), InputError<C::Error>> {
        let (message, bits) = packet(self.backend.scan(vk), up, alt);
        if !up && self.receipt.messages_queued == 0 {
            // Recheck AFTER the admission fresh INPUT. The existing final
            // control/guard/window check below must still follow this all-VK scan.
            // Once the chord begins, physical activity never inserts a wait or
            // interrupts its owned-key release sequence. Control errors still
            // trigger the existing independently bounded emergency release.
            self.backend
                .keyboard_ready()
                .map_err(InputError::Platform)?;
        }
        // Readiness may consume time or coincide with cancellation/identity
        // changes. Never post based on the pre-scan guard, even when it was quiet.
        self.check()?;
        self.backend
            .post(message, vk, bits)
            .map_err(InputError::Platform)?;
        self.receipt.messages_queued += 1;
        Ok(())
    }
    fn wait(&mut self, duration: Duration) -> Result<(), InputError<C::Error>> {
        let until = self.backend.now() + duration;
        loop {
            self.control()?;
            let now = self.backend.now();
            if now >= until {
                return Ok(());
            }
            self.backend.sleep(
                (until - now)
                    .min(WAIT_SLICE)
                    .min(self.control.deadline().saturating_duration_since(now)),
            );
        }
    }
    fn release(&mut self) -> Vec<String> {
        // Independent cleanup ignores caller cancellation/ownership changes but
        // never identity changes. No new chord, physical key release or focus.
        let deadline = self.backend.now() + CLEANUP_LIMIT;
        let mut errors = Vec::new();
        let mut alt = self.down.contains(&ALT);
        for vk in self.down.drain(..).rev() {
            if self.backend.now() >= deadline {
                errors.push("desktop.release_deadline".into());
                continue;
            }
            if let Err(error) = self.backend.verify() {
                errors.push(error);
                continue;
            }
            let (message, bits) = packet(self.backend.scan(vk), true, alt);
            if self.backend.now() >= deadline {
                errors.push("desktop.release_deadline".into());
                continue;
            }
            match self.backend.post(message, vk, bits) {
                Ok(()) => self.receipt.messages_queued += 1,
                Err(error) => errors.push(error),
            }
            if vk == ALT {
                alt = false;
            }
        }
        errors
    }
}
fn packet(scan: u32, up: bool, alt: bool) -> (u32, u32) {
    let mut bits = 1 | scan << 16;
    let mut message = if alt { 0x104 } else { 0x100 };
    if alt {
        bits |= 1 << 29;
    }
    if up {
        message += 1;
        bits |= (1 << 30) | (1 << 31);
    }
    (message, bits)
}
#[cfg(test)]
#[path = "tests.rs"]
mod tests;
