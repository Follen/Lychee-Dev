//! Assignment-only rules; recursive comparisons stay inside the owned relation.
use super::{
    OwnedQuery, QueryResult,
    relation::{Relation, RelationMode, next_relation_level, relation_from_bool},
};
use crate::session::{OwnedValue, SelectedProjectionBackend};
use emmylua_code_analysis::{LuaMemberKey, LuaType, SemanticId, TypeDef, TypeDefKind, TypeScope};

macro_rules! next_optional_relation_level {
    ($depth:expr) => {{
        if $depth >= 100 {
            return Ok(Some(Relation::Recursion));
        }
        $depth + 1
    }};
}

impl<E> OwnedQuery<'_, '_, E> {
    pub(super) fn assign_relation_prefix(
        &mut self,
        source: &OwnedValue,
        target: &OwnedValue,
        depth: usize,
    ) -> QueryResult<Option<Relation>, E> {
        self.checkpoint()?;
        let s = source.structural_type();
        let t = target.structural_type();
        match (s, t) {
            (
                LuaType::Integer | LuaType::IntegerConst(_) | LuaType::DocIntegerConst(_),
                LuaType::Number,
            ) => return Ok(Some(Relation::Compatible)),
            (
                LuaType::Number | LuaType::FloatConst(_),
                LuaType::Integer | LuaType::IntegerConst(_) | LuaType::DocIntegerConst(_),
            ) => {
                return Ok(Some(self.relation_mismatch(
                    RelationMode::Assign,
                    source.structural_type(),
                    target.structural_type(),
                )));
            }
            _ => {}
        }
        if let (LuaType::TableConst(table), LuaType::Ref(id) | LuaType::Def(id)) = (s, t) {
            let source_def = self
                .model
                .file_facts_of(table.file_id)
                .and_then(|facts| {
                    facts
                        .decl_by_value_range(table.value)
                        .and_then(|decl| decl.owner_syntax)
                        .and_then(|syntax| facts.type_def_by_owner_syntax(syntax))
                })
                .cloned();
            let target_def = self.model.resolve_type_def(id.get_name());
            if let (Some(source_def), Some(target_def)) = (source_def, target_def)
                && !self.assignment_def_extends(&source_def, &target_def, &mut Vec::new())?
                && !self.assignment_def_extends(&target_def, &source_def, &mut Vec::new())?
            {
                return Ok(Some(self.relation_mismatch(
                    RelationMode::Assign,
                    source.structural_type(),
                    target.structural_type(),
                )));
            }
        }
        if let LuaType::Ref(id) | LuaType::Def(id) = t
            && let Some(def) = self.model.resolve_type_def(id.get_name())
            && def.kind == TypeDefKind::Enum
        {
            match s {
                LuaType::Integer | LuaType::DocIntegerConst(_) => {
                    return Ok(Some(Relation::Compatible));
                }
                LuaType::IntegerConst(value) => {
                    return Ok(Some(relation_from_bool!(
                        self,
                        RelationMode::Assign,
                        source.structural_type(),
                        target.structural_type(),
                        self.assignment_enum_values(&def)?.contains(value),
                    )));
                }
                _ => {}
            }
        }
        if let Some(members) = target.union_components() {
            for member in members {
                let next = next_optional_relation_level!(depth);
                if self.relation_at(source, &member, RelationMode::Assign, next)?
                    == Relation::Compatible
                {
                    return Ok(Some(Relation::Compatible));
                }
            }
            return Ok(Some(self.relation_mismatch(
                RelationMode::Assign,
                source.structural_type(),
                target.structural_type(),
            )));
        }
        if let (LuaType::Object(object), LuaType::Ref(_) | LuaType::Def(_)) = (s, t)
            && let Some(def) = self.assignment_named_def(t)
        {
            let mut signatures = Vec::new();
            self.assignment_index_members(&def, &mut Vec::new(), &mut signatures)?;
            if !signatures.is_empty() {
                let mut matched = false;
                for (index, (key, value)) in object.get_index_access().iter().enumerate() {
                    self.checkpoint()?;
                    let Some((_, expected)) = signatures.iter().find(|(signature, _)| {
                        matches!(signature.as_str(), "integer" | "number" | "int")
                            && matches!(
                                key,
                                LuaType::Integer | LuaType::Number | LuaType::IntegerConst(_)
                            )
                    }) else {
                        continue;
                    };
                    matched = true;
                    if !matches!(expected.structural_type(), LuaType::Any | LuaType::Unknown) {
                        let next = next_optional_relation_level!(depth);
                        if self.relation_at(
                            &self.owned_index(source, index, false, value),
                            expected,
                            RelationMode::Assign,
                            next,
                        )? != Relation::Compatible
                        {
                            return Ok(Some(self.relation_mismatch(
                                RelationMode::Assign,
                                source.structural_type(),
                                target.structural_type(),
                            )));
                        }
                    }
                }
                if matched {
                    return Ok(Some(Relation::Compatible));
                }
            }
        }
        if let LuaType::Intersection(intersection) = t
            && let Some(source_def) = self.assignment_named_def(s)
        {
            let mut actuals = Vec::new();
            self.assignment_named_members(&source_def, &mut Vec::new(), &mut actuals)?;
            for component in intersection.get_types() {
                self.checkpoint()?;
                let Some(def) = self.assignment_named_def(component) else {
                    return Ok(Some(self.relation_mismatch(
                        RelationMode::Assign,
                        source.structural_type(),
                        target.structural_type(),
                    )));
                };
                let mut expected = Vec::new();
                self.assignment_named_members(&def, &mut Vec::new(), &mut expected)?;
                for (key, expected) in expected {
                    let Some((_, actual)) = actuals.iter().find(|(existing, _)| existing == &key)
                    else {
                        if expected.structural_type().is_nullable() {
                            continue;
                        }
                        return Ok(Some(self.relation_mismatch(
                            RelationMode::Assign,
                            source.structural_type(),
                            target.structural_type(),
                        )));
                    };
                    let next = next_optional_relation_level!(depth);
                    if self.relation_at(actual, &expected, RelationMode::Assign, next)?
                        != Relation::Compatible
                    {
                        return Ok(Some(self.relation_mismatch(
                            RelationMode::Assign,
                            source.structural_type(),
                            target.structural_type(),
                        )));
                    }
                }
            }
            return Ok(Some(Relation::Compatible));
        }
        if matches!((s, t), (LuaType::DocFunction(_), LuaType::DocFunction(_))) {
            let source_params = source
                .function_parameters()
                .expect("source function parameters");
            let target_params = target
                .function_parameters()
                .expect("target function parameters");
            if source_params.len() != target_params.len() {
                return Ok(Some(self.relation_mismatch(
                    RelationMode::Assign,
                    source.structural_type(),
                    target.structural_type(),
                )));
            }
            for (source_param, target_param) in source_params.iter().zip(&target_params) {
                if let (Some(source_param), Some(target_param)) = (source_param, target_param) {
                    let next = next_optional_relation_level!(depth);
                    if self.assignment_parameter_accepts(target_param, source_param, next)?
                        != Relation::Compatible
                    {
                        return Ok(Some(self.relation_mismatch(
                            RelationMode::Assign,
                            source.structural_type(),
                            target.structural_type(),
                        )));
                    }
                }
            }
            let next = next_optional_relation_level!(depth);
            return self
                .relation_at(
                    &source.function_return().expect("source return"),
                    &target.function_return().expect("target return"),
                    RelationMode::Assign,
                    next,
                )
                .map(Some);
        }
        Ok(None)
    }

