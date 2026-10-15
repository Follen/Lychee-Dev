//! Fixed nominal/class/enum relation and generic inheritance facts.
use super::{
    OwnedQuery, QueryResult,
    relation::{Relation, RelationMode, next_relation_level},
    relation_simple::base_type_name,
};
use crate::session::{OwnedValue, SelectedProjectionBackend};
use emmylua_code_analysis::{LuaGenericType, LuaType, LuaTypeDeclId, TypeDef, TypeDefKind};
use emmylua_parser::{LuaAstNode, LuaDocTagClass};
use std::collections::HashSet;

impl<E> OwnedQuery<'_, '_, E> {
    pub(super) fn ref_relation(
        &mut self,
        source: &OwnedValue,
        target: &OwnedValue,
        mode: RelationMode,
        depth: usize,
    ) -> QueryResult<Relation, E> {
        self.checkpoint()?;
        let (LuaType::Ref(id) | LuaType::Def(id)) = source.structural_type() else {
            unreachable!("reference relation source");
        };
        let Some(def) = self.model.resolve_type_def(id.get_name()) else {
            return Ok(Relation::Mismatch);
        };
        self.checkpoint()?;
        let t = target.structural_type();
        if def.kind == TypeDefKind::Alias {
            if let Some(alias) = self.model.alias_target(&def) {
                self.checkpoint()?;
                return self.relation_at(
                    &OwnedValue::plain(alias),
                    target,
                    mode,
                    next_relation_level!(depth),
                );
            }
            if let Some(members) = target.union_components() {
                for member in members {
                    let result =
                        self.ref_relation(source, &member, mode, next_relation_level!(depth))?;
                    if result != Relation::Compatible {
                        return Ok(result);
                    }
                }
                return Ok(Relation::Compatible);
            }
            return Ok(Relation::from_bool(
                matches!(t, LuaType::Ref(other) | LuaType::Def(other) if other == id),
            ));
        }
        if def.kind == TypeDefKind::Enum {
            if matches!(t, LuaType::Ref(other) | LuaType::Def(other) if other == id)
                || matches!(
                    t,
                    LuaType::Integer
                        | LuaType::IntegerConst(_)
                        | LuaType::DocIntegerConst(_)
                        | LuaType::Table
                )
            {
                return Ok(Relation::Compatible);
            }
            return Ok(Relation::from_bool(if let Some(base) = base_type_name(t) {
                self.nominal_subtype(id, &LuaTypeDeclId::global(base))?
            } else {
                false
            }));
        }
        match t {
            LuaType::Ref(other) | LuaType::Def(other) => {
                // The fixed general checker deliberately permits both directions.
                // Its enum fallback asks for the base of Ref(id), which is None.
                Ok(Relation::from_bool(
                    id == other
                        || self.nominal_subtype(other, id)?
                        || self.nominal_subtype(id, other)?,
                ))
            }
            LuaType::Table => Ok(Relation::Compatible),
            LuaType::Union(_) => {
                let source = OwnedValue::plain(LuaType::Ref(id.clone()));
                for member in target.union_components().expect("union components") {
                    let result =
                        self.relation_at(&source, &member, mode, next_relation_level!(depth))?;
                    if result != Relation::Compatible {
                        return Ok(result);
                    }
                }
                Ok(Relation::Compatible)
            }
            LuaType::Generic(generic) => {
                let base = generic.get_base_type_id();
                if mode == RelationMode::Strict {
                    Ok(Relation::from_bool(
                        self.relation_subtype_of_generic(id, target)?,
                    ))
                } else {
                    Ok(Relation::from_bool(
                        id == &base
                            || self.nominal_subtype(&base, id)?
                            || self.nominal_subtype(id, &base)?,
                    ))
                }
            }
            LuaType::TableConst(_)
            | LuaType::Object(_)
            | LuaType::Tuple(_)
            | LuaType::Intersection(_) => {
                Ok(Relation::from_bool(if let Some(base) = base_type_name(t) {
                    self.nominal_subtype(id, &LuaTypeDeclId::global(base))?
                } else {
                    false
                }))
            }
            _ => Ok(Relation::from_bool(if let Some(base) = base_type_name(t) {
                let base = LuaTypeDeclId::global(base);
                id == &base
                    || self.nominal_subtype(&base, id)?
                    || self.nominal_subtype(id, &base)?
            } else {
                false
            })),
        }
    }

