//! Function compatibility preserves owned parameter/return carriers. Signature
//! conversion uses the owned producer and never the raw public inference path.
use super::{
    OwnedQuery, QueryResult,
    relation::{Relation, RelationMode, next_relation_level, relation_from_bool},
};
use crate::session::{OwnedValue, SelectedProjectionBackend};
use emmylua_code_analysis::LuaType;

impl<E> OwnedQuery<'_, '_, E> {
    pub(super) fn function_relation(
        &mut self,
        source: &OwnedValue,
        target: &OwnedValue,
        mode: RelationMode,
        depth: usize,
    ) -> QueryResult<Relation, E> {
        self.checkpoint()?;
        match target.structural_type() {
            LuaType::DocFunction(_) => {
                self.function_parameters_relation(source, target, mode, depth)
            }
            LuaType::Signature(id) => {
                let Some(signature) = self.signature_by_id(id)? else {
                    return Ok(self.relation_mismatch(
                        mode,
                        source.structural_type(),
                        target.structural_type(),
                    ));
                };
                self.function_parameters_relation(
                    source,
                    &signature,
                    mode,
                    next_relation_level!(depth),
                )
            }
            LuaType::Ref(id) | LuaType::Def(id) => {
                self.checkpoint()?;
                Ok(relation_from_bool!(
                    self,
                    mode,
                    source.structural_type(),
                    target.structural_type(),
                    self.model
                        .resolve_type_def(id.get_name())
                        .is_some_and(|def| !def.call_overloads.is_empty()),
                ))
            }
            LuaType::Union(_) => {
                for member in target.union_components().expect("union components") {
                    let result =
                        self.function_relation(source, &member, mode, next_relation_level!(depth))?;
                    if result != Relation::Compatible {
                        return Ok(result);
                    }
                }
                Ok(Relation::Compatible)
            }
            LuaType::Function => Ok(Relation::Compatible),
            _ => Ok(self.relation_mismatch(
                mode,
                source.structural_type(),
                target.structural_type(),
            )),
        }
    }

    fn function_parameters_relation(
        &mut self,
        source: &OwnedValue,
        target: &OwnedValue,
        mode: RelationMode,
        depth: usize,
    ) -> QueryResult<Relation, E> {
        self.checkpoint()?;
        let LuaType::DocFunction(source_function) = source.structural_type() else {
            unreachable!("source function");
        };
        let LuaType::DocFunction(target_function) = target.structural_type() else {
            unreachable!("target function");
        };
        let source_params = source
            .function_parameters()
            .expect("source function parameters");
        let mut target_params = target_function
            .get_params()
            .iter()
            .map(|(name, _)| name.clone())
            .zip(
                target
                    .function_parameters()
                    .expect("target function parameters"),
            )
            .collect::<Vec<_>>();
        if target_function.is_colon_define() {
            target_params.insert(0, ("self".into(), None));
        }
        let source_len = source_params.len();
        let target_len = target_params.len();
        for (index, (target_name, target_type)) in target_params.iter().enumerate() {
            self.checkpoint()?;
            let Some(source_type) = source_params.get(index) else {
                break;
            };
            if source_function.is_variadic() && index + 1 == source_len {
                let result = self.function_varargs_relation(
                    source_type.as_ref(),
                    &target_params[index..],
                    mode,
                    next_relation_level!(depth),
                )?;
                if result != Relation::Compatible {
                    return Ok(result);
                }
            }
            if target_function.is_variadic() && index + 1 == target_len {
                break;
            }
            if let (Some(source_type), Some(target_type)) = (source_type, target_type) {
                // Parameters use the original contravariant direction.
                let result =
                    self.relation_at(target_type, source_type, mode, next_relation_level!(depth))?;
                if result != Relation::Compatible {
                    if result.is_mismatch()
                        && index == 0
                        && source_type.structural_type().is_self_infer()
                        && target_name == "self"
                    {
                        continue;
                    }
                    return Ok(result);
                }
            }
        }
        self.relation_at(
            &source.function_return().expect("source function return"),
            &target.function_return().expect("target function return"),
            mode,
            next_relation_level!(depth),
        )
    }

    fn function_varargs_relation(
        &mut self,
        vararg: Option<&OwnedValue>,
        targets: &[(String, Option<OwnedValue>)],
        mode: RelationMode,
        depth: usize,
    ) -> QueryResult<Relation, E> {
        if let Some(vararg) = vararg {
            for (_, target) in targets {
                self.checkpoint()?;
                if let Some(target) = target {
                    let result =
                        self.relation_at(target, vararg, mode, next_relation_level!(depth))?;
                    if result != Relation::Compatible {
                        return Ok(result);
                    }
                }
            }
        }
        Ok(Relation::Compatible)
    }
}
