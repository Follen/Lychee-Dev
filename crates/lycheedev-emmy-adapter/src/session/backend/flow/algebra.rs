//! The fixed flow algebra, retaining selected identities in every component.
use super::{OwnedQuery, OwnedValue, QueryResult};
use crate::session::SelectedProjectionBackend;
use emmylua_code_analysis::{LuaType, VariadicType};
use std::sync::Arc;
pub(super) fn components(value: OwnedValue) -> Vec<OwnedValue> {
    value.union_components().unwrap_or_else(|| vec![value])
}
pub(super) fn from_components(mut values: Vec<OwnedValue>) -> OwnedValue {
    match values.len() {
        0 => OwnedValue::plain(LuaType::Unknown),
        1 => values.pop().unwrap(),
        _ => OwnedValue::raw_union(values),
    }
}

fn broader(broad: &OwnedValue, narrow: &OwnedValue) -> bool {
    matches!(
        (broad.structural_type(), narrow.structural_type()),
        (
            LuaType::String,
            LuaType::StringConst(_) | LuaType::DocStringConst(_)
        ) | (
            LuaType::Integer,
            LuaType::IntegerConst(_) | LuaType::DocIntegerConst(_) | LuaType::Number
        ) | (
            LuaType::Number,
            LuaType::Integer
                | LuaType::IntegerConst(_)
                | LuaType::DocIntegerConst(_)
                | LuaType::FloatConst(_)
        ) | (
            LuaType::Boolean,
            LuaType::BooleanConst(_) | LuaType::DocBooleanConst(_)
        ) | (
            LuaType::Table,
            LuaType::TableConst(_) | LuaType::Array(_) | LuaType::Object(_) | LuaType::Tuple(_)
        ) | (
            LuaType::Function,
            LuaType::DocFunction(_) | LuaType::Signature(_)
        )
    )
}
fn removed_by(component: &OwnedValue, removed: &OwnedValue) -> bool {
    component == removed
        || matches!(
            (component.structural_type(), removed.structural_type()),
            (
                LuaType::Integer | LuaType::IntegerConst(_) | LuaType::DocIntegerConst(_),
                LuaType::Number
            ) | (
                LuaType::IntegerConst(_) | LuaType::DocIntegerConst(_),
                LuaType::Integer
            ) | (
                LuaType::StringConst(_) | LuaType::DocStringConst(_),
                LuaType::String
            ) | (
                LuaType::BooleanConst(_) | LuaType::DocBooleanConst(_),
                LuaType::Boolean
            )
        )
}
impl<E> OwnedQuery<'_, '_, E> {
    pub(in crate::session::backend) fn widen_flow_const(
        &mut self,
        value: OwnedValue,
    ) -> QueryResult<OwnedValue, E> {
        self.checkpoint()?;
        Ok(match value.structural_type() {
            LuaType::IntegerConst(_) | LuaType::DocIntegerConst(_) => {
                OwnedValue::plain(LuaType::Integer)
            }
            LuaType::FloatConst(_) => OwnedValue::plain(LuaType::Number),
            LuaType::StringConst(_) | LuaType::DocStringConst(_) => {
                OwnedValue::plain(LuaType::String)
            }
            LuaType::Union(_) => {
                let mut values = Vec::new();
                for member in components(value) {
                    for widened in components(self.widen_flow_const(member)?) {
                        self.checkpoint()?;
                        if !values.contains(&widened) {
                            values.push(widened);
                        }
                    }
                }
                from_components(values)
            }
            LuaType::Variadic(variadic) => match variadic.as_ref() {
                VariadicType::Base(base) => {
                    let base = self.owned_child(&value, 0, base);
                    let base = self.widen_flow_const(base)?;
                    OwnedValue::with_structural_children(
                        LuaType::Variadic(Arc::new(VariadicType::Base(
                            base.structural_type().clone(),
                        ))),
                        vec![base],
                    )
                }
                VariadicType::Multi(types) => {
                    let slots = value
                        .carried_slots()
                        .map(|s| s.to_vec())
                        .unwrap_or_else(|| types.iter().cloned().map(OwnedValue::plain).collect());
                    let mut values = Vec::new();
                    for slot in slots {
                        values.push(self.widen_flow_const(slot)?);
                    }
                    OwnedValue::pack(values)
                }
            },
            _ => value,
        })
    }