    pub(super) fn relation_full_super_types(
        &mut self,
        def: &TypeDef,
    ) -> QueryResult<Vec<LuaType>, E> {
        self.checkpoint()?;
        let Some(tree) = self.model.syntax_tree_of(def.file_id) else {
            return Ok(Vec::new());
        };
        let root = tree.get_red_root();
        let Some(token) = root.token_at_offset(def.name_range.start()).right_biased() else {
            return Ok(Vec::new());
        };
        let mut tag = None;
        for ancestor in token.parent_ancestors() {
            self.checkpoint()?;
            if let Some(found) = LuaDocTagClass::cast(ancestor) {
                tag = Some(found);
                break;
            }
        }
        let Some(supers) = tag.and_then(|tag| tag.get_supers()) else {
            return Ok(Vec::new());
        };
        let mut types = Vec::new();
        for super_type in supers.get_types() {
            self.checkpoint()?;
            let ty = self.model.doc_type_lua_in(
                def.file_id,
                super_type.get_syntax_id(),
                &def.generic_params,
            );
            self.checkpoint()?;
            if !matches!(ty, LuaType::Unknown) {
                types.push(ty);
            }
        }
        Ok(types)
    }

    pub(super) fn relation_super_types(
        &mut self,
        id: &LuaTypeDeclId,
    ) -> QueryResult<Vec<LuaType>, E> {
        self.checkpoint()?;
        let Some(def) = self.model.resolve_type_def(id.get_name()) else {
            return Ok(Vec::new());
        };
        let mut types = Vec::new();
        for name in def.super_names {
            self.checkpoint()?;
            types.push(LuaType::Ref(LuaTypeDeclId::global(&name)));
        }
        Ok(types)
    }

    pub(super) fn relation_generic_inheritance(
        &mut self,
        source: &LuaGenericType,
        target: &OwnedValue,
    ) -> QueryResult<bool, E> {
        let LuaType::Generic(target_type) = target.structural_type() else {
            unreachable!("generic target");
        };
        let target_base = target_type.get_base_type_id();
        let source_base = source.get_base_type_id();
        let mut stack = vec![source_base.clone()];
        let mut visited = HashSet::from([source_base]);
        while let Some(current) = stack.pop() {
            self.checkpoint()?;
            let Some(def) = self.model.resolve_type_def(current.get_name()) else {
                continue;
            };
            for parent in self.relation_full_super_types(&def)? {
                self.checkpoint()?;
                match parent {
                    LuaType::Generic(generic) if generic.get_base_type_id() == target_base => {
                        if OwnedValue::plain(LuaType::Generic(generic.clone())) == *target {
                            return Ok(true);
                        }
                    }
                    LuaType::Ref(id) => {
                        if id == target_base && target_type.get_params().is_empty() {
                            return Ok(true);
                        }
                        if visited.insert(id.clone()) {
                            stack.push(id);
                        }
                    }
                    _ => {}
                }
            }
        }
        Ok(false)
    }
    fn relation_subtype_of_generic(
        &mut self,
        source: &LuaTypeDeclId,
        target: &OwnedValue,
    ) -> QueryResult<bool, E> {
        let LuaType::Generic(target_type) = target.structural_type() else {
            unreachable!("generic target");
        };
        let target_base = target_type.get_base_type_id();
        if source == &target_base && target_type.get_params().is_empty() {
            return Ok(true);
        }
        let mut stack = vec![source.clone()];
        let mut visited = HashSet::from([source.clone()]);
        while let Some(current) = stack.pop() {
            self.checkpoint()?;
            let parents = if let Some(def) = self.model.resolve_type_def(current.get_name()) {
                self.relation_full_super_types(&def)?
            } else {
                self.relation_super_types(&current)?
            };
            for parent in parents {
                self.checkpoint()?;
                match parent {
                    LuaType::Generic(generic) if generic.get_base_type_id() == target_base => {
                        if OwnedValue::plain(LuaType::Generic(generic)) == *target {
                            return Ok(true);
                        }
                    }
                    LuaType::Ref(id) => {
                        if id == target_base && target_type.get_params().is_empty() {
                            return Ok(true);
                        }
                        if visited.insert(id.clone()) {
                            stack.push(id);
                        }
                    }
                    _ => {}
                }
            }
        }
        Ok(false)
    }
}
