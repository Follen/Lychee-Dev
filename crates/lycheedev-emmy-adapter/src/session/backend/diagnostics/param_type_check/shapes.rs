//! Object/array checks preserve owned fields, slots and inferred values.
use super::*;

impl<E> OwnedQuery<'_, '_, E> {
    pub(super) fn pt_member_missing_in_union(
        &mut self,
        index: &LuaIndexExpr,
    ) -> QueryResult<bool, E> {
        let Some(key) = index.get_index_key() else {
            return Ok(false);
        };
        let Some(prefix) = index.get_prefix_expr() else {
            return Ok(false);
        };
        let prefix = self.owned_prefix_resolution_type(&prefix)?;
        let Some(components) = prefix.union_components() else {
            return Ok(false);
        };
        let key = LuaMemberKey::Name(key.get_path_part().into());
        for component in &components {
            if self.member_type(component, &key)?.is_none() {
                return Ok(true);
            }
        }
        Ok(false)
    }

    pub(super) fn pt_table_literal_mismatch(
        &mut self,
        param: &OwnedValue,
        table: &emmylua_parser::LuaTableExpr,
    ) -> QueryResult<Option<Mismatch>, E> {
        self.checkpoint()?;
        match param.structural_type() {
            LuaType::Object(object) => {
                for (field, key) in table.get_fields_with_keys() {
                    self.checkpoint()?;
                    let path = key.get_path_part();
                    let key = LuaMemberKey::Name(path.clone().into());
                    let Some(expected) = object.get_fields().get(&key) else {
                        continue;
                    };
                    let expected = self.owned_field(param, &key, expected);
                    let Some(expr) = field.get_value_expr() else {
                        continue;
                    };
                    let actual = self.type_of_expr(expr.get_syntax_id())?;
                    if actual == expected || self.type_check(&actual, &expected)? {
                        continue;
                    }
                    return Ok(Some(Mismatch {
                        range: expr.get_range(),
                        message: self.pt_mismatch_message(&actual, &expected, &path),
                    }));
                }
                Ok(None)
            }
            LuaType::Intersection(intersection) => {
                for (index, raw) in intersection.get_types().iter().enumerate() {
                    let component = self.owned_child(param, index, raw);
                    if let Some(mismatch) = self.pt_table_literal_mismatch(&component, table)? {
                        return Ok(Some(mismatch));
                    }
                }
                Ok(None)
            }
            LuaType::Union(_) => {
                let mut last = None;
                for component in param.union_components().expect("union") {
                    // Original `?` returns None as soon as any branch accepts.
                    let Some(mismatch) = self.pt_table_literal_mismatch(&component, table)? else {
                        return Ok(None);
                    };
                    last = Some(mismatch);
                }
                Ok(last)
            }
            _ => Ok(None),
        }
    }

    pub(super) fn pt_generic_table_required_mismatch(
        &mut self,
        param: &OwnedValue,
        table: &emmylua_parser::LuaTableExpr,
    ) -> QueryResult<Option<Mismatch>, E> {
        self.checkpoint()?;
        let id = match param.structural_type() {
            LuaType::Generic(generic) => generic.get_base_type_id(),
            LuaType::Ref(id) | LuaType::Def(id) => id.clone(),
            LuaType::Union(_) => {
                for component in param.union_components().expect("union") {
                    if let Some(mismatch) =
                        self.pt_generic_table_required_mismatch(&component, table)?
                    {
                        return Ok(Some(mismatch));
                    }
                }
                return Ok(None);
            }
            LuaType::Intersection(intersection) => {
                for (index, raw) in intersection.get_types().iter().enumerate() {
                    let component = self.owned_child(param, index, raw);
                    if let Some(mismatch) =
                        self.pt_generic_table_required_mismatch(&component, table)?
                    {
                        return Ok(Some(mismatch));
                    }
                }
                return Ok(None);
            }
            _ => return Ok(None),
        };
        let Some(def) = self.model.type_def_of(&id) else {
            return Ok(None);
        };
        if def.kind != TypeDefKind::Class {
            return Ok(None);
        }
        let provided: Vec<String> = table
            .get_fields_with_keys()
            .iter()
            .map(|(_, key)| key.get_path_part())
            .collect();
        for reference in self.model.members_of_owner(&def.id).iter() {
            self.checkpoint()?;
            let Some(facts) = self.model.file_facts_of(reference.file_id) else {
                continue;
            };
            let Some(member) = facts.member_by_id(&reference.id) else {
                continue;
            };
            if member.is_nullable || member.is_index_signature {
                continue;
            }
            if !provided.contains(&reference.name.to_string()) {
                let expected = self
                    .type_of_member(&reference.id)?
                    .unwrap_or_else(|| OwnedValue::plain(LuaType::Unknown));
                let actual = self.type_of_expr(table.get_syntax_id())?;
                return Ok(Some(Mismatch {
                    range: table.get_range(),
                    message: self.pt_mismatch_message(&actual, &expected, &reference.name),
                }));
            }
        }
        Ok(None)
    }

