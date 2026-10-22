use super::*;
use emmylua_code_analysis::FileId;
use emmylua_parser::{LuaSyntaxId, LuaSyntaxKind};
use std::sync::atomic::{AtomicUsize, Ordering};

#[derive(Debug)]
struct Stop {
    ordinal: usize,
    token: Arc<()>,
}
fn ok() -> Result<(), Stop> {
    Ok(())
}
fn site(id: u32) -> BindingSite {
    BindingSite {
        file_id: FileId::new(id),
        syntax: LuaSyntaxId::new(LuaSyntaxKind::NameExpr.into(), Default::default()),
    }
}
fn row(id: u32, operation: &'static str) -> BindingCoverage {
    BindingCoverage {
        site: site(id),
        operation,
        state: if operation == "callable_check" {
            "owned_checked"
        } else {
            "native_checked"
        },
        proof: format!("proof-{id}"),
        reason: None,
        parameter: None,
        parameter_index: None,
        argument: None,
    }
}
struct Provider {
    identity: &'static str,
    reads: AtomicUsize,
    malformed: Option<&'static str>,
}
impl BindingContext for Provider {
    fn identity(&self) -> &str {
        self.identity
    }
    fn resolve(&self, _: BindingSite) -> BindingDecision<BoundValue> {
        BindingDecision::NotManaged
    }
    fn object_shape(&self, key: &ObjectViewKey) -> BindingDecision<ObjectShape> {
        self.reads.fetch_add(1, Ordering::Relaxed);
        let mut shape = ObjectShape {
            key: key.clone(),
            fields: vec![],
            enumeration: ShapeEnumeration::Complete,
            proof: "shape-proof".into(),
        };
        match self.malformed {
            Some("key") => shape.key.object_id += 1,
            Some("proof") => shape.proof.clear(),
            Some("fields") => {
                shape.fields = (0..129)
                    .map(|i| ObjectField {
                        key: format!("field-{i}"),
                        presence: FieldPresence::Absent,
                        value: BindingDecision::KnownAbsent {
                            proof: "absent".into(),
                        },
                        destination_write: FieldWriteOrigin::Lua(SourceSpan {
                            file_id: FileId::new(1),
                            range: crate::ByteRange { start: 0, end: 1 },
                        }),
                        source_write: None,
                        copy_origin: None,
                    })
                    .collect()
            }
            Some("unavailable") => {
                return BindingDecision::Unavailable {
                    proof: "unavailable".into(),
                    reason: "unavailable".into(),
                };
            }
            _ => {}
        }
        BindingDecision::Bound(shape)
    }
}
fn provider() -> Arc<Provider> {
    Arc::new(Provider {
        identity: "context",
        reads: AtomicUsize::new(0),
        malformed: None,
    })
}
fn session() -> Arc<OwnedBindingSession> {
    OwnedBindingSession::new_checked(provider(), ok).unwrap()
}
fn object(id: usize) -> ObjectRef {
    ObjectRef {
        identity: format!("object-{id}"),
        epoch: "epoch-1".into(),
        origin: ObjectOrigin::Lua(site(id as u32)),
        view: ObjectViewKey {
            context: "context".into(),
            observation: 0,
            object_id: id,
        },
    }
}

// Frozen old record body, copied from exact tree 5fd1358357... binding_context.rs
// 231-270. The only abstraction is a supplied row instead of its five arguments;
// all compared legacy stream rows retain None for parameter/argument fields.
#[derive(Default)]
struct LegacyRecord {
    rows: Vec<BindingCoverage>,
    seen: HashSet<BindingCoverage>,
}
impl LegacyRecord {
    fn append(&mut self, row: BindingCoverage) {
        if self.rows.len() >= 16_384 {
            return;
        }
        if self.seen.contains(&row) {
            return;
        }
        if self.rows.len() == 16_383 {
            self.rows.push(BindingCoverage {
                site: row.site,
                operation: "coverage",
                state: "unavailable",
                proof: String::new(),
                reason: Some("binding_coverage_budget".into()),
                parameter: None,
                parameter_index: None,
                argument: None,
            });
            return;
        }
        self.seen.insert(row.clone());
        self.rows.push(row);
    }
}

