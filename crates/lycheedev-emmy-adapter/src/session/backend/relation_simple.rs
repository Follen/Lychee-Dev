//! Fixed general-mode primitive compatibility. Recursive simple checks do not
//! re-enter the structure gate or expand target aliases a second time.
use super::{
    OwnedQuery, QueryResult,
    relation::{Relation, RelationMode, next_relation_level, relation_from_bool},
};
use crate::session::{OwnedValue, SelectedProjectionBackend};
use emmylua_code_analysis::{LuaType, LuaTypeDeclId, TypeDefKind, VariadicType};

impl<E> OwnedQuery<'_, '_, E> {
    pub(super) fn simple_relation(
        &mut self,
        source: &OwnedValue,
        target: &OwnedValue,
        mode: RelationMode,
        depth: usize,
    ) -> QueryResult<Relation, E> {
        self.checkpoint()?;
        let s = source.structural_type();
        let t = target.structural_type();
        let pass = match s {
            LuaType::Unknown | LuaType::Any | LuaType::TplRef(_) => true,
            LuaType::Nil => matches!(t, LuaType::Nil),
            LuaType::Table | LuaType::TableConst(_) => matches!(
                t,
                LuaType::Table
                    | LuaType::TableConst(_)
                    | LuaType::Tuple(_)
                    | LuaType::Array(_)
                    | LuaType::Object(_)
                    | LuaType::Ref(_)
                    | LuaType::Def(_)
                    | LuaType::TableGeneric(_)
                    | LuaType::Generic(_)
                    | LuaType::Global
                    | LuaType::Userdata
                    | LuaType::Instance(_)
                    | LuaType::Any
            ),
            LuaType::Userdata => matches!(t, LuaType::Userdata | LuaType::Ref(_) | LuaType::Def(_)),
            LuaType::Function => matches!(
                t,
                LuaType::Function | LuaType::DocFunction(_) | LuaType::Signature(_)
            ),
            LuaType::Thread => matches!(t, LuaType::Thread),
            LuaType::Boolean | LuaType::BooleanConst(_) => {
                if t.is_boolean() {
                    true
                } else if let Some(members) = target.union_components() {
                    let mut found = false;
                    for member in members {
                        self.checkpoint()?;
                        if member.structural_type().is_boolean() {
                            found = true;
                            break;
                        }
                    }
                    found
                } else {
                    false
                }
            }
            LuaType::String | LuaType::StringConst(_) => match t {
                LuaType::String
                | LuaType::StringConst(_)
                | LuaType::DocStringConst(_)
                | LuaType::StrTplRef(_)
                | LuaType::Language(_) => true,
                LuaType::Ref(id) => self.base_matches_ref(s, id)?,
                LuaType::Def(id) => id.get_name() == "string",
                _ => false,
            },
            LuaType::Integer => match t {
                LuaType::Integer | LuaType::IntegerConst(_) | LuaType::DocIntegerConst(_) => true,
                LuaType::Ref(id) => self.base_matches_ref(s, id)?,
                _ => false,
            },
            LuaType::IntegerConst(a) | LuaType::DocIntegerConst(a) => match t {
                LuaType::IntegerConst(b) | LuaType::DocIntegerConst(b) => {
                    return Ok(relation_from_bool!(
                        self,
                        mode,
                        source.structural_type(),
                        target.structural_type(),
                        a == b
                            || (mode == RelationMode::Assign
                                && matches!(s, LuaType::IntegerConst(_))),
                    ));
                }
                LuaType::Integer | LuaType::Number => true,
                LuaType::Ref(id) => {
                    let result = self.base_matches_ref(s, id)?;
                    // A doc-integer reference mismatch returns directly, while
                    // a runtime integer falls through to union handling.
                    if matches!(s, LuaType::DocIntegerConst(_)) {
                        return Ok(relation_from_bool!(
                            self,
                            mode,
                            source.structural_type(),
                            target.structural_type(),
                            result,
                        ));
                    }
                    result
                }
                _ => false,
            },
            LuaType::Number | LuaType::FloatConst(_) => matches!(
                t,
                LuaType::Number
                    | LuaType::FloatConst(_)
                    | LuaType::Integer
                    | LuaType::IntegerConst(_)
                    | LuaType::DocIntegerConst(_)
            ),
            LuaType::Io => matches!(t, LuaType::Io),
            LuaType::Global => matches!(t, LuaType::Global),
            LuaType::DocStringConst(a) => match t {
                LuaType::StringConst(b) | LuaType::DocStringConst(b) => {
                    return Ok(relation_from_bool!(
                        self,
                        mode,
                        source.structural_type(),
                        target.structural_type(),
                        a == b,
                    ));
                }
                LuaType::String => {
                    return Ok(self.relation_mismatch(
                        mode,
                        source.structural_type(),
                        target.structural_type(),
                    ));
                }
                _ => false,
            },
            LuaType::DocBooleanConst(a) => match t {
                LuaType::BooleanConst(b) | LuaType::DocBooleanConst(b) => {
                    return Ok(relation_from_bool!(
                        self,
                        mode,
                        source.structural_type(),
                        target.structural_type(),
                        a == b,
                    ));
                }
                LuaType::Boolean => {
                    return Ok(self.relation_mismatch(
                        mode,
                        source.structural_type(),
                        target.structural_type(),
                    ));
                }
                _ => false,
            },
            LuaType::StrTplRef(_) => t.is_string(),
            LuaType::Namespace(a) => matches!(t,LuaType::Namespace(b) if a==b),
            LuaType::Language(a) => match t {
                LuaType::Language(b) => a == b,
                LuaType::DocStringConst(_) | LuaType::String | LuaType::StringConst(_) => true,
                _ => false,
            },
            LuaType::Variadic(variadic) => {
                if let VariadicType::Base(base) = variadic.as_ref() {
                    let base = self.owned_child(source, 0, base);
                    match t {
                        LuaType::Variadic(other) => match other.as_ref() {
                            // An unequal Base/Base still reaches the old final
                            // success. Do not "repair" that compatibility rule.
                            VariadicType::Base(_) => {}
                            VariadicType::Multi(types) => {
                                for (index, ty) in types.iter().enumerate() {
                                    let ty = target
                                        .carried_slots()
                                        .map(|slots| slots[index].clone())
                                        .unwrap_or_else(|| OwnedValue::plain(ty.clone()));
                                    let result = self.simple_relation(
                                        &base,
                                        &ty,
                                        mode,
                                        next_relation_level!(depth),
                                    )?;
                                    if result != Relation::Compatible {
                                        return Ok(result);
                                    }
                                }
                            }
                        },
                        _ => {
                            return self.simple_relation(
                                &base,
                                target,
                                mode,
                                next_relation_level!(depth),
                            );
                        }
                    }
                }
                return Ok(Relation::Compatible);
            }
            _ => false,
        };
        if pass {
            return Ok(Relation::Compatible);
        }
        if let Some(members) = target.union_components() {
            for member in members {
                let result =
                    self.simple_relation(source, &member, mode, next_relation_level!(depth))?;
                if result != Relation::Compatible {
                    return Ok(result);
                }
            }
            return Ok(Relation::Compatible);
        }
        Ok(self.relation_mismatch(mode, source.structural_type(), target.structural_type()))
    }

