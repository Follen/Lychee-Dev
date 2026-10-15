//! Logical successors after the original RHS dense-table projection.
use super::{OwnedQuery, QueryResult};
use crate::session::{OwnedValue, SelectedProjectionBackend};
use emmylua_code_analysis::{LuaType, SemanticId, TypeDef, TypeDefKind, TypeScope};
use emmylua_parser::BinaryOperator;

impl<E> OwnedQuery<'_, '_, E> {
    pub(super) fn finish_logical(
        &mut self,
        op: BinaryOperator,
        left: OwnedValue,
        right: OwnedValue,
    ) -> QueryResult<OwnedValue, E> {
        self.checkpoint()?;
        if op == BinaryOperator::OpAnd {
            if left.structural_type().is_always_falsy() {
                return Ok(left);
            }
            if left.structural_type().is_always_truthy() {
                return Ok(right);
            }
            let mut components = left.logical_components(false, || self.checkpoint())?;
            components.push(right);
            return OwnedValue::union_checked(components, || self.checkpoint());
        }
        debug_assert_eq!(op, BinaryOperator::OpOr);
        if left.structural_type().is_always_truthy() {
            return Ok(left);
        }
        if left.structural_type().is_always_falsy() {
            return Ok(right);
        }
        let components = left.logical_components(true, || self.checkpoint())?;
        let target = OwnedValue::union_checked(components, || self.checkpoint())?;
        let right_is_enum_literal =
            matches!(
                right.structural_type(),
                LuaType::Integer
                    | LuaType::Number
                    | LuaType::String
                    | LuaType::IntegerConst(_)
                    | LuaType::DocIntegerConst(_)
                    | LuaType::StringConst(_)
                    | LuaType::DocStringConst(_)
            ) && matches!(target.structural_type(), LuaType::Ref(_) | LuaType::Def(_));
        // Compute coverage first even when the nominal literal shortcut is true:
        // compatibility may emit selected structure receipts.
        if self.fully_covered_by(&right, &target)? || right_is_enum_literal {
            return Ok(target);
        }
        let mut components = left.logical_components(true, || self.checkpoint())?;
        components.push(right);
        OwnedValue::union_checked(components, || self.checkpoint())
    }

    fn fully_covered_by(
        &mut self,
        value: &OwnedValue,
        target: &OwnedValue,
    ) -> QueryResult<bool, E> {
        self.checkpoint()?;
        if let LuaType::TableConst(table) = value.structural_type() {
            let empty = self
                .model
                .members_of_owner(&SemanticId::member(table.file_id, table.value))
                .is_empty();
            self.checkpoint()?;
            if empty {
                return self.empty_table_covered_by(target);
            }
        }
        let literal = matches!(
            target.structural_type(),
            LuaType::BooleanConst(_)
                | LuaType::DocBooleanConst(_)
                | LuaType::StringConst(_)
                | LuaType::DocStringConst(_)
                | LuaType::IntegerConst(_)
                | LuaType::DocIntegerConst(_)
        );
        if let Some(members) = value.union_components() {
            for member in members {
                self.checkpoint()?;
                if if literal {
                    member != *target
                } else {
                    !self.type_check(&member, target)?
                } {
                    return Ok(false);
                }
            }
            Ok(true)
        } else if literal {
            Ok(value == target)
        } else {
            self.type_check(value, target)
        }
    }

    pub(super) fn empty_table_covered_by(&mut self, target: &OwnedValue) -> QueryResult<bool, E> {
        // A frontier instead of Rust-stack recursion keeps the existing caller
        // cancellation effective even for cyclic alias declarations. It does not
        // turn a cycle into a successful semantic result.
        let mut pending = vec![target.clone()];
        while let Some(value) = pending.pop() {
            self.checkpoint()?;
            if let Some(members) = value.union_components() {
                pending.extend(members.into_iter().rev());
                continue;
            }
            match value.structural_type() {
                LuaType::Ref(id) | LuaType::Def(id) => {
                    let Some(def) = self.model.type_def_of(id) else {
                        return Ok(false);
                    };
                    match def.kind {
                        TypeDefKind::Alias => {
                            let Some(alias) = self.model.alias_target(&def) else {
                                return Ok(false);
                            };
                            self.checkpoint()?;
                            pending.push(OwnedValue::plain(alias));
                        }
                        TypeDefKind::Class => {
                            if self.class_has_required_field(def)? {
                                return Ok(false);
                            }
                        }
                        TypeDefKind::Enum => return Ok(false),
                    }
                }
                LuaType::Object(object) => {
                    for field in object.get_fields().values() {
                        self.checkpoint()?;
                        if !field.is_nullable() {
                            return Ok(false);
                        }
                    }
                }
                LuaType::Table
                | LuaType::TableGeneric(_)
                | LuaType::Array(_)
                | LuaType::Tuple(_)
                | LuaType::Generic(_)
                | LuaType::Unknown
                | LuaType::Any => {}
                _ => return Ok(false),
            }
        }
        self.checkpoint()?;
        Ok(true)
    }

    fn class_has_required_field(&mut self, def: TypeDef) -> QueryResult<bool, E> {
        // Frames preserve the recursive producer's member-before-parent and
        // parent declaration order, without retaining a second database.
        let mut pending = vec![def];
        let mut visited = std::collections::HashSet::new();
        while let Some(def) = pending.pop() {
            self.checkpoint()?;
            if !visited.insert(def.id.clone()) {
                continue;
            }
            for member_ref in self.model.members_of_owner(&def.id).iter() {
                self.checkpoint()?;
                let Some(facts) = self.model.file_facts_of(member_ref.file_id) else {
                    continue;
                };
                let Some(member) = facts.member_by_id(&member_ref.id) else {
                    continue;
                };
                if !member.is_index_signature && !member.is_nullable {
                    return Ok(true);
                }
            }
            // Resolution is side-effect-free public fact lookup; push reverse
            // so the actual visited inheritance walk stays first-parent-first.
            for name in def.super_names.iter().rev() {
                self.checkpoint()?;
                if let Some(parent) = self
                    .model
                    .resolve_type_def_in(def.file_id, name.as_str())
                    .or_else(|| {
                        self.model
                            .type_defs_in_scope(TypeScope::Global, name.as_str())
                            .iter()
                            .next()
                            .cloned()
                    })
                {
                    pending.push(parent);
                }
            }
        }
        self.checkpoint()?;
        Ok(false)
    }
}
