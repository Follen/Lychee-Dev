use super::*;
use crate::mailbox::PREVIOUS_SCHEMA;

fn previous_identity() -> Identity {
    let mut i = identity('2');
    i.schema = PREVIOUS_SCHEMA.into();
    i.input_state = PREVIOUS_SCHEMA.into();
    i
}

#[test]
fn connect_previous_runtime_uses_upgrade_activation_without_business_input() {
    let f = Fixture::new();
    let counts = Rc::new(RefCell::new(Counts {
        candidates: vec![previous_identity()],
        ..Counts::default()
    }));
    let clock = Rc::new(Cell::new(1000));
    let mut service = service(&f, &counts, &clock);
    let out = service.connect_with_activation(&f.request(), &OperationControl::default());
    assert!(out.error().is_some());
    let report = out.into_report().unwrap();
    assert_eq!(report.stage, "activation_waiting_manual_reload");
    assert!(
        f.read(&report.session)
            .request
            .starts_with("connect-upgrade-")
    );
    assert_eq!(counts.borrow().inputs, 0);
}

#[test]
fn previous_two_key_manual_wait_hands_off_same_con_and_starts_activation_budget_once() {
    let f = Fixture::new();
    let counts = Rc::new(RefCell::new(Counts::default()));
    let clock = Rc::new(Cell::new(1000));
    let (mut service, id) = start(&f, &counts, &clock);
    clock.set(9_000_000);
    counts.borrow_mut().candidates = vec![previous_identity()];
    counts.borrow_mut().capability = true;
    let out = service.resume_with_activation(&id, &OperationControl::default());
    assert_eq!(out.into_report().unwrap().session, id);
    let book = ConnectionBook::open(&f.root).unwrap();
    let state = book
        .status(&id, &OperationControl::default())
        .unwrap()
        .state;
    assert_eq!(state.identity, previous_identity());
    assert!(!state.bound);
    assert_eq!(
        state.connect_budget.as_ref().unwrap().started_at_ms,
        9_000_000
    );
    let reload = state.reload.as_ref().unwrap();
    assert_eq!(reload.request, "activation-key");
    assert_eq!(reload.recovery_budget, state.connect_budget);
    assert_eq!(
        reload.phase,
        crate::mailbox::state::ReloadPhase::InputAttempted
    );
    assert_eq!(
        counts.borrow().actions,
        [crate::mailbox::state::InputKind::Reload]
    );
    let original_deadline = state.connect_budget.unwrap().deadline_ms;
    clock.set(9_000_100);
    let _ = service.resume_with_activation(&id, &OperationControl::default());
    let resumed = book
        .status(&id, &OperationControl::default())
        .unwrap()
        .state;
    assert_eq!(
        resumed.connect_budget.unwrap().deadline_ms,
        original_deadline
    );
    assert_eq!(counts.borrow().inputs, 1);
}

#[test]
fn prepared_previous_runtime_reload_keeps_original_activation_deadline() {
    let f = Fixture::new();
    let counts = Rc::new(RefCell::new(Counts::default()));
    let clock = Rc::new(Cell::new(1000));
    let (mut service, id) = start(&f, &counts, &clock);
    f.mutate(&id, |a| a.phase = ActivationPhase::Prepared);
    let original = f.read(&id).budget.unwrap();
    counts.borrow_mut().candidates = vec![previous_identity()];
    counts.borrow_mut().capability = true;
    clock.set(1100);
    let out = service.resume_with_activation(&id, &OperationControl::default());
    assert_eq!(out.into_report().unwrap().session, id);
    let state = ConnectionBook::open(&f.root)
        .unwrap()
        .status(&id, &OperationControl::default())
        .unwrap()
        .state;
    let budget = state.connect_budget.unwrap();
    assert_eq!(budget.started_at_ms, original.started_at_ms);
    assert_eq!(budget.deadline_ms, original.deadline_ms);
    assert_eq!(
        counts.borrow().actions,
        [crate::mailbox::state::InputKind::Reload]
    );
}
