//! Real object projection comparison: every structural and visible metadata field,
//! all provider calls/coverage, and opaque session-token equality relationships.
#![cfg(feature = "emmy-prototype")]
#[path = "../../lycheedev-emmy-adapter/tests/support/query_backend_cases.rs"]
mod query_support;
#[path = "../../lycheedev-emmy-adapter/tests/support/object_carrier_cases.rs"]
mod support;
use emmylua_code_analysis::{
    EmmyLuaAnalysis, LuaMemberKey, LuaTupleStatus, LuaType, VariadicType, binding_context as native,
};
use lycheedev_emmy_adapter::session as owned;
use std::{
    collections::{BTreeMap, HashSet},
    sync::Arc,
};
use support::*;
fn site(s: owned::BindingSite) -> native::BindingSite {
    native::BindingSite {
        file_id: s.file_id,
        syntax: s.syntax,
    }
}
fn osite(s: native::BindingSite) -> owned::BindingSite {
    owned::BindingSite {
        file_id: s.file_id,
        syntax: s.syntax,
    }
}
fn external(s: owned::ExternalSourceSpan) -> native::ExternalSourceSpan {
    native::ExternalSourceSpan {
        path: s.path,
        content_sha256: s.content_sha256,
        range: rowan::TextRange::new(s.range.start.into(), s.range.end.into()),
    }
}
fn write(s: owned::FieldWriteOrigin) -> native::FieldWriteOrigin {
    match s {
        owned::FieldWriteOrigin::Xml(s) => native::FieldWriteOrigin::Xml(external(s)),
        owned::FieldWriteOrigin::Lua(s) => native::FieldWriteOrigin::Lua(native::SourceSpan {
            file_id: s.file_id,
            range: rowan::TextRange::new(s.range.start.into(), s.range.end.into()),
        }),
    }
}
fn function(f: owned::FunctionRef) -> native::FunctionRef {
    native::FunctionRef {
        file_id: f.file_id,
        closure: f.closure,
        declaration: f.declaration,
    }
}
fn nobject(o: owned::ObjectRef) -> native::ObjectRef {
    native::ObjectRef {
        identity: o.identity,
        epoch: o.epoch,
        view: o.view,
        origin: match o.origin {
            owned::ObjectOrigin::Lua(s) => native::ObjectOrigin::Lua(site(s)),
            owned::ObjectOrigin::Xml(s) => native::ObjectOrigin::Xml(external(s)),
        },
    }
}
fn decision(
    d: owned::BindingDecision<owned::BoundValue>,
) -> native::BindingDecision<native::BoundValue> {
    match d {
        owned::BindingDecision::NotManaged => native::BindingDecision::NotManaged,
        owned::BindingDecision::KnownAbsent { proof } => {
            native::BindingDecision::KnownAbsent { proof }
        }
        owned::BindingDecision::Unavailable { proof, reason } => {
            native::BindingDecision::Unavailable { proof, reason }
        }
        owned::BindingDecision::Bound(v) => native::BindingDecision::Bound(native::BoundValue {
            identity: v.identity,
            proof: v.proof,
            value: match v.value {
                owned::SelectedValue::Object(o) => native::SelectedValue::Object(nobject(o)),
                owned::SelectedValue::Expression(s) => native::SelectedValue::Expression(site(s)),
                owned::SelectedValue::Function(f) => native::SelectedValue::Function(function(f)),
                owned::SelectedValue::EnvironmentReturn(v) => {
                    native::SelectedValue::EnvironmentReturn(v)
                }
                _ => panic!("unrepresented fixture value must not masquerade as oracle coverage"),
            },
        }),
    }
}
struct Native(Provider);
impl native::BindingContext for Native {
    fn identity(&self) -> &str {
        "owned-query-backend-oracle"
    }
    fn resolve(&self, s: native::BindingSite) -> native::BindingDecision<native::BoundValue> {
        decision(owned::BindingContext::resolve(&self.0, osite(s)))
    }
    fn call(&self, s: native::BindingSite) -> native::BindingDecision<native::CallPermit> {
        match owned::BindingContext::call(&self.0, osite(s)) {
            owned::BindingDecision::NotManaged => native::BindingDecision::NotManaged,
            owned::BindingDecision::KnownAbsent { proof } => {
                native::BindingDecision::KnownAbsent { proof }
            }
            owned::BindingDecision::Unavailable { proof, reason } => {
                native::BindingDecision::Unavailable { proof, reason }
            }
            owned::BindingDecision::Bound(p) => {
                native::BindingDecision::Bound(native::CallPermit {
                    callee: site(p.callee),
                    proof: p.proof,
                    capability_profile: p.capability_profile,
                })
            }
        }
    }
    fn call_observation(
        &self,
        s: native::BindingSite,
    ) -> native::BindingDecision<native::CallObservation> {
        match owned::BindingContext::call_observation(&self.0, osite(s)) {
            owned::BindingDecision::NotManaged => native::BindingDecision::NotManaged,
            owned::BindingDecision::KnownAbsent { proof } => {
                native::BindingDecision::KnownAbsent { proof }
            }
            owned::BindingDecision::Unavailable { proof, reason } => {
                native::BindingDecision::Unavailable { proof, reason }
            }
            owned::BindingDecision::Bound(o) => {
                native::BindingDecision::Bound(native::CallObservation {
                    callee: match o.callee {
                        owned::ObservedCallee::Source(f) => {
                            native::ObservedCallee::Source(function(f))
                        }
                        owned::ObservedCallee::Environment(e) => {
                            native::ObservedCallee::Environment(e)
                        }
                    },
                    selected_callee_identity: o.selected_callee_identity,
                    closure_identity: o.closure_identity,
                    invocation_identity: o.invocation_identity,
                    returns: o.returns.into_iter().map(decision).collect(),
                    proof: o.proof,
                })
            }
        }
    }
    fn object_shape(
        &self,
        k: &native::ObjectViewKey,
    ) -> native::BindingDecision<native::ObjectShape> {
        match owned::BindingContext::object_shape(&self.0, k) {
            owned::BindingDecision::NotManaged => native::BindingDecision::NotManaged,
            owned::BindingDecision::KnownAbsent { proof } => {
                native::BindingDecision::KnownAbsent { proof }
            }
            owned::BindingDecision::Unavailable { proof, reason } => {
                native::BindingDecision::Unavailable { proof, reason }
            }
            owned::BindingDecision::Bound(s) => {
                native::BindingDecision::Bound(native::ObjectShape {
                    key: s.key,
                    enumeration: s.enumeration,
                    proof: s.proof,
                    fields: s
                        .fields
                        .into_iter()
                        .map(|f| native::ObjectField {
                            key: f.key,
                            presence: f.presence,
                            value: decision(f.value),
                            destination_write: write(f.destination_write),
                            source_write: f.source_write.map(write),
                            copy_origin: f.copy_origin.map(write),
                        })
                        .collect(),
                })
            }
        }
    }
}
#[derive(Debug, PartialEq, Eq)]
struct Metadata {
    context: String,
    identity: String,
    epoch: String,
    view: owned::ObjectViewKey,
    site: owned::BindingSite,
    proof: String,
    complete: bool,
}
#[derive(Debug, PartialEq, Eq)]
enum Type {
    Plain(LuaType),
    Object {
        metadata: Option<Metadata>,
        fields: BTreeMap<LuaMemberKey, Type>,
        indexes: Vec<(Type, Type)>,
    },
    Tuple(LuaTupleStatus, Vec<Type>),
    Base(Box<Type>),
    Pack(Vec<Type>),
}
fn old_type(ty: &LuaType) -> Type {
    match ty {
        LuaType::Object(o) => Type::Object {
            metadata: o.binding().map(|m| Metadata {
                context: m.context.clone(),
                identity: m.identity.clone(),
                epoch: m.epoch.clone(),
                view: m.view.clone(),
                site: osite(m.site),
                proof: m.proof.clone(),
                complete: m.complete,
            }),
            fields: o
                .get_fields()
                .iter()
                .map(|(k, v)| (k.clone(), old_type(v)))
                .collect(),
            indexes: o
                .get_index_access()
                .iter()
                .map(|(k, v)| (old_type(k), old_type(v)))
                .collect(),
        },
        LuaType::Tuple(t) => Type::Tuple(t.status, t.get_types().iter().map(old_type).collect()),
        LuaType::Variadic(v) => match v.as_ref() {
            VariadicType::Base(t) => Type::Base(Box::new(old_type(t))),
            VariadicType::Multi(ts) => Type::Pack(ts.iter().map(old_type).collect()),
        },
        _ => Type::Plain(ty.clone()),
    }
}
fn new_type(v: &owned::OwnedValue) -> Type {
    match v.structural_type() {
        LuaType::Object(o) => Type::Object {
            metadata: v.object_info().map(|m| Metadata {
                context: m.context.into(),
                identity: m.identity.into(),
                epoch: m.epoch.into(),
                view: m.view.clone(),
                site: m.site,
                proof: m.proof.into(),
                complete: m.complete,
            }),
            fields: o
                .get_fields()
                .iter()
                .map(|(k, t)| {
                    (
                        k.clone(),
                        v.carried_fields()
                            .and_then(|f| f.get(k))
                            .map(new_type)
                            .unwrap_or_else(|| old_type(t)),
                    )
                })
                .collect(),
            indexes: o
                .get_index_access()
                .iter()
                .map(|(k, v)| (old_type(k), old_type(v)))
                .collect(),
        },
        LuaType::Tuple(t) => Type::Tuple(
            t.status,
            v.carried_slots()
                .map(|slots| slots.iter().map(new_type).collect())
                .unwrap_or_else(|| t.get_types().iter().map(old_type).collect()),
        ),
        LuaType::Variadic(t) => match t.as_ref() {
            VariadicType::Base(t) => Type::Base(Box::new(old_type(t))),
            VariadicType::Multi(ts) => Type::Pack(
                v.carried_slots()
                    .map(|slots| slots.iter().map(new_type).collect())
                    .unwrap_or_else(|| ts.iter().map(old_type).collect()),
            ),
        },
        _ => Type::Plain(v.structural_type().clone()),
    }
}
fn row(r: native::BindingCoverage) -> owned::BindingCoverage {
    owned::BindingCoverage {
        site: osite(r.site),
        operation: r.operation,
        state: r.state,
        proof: r.proof,
        reason: r.reason,
        parameter: r.parameter,
        parameter_index: r.parameter_index,
        argument: r.argument.map(osite),
    }
}
fn compare(
    analysis: &EmmyLuaAnalysis,
    sites: &[owned::BindingSite],
    provider: &Provider,
) -> (
    Vec<LuaType>,
    Vec<owned::OwnedValue>,
    Vec<owned::BindingCoverage>,
) {
    let old_provider = provider.isolated();
    let new_provider = provider.isolated();
    let old_session = native::BindingSession::new(Arc::new(Native(old_provider.clone())));
    let old_model = analysis.semantic_model_with_context(sites[0].file_id, old_session.clone());
    let new_session = session(&new_provider);
    let mut check = || Ok::<_, Stop>(());
    let mut query = owned::OwnedQuery::new(analysis, &new_session, sites[0].file_id, &mut check);
    let mut old_values = vec![];
    let mut new_values = vec![];
    for site in sites {
        let old = old_model.type_of_expr(site.syntax);
        let new = query.type_of_expr(site.syntax).unwrap();
        assert_eq!(
            new_type(&new),
            old_type(&old),
            "all structure and visible object metadata at {site:?}"
        );
        old_values.push(old);
        new_values.push(new);
    }
    let coverage = rows(&new_session);
    assert_eq!(
        coverage,
        old_session
            .coverage()
            .into_iter()
            .map(row)
            .collect::<Vec<_>>()
    );
    assert_eq!(
        *new_provider.events.lock().unwrap(),
        *old_provider.events.lock().unwrap()
    );
    println!(
        "OWNED_OBJECT_NATIVE queries={} coverage={} providerCalls={}",
        sites.len(),
        coverage.len(),
        new_provider.events.lock().unwrap().len()
    );
    (old_values, new_values, coverage)
}

