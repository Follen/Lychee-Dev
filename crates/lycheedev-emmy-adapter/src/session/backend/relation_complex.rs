//! Fixed general structural compatibility. Selected tuple members are compared
//! as carriers, while shallow object compatibility stays intentionally shallow.
use super::{
    OwnedQuery, QueryResult,
    relation::{Relation, RelationMode, next_relation_level},
};
use crate::session::{OwnedValue, SelectedProjectionBackend};
use emmylua_code_analysis::{LuaMemberKey, LuaType};

impl<E> OwnedQuery<'_, '_, E> {
    pub(super) fn complex_relation(
        &mut self,
        source: &OwnedValue,
        target: &OwnedValue,
        mode: RelationMode,
        depth: usize,
    ) -> QueryResult<Relation, E> {
        self.checkpoint()?;
        let t = target.structural_type();
        match source.structural_type() {
            LuaType::Array(array) => {
                let base = self.owned_child(source, 0, array.get_base());
                match t {
                    LuaType::Array(other) => self.relation_at(
                        &base,
                        &self.owned_child(target, 0, other.get_base()),
                        mode,
                        next_relation_level!(depth),
                    ),
                    LuaType::Object(object) if mode == RelationMode::Assign => {
                        for (index, (key, expected)) in object.get_index_access().iter().enumerate()
                        {
                            self.checkpoint()?;
                            if matches!(
                                key,
                                LuaType::Integer | LuaType::Number | LuaType::IntegerConst(_)
                            ) {
                                if matches!(expected, LuaType::Any | LuaType::Unknown) {
                                    return Ok(Relation::Compatible);
                                }
                                let next = next_relation_level!(depth);
                                if self.relation_at(
                                    &base,
                                    &self.owned_index(target, index, false, expected),
                                    mode,
                                    next,
                                )? == Relation::Compatible
                                {
                                    return Ok(Relation::Compatible);
                                }
                            }
                        }
                        Ok(Relation::Mismatch)
                    }
                    LuaType::Tuple(tuple) => {
                        let next = next_relation_level!(depth);
                        for (index, ty) in tuple.get_types().iter().enumerate() {
                            let element = target
                                .carried_slots()
                                .map(|slots| slots[index].clone())
                                .unwrap_or_else(|| OwnedValue::plain(ty.clone()));
                            let result = self.relation_at(&base, &element, mode, next)?;
                            if result != Relation::Compatible {
                                return Ok(result);
                            }
                        }
                        Ok(Relation::Compatible)
                    }
                    LuaType::Table
                    | LuaType::TableGeneric(_)
                    | LuaType::Ref(_)
                    | LuaType::Def(_)
                    | LuaType::Generic(_) => Ok(Relation::Compatible),
                    _ => Ok(Relation::Mismatch),
                }
            }
            LuaType::TableGeneric(types) => match t {
                LuaType::TableGeneric(others) => {
                    if types.len() != others.len() {
                        return Ok(Relation::Mismatch);
                    }
                    let next = next_relation_level!(depth);
                    for (index, (a, b)) in types.iter().zip(others.iter()).enumerate() {
                        let result = self.relation_at(
                            &self.owned_child(source, index, a),
                            &self.owned_child(target, index, b),
                            mode,
                            next,
                        )?;
                        if result != Relation::Compatible {
                            return Ok(result);
                        }
                    }
                    Ok(Relation::Compatible)
                }
                LuaType::Table
                | LuaType::Array(_)
                | LuaType::Tuple(_)
                | LuaType::Object(_)
                | LuaType::Ref(_)
                | LuaType::Def(_) => Ok(Relation::Compatible),
                _ => Ok(Relation::Mismatch),
            },
            LuaType::Tuple(tuple) => match t {
                LuaType::Tuple(other) => {
                    if tuple.get_types().len() != other.get_types().len() {
                        return Ok(Relation::Mismatch);
                    }
                    let next = next_relation_level!(depth);
                    for (index, (a, b)) in
                        tuple.get_types().iter().zip(other.get_types()).enumerate()
                    {
                        let a = source
                            .carried_slots()
                            .map(|slots| slots[index].clone())
                            .unwrap_or_else(|| OwnedValue::plain(a.clone()));
                        let b = target
                            .carried_slots()
                            .map(|slots| slots[index].clone())
                            .unwrap_or_else(|| OwnedValue::plain(b.clone()));
                        let result = self.relation_at(&a, &b, mode, next)?;
                        if result != Relation::Compatible {
                            return Ok(result);
                        }
                    }
                    Ok(Relation::Compatible)
                }
                LuaType::Table
                | LuaType::Array(_)
                | LuaType::TableGeneric(_)
                | LuaType::Ref(_)
                | LuaType::Def(_) => Ok(Relation::Compatible),
                _ => Ok(Relation::Mismatch),
            },
            LuaType::Object(object) if mode == RelationMode::Strict => match t {
                LuaType::Object(other) => {
                    let next = next_relation_level!(depth);
                    for (key, expected) in object.get_fields() {
                        if let Some(actual) = other.get_field(key) {
                            let result = self.relation_at(
                                &self.owned_field(source, key, expected),
                                &self.owned_field(target, key, &actual),
                                mode,
                                next,
                            )?;
                            if result != Relation::Compatible {
                                return Ok(result);
                            }
                        }
                    }
                    Ok(Relation::Compatible)
                }
                LuaType::Tuple(tuple) => {
                    let next = next_relation_level!(depth);
                    for (index, actual) in tuple.get_types().iter().enumerate() {
                        let key = LuaMemberKey::Integer(index as i64 + 1);
                        let Some(expected) = object.get_fields().get(&key) else {
                            return Ok(Relation::Mismatch);
                        };
                        let result = self.relation_at(
                            &self.owned_field(source, &key, expected),
                            &self.owned_child(target, index, actual),
                            mode,
                            next,
                        )?;
                        if result != Relation::Compatible {
                            return Ok(result);
                        }
                    }
                    Ok(Relation::Compatible)
                }
                LuaType::TableConst(_) => {
                    let infos = self.member_infos(target)?;
                    let next = next_relation_level!(depth);
                    for (key, expected) in object.get_fields() {
                        if let Some(actual) = infos.iter().find(|info| &info.key == key) {
                            let result = self.relation_at(
                                &self.owned_field(source, key, expected),
                                &actual.typ,
                                mode,
                                next,
                            )?;
                            if result != Relation::Compatible {
                                return Ok(result);
                            }
                        }
                    }
                    Ok(Relation::Compatible)
                }
                LuaType::Table | LuaType::Ref(_) | LuaType::Def(_) | LuaType::Generic(_) => {
                    Ok(Relation::Compatible)
                }
                _ => Ok(Relation::Mismatch),
            },
            LuaType::Object(_) => Ok(Relation::from_bool(matches!(
                t,
                LuaType::Object(_)
                    | LuaType::Table
                    | LuaType::Ref(_)
                    | LuaType::Def(_)
                    | LuaType::Generic(_)
            ))),
            LuaType::Call(_) => Ok(Relation::from_bool(
                matches!(t, LuaType::Call(_)) && source == target,
            )),
            LuaType::MultiLineUnion(multi) => {
                let mut members = Vec::new();
                for (index, (ty, _)) in multi.get_unions().iter().enumerate() {
                    members.push(self.owned_child(source, index, ty));
                }
                let union = OwnedValue::raw_union(members);
                self.relation_at(&union, target, mode, next_relation_level!(depth))
            }
            _ => Ok(Relation::Mismatch),
        }
    }
}
