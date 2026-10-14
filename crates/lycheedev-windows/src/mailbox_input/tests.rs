use super::*;
use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
    rc::Rc,
};

#[derive(Clone, Debug, PartialEq, Eq)]
enum Fault {
    Cancelled,
    Deadline,
    Guard,
    OrderedUnavailable,
}
#[derive(Clone)]
struct Control {
    now: Rc<Cell<Instant>>,
    cancelled: Rc<Cell<Option<Instant>>>,
    deadline: Instant,
    child_deadline: Rc<Cell<Option<Instant>>>,
    reject: bool,
}
impl MailboxControl for Control {
    type Error = Fault;
    fn deadline(&self) -> Instant {
        self.deadline
    }
    fn ordered_child(&self, deadline: Instant) -> Result<Self, Fault> {
        if self.reject {
            return Err(Fault::OrderedUnavailable);
        }
        let mut child = self.clone();
        child.deadline = deadline.min(self.deadline);
        self.child_deadline.set(Some(child.deadline));
        Ok(child)
    }
    fn check(&self) -> Result<(), Fault> {
        if self.cancelled.get().is_some_and(|at| at < self.deadline) {
            Err(Fault::Cancelled)
        } else if self.now.get() >= self.deadline {
            Err(Fault::Deadline)
        } else {
            Ok(())
        }
    }
}
#[derive(Debug, Clone)]
struct Event {
    at: Instant,
    message: u32,
    vk: u16,
    bits: u32,
}
struct Fake {
    now: Rc<Cell<Instant>>,
    held: Option<u16>,
    minimized: bool,
    events: Vec<Event>,
    posts: usize,
    fail_posts: BTreeMap<usize, String>,
    verify_error: Rc<RefCell<Option<String>>>,
    verify_delay: Rc<Cell<Duration>>,
    acquired: usize,
    lock_error: Option<String>,
    lock_errors: Vec<String>,
    finishes: usize,
    cancel_on_sleep: Option<Rc<Cell<Option<Instant>>>>,
}
impl Backend for Fake {
    fn now(&self) -> Instant {
        self.now.get()
    }
    fn sleep(&mut self, duration: Duration) {
        self.now.set(self.now.get() + duration);
        if let Some(signal) = self.cancel_on_sleep.take() {
            signal.set(Some(self.now.get()));
        }
    }
    fn verify(&mut self) -> Result<(), String> {
        self.now.set(self.now.get() + self.verify_delay.get());
        match self.verify_error.borrow().as_ref() {
            Some(error) => Err(error.clone()),
            None => Ok(()),
        }
    }
    fn minimized(&mut self) -> bool {
        self.minimized
    }
    fn pressed(&mut self, vk: u16) -> bool {
        self.held == Some(vk)
    }
    fn scan(&mut self, vk: u16) -> u32 {
        u32::from(vk)
    }
    fn post(&mut self, message: u32, vk: u16, bits: u32) -> Result<(), String> {
        self.posts += 1;
        if let Some(error) = self.fail_posts.get(&self.posts) {
            return Err(error.clone());
        }
        self.events.push(Event {
            at: self.now.get(),
            message,
            vk,
            bits,
        });
        Ok(())
    }
    fn acquire(&mut self) -> Result<(), String> {
        self.acquired += 1;
        match &self.lock_error {
            Some(error) => Err(error.clone()),
            None => Ok(()),
        }
    }
    fn finish_lock(&mut self) -> Vec<String> {
        self.finishes += 1;
        std::mem::take(&mut self.lock_errors)
    }
}
fn setup() -> (Fake, Control) {
    let now = Rc::new(Cell::new(Instant::now()));
    let control = Control {
        now: now.clone(),
        cancelled: Rc::new(Cell::new(None)),
        deadline: now.get() + Duration::from_secs(120),
        child_deadline: Rc::new(Cell::new(None)),
        reject: false,
    };
    let backend = Fake {
        now,
        held: None,
        minimized: false,
        events: Vec::new(),
        posts: 0,
        fail_posts: BTreeMap::new(),
        verify_error: Rc::new(RefCell::new(None)),
        verify_delay: Rc::new(Cell::new(Duration::ZERO)),
        acquired: 0,
        lock_error: None,
        lock_errors: Vec::new(),
        finishes: 0,
        cancel_on_sleep: None,
    };
    (backend, control)
}
fn bindings() -> MailboxBindings {
    MailboxBindings::new("ALT-CTRL-F12", "ALT-CTRL-F11").unwrap()
}

