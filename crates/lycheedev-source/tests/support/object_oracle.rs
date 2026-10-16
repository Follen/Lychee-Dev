//! Full visible structure and selected metadata of the actual owned carrier.
//! Expected projections are produced separately by the historical reference.
#![allow(dead_code)]
use emmylua_code_analysis::{LuaMemberKey, LuaTupleStatus, LuaType, VariadicType};
use lycheedev_emmy_adapter::session as owned;
use std::collections::BTreeMap;
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
fn structural_type(ty: &LuaType) -> Type {
    match ty {
        LuaType::Object(o) => Type::Object {
            metadata: None,
            fields: o
                .get_fields()
                .iter()
                .map(|(k, v)| (k.clone(), structural_type(v)))
                .collect(),
            indexes: o
                .get_index_access()
                .iter()
                .map(|(k, v)| (structural_type(k), structural_type(v)))
                .collect(),
        },
        LuaType::Tuple(t) => Type::Tuple(
            t.status,
            t.get_types().iter().map(structural_type).collect(),
        ),
        LuaType::Variadic(v) => match v.as_ref() {
            VariadicType::Base(t) => Type::Base(Box::new(structural_type(t))),
            VariadicType::Multi(ts) => Type::Pack(ts.iter().map(structural_type).collect()),
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
                            .unwrap_or_else(|| structural_type(t)),
                    )
                })
                .collect(),
            indexes: o
                .get_index_access()
                .iter()
                .map(|(k, v)| (structural_type(k), structural_type(v)))
                .collect(),
        },
        LuaType::Tuple(t) => Type::Tuple(
            t.status,
            v.carried_slots()
                .map(|slots| slots.iter().map(new_type).collect())
                .unwrap_or_else(|| t.get_types().iter().map(structural_type).collect()),
        ),
        LuaType::Variadic(t) => match t.as_ref() {
            VariadicType::Base(t) => Type::Base(Box::new(structural_type(t))),
            VariadicType::Multi(ts) => Type::Pack(
                v.carried_slots()
                    .map(|slots| slots.iter().map(new_type).collect())
                    .unwrap_or_else(|| ts.iter().map(structural_type).collect()),
            ),
        },
        _ => Type::Plain(v.structural_type().clone()),
    }
}