#[test]
fn object_presence_origin_shape_and_field_failures_match_all_metadata_and_producer_rows() {
    let mut comparisons = 0;
    for case in 0..23 {
        let (analysis, site, mut provider, mut selected) = basic_case();
        let mut fields = vec![field("nil", owned::FieldPresence::Present, absent("nil"))];
        let mut enumeration = owned::ShapeEnumeration::Complete;
        match case {
            0 => {}
            1 => {
                fields = vec![field(
                    "any",
                    owned::FieldPresence::Present,
                    environment_any(),
                )]
            }
            2 => {
                fields = vec![field(
                    "absent",
                    owned::FieldPresence::Absent,
                    absent("absent"),
                )]
            }
            3 => {
                fields = vec![field(
                    "absent",
                    owned::FieldPresence::Absent,
                    owned::BindingDecision::NotManaged,
                )]
            }
            4 => {
                fields = vec![field(
                    "unavailable",
                    owned::FieldPresence::Unavailable,
                    environment_any(),
                )]
            }
            5 => {
                fields = vec![field(
                    "unmanaged",
                    owned::FieldPresence::Present,
                    owned::BindingDecision::NotManaged,
                )]
            }
            6 => {
                fields = vec![field(
                    "unavailable",
                    owned::FieldPresence::Present,
                    owned::BindingDecision::Unavailable {
                        proof: "missing-field".into(),
                        reason: "not-observed".into(),
                    },
                )]
            }
            7 => {
                fields = vec![field(
                    "cycle",
                    owned::FieldPresence::Present,
                    expression(site, "recursive-expression"),
                )]
            }
            8 => {
                fields = vec![field(
                    "cycle",
                    owned::FieldPresence::Present,
                    bound_object(&selected),
                )]
            }
            9 => {
                fields = vec![
                    field("", owned::FieldPresence::Present, absent("empty-key")),
                    field("1", owned::FieldPresence::Present, absent("numeric-name")),
                ]
            }
            10 => fields.push(field(
                "nil",
                owned::FieldPresence::Absent,
                absent("duplicate-even-absent"),
            )),
            11 => enumeration = owned::ShapeEnumeration::Unavailable("not-enumerated".into()),
            12 => selected.view.context = "foreign-context".into(),
            13 => selected.identity.clear(),
            14 => selected.epoch.clear(),
            15 => selected.origin = owned::ObjectOrigin::Xml(xml()),
            16 => {
                let mut origin = xml();
                origin.path.clear();
                selected.origin = owned::ObjectOrigin::Xml(origin);
            }
            17 => {
                let mut origin = xml();
                origin.content_sha256.pop();
                selected.origin = owned::ObjectOrigin::Xml(origin);
            }
            18 => {
                let mut origin = site;
                origin.file_id.id = u32::MAX;
                selected.origin = owned::ObjectOrigin::Lua(origin);
            }
            19 => {
                fields[0].source_write = Some(owned::FieldWriteOrigin::Xml(xml()));
                fields[0].copy_origin = Some(owned::FieldWriteOrigin::Xml(xml()));
            }
            20 => {
                fields = (0..128)
                    .map(|i| {
                        field(
                            &format!("key{i}"),
                            owned::FieldPresence::Present,
                            absent(&format!("proof{i}")),
                        )
                    })
                    .collect()
            }
            21 => {
                fields = (0..129)
                    .map(|i| {
                        field(
                            &format!("key{i}"),
                            owned::FieldPresence::Present,
                            absent(&format!("proof{i}")),
                        )
                    })
                    .collect()
            }
            22 => {
                use emmylua_parser::{LuaAstNode, LuaReturnStat};
                let ret = analysis
                    .semantic_model(site.file_id)
                    .syntax_tree()
                    .unwrap()
                    .get_red_root()
                    .descendants()
                    .find_map(LuaReturnStat::cast)
                    .unwrap();
                selected.origin = owned::ObjectOrigin::Lua(owned::BindingSite {
                    file_id: site.file_id,
                    syntax: ret.get_syntax_id(),
                });
            }
            _ => unreachable!(),
        }
        let mut object_shape = shape(&selected, fields);
        object_shape.enumeration = enumeration;
        select_shape(&mut provider, site, &selected, object_shape);
        compare(&analysis, &[site, site], &provider);
        comparisons += 2;
    }
    assert_eq!(comparisons, 46);
}

