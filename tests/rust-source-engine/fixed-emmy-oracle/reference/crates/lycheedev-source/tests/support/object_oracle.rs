//! Shared real fixed-engine conversion and full carrier comparison for object tests.
#![allow(dead_code)]
use super::support::*;
use emmylua_code_analysis::{
    LuaMemberKey, LuaTupleStatus, LuaType, VariadicType, binding_context as native,
};
use lycheedev_emmy_adapter::session as owned;
use std::collections::BTreeMap;
fn site(s: owned::BindingSite) -> native::BindingSite {
    native::BindingSite {
        file_id: s.file_id,
        syntax: s.syntax,
    }
}
pub fn osite(s: native::BindingSite) -> owned::BindingSite {
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
                owned::SelectedValue::EnvironmentCallable(v) => {
                    native::SelectedValue::EnvironmentCallable(v)
                }
                owned::SelectedValue::EnvironmentReturn(v) => {
                    native::SelectedValue::EnvironmentReturn(v)
                }
                _ => panic!("unrepresented fixture value must not masquerade as oracle coverage"),
            },
        }),
    }
}
pub struct Native(pub Provider);
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
pub struct Metadata {
    context: String,
    identity: String,
    epoch: String,
    view: owned::ObjectViewKey,
    site: owned::BindingSite,
    proof: String,
    complete: bool,
}
#[derive(Debug, PartialEq, Eq)]
pub enum Type {
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
pub fn old_type(ty: &LuaType) -> Type {
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
pub fn new_type(v: &owned::OwnedValue) -> Type {
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
pub fn row(r: native::BindingCoverage) -> owned::BindingCoverage {
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