    fn assignment_parameter_accepts(
        &mut self,
        target: &OwnedValue,
        source: &OwnedValue,
        depth: usize,
    ) -> QueryResult<Relation, E> {
        self.checkpoint()?;
        if let Some(members) = target.union_components() {
            for member in members {
                let result = self.assignment_parameter_accepts(
                    &member,
                    source,
                    next_relation_level!(depth),
                )?;
                if result != Relation::Compatible {
                    return Ok(result);
                }
            }
            return Ok(Relation::Compatible);
        }
        if let Some(members) = source.union_components() {
            for member in members {
                let next = next_relation_level!(depth);
                if self.assignment_parameter_accepts(target, &member, next)? == Relation::Compatible
                {
                    return Ok(Relation::Compatible);
                }
            }
            return Ok(self.relation_mismatch(
                RelationMode::Assign,
                source.structural_type(),
                target.structural_type(),
            ));
        }
        self.relation_at(target, source, RelationMode::Assign, depth)
    }

    fn assignment_named_def(&self, ty: &LuaType) -> Option<TypeDef> {
        match ty {
            LuaType::Ref(id) | LuaType::Def(id) => self.model.type_def_of(id),
            LuaType::Generic(generic) => self.model.type_def_of(&generic.get_base_type_id()),
            _ => None,
        }
    }