#[test]
fn nested_incomplete_fields_and_active_object_depth_match_without_flattening_identity() {
    for depth in [2, 64, 65] {
        let (analysis, site, mut provider, root) = basic_case();
        for index in 1..=depth {
            let current = object(site, index);
            let fields = if index == depth {
                vec![field(
                    "missing",
                    owned::FieldPresence::Unavailable,
                    absent("ignored"),
                )]
            } else {
                vec![field(
                    "child",
                    owned::FieldPresence::Present,
                    bound_object(&object(site, index + 1)),
                )]
            };
            provider.shapes.insert(
                current.view.clone(),
                owned::BindingDecision::Bound(shape(&current, fields)),
            );
        }
        provider.values.insert(site, bound_object(&root));
        let (_, values, _) = compare(&analysis, &[site, site], &provider);
        assert!(values[0].object_info().unwrap().complete);
    }
}

#[test]
fn tokens_projection_sites_hash_sets_and_cache_hits_have_the_same_equality_relations() {
    let (analysis, site, mut provider) = load("return first,second", "first");
    let second = provider.named("second");
    let selected = object(site, 1);
    select_shape(&mut provider, site, &selected, shape(&selected, vec![]));
    provider.values.insert(second, bound_object(&selected));
    let (mut old, mut new, _) = compare(&analysis, &[site, site, second], &provider);
    let (other_old, other_new, _) = compare(&analysis, &[site, site, second], &provider);
    old.extend(other_old);
    new.extend(other_new);
    for a in 0..old.len() {
        for b in 0..old.len() {
            assert_eq!(
                old[a] == old[b],
                new[a] == new[b],
                "token/site equality ({a},{b})"
            );
        }
    }
    assert_eq!(
        old.into_iter().collect::<HashSet<_>>().len(),
        new.into_iter().collect::<HashSet<_>>().len()
    );
}

