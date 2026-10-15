//! Selected-identity-aware general compatibility and its per-model result cache.
//! Dispatch mirrors the fixed engine, including the deliberately different
//! recursive producers: only `relation_at` re-enters the structure gate.
use super::{OwnedQuery, QueryResult};
use crate::session::{OwnedValue, SelectedProjectionBackend};
use emmylua_code_analysis::{LuaType, LuaUnionType, TypeDefKind};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum RelationMode {
    General,
    Assign,
    Strict,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Relation {
    Compatible,
    Mismatch,
    Recursion,
}
impl Relation {
    pub(super) fn from_bool(value: bool) -> Self {
        if value {
            Self::Compatible
        } else {
            Self::Mismatch
        }
    }
}

// The fixed guard fails at the caller, before entering the recursive producer.
// In particular an `is_ok` alternative may swallow the child's Recursion, but
// may not swallow its own next_level failure. Keep the macro at each call site.
macro_rules! next_relation_level {
    ($depth:expr) => {{
        if $depth >= 100 {
            return Ok(Relation::Recursion);
        }
        $depth + 1
    }};
}
pub(super) use next_relation_level;

impl<E> OwnedQuery<'_, '_, E> {
    /// Full OwnedValue identity is the key. A hit checks caller control without
    /// replaying structure receipts, exactly as the original local result cache.
    pub fn type_check(&mut self, source: &OwnedValue, target: &OwnedValue) -> QueryResult<bool, E> {
        self.checkpoint()?;
        if let Some(result) = self
            .type_relations
            .get(&(source.clone(), target.clone()))
            .copied()
        {
            self.checkpoint()?;
            return Ok(result);
        }
        let result =
            self.relation_at(source, target, RelationMode::General, 0)? == Relation::Compatible;
        self.checkpoint()?;
        self.type_relations
            .insert((source.clone(), target.clone()), result);
        Ok(result)
    }

    pub fn type_check_assign(
        &mut self,
        source: &OwnedValue,
        target: &OwnedValue,
    ) -> QueryResult<bool, E> {
        self.relation_at(source, target, RelationMode::Assign, 0)
            .map(|result| result == Relation::Compatible)
    }

    pub fn type_check_subtype(
        &mut self,
        source: &OwnedValue,
        target: &OwnedValue,
    ) -> QueryResult<bool, E> {
        self.relation_at(source, target, RelationMode::Strict, 0)
            .map(|result| result == Relation::Compatible)
    }

    pub(super) fn relation_at(
        &mut self,
        source: &OwnedValue,
        target: &OwnedValue,
        mode: RelationMode,
        depth: usize,
    ) -> QueryResult<Relation, E> {
        self.checkpoint()?;
        if !self.structure_allowed(source)? || !self.structure_allowed(target)? {
            return Ok(Relation::Mismatch);
        }
        if source == target {
            return Ok(Relation::Compatible);
        }
        let s = source.structural_type();
        let t = target.structural_type();
        if matches!(t, LuaType::Any | LuaType::Unknown)
            || matches!(t, LuaType::TplRef(p) if p.get_constraint().is_none())
        {
            return Ok(Relation::Compatible);
        }
        if matches!(t, LuaType::SelfInfer) {
            return Ok(Relation::from_bool(matches!(
                s,
                LuaType::SelfInfer
                    | LuaType::Ref(_)
                    | LuaType::Def(_)
                    | LuaType::Generic(_)
                    | LuaType::Table
                    | LuaType::TableConst(_)
                    | LuaType::TableGeneric(_)
                    | LuaType::Object(_)
                    | LuaType::Intersection(_)
            )));
        }
        if let LuaType::Ref(id) | LuaType::Def(id) = t {
            self.checkpoint()?;
            if let Some(def) = self.model.resolve_type_def(id.get_name())
                && def.kind == TypeDefKind::Alias
                && let Some(alias) = self.model.alias_target(&def)
            {
                self.checkpoint()?;
                return self.relation_at(
                    source,
                    &OwnedValue::plain(alias),
                    mode,
                    next_relation_level!(depth),
                );
            }
        }
        if let LuaType::Generic(generic) = s {
            self.checkpoint()?;
            if let Some(def) = self
                .model
                .resolve_type_def(generic.get_base_type_id().get_name())
                && def.kind == TypeDefKind::Alias
                && let Some(alias) = self.model.alias_target(&def)
                && !matches!(
                    alias,
                    LuaType::Mapped(_) | LuaType::Call(_) | LuaType::Conditional(_)
                )
            {
                // This public API is exactly expand_alias_generic, the original
                // producer. Generic is a context-free carrier variant.
                self.checkpoint()?;
                let expanded = self.expand_aliases(source.clone())?;
                self.checkpoint()?;
                if expanded != *source {
                    return self.relation_at(&expanded, target, mode, next_relation_level!(depth));
                }
            }
        }
        // All other fast equality cases were handled by full owned equality.
        if let (LuaType::Union(union), LuaType::Ref(id)) = (s, t)
            && matches!(union.as_ref(), LuaUnionType::Nullable(LuaType::Ref(left)) if left == id)
        {
            return Ok(Relation::Compatible);
        }
        if mode == RelationMode::Assign {
            if let Some(result) = self.assign_relation_prefix(source, target, depth)? {
                return Ok(result);
            }
        }
        // Despite its upstream comment, general target intersections use ANY.
        if let LuaType::Intersection(targets) = t
            && !matches!(s, LuaType::Intersection(_))
        {
            for (index, ty) in targets.get_types().iter().enumerate() {
                let next = next_relation_level!(depth);
                if self.relation_at(source, &self.owned_child(target, index, ty), mode, next)?
                    == Relation::Compatible
                {
                    return Ok(Relation::Compatible);
                }
            }
            return Ok(Relation::Mismatch);
        }
        match s {
            LuaType::Unknown | LuaType::Any | LuaType::Never | LuaType::ModuleRef(_) => {
                Ok(Relation::Compatible)
            }
            LuaType::TplRef(tpl) => {
                if let Some(constraint) = tpl.get_constraint() {
                    self.relation_at(
                        &OwnedValue::plain(constraint.clone()),
                        target,
                        mode,
                        next_relation_level!(depth),
                    )
                } else {
                    self.simple_relation(source, target, mode, depth)
                }
            }
            LuaType::Nil
            | LuaType::Table
            | LuaType::Userdata
            | LuaType::Function
            | LuaType::Thread
            | LuaType::Boolean
            | LuaType::String
            | LuaType::Integer
            | LuaType::Number
            | LuaType::Io
            | LuaType::Global
            | LuaType::BooleanConst(_)
            | LuaType::StringConst(_)
            | LuaType::IntegerConst(_)
            | LuaType::FloatConst(_)
            | LuaType::TableConst(_)
            | LuaType::DocStringConst(_)
            | LuaType::DocIntegerConst(_)
            | LuaType::DocBooleanConst(_)
            | LuaType::StrTplRef(_)
            | LuaType::Namespace(_)
            | LuaType::Variadic(_)
            | LuaType::Language(_) => self.simple_relation(source, target, mode, depth),
            LuaType::Ref(_) | LuaType::Def(_) => self.ref_relation(source, target, mode, depth),
            LuaType::DocFunction(_) => self.function_relation(source, target, mode, depth),
            LuaType::Signature(id) => {
                let Some(signature) = self.signature_by_id(id)? else {
                    return Ok(Relation::Mismatch);
                };
                self.function_relation(&signature, target, mode, next_relation_level!(depth))
            }
            LuaType::Union(_) => {
                if mode == RelationMode::Assign {
                    for member in source.union_components().expect("union components") {
                        let result =
                            self.relation_at(&member, target, mode, next_relation_level!(depth))?;
                        if result != Relation::Compatible {
                            return Ok(result);
                        }
                    }
                    return Ok(Relation::Compatible);
                }
                if mode == RelationMode::Strict && matches!(t, LuaType::Union(_)) {
                    let next = next_relation_level!(depth);
                    for member in target.union_components().expect("union components") {
                        let result = self.relation_at(source, &member, mode, next)?;
                        if result != Relation::Compatible {
                            return Ok(result);
                        }
                    }
                    return Ok(Relation::Compatible);
                }
                for member in source.union_components().expect("union components") {
                    let next = next_relation_level!(depth);
                    match self.relation_at(&member, target, mode, next)? {
                        Relation::Compatible => return Ok(Relation::Compatible),
                        Relation::Recursion => return Ok(Relation::Recursion),
                        Relation::Mismatch => {}
                    }
                }
                Ok(Relation::Mismatch)
            }
            LuaType::Intersection(members) => {
                for (index, member) in members.get_types().iter().enumerate() {
                    let next = next_relation_level!(depth);
                    if self.relation_at(
                        &self.owned_child(source, index, member),
                        target,
                        mode,
                        next,
                    )? == Relation::Compatible
                    {
                        return Ok(Relation::Compatible);
                    }
                }
                Ok(Relation::Mismatch)
            }
            LuaType::Array(_)
            | LuaType::Tuple(_)
            | LuaType::Object(_)
            | LuaType::TableGeneric(_)
            | LuaType::Call(_)
            | LuaType::MultiLineUnion(_) => self.complex_relation(source, target, mode, depth),
            LuaType::Generic(_) => self.generic_relation(source, target, mode, depth),
            LuaType::Instance(instance) => self.relation_at(
                &self.owned_child(source, 0, instance.get_base()),
                target,
                mode,
                next_relation_level!(depth),
            ),
            LuaType::TypeGuard(_) => Ok(Relation::from_bool(t.is_boolean())),
            _ => Ok(Relation::Mismatch),
        }
    }
}