    fn assignment_def_extends(
        &mut self,
        source: &TypeDef,
        target: &TypeDef,
        visited: &mut Vec<SemanticId>,
    ) -> QueryResult<bool, E> {
        self.checkpoint()?;
        if source.id == target.id {
            return Ok(true);
        }
        if visited.contains(&source.id) {
            return Ok(false);
        }
        visited.push(source.id.clone());
        for name in &source.super_names {
            self.checkpoint()?;
            let parent = self
                .model
                .resolve_type_def_in(source.file_id, name.as_str())
                .or_else(|| {
                    self.model
                        .type_defs_in_scope(TypeScope::Global, name.as_str())
                        .iter()
                        .next()
                        .cloned()
                });
            if let Some(parent) = parent
                && self.assignment_def_extends(&parent, target, visited)?
            {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn assignment_enum_values(
        &mut self,
        def: &TypeDef,
    ) -> QueryResult<std::collections::HashSet<i64>, E> {
        let members = self.model.members_of_owner(&def.id);
        let mut out = std::collections::HashSet::new();
        for reference in members.iter() {
            self.checkpoint()?;
            let text = self
                .model
                .file_facts_of(reference.file_id)
                .and_then(|facts| facts.member_by_id(&reference.id))
                .and_then(|member| member.value_syntax)
                .and_then(|syntax| {
                    self.model
                        .syntax_tree_of(reference.file_id)
                        .and_then(|tree| syntax.to_node_from_root(&tree.get_red_root()))
                })
                .map(|node| node.text().to_string());
            if let Some(text) = text
                && let Some(value) = assignment_enum_integer(&text)
            {
                out.insert(value);
            }
        }
        Ok(out)
    }

    fn assignment_named_members(
        &mut self,
        def: &TypeDef,
        visited: &mut Vec<SemanticId>,
        out: &mut Vec<(LuaMemberKey, OwnedValue)>,
    ) -> QueryResult<(), E> {
        self.checkpoint()?;
        if visited.contains(&def.id) {
            return Ok(());
        }
        visited.push(def.id.clone());
        let members = self.model.members_of_owner(&def.id);
        for reference in members.iter() {
            self.checkpoint()?;
            let key = self
                .model
                .file_facts_of(reference.file_id)
                .and_then(|facts| facts.member_by_id(&reference.id))
                .map(|member| member.key.clone());
            if let Some(key) = key
                && !out.iter().any(|(existing, _)| existing == &key)
            {
                let value = self
                    .type_of_member(&reference.id)?
                    .unwrap_or_else(|| OwnedValue::plain(LuaType::Unknown));
                out.push((key, value));
            }
        }
        for name in &def.super_names {
            let parent = self
                .model
                .resolve_type_def_in(def.file_id, name.as_str())
                .or_else(|| {
                    self.model
                        .type_defs_in_scope(TypeScope::Global, name.as_str())
                        .iter()
                        .next()
                        .cloned()
                });
            if let Some(parent) = parent {
                self.assignment_named_members(&parent, visited, out)?;
            }
        }
        Ok(())
    }

    fn assignment_index_members(
        &mut self,
        def: &TypeDef,
        visited: &mut Vec<SemanticId>,
        out: &mut Vec<(String, OwnedValue)>,
    ) -> QueryResult<(), E> {
        self.checkpoint()?;
        if visited.contains(&def.id) {
            return Ok(());
        }
        visited.push(def.id.clone());
        let members = self.model.members_of_owner(&def.id);
        for reference in members.iter() {
            self.checkpoint()?;
            let Some(member) = self
                .model
                .file_facts_of(reference.file_id)
                .and_then(|facts| facts.member_by_id(&reference.id))
                .cloned()
            else {
                continue;
            };
            // All members are projected, including non-index members, before
            // deciding which output list receives them in the original routine.
            let value = self
                .type_of_member(&reference.id)?
                .unwrap_or_else(|| OwnedValue::plain(LuaType::Unknown));
            let name = reference.name.to_string();
            if member.is_index_signature && !out.iter().any(|(existing, _)| existing == &name) {
                out.push((name, value));
            }
        }
        for name in &def.super_names {
            let parent = self
                .model
                .type_defs_in_scope(TypeScope::Global, name.as_str())
                .iter()
                .next()
                .cloned();
            if let Some(parent) = parent {
                self.assignment_index_members(&parent, visited, out)?;
            }
        }
        Ok(())
    }
}
fn assignment_enum_integer(text: &str) -> Option<i64> {
    let text = text.trim();
    if let Ok(value) = text.parse::<i64>() {
        return Some(value);
    }
    let (left, right) = text.split_once("<<")?;
    left.trim()
        .parse::<i64>()
        .ok()?
        .checked_shl(right.trim().parse::<u32>().ok()?)
}