    pub(super) fn flow_merge(
        &mut self,
        left: OwnedValue,
        right: OwnedValue,
    ) -> QueryResult<OwnedValue, E> {
        self.checkpoint()?;
        if left == right {
            return Ok(left);
        }
        if left.structural_type().is_never() {
            return Ok(right);
        }
        if right.structural_type().is_never() {
            return Ok(left);
        }
        let mut values = Vec::new();
        for value in [left, right] {
            if value.structural_type().is_unknown() {
                continue;
            }
            for candidate in components(value) {
                self.checkpoint()?;
                if values.iter().any(|existing| broader(existing, &candidate)) {
                    continue;
                }
                values.retain(|existing| !broader(&candidate, existing));
                values.push(candidate);
            }
        }
        Ok(from_components(values))
    }
    pub(super) fn flow_intersect(
        &mut self,
        left: OwnedValue,
        right: OwnedValue,
    ) -> QueryResult<OwnedValue, E> {
        self.checkpoint()?;
        if left == right {
            return Ok(left);
        }
        if left.structural_type().is_unknown() {
            return Ok(right);
        }
        if right.structural_type().is_unknown() {
            return Ok(from_components(components(left)));
        }
        let right = components(right);
        let mut common = Vec::new();
        for value in components(left) {
            self.checkpoint()?;
            if right.contains(&value) {
                common.push(value);
            }
        }
        Ok(from_components(common))
    }
    pub(super) fn flow_remove(
        &mut self,
        value: OwnedValue,
        removed: &OwnedValue,
    ) -> QueryResult<OwnedValue, E> {
        let removed = components(removed.clone());
        let mut out = Vec::new();
        for value in components(value) {
            self.checkpoint()?;
            if !removed.iter().any(|r| removed_by(&value, r)) {
                out.push(value);
            }
        }
        Ok(from_components(out))
    }
    pub(super) fn flow_truthy(
        &mut self,
        value: OwnedValue,
        truthy: bool,
    ) -> QueryResult<OwnedValue, E> {
        let mut out = Vec::new();
        for value in components(value) {
            self.checkpoint()?;
            let keep = if truthy {
                !matches!(
                    value.structural_type(),
                    LuaType::Nil
                        | LuaType::Boolean
                        | LuaType::BooleanConst(false)
                        | LuaType::DocBooleanConst(false)
                )
            } else {
                matches!(
                    value.structural_type(),
                    LuaType::Nil | LuaType::BooleanConst(false) | LuaType::DocBooleanConst(false)
                )
            };
            if keep {
                out.push(value);
            }
        }
        Ok(from_components(out))
    }
    pub(super) fn widen_for_target(
        &mut self,
        value: OwnedValue,
        target: &OwnedValue,
    ) -> QueryResult<OwnedValue, E> {
        if matches!(target.structural_type(), LuaType::Unknown | LuaType::Any) {
            return Ok(value);
        }
        let target = components(target.clone());
        let non_nil: Vec<_> = target
            .iter()
            .filter(|v| !v.structural_type().is_nil())
            .cloned()
            .collect();
        if non_nil.is_empty()
            || !non_nil.iter().all(|v| {
                matches!(
                    v.structural_type(),
                    LuaType::Integer | LuaType::Number | LuaType::String | LuaType::Boolean
                )
            })
        {
            return Ok(value);
        }
        if target.iter().any(|v| v.structural_type().is_nil())
            && !self.type_check(&value, &from_components(non_nil))?
        {
            return Ok(value);
        }
        self.checkpoint()?;
        self.widen_flow_const(value)
    }
}
