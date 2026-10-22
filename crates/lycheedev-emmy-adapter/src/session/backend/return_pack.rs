//! Lua return-list adjustment shared by signature and diagnostic consumers.
use super::{OwnedQuery, QueryResult};
use crate::session::{OwnedValue, SelectedProjectionBackend};
use emmylua_code_analysis::{LuaType, VariadicType};
use emmylua_parser::{LuaExpr, LuaLiteralToken};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct ReturnArity {
    pub min: usize,
    pub max: Option<usize>,
}

impl ReturnArity {
    pub fn fixed(count: usize) -> Self {
        Self {
            min: count,
            max: Some(count),
        }
    }
}

pub(super) fn return_expands(expression: &LuaExpr, last: bool) -> bool {
    last && (matches!(expression, LuaExpr::CallExpr(_))
        || matches!(expression, LuaExpr::LiteralExpr(literal)
            if matches!(literal.get_literal(), Some(LuaLiteralToken::Dots(_)))))
}

/// Shape of an expanded expression. None means no normal return (Never),
/// whereas Unknown/Any retain an open, unavailable sequence.
pub(super) fn return_arity(ty: &LuaType) -> Option<ReturnArity> {
    match ty {
        LuaType::Never => None,
        LuaType::Unknown | LuaType::Any => Some(ReturnArity { min: 0, max: None }),
        LuaType::Variadic(values) => match values.as_ref() {
            VariadicType::Base(LuaType::Tuple(tuple)) => {
                Some(ReturnArity::fixed(tuple.get_types().len()))
            }
            VariadicType::Base(_) => Some(ReturnArity { min: 0, max: None }),
            VariadicType::Multi(types) => {
                let Some((last, prefix)) = types.split_last() else {
                    return Some(ReturnArity::fixed(0));
                };
                if prefix.iter().any(|ty| matches!(ty, LuaType::Never)) {
                    return None;
                }
                let tail = if last.contain_multi_return() || matches!(last, LuaType::Never) {
                    return_arity(last)?
                } else {
                    ReturnArity::fixed(1)
                };
                Some(ReturnArity {
                    min: prefix.len() + tail.min,
                    max: tail.max.map(|n| prefix.len() + n),
                })
            }
        },
        LuaType::Union(union) if ty.contain_multi_return() => {
            let ranges = union
                .into_vec()
                .iter()
                .filter_map(|member| {
                    if member.contain_multi_return() || matches!(member, LuaType::Never) {
                        return_arity(member)
                    } else {
                        Some(ReturnArity::fixed(1))
                    }
                })
                .collect::<Vec<_>>();
            let min = ranges.iter().map(|range| range.min).min()?;
            let max = ranges
                .iter()
                .map(|range| range.max)
                .collect::<Option<Vec<_>>>()
                .map(|maxima| maxima.into_iter().max().unwrap_or(0));
            Some(ReturnArity { min, max })
        }
        _ => Some(ReturnArity::fixed(1)),
    }
}

/// Last boundary before an open tail; finite branches also contribute their
/// extent so a union compares every known prefix before a tail representative.
pub(super) fn return_prefix_extent(ty: &LuaType) -> usize {
    match ty {
        LuaType::Unknown | LuaType::Any | LuaType::Never => 0,
        LuaType::Variadic(values) => match values.as_ref() {
            VariadicType::Base(LuaType::Tuple(tuple)) => tuple.get_types().len(),
            VariadicType::Base(_) => 0,
            VariadicType::Multi(types) => types.split_last().map_or(0, |(last, prefix)| {
                prefix.len()
                    + if last.contain_multi_return() {
                        return_prefix_extent(last)
                    } else {
                        1
                    }
            }),
        },
        LuaType::Union(union) if ty.contain_multi_return() => union
            .into_vec()
            .iter()
            .map(|member| {
                if member.contain_multi_return() {
                    return_prefix_extent(member)
                } else {
                    1
                }
            })
            .max()
            .unwrap_or(0),
        _ => 1,
    }
}

pub(super) fn return_has_typed_open_tail(ty: &LuaType) -> bool {
    match ty {
        LuaType::Variadic(values) => match values.as_ref() {
            VariadicType::Base(
                LuaType::Tuple(_) | LuaType::Unknown | LuaType::Any | LuaType::Never,
            ) => false,
            VariadicType::Base(_) => true,
            VariadicType::Multi(types) => types.last().is_some_and(return_has_typed_open_tail),
        },
        LuaType::Union(union) if ty.contain_multi_return() => {
            union.into_vec().iter().any(return_has_typed_open_tail)
        }
        _ => false,
    }
}

impl<E> OwnedQuery<'_, '_, E> {
    pub(super) fn owned_return_slot(
        &mut self,
        value: &OwnedValue,
        index: usize,
    ) -> QueryResult<Option<OwnedValue>, E> {
        self.checkpoint()?;
        match value.structural_type() {
            LuaType::Never => Ok(None),
            LuaType::Unknown | LuaType::Any => Ok(Some(value.clone())),
            LuaType::Variadic(values) => match values.as_ref() {
                VariadicType::Multi(types) => {
                    let Some(last) = types.len().checked_sub(1) else {
                        return Ok(None);
                    };
                    let child = index.min(last);
                    let value = self.owned_child(value, child, &types[child]);
                    if index < last {
                        self.owned_return_scalar(&value).map(Some)
                    } else if types[last].contain_multi_return() {
                        self.owned_return_slot(&value, index - last)
                    } else {
                        Ok(
                            (index == last && !matches!(value.structural_type(), LuaType::Never))
                                .then_some(value),
                        )
                    }
                }
                VariadicType::Base(base) => {
                    let child = self.owned_child(value, 0, base);
                    if let LuaType::Tuple(tuple) = base {
                        Ok(tuple
                            .get_types()
                            .get(index)
                            .map(|ty| self.owned_child(&child, index, ty)))
                    } else if matches!(base, LuaType::Never) {
                        Ok(None)
                    } else {
                        Ok(Some(child))
                    }
                }
            },
            LuaType::Union(union) if value.structural_type().contain_multi_return() => {
                let mut slots = Vec::new();
                let mut present = false;
                for (member, ty) in union.into_vec().iter().enumerate() {
                    if matches!(ty, LuaType::Never) {
                        continue;
                    }
                    let child = self.owned_child(value, member, ty);
                    let slot = if ty.contain_multi_return() {
                        self.owned_return_slot(&child, index)?
                    } else {
                        (index == 0).then_some(child)
                    };
                    present |= slot.is_some();
                    slots.push(slot.unwrap_or_else(|| OwnedValue::plain(LuaType::Nil)));
                }
                if present {
                    Ok(Some(OwnedValue::union_checked(slots, || {
                        self.checkpoint()
                    })?))
                } else {
                    Ok(None)
                }
            }
            _ => Ok((index == 0).then(|| value.clone())),
        }
    }

    pub(super) fn owned_return_scalar(&mut self, value: &OwnedValue) -> QueryResult<OwnedValue, E> {
        if matches!(value.structural_type(), LuaType::Never) {
            return Ok(value.clone());
        }
        self.owned_return_slot(value, 0)
            .map(|slot| slot.unwrap_or_else(|| OwnedValue::plain(LuaType::Nil)))
    }
}