#[test]
fn actual_unary_tuple_logical_declaration_and_observed_return_transports_keep_nested_carriers() {
    for expression in [
        "-selected",
        "#selected",
        "true and {selected}",
        "selected or {}",
    ] {
        let (analysis, site, mut provider) = load(&format!("return {expression}"), expression);
        let selected_site = provider.named("selected");
        let selected = object(selected_site, 1);
        select_shape(
            &mut provider,
            selected_site,
            &selected,
            shape(&selected, vec![]),
        );
        compare(&analysis, &[site, site], &provider);
    }
    for source in [
        "local function f() end\nreturn f()",
        "local function f() end\nlocal first,second=f()\nreturn second",
    ] {
        let query = if source.contains("second") {
            "second"
        } else {
            "f()"
        };
        let (analysis, site, mut provider) = load(source, query);
        let call = provider.named("f()");
        let selected = object(call, 1);
        provider.shapes.insert(
            selected.view.clone(),
            owned::BindingDecision::Bound(shape(&selected, vec![])),
        );
        observed_call(
            &analysis,
            &mut provider,
            call,
            vec![absent("nil-slot"), bound_object(&selected)],
        );
        compare(&analysis, &[site, site], &provider);
    }
}

#[test]
fn the_1025th_shape_keeps_its_real_unavailable_receipt_after_the_shared_freeze_budget() {
    let names = (0..1025)
        .map(|i| format!("object{i}"))
        .collect::<Vec<_>>()
        .join(",");
    let (analysis, _, mut provider) = load(&format!("return {names}"), "object0");
    let mut sites = vec![];
    for i in 0..1025 {
        let site = provider.named(&format!("object{i}"));
        let selected = object(site, i + 1);
        select_shape(&mut provider, site, &selected, shape(&selected, vec![]));
        sites.push(site);
    }
    let (_, new, ledger) = compare(&analysis, &sites, &provider);
    assert!(new[..1024].iter().all(|v| v.object_info().is_some()));
    assert_eq!(new[1024].structural_type(), &LuaType::Unknown);
    assert_eq!(
        ledger.last().unwrap().reason.as_deref(),
        Some("object_shape_budget")
    );
}

#[test]
fn a_second_projection_with_the_same_view_and_different_epoch_keeps_the_frozen_identity_failure() {
    let (analysis, first, mut provider) = load("return first,second", "first");
    let second = provider.named("second");
    let selected = object(first, 1);
    select_shape(
        &mut provider,
        first,
        &selected,
        shape(
            &selected,
            vec![field("nil", owned::FieldPresence::Present, absent("nil"))],
        ),
    );
    let mut changed = selected.clone();
    changed.epoch = "epoch-2".into();
    provider.values.insert(second, bound_object(&changed));
    let (_, new, ledger) = compare(&analysis, &[first, second], &provider);
    assert!(new[0].object_info().is_some());
    assert_eq!(new[1].structural_type(), &LuaType::Unknown);
    assert_eq!(
        ledger.last().unwrap().reason.as_deref(),
        Some("object_frozen_identity_mismatch")
    );
}