#[test]
fn append_matches_old_full_order_and_every_row_field_across_limit() {
    let session = session();
    let mut old = LegacyRecord::default();
    let ops = [
        "call_permit",
        "resolve",
        "argument_check",
        "return_check",
        "argument_count_check",
        "callback_count_check",
        "return_count_check",
        "callable_check",
    ];
    for i in 0..25_000u32 {
        let event = row(i, ops[i as usize % ops.len()]);
        old.append(event.clone());
        session.record(event.clone(), ok).unwrap();
        if i % 7 == 0 {
            old.append(event.clone());
            session.record(event, ok).unwrap();
        }
        if [0, 1, 16_381, 16_382, 16_383, 16_384, 24_999].contains(&i) {
            assert_eq!(
                session.coverage_checked(ok).unwrap(),
                old.rows,
                "producer index {i}"
            );
        }
    }
    assert_eq!(old.rows.len(), 16_384);
    assert_eq!(old.rows.last().unwrap().site, site(16_383));
    assert_eq!(
        old.rows.last().unwrap().reason.as_deref(),
        Some("binding_coverage_budget")
    );
}

#[test]
fn duplicate_at_last_slot_does_not_create_a_false_truncation() {
    let session = session();
    for i in 0..16_383 {
        session.record(row(i, "resolve"), ok).unwrap();
    }
    session.record(row(0, "resolve"), ok).unwrap();
    assert_eq!(session.coverage_checked(ok).unwrap().len(), 16_383);
    session.record(row(40_000, "callable_check"), ok).unwrap();
    let snapshot = session.coverage_checked(ok).unwrap();
    assert_eq!(snapshot.last().unwrap().site, site(40_000));
    session.record(row(40_001, "return_check"), ok).unwrap();
    assert_eq!(session.coverage_checked(ok).unwrap(), snapshot);
}

#[test]
fn dedup_considers_proof_reason_parameter_and_argument_fields() {
    let session = session();
    let first = row(1, "argument_check");
    let mut rows = vec![first.clone()];
    let mut x = first.clone();
    x.proof = "other".into();
    rows.push(x);
    let mut x = first.clone();
    x.reason = Some("reason".into());
    rows.push(x);
    let mut x = first.clone();
    x.parameter = Some("parameter".into());
    rows.push(x);
    let mut x = first.clone();
    x.parameter_index = Some(2);
    rows.push(x);
    let mut x = first;
    x.argument = Some(site(2));
    rows.push(x);
    for x in &rows {
        session.record(x.clone(), ok).unwrap();
        session.record(x.clone(), ok).unwrap();
    }
    assert_eq!(session.coverage_checked(ok).unwrap(), rows);
}

#[test]
fn record_and_snapshot_propagate_typed_cancel_without_partial_output() {
    let session = session();
    let token = Arc::new(());
    let Err(error) = session.record(row(0, "resolve"), || {
        Err(Stop {
            ordinal: 1,
            token: token.clone(),
        })
    }) else {
        panic!("cancel ignored")
    };
    assert!(Arc::ptr_eq(&error.token, &token));
    assert!(session.coverage_checked(ok).unwrap().is_empty());
    for i in 0..5 {
        session.record(row(i, "resolve"), ok).unwrap();
    }
    let original = session.coverage_checked(ok).unwrap();
    let mut count = 0;
    session
        .coverage_checked(|| {
            count += 1;
            ok()
        })
        .unwrap();
    assert_eq!(count, 7);
    for ordinal in 1..=count {
        let mut seen = 0;
        let result = session.coverage_checked(|| {
            seen += 1;
            if seen == ordinal {
                Err(Stop {
                    ordinal,
                    token: token.clone(),
                })
            } else {
                ok()
            }
        });
        let Err(error) = result else {
            panic!("checkpoint {ordinal} produced output")
        };
        assert_eq!(error.ordinal, ordinal);
        assert!(Arc::ptr_eq(&error.token, &token));
        assert_eq!(seen, ordinal);
        assert_eq!(session.coverage_checked(ok).unwrap(), original);
    }
}

#[test]
fn provider_is_shared_but_equal_labels_never_mean_equal_sessions() {
    let provider = provider();
    let count = Arc::strong_count(&provider);
    let a = OwnedBindingSession::new_checked(provider.clone(), ok).unwrap();
    let b = OwnedBindingSession::new_checked(provider.clone(), ok).unwrap();
    assert_eq!(Arc::strong_count(&provider), count + 2);
    assert_eq!(a.identity(), b.identity());
    assert!(!a.same_session(&b));
    assert!(a.same_session(&a));
    let frozen = a.freeze_shape(&object(0), ok).unwrap().unwrap();
    assert!(a.owns_shape(&frozen));
    assert!(!b.owns_shape(&frozen));
    assert!(
        a.coverage_checked(ok).unwrap().is_empty(),
        "shape ownership is not a checker completion"
    );
    let again = a.freeze_shape(&object(0), ok).unwrap().unwrap();
    assert!(Arc::ptr_eq(&frozen.shape, &again.shape));
    assert_eq!(provider.reads.load(Ordering::Relaxed), 1);
}

