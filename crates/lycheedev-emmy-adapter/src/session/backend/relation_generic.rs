//! Fixed general generic relation, including direct-vs-general recursion.
use super::{
    OwnedQuery, QueryResult,
    relation::{Relation, RelationMode, next_relation_level, relation_from_bool},
};
use crate::session::{OwnedValue, SelectedProjectionBackend};
use emmylua_code_analysis::{LuaGenericType, LuaType, LuaTypeDeclId};

impl<E> OwnedQuery<'_, '_, E> {
    pub(super) fn generic_relation(
        &mut self,
        source: &OwnedValue,
        target: &OwnedValue,
        mode: RelationMode,
        depth: usize,
    ) -> QueryResult<Relation, E> {
        self.checkpoint()?;
        let LuaType::Generic(generic) = source.structural_type() else {
            unreachable!("generic relation source");
        };
        match target.structural_type() {
            LuaType::Generic(other) => {
                if generic.contain_tpl() {
                    return Ok(Relation::Compatible);
                }
                let first = self.generic_parameters_relation(
                    source,
                    target,
                    mode,
                    next_relation_level!(depth),
                )?;
                if first == Relation::Compatible {
                    return Ok(first);
                }
                if self.relation_generic_inheritance(generic, target)? {
                    return Ok(Relation::Compatible);
                }
                for parent in self.relation_super_types(&other.get_base_type_id())? {
                    let next = next_relation_level!(depth);
                    if self.relation_at(source, &OwnedValue::plain(parent), mode, next)?
                        == Relation::Compatible
                    {
                        return Ok(Relation::Compatible);
                    }
                }
                Ok(first)
            }
            LuaType::Ref(id) | LuaType::Def(id) => {
                if generic.contain_tpl() {
                    return Ok(Relation::Compatible);
                }
                self.generic_ref_relation(generic, id, mode, next_relation_level!(depth))
            }
            LuaType::Table | LuaType::TableConst(_) | LuaType::Object(_) | LuaType::Tuple(_) => {
                Ok(Relation::Compatible)
            }
            LuaType::Union(_) => {
                for member in target.union_components().expect("union components") {
                    let result =
                        self.generic_relation(source, &member, mode, next_relation_level!(depth))?;
                    if result != Relation::Compatible {
                        return Ok(result);
                    }
                }
                Ok(Relation::Compatible)
            }
            _ => Ok(self.relation_mismatch(
                mode,
                source.structural_type(),
                target.structural_type(),
            )),
        }
    }

    fn generic_parameters_relation(
        &mut self,
        source: &OwnedValue,
        target: &OwnedValue,
        mode: RelationMode,
        depth: usize,
    ) -> QueryResult<Relation, E> {
        self.checkpoint()?;
        let LuaType::Generic(source_type) = source.structural_type() else {
            unreachable!();
        };
        let LuaType::Generic(target_type) = target.structural_type() else {
            unreachable!();
        };
        if source_type.get_base_type_id() != target_type.get_base_type_id()
            || source_type.get_params().len() != target_type.get_params().len()
        {
            return Ok(self.relation_mismatch(
                mode,
                source.structural_type(),
                target.structural_type(),
            ));
        }
        let next = next_relation_level!(depth);
        for (index, (a, b)) in source_type
            .get_params()
            .iter()
            .zip(target_type.get_params())
            .enumerate()
        {
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

    fn generic_ref_relation(
        &mut self,
        source: &LuaGenericType,
        target: &LuaTypeDeclId,
        mode: RelationMode,
        depth: usize,
    ) -> QueryResult<Relation, E> {
        self.checkpoint()?;
        let base = source.get_base_type_id();
        if &base == target
            || self.nominal_subtype(target, &base)?
            || self.nominal_subtype(&base, target)?
        {
            return Ok(Relation::Compatible);
        }
        if source.get_params().iter().any(|param| param.is_any()) {
            return self.relation_at(
                &OwnedValue::plain(source.get_base_type()),
                &OwnedValue::plain(LuaType::Ref(target.clone())),
                mode,
                next_relation_level!(depth),
            );
        }
        Ok(self.relation_mismatch(
            mode,
            &LuaType::Generic(std::sync::Arc::new(source.clone())),
            &LuaType::Ref(target.clone()),
        ))
    }
}