    fn base_matches_ref(
        &mut self,
        source: &LuaType,
        target: &LuaTypeDeclId,
    ) -> QueryResult<bool, E> {
        self.checkpoint()?;
        let alias = self
            .model
            .resolve_type_def(target.get_name())
            .is_some_and(|def| def.kind == TypeDefKind::Alias);
        if !alias
            && matches!(
                target.get_name(),
                "integer" | "number" | "string" | "boolean" | "table" | "function"
            )
            && base_type_name(source) == Some(target.get_name())
        {
            return Ok(true);
        }
        if let Some(base) = base_type_name(source) {
            self.nominal_subtype(target, &LuaTypeDeclId::global(base))
        } else {
            Ok(false)
        }
    }

    pub(super) fn nominal_subtype(
        &mut self,
        sub: &LuaTypeDeclId,
        sup: &LuaTypeDeclId,
    ) -> QueryResult<bool, E> {
        self.checkpoint()?;
        if sub == sup {
            return Ok(true);
        }
        let mut stack = vec![sub.clone()];
        let mut visited = std::collections::HashSet::new();
        visited.insert(sub.clone());
        while let Some(current) = stack.pop() {
            self.checkpoint()?;
            let Some(def) = self.model.resolve_type_def(current.get_name()) else {
                continue;
            };
            for name in &def.super_names {
                self.checkpoint()?;
                let id = LuaTypeDeclId::global(name);
                if &id == sup {
                    return Ok(true);
                }
                if visited.insert(id.clone()) {
                    stack.push(id);
                }
            }
        }
        Ok(false)
    }
}

pub(super) fn base_type_name(ty: &LuaType) -> Option<&'static str> {
    use LuaType::*;
    match ty {
        Integer | IntegerConst(_) | DocIntegerConst(_) => Some("integer"),
        Number | FloatConst(_) => Some("number"),
        Boolean | BooleanConst(_) | DocBooleanConst(_) => Some("boolean"),
        String | StringConst(_) | DocStringConst(_) => Some("string"),
        Table | TableGeneric(_) | TableConst(_) | Tuple(_) | Array(_) | Object(_) => Some("table"),
        Intersection(types) => types.get_types().iter().find_map(base_type_name),
        DocFunction(_) | Function | Signature(_) => Some("function"),
        Thread => Some("thread"),
        Userdata => Some("userdata"),
        Io => Some("io"),
        Global => Some("global"),
        SelfInfer => Some("self"),
        Nil => Some("nil"),
        _ => None,
    }
}