    pub(super) fn pt_object_array_compatible(
        &mut self,
        arg: &OwnedValue,
        param: &OwnedValue,
        bindings: &mut Bindings,
    ) -> QueryResult<bool, E> {
        let (LuaType::Object(object), LuaType::Array(array)) =
            (arg.structural_type(), param.structural_type())
        else {
            return Ok(false);
        };
        let base = self.owned_child(param, 0, array.get_base());
        for (key, ty) in object.get_fields() {
            self.checkpoint()?;
            if key.get_integer().is_some()
                && self.pt_object_array_value(&self.owned_field(arg, key, ty), &base, bindings)?
            {
                return Ok(true);
            }
        }
        for (index, (key, ty)) in object.get_index_access().iter().enumerate() {
            self.checkpoint()?;
            if matches!(
                key,
                LuaType::Integer | LuaType::Number | LuaType::IntegerConst(_)
            ) && self.pt_object_array_value(
                &self.owned_index(arg, index, false, ty),
                &base,
                bindings,
            )? {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn pt_object_array_value(
        &mut self,
        value: &OwnedValue,
        base: &OwnedValue,
        bindings: &mut Bindings,
    ) -> QueryResult<bool, E> {
        if let LuaType::TplRef(tpl) = base.structural_type() {
            match bindings.get(&tpl.get_tpl_id()) {
                Some(existing) => return Ok(existing == value),
                None => {
                    bindings.insert(tpl.get_tpl_id(), value.clone());
                    return Ok(true);
                }
            }
        }
        self.type_check(value, base)
    }

    pub(super) fn pt_named_kind_compatible(
        &mut self,
        arg: &OwnedValue,
        param: &OwnedValue,
    ) -> QueryResult<bool, E> {
        self.checkpoint()?;
        Ok(named_kind_compatible(
            &self.model,
            arg.structural_type(),
            param.structural_type(),
        ))
    }
    pub(super) fn pt_literal_compatible(
        &mut self,
        arg: &OwnedValue,
        param: &OwnedValue,
    ) -> QueryResult<bool, E> {
        self.checkpoint()?;
        Ok(literal_compatible(
            &self.model,
            arg.structural_type(),
            param.structural_type(),
        ))
    }
}

// The helpers below only classify scalar/literal domains or inspect lexical
// declarations. They do not infer runtime values, construct types, compare
// object identity, or use structural equality as selected-value compatibility.

pub(super) fn has_table_literal_shape(ty: &LuaType) -> bool {
    match ty {
        LuaType::Object(_) => true,
        LuaType::Intersection(intersection) => {
            intersection.get_types().iter().any(has_table_literal_shape)
        }
        LuaType::Union(union) => union.into_vec().iter().any(has_table_literal_shape),
        _ => false,
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ValueKind {
    Nil,
    Boolean,
    String,
    Integer,
    Number,
    Other,
}

fn value_kind(semantic_model: &SemanticModel<'_>, ty: &LuaType) -> ValueKind {
    value_kind_inner(semantic_model, ty, &mut Vec::new())
}

fn value_kind_inner(
    semantic_model: &SemanticModel<'_>,
    ty: &LuaType,
    visited: &mut Vec<LuaTypeDeclId>,
) -> ValueKind {
    match ty {
        LuaType::Nil => ValueKind::Nil,
        LuaType::Boolean | LuaType::BooleanConst(_) | LuaType::DocBooleanConst(_) => {
            ValueKind::Boolean
        }
        LuaType::String | LuaType::StringConst(_) | LuaType::DocStringConst(_) => ValueKind::String,
        LuaType::Integer | LuaType::IntegerConst(_) | LuaType::DocIntegerConst(_) => {
            ValueKind::Integer
        }
        LuaType::Number | LuaType::FloatConst(_) => ValueKind::Number,
        LuaType::Union(union) => {
            let mut kind = None;
            for component in union.into_vec() {
                let component_kind = value_kind_inner(semantic_model, &component, visited);
                if kind.is_some_and(|kind| kind != component_kind) {
                    return ValueKind::Other;
                }
                kind = Some(component_kind);
            }
            kind.unwrap_or(ValueKind::Other)
        }
        LuaType::Ref(id) | LuaType::Def(id) => {
            if visited.contains(id) {
                return ValueKind::Other;
            }
            visited.push(id.clone());
            let Some(def) = semantic_model.type_def_of(id) else {
                return ValueKind::Other;
            };
            match def.kind {
                TypeDefKind::Alias => semantic_model
                    .alias_target(&def)
                    .map(|target| value_kind_inner(semantic_model, &target, visited))
                    .unwrap_or(ValueKind::Other),
                TypeDefKind::Class => {
                    if def
                        .super_names
                        .iter()
                        .any(|name| matches!(name.as_str(), "integer" | "int"))
                    {
                        ValueKind::Integer
                    } else if def
                        .super_names
                        .iter()
                        .any(|name| matches!(name.as_str(), "number"))
                    {
                        ValueKind::Number
                    } else if def.super_names.iter().any(|name| name.as_str() == "string") {
                        ValueKind::String
                    } else {
                        ValueKind::Other
                    }
                }
                TypeDefKind::Enum => enum_value_kind(semantic_model, &def),
            }
        }
        _ => ValueKind::Other,
    }
}

fn enum_value_kind(semantic_model: &SemanticModel<'_>, def: &TypeDef) -> ValueKind {
    let mut kind: Option<ValueKind> = None;
    let Some(facts) = semantic_model.file_facts_of(def.file_id) else {
        return ValueKind::Other;
    };
    let Some(decl) = facts.decl_named(def.name.as_str()) else {
        return ValueKind::Other;
    };
    for member_ref in semantic_model.members_of_owner(&decl.id).iter() {
        let Some(member_facts) = semantic_model.file_facts_of(member_ref.file_id) else {
            continue;
        };
        let Some(member) = member_facts.member_by_id(&member_ref.id) else {
            continue;
        };
        let Some(value_syntax) = member.value_syntax else {
            continue;
        };
        let Some(node) = semantic_model
            .syntax_tree()
            .and_then(|tree| value_syntax.to_node_from_root(&tree.get_red_root()))
        else {
            continue;
        };
        let raw = node.text().to_string();
        let item_kind = if raw.starts_with(['"', '\'']) {
            ValueKind::String
        } else if raw.parse::<i64>().is_ok() {
            ValueKind::Integer
        } else if raw.parse::<f64>().is_ok() {
            ValueKind::Number
        } else {
            ValueKind::Other
        };
        if let Some(kind) = kind
            && kind != item_kind
        {
            return ValueKind::Other;
        }
        kind = Some(item_kind);
    }
    kind.unwrap_or(ValueKind::Other)
}

fn named_kind_compatible(
    semantic_model: &SemanticModel<'_>,
    arg_ty: &LuaType,
    param_ty: &LuaType,
) -> bool {
    // Literal constants must match exactly (`"a"` != `"beforeAll"`).
    if let (LuaType::StringConst(a), LuaType::StringConst(b)) = (arg_ty, param_ty) {
        return a == b;
    }
    if let (
        LuaType::IntegerConst(a) | LuaType::DocIntegerConst(a),
        LuaType::IntegerConst(b) | LuaType::DocIntegerConst(b),
    ) = (arg_ty, param_ty)
    {
        return a == b;
    }
    let arg_kind = value_kind(semantic_model, arg_ty);
    let param_kind = value_kind(semantic_model, param_ty);
    if matches!(arg_kind, ValueKind::Other) || matches!(param_kind, ValueKind::Other) {
        return false;
    }
    arg_kind == param_kind
        || matches!(
            (arg_kind, param_kind),
            (ValueKind::Integer, ValueKind::Number) | (ValueKind::Number, ValueKind::Integer)
        )
}

fn literal_compatible(
    semantic_model: &SemanticModel<'_>,
    arg_ty: &LuaType,
    param_ty: &LuaType,
) -> bool {
    let arg_text = match arg_ty {
        LuaType::StringConst(s) | LuaType::DocStringConst(s) => Some(s.as_ref().to_string()),
        LuaType::IntegerConst(i) | LuaType::DocIntegerConst(i) => Some(i.to_string()),
        LuaType::BooleanConst(b) | LuaType::DocBooleanConst(b) => Some(b.to_string()),
        _ => None,
    };
    let Some(arg_text) = arg_text else {
        return false;
    };
    literal_values(semantic_model, param_ty, &mut Vec::new())
        .is_some_and(|values| values.iter().any(|value| value == &arg_text))
}

fn literal_values(
    semantic_model: &SemanticModel<'_>,
    ty: &LuaType,
    visited: &mut Vec<LuaTypeDeclId>,
) -> Option<Vec<String>> {
    match ty {
        LuaType::StringConst(s) | LuaType::DocStringConst(s) => Some(vec![s.as_ref().to_string()]),
        LuaType::IntegerConst(i) | LuaType::DocIntegerConst(i) => Some(vec![i.to_string()]),
        LuaType::BooleanConst(b) | LuaType::DocBooleanConst(b) => Some(vec![b.to_string()]),
        LuaType::Union(union) => {
            let mut values = Vec::new();
            for component in union.into_vec() {
                if let Some(mut component_values) =
                    literal_values(semantic_model, &component, visited)
                {
                    values.append(&mut component_values);
                }
            }
            (!values.is_empty()).then_some(values)
        }
        LuaType::Ref(id) | LuaType::Def(id) => {
            if visited.contains(id) {
                return None;
            }
            visited.push(id.clone());
            let def = semantic_model.type_def_of(id)?;
            if def.kind == TypeDefKind::Alias {
                return semantic_model
                    .alias_target(&def)
                    .and_then(|target| literal_values(semantic_model, &target, visited));
            }
            // enum: runtime table member names and values.
            if def.kind == TypeDefKind::Enum {
                let facts = semantic_model.file_facts_of(def.file_id)?;
                let decl = facts.decl_named(def.name.as_str())?;
                let mut values = Vec::new();
                for member_ref in semantic_model.members_of_owner(&decl.id).iter() {
                    values.push(member_ref.name.to_string());
                    if let Some(member_facts) = semantic_model.file_facts_of(member_ref.file_id)
                        && let Some(member) = member_facts.member_by_id(&member_ref.id)
                        && let Some(value_syntax) = member.value_syntax
                        && let Some(node) = semantic_model
                            .syntax_tree()
                            .and_then(|tree| value_syntax.to_node_from_root(&tree.get_red_root()))
                    {
                        let raw = node.text().to_string();
                        values.push(raw.trim_matches(['"', '\'']).to_string());
                    }
                }
                return (!values.is_empty()).then_some(values);
            }
            None
        }
        _ => None,
    }
}