#[test]
fn projection_guards_keep_64_bound_and_release_on_error_and_unwind() {
    let session = session();
    let mut guards = Vec::new();
    for i in 0..64 {
        guards.push(session.enter(site(i), ok).unwrap().unwrap());
    }
    assert!(session.enter(site(65), ok).unwrap().is_none());
    guards.pop();
    assert!(session.enter(site(63), ok).unwrap().is_some());
    drop(guards);
    assert!(session.enter(site(0), ok).unwrap().is_some());
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _guard = session.enter(site(0), ok).unwrap().unwrap();
        panic!("exercise RAII only");
    }));
    assert!(session.enter(site(0), ok).unwrap().is_some());
    let result = (|| -> Result<(), Stop> {
        let _guard = session.enter(site(0), ok)?.unwrap();
        Err(Stop {
            ordinal: 1,
            token: Arc::new(()),
        })
    })();
    assert!(result.is_err());
    assert!(session.enter(site(0), ok).unwrap().is_some());
}

#[test]
fn object_guards_and_frozen_epochs_reject_cross_view_substitution() {
    let session = session();
    let selected = object(1);
    let _frozen = session.freeze_shape(&selected, ok).unwrap().unwrap();
    for variant in ["identity", "epoch", "origin"] {
        let mut changed = selected.clone();
        match variant {
            "identity" => changed.identity = "other".into(),
            "epoch" => changed.epoch = "other".into(),
            _ => changed.origin = ObjectOrigin::Lua(site(99)),
        }
        assert_eq!(
            session.freeze_shape(&changed, ok).unwrap().unwrap_err(),
            "object_frozen_identity_mismatch"
        );
    }
    for variant in ["empty-identity", "empty-epoch", "context"] {
        let mut changed = object(2);
        match variant {
            "empty-identity" => changed.identity.clear(),
            "empty-epoch" => changed.epoch.clear(),
            _ => changed.view.context = "foreign".into(),
        }
        assert_eq!(
            session.freeze_shape(&changed, ok).unwrap().unwrap_err(),
            "object_context_or_epoch_mismatch"
        );
    }
    let mut guards = Vec::new();
    for i in 0..64 {
        guards.push(session.enter_object(object(i).view, ok).unwrap().unwrap());
    }
    assert!(session.enter_object(object(64).view, ok).unwrap().is_none());
    drop(guards);
    let guard = session
        .enter_object(selected.view.clone(), ok)
        .unwrap()
        .unwrap();
    assert!(
        session
            .enter_object(selected.view.clone(), ok)
            .unwrap()
            .is_none()
    );
    drop(guard);
    assert!(session.enter_object(selected.view, ok).unwrap().is_some());
}

#[test]
fn shape_limits_and_cancel_do_not_create_coverage_or_poison_cache() {
    for (bad, reason) in [
        ("key", "object_shape_identity_or_budget"),
        ("proof", "object_shape_identity_or_budget"),
        ("fields", "object_shape_identity_or_budget"),
        ("unavailable", "object_shape_unavailable"),
    ] {
        let p = Arc::new(Provider {
            identity: "context",
            reads: AtomicUsize::new(0),
            malformed: Some(bad),
        });
        let session = OwnedBindingSession::new_checked(p, ok).unwrap();
        assert_eq!(
            session.freeze_shape(&object(0), ok).unwrap().unwrap_err(),
            reason
        );
        assert!(session.coverage_checked(ok).unwrap().is_empty());
    }
    let p = provider();
    let session = OwnedBindingSession::new_checked(p.clone(), ok).unwrap();
    let mut seen = 0;
    let token = Arc::new(());
    let result = session.freeze_shape(&object(0), || {
        seen += 1;
        if seen == 2 {
            Err(Stop {
                ordinal: 2,
                token: token.clone(),
            })
        } else {
            ok()
        }
    });
    let Err(error) = result else {
        panic!("cancel after provider read ignored")
    };
    assert!(Arc::ptr_eq(&error.token, &token));
    assert_eq!(p.reads.load(Ordering::Relaxed), 1);
    session.freeze_shape(&object(0), ok).unwrap().unwrap();
    assert_eq!(p.reads.load(Ordering::Relaxed), 2);
    for i in 1..1_024 {
        session.freeze_shape(&object(i), ok).unwrap().unwrap();
    }
    assert_eq!(
        session
            .freeze_shape(&object(1_024), ok)
            .unwrap()
            .unwrap_err(),
        "object_shape_budget"
    );
    assert!(session.coverage_checked(ok).unwrap().is_empty());
}