#[test]
fn only_two_distinct_canonical_function_bindings_are_accepted() {
    for n in 1..=12 {
        let other = if n == 12 { 11 } else { 12 };
        assert!(
            MailboxBindings::new(&format!("ALT-CTRL-F{n}"), &format!("ALT-CTRL-F{other}")).is_ok()
        );
    }
    for bad in [
        "",
        "ALT-CTRL-F0",
        "ALT-CTRL-F13",
        "ALT-CTRL-F01",
        "ALT-CTRL-F+1",
        "ALT-CTRL-F1 ",
        "ALT-CTRL-f1",
        "CTRL-ALT-F1",
        "ALT-CTRL-SHIFT-F1",
        "ALT-CTRL-A",
        "ALT-CTRL-[",
        "ESCAPE",
        "ENTER",
        "/reload",
    ] {
        assert_eq!(
            MailboxBindings::new(bad, "ALT-CTRL-F11"),
            Err("desktop.mailbox_binding_invalid".into()),
            "{bad}"
        );
        assert_eq!(
            MailboxBindings::new("ALT-CTRL-F11", bad),
            Err("desktop.mailbox_binding_invalid".into()),
            "{bad}"
        );
    }
    assert_eq!(
        MailboxBindings::new("ALT-CTRL-F1", "ALT-CTRL-F1"),
        Err("desktop.mailbox_binding_duplicate".into())
    );
}
#[test]
fn both_actions_have_exact_six_messages_alt_context_and_forty_ms_cadence() {
    for (action, key) in [(MailboxAction::Invoke, 0x7b), (MailboxAction::Reload, 0x7a)] {
        let (mut backend, control) = setup();
        let start = backend.now();
        let receipt = send(&mut backend, &control, bindings(), action, |_| Ok(())).unwrap();
        assert_eq!(
            receipt,
            InputReceipt {
                messages_queued: 6,
                submission_complete: true,
                release_complete: true
            }
        );
        assert_eq!(
            backend.events.iter().map(|e| e.vk).collect::<Vec<_>>(),
            [CTRL, ALT, key, key, ALT, CTRL]
        );
        assert_eq!(
            backend.events.iter().map(|e| e.message).collect::<Vec<_>>(),
            [0x100, 0x104, 0x104, 0x105, 0x105, 0x101]
        );
        for (i, event) in backend.events.iter().enumerate() {
            assert_eq!(event.bits & 0xffff, 1);
            assert_eq!(event.bits & (1 << 29) != 0, (1..5).contains(&i));
            assert_eq!(event.bits & (3 << 30), if i >= 3 { 3 << 30 } else { 0 });
            assert_eq!(event.at - start, KEY_CADENCE * i as u32);
        }
        assert_eq!(backend.now() - start, KEY_CADENCE * 6);
        assert_eq!(control.child_deadline.get(), Some(start + INPUT_LIMIT));
        assert_eq!(backend.finishes, 1);
    }
}
#[test]
fn extra_physical_modifiers_and_chord_key_block_without_releasing_them() {
    for held in [0x10, CTRL, ALT, 0x5b, 0x5c, 0x7b, 0x41, 0x7a] {
        let (mut backend, control) = setup();
        backend.held = Some(held);
        let result = send(
            &mut backend,
            &control,
            bindings(),
            MailboxAction::Invoke,
            |_| Ok(()),
        );
        if [0x41, 0x7a].contains(&held) {
            assert!(result.is_ok());
        } else {
            let error = result.unwrap_err();
            assert!(
                matches!(error.cause, Some(InputError::Platform(ref value)) if value == "desktop.key_already_held")
            );
            assert_eq!(error.receipt.messages_queued, 0);
            assert!(error.receipt.release_complete);
            assert!(backend.events.is_empty());
        }
    }
}
#[test]
fn cancelled_third_guard_releases_only_the_first_injected_control_key() {
    let (mut backend, control) = setup();
    let mut calls = 0;
    let error = send(
        &mut backend,
        &control,
        bindings(),
        MailboxAction::Invoke,
        |child| {
            calls += 1;
            if calls == 3 {
                child.cancelled.set(Some(child.now.get()));
            }
            child.check()
        },
    )
    .unwrap_err();
    assert!(matches!(
        error.cause,
        Some(InputError::Guard(Fault::Cancelled))
    ));
    assert_eq!(
        error.receipt,
        InputReceipt {
            messages_queued: 2,
            submission_complete: false,
            release_complete: true
        }
    );
    assert_eq!(
        backend
            .events
            .iter()
            .map(|e| (e.message, e.vk))
            .collect::<Vec<_>>(),
        [(0x100, CTRL), (0x101, CTRL)]
    );
    assert_eq!(calls, 3); // Cleanup never invokes the now-revoked owner guard.
}
#[test]
fn cancellation_during_cadence_is_observed_before_the_next_down() {
    let (mut backend, control) = setup();
    backend.cancel_on_sleep = Some(control.cancelled.clone());
    let error = send(
        &mut backend,
        &control,
        bindings(),
        MailboxAction::Invoke,
        |_| Ok(()),
    )
    .unwrap_err();
    assert!(matches!(
        error.cause,
        Some(InputError::Control(Fault::Cancelled))
    ));
    assert_eq!(
        backend.events.iter().map(|e| e.vk).collect::<Vec<_>>(),
        [CTRL, CTRL]
    );
    assert!(error.receipt.release_complete);
}
#[test]
fn original_deadline_wins_late_cancel_and_cleanup_has_independent_budget() {
    let (mut backend, mut control) = setup();
    control.deadline = backend.now() + Duration::from_millis(50);
    let mut calls = 0;
    let error = send(
        &mut backend,
        &control,
        bindings(),
        MailboxAction::Invoke,
        |child| {
            calls += 1;
            if calls == 3 {
                child.now.set(child.deadline + Duration::from_millis(10));
                child.cancelled.set(Some(child.now.get()));
            }
            child.check()
        },
    )
    .unwrap_err();
    assert_eq!(control.child_deadline.get(), Some(control.deadline));
    assert!(matches!(
        error.cause,
        Some(InputError::Guard(Fault::Deadline))
    ));
    assert_eq!(backend.events.len(), 2);
    assert!(backend.events[1].at > control.deadline);
    assert!(error.receipt.release_complete);
}
#[test]
fn cancellation_before_deadline_remains_original_cause_after_both_elapsed() {
    let (mut backend, mut control) = setup();
    control.deadline = backend.now() + Duration::from_millis(50);
    let mut calls = 0;
    let error = send(
        &mut backend,
        &control,
        bindings(),
        MailboxAction::Invoke,
        |child| {
            calls += 1;
            if calls == 3 {
                child.cancelled.set(Some(child.now.get()));
                child.now.set(child.deadline + Duration::from_millis(10));
            }
            child.check()
        },
    )
    .unwrap_err();
    assert!(matches!(
        error.cause,
        Some(InputError::Guard(Fault::Cancelled))
    ));
    assert_eq!(error.receipt.messages_queued, 2);
}
#[test]
fn all_release_and_mutex_failures_are_retained_beside_original_guard_error() {
    let (mut backend, control) = setup();
    for i in 4..=6 {
        backend.fail_posts.insert(i, format!("release-{i}"));
    }
    backend.lock_errors = vec!["release-mutex".into(), "close-mutex".into()];
    let mut calls = 0;
    let error = send(
        &mut backend,
        &control,
        bindings(),
        MailboxAction::Invoke,
        |_| {
            calls += 1;
            if calls == 5 {
                Err(Fault::Guard)
            } else {
                Ok(())
            }
        },
    )
    .unwrap_err();
    assert!(matches!(error.cause, Some(InputError::Guard(Fault::Guard))));
    assert_eq!(
        error.cleanup_errors,
        [
            "release-4",
            "release-5",
            "release-6",
            "release-mutex",
            "close-mutex"
        ]
    );
    assert_eq!(error.receipt.messages_queued, 3);
    assert!(!error.receipt.release_complete);
    assert_eq!(backend.posts, 6);
}
#[test]
fn cleanup_identity_change_blocks_all_remaining_keys_without_retargeting() {
    let (mut backend, control) = setup();
    let identity = backend.verify_error.clone();
    let mut calls = 0;
    let error = send(
        &mut backend,
        &control,
        bindings(),
        MailboxAction::Invoke,
        |_| {
            calls += 1;
            if calls == 5 {
                *identity.borrow_mut() = Some("window.reused".into());
                return Err(Fault::Guard);
            }
            Ok(())
        },
    )
    .unwrap_err();
    assert_eq!(
        error.cleanup_errors,
        ["window.reused", "window.reused", "window.reused"]
    );
    assert_eq!(backend.posts, 3);
    assert!(!error.receipt.release_complete);
}
#[test]
fn cleanup_expiry_reports_each_unreleased_owned_key_and_never_posts_late() {
    let (mut backend, control) = setup();
    let delay = backend.verify_delay.clone();
    let mut calls = 0;
    let error = send(
        &mut backend,
        &control,
        bindings(),
        MailboxAction::Invoke,
        |_| {
            calls += 1;
            if calls == 5 {
                delay.set(CLEANUP_LIMIT);
                return Err(Fault::Guard);
            }
            Ok(())
        },
    )
    .unwrap_err();
    assert_eq!(
        error.cleanup_errors,
        [
            "desktop.release_deadline",
            "desktop.release_deadline",
            "desktop.release_deadline"
        ]
    );
    assert_eq!(backend.posts, 3);
}
#[test]
fn failed_normal_up_is_released_once_by_cleanup_and_not_replayed_as_a_chord() {
    let (mut backend, control) = setup();
    backend.fail_posts.insert(4, "post-up-failed".into());
    let error = send(
        &mut backend,
        &control,
        bindings(),
        MailboxAction::Invoke,
        |_| Ok(()),
    )
    .unwrap_err();
    assert!(
        matches!(error.cause, Some(InputError::Platform(ref value)) if value == "post-up-failed")
    );
    assert_eq!(backend.posts, 7);
    assert_eq!(
        backend.events.iter().map(|e| e.vk).collect::<Vec<_>>(),
        [CTRL, ALT, 0x7b, 0x7b, ALT, CTRL]
    );
    assert!(error.receipt.release_complete);
    assert!(!error.receipt.submission_complete);
}
#[test]
fn busy_abandoned_minimized_and_unordered_controls_are_fail_closed() {
    for status in ["desktop.input_busy", "desktop.input_abandoned"] {
        let (mut backend, control) = setup();
        backend.lock_error = Some(status.into());
        let error = send(
            &mut backend,
            &control,
            bindings(),
            MailboxAction::Invoke,
            |_| panic!("guard must not run before lock"),
        )
        .unwrap_err();
        assert!(matches!(error.cause, Some(InputError::Platform(ref value)) if value == status));
        assert_eq!(backend.posts, 0);
        assert_eq!(backend.finishes, 1);
    }
    let (mut backend, mut control) = setup();
    backend.minimized = true;
    let error = send(
        &mut backend,
        &control,
        bindings(),
        MailboxAction::Invoke,
        |_| Ok(()),
    )
    .unwrap_err();
    assert!(
        matches!(error.cause, Some(InputError::Platform(ref value)) if value == "desktop.window_minimized")
    );
    assert_eq!(backend.posts, 0);
    control.reject = true;
    let error = send(
        &mut backend,
        &control,
        bindings(),
        MailboxAction::Invoke,
        |_| Ok(()),
    )
    .unwrap_err();
    assert!(matches!(
        error.cause,
        Some(InputError::Control(Fault::OrderedUnavailable))
    ));
    assert_eq!(backend.acquired, 1);
}
#[test]
fn guard_panic_still_releases_owned_key_and_closes_lock() {
    let (mut backend, control) = setup();
    let mut calls = 0;
    let error = send(
        &mut backend,
        &control,
        bindings(),
        MailboxAction::Invoke,
        |_| {
            calls += 1;
            assert!(calls != 3, "fixture guard panic");
            Ok(())
        },
    )
    .unwrap_err();
    assert!(matches!(error.cause, Some(InputError::CallbackPanicked)));
    assert_eq!(error.receipt.messages_queued, 2);
    assert!(error.receipt.release_complete);
    assert_eq!(backend.finishes, 1);
}
