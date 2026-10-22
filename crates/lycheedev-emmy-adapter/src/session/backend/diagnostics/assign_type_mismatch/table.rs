//! Original assignment table traversal, retaining each selected nested carrier.
use super::*;

impl<E> OwnedQuery<'_, '_, E> {
    pub(super) fn assignment_table_fields(
        &mut self,
        context: &mut CheckContext<'_>,
        target: &OwnedValue,
        table: &LuaTableExpr,
    ) -> QueryResult<(), E> {
        self.checkpoint()?;
        if let LuaType::Union(union) = target.structural_type() {
            for (index, component) in union.into_vec().iter().enumerate() {
                if is_structured_table_target(&self.model, component) {
                    let component = self.owned_child(target, index, component);
                    return self.assignment_table_fields(context, &component, table);
                }
            }
        }
        if let LuaType::Array(array) = target.structural_type() {
            let expected = self.owned_child(target, 0, array.get_base());
            for (field, _) in table.get_fields_with_keys() {
                if let Some(value) = field.get_value_expr() {
                    self.assignment_table_value(context, &value, &expected)?;
                }
            }
            return Ok(());
        }
        if let LuaType::Generic(generic) = target.structural_type()
            && generic.get_base_type_id().get_name() == "array"
            && let Some(expected) = generic.get_params().first()
        {
            let expected = self.owned_child(target, 0, expected);
            for (field, _) in table.get_fields_with_keys() {
                if let Some(value) = field.get_value_expr() {
                    self.assignment_table_value(context, &value, &expected)?;
                }
            }
            return Ok(());
        }
        if let LuaType::Object(object) = target.structural_type() {
            for (field, key) in table.get_fields_with_keys() {
                self.checkpoint()?;
                let Some(value) = field.get_value_expr() else {
                    continue;
                };
                let member_key = index_key_to_member_key(&self.model, &key);
                if let Some(key) = member_key.as_ref()
                    && let Some(expected) = object.get_fields().get(key)
                {
                    let expected = self.owned_field(target, key, expected);
                    self.assignment_table_value(context, &value, &expected)?;
                    continue;
                }
                let key_ty = self.assignment_index_key_type(&key)?;
                for (index, (signature, expected)) in object.get_index_access().iter().enumerate() {
                    let signature = self.owned_index(target, index, true, signature);
                    if self.type_check_assign(&key_ty, &signature)? {
                        let expected = self.owned_index(target, index, false, expected);
                        self.assignment_table_value(context, &value, &expected)?;
                        break;
                    }
                }
            }
            return Ok(());
        }
        if matches!(target.structural_type(), LuaType::Ref(_) | LuaType::Def(_))
            && let Some(def) = named_def(&self.model, target.structural_type())
            && def.kind == TypeDefKind::Alias
            && let Some(alias) = self.model.alias_target(&def)
            && is_structured_table_target(&self.model, &alias)
        {
            return self.assignment_table_fields(context, &OwnedValue::plain(alias), table);
        }
        let Some(def) = named_def(&self.model, target.structural_type()) else {
            return Ok(());
        };
        let mut members = Vec::new();
        let mut required = Vec::new();
        let mut signatures = Vec::new();
        self.assignment_collect_members(
            &def,
            &mut Vec::new(),
            &mut members,
            &mut required,
            &mut signatures,
        )?;
        let fields = table.get_fields_with_keys();
        let by_name: HashMap<&str, &OwnedValue> = members
            .iter()
            .map(|(name, value)| (name.as_str(), value))
            .collect();
        for (field, key) in &fields {
            self.checkpoint()?;
            let name = key.get_path_part();
            if let Some(expected) = by_name.get(name.as_str()) {
                if let Some(value) = field.get_value_expr() {
                    self.assignment_table_value(context, &value, expected)?;
                }
                continue;
            }
            let mut matching = None;
            for (signature, expected) in &signatures {
                if self.assignment_signature_matches(signature, key)? {
                    matching = Some(expected);
                    break;
                }
            }
            let Some(expected) = matching else {
                continue;
            };
            if let Some(value) = field.get_value_expr() {
                self.assignment_table_value(context, &value, expected)?;
            }
        }
        if fields
            .iter()
            .any(|(_, key)| matches!(key, LuaIndexKey::Integer(_) | LuaIndexKey::Idx(_)))
        {
            let names: HashSet<String> =
                fields.iter().map(|(_, key)| key.get_path_part()).collect();
            for (name, expected) in &required {
                self.checkpoint()?;
                if !names.contains(name.as_str()) {
                    if let Some((field, _)) = fields.first()
                        && let Some(value) = field.get_value_expr()
                    {
                        let actual = self.type_of_expr_at(
                            value.get_syntax_id(),
                            value.get_range().start().into(),
                        )?;
                        self.assignment_mismatch(context, value.get_range(), expected, &actual)?;
                    }
                    break;
                }
            }
        }
        Ok(())
    }

    fn assignment_table_value(
        &mut self,
        context: &mut CheckContext<'_>,
        value: &LuaExpr,
        expected: &OwnedValue,
    ) -> QueryResult<(), E> {
        if !self.binding_manages(value.get_syntax_id())?
            && let LuaExpr::TableExpr(nested) = value
        {
            if is_structured_table_target(&self.model, expected.structural_type()) {
                self.assignment_table_fields(context, expected, nested)?;
            } else {
                let actual =
                    self.type_of_expr_at(value.get_syntax_id(), value.get_range().start().into())?;
                if !matches!(
                    actual.structural_type(),
                    LuaType::Unknown | LuaType::Any | LuaType::Never
                ) {
                    self.assignment_mismatch(context, value.get_range(), expected, &actual)?;
                }
            }
            return Ok(());
        }
        let actual =
            self.type_of_expr_at(value.get_syntax_id(), value.get_range().start().into())?;
        if !self.binding_manages(value.get_syntax_id())?
            && matches!(expected.structural_type(), LuaType::Integer)
            && matches!(actual.structural_type(), LuaType::Number)
            && matches!(value, LuaExpr::LiteralExpr(_))
        {
            return Ok(());
        }
        if let LuaType::Union(union) = actual.structural_type() {
            for (index, component) in union.into_vec().iter().enumerate() {
                let component = self.owned_child(&actual, index, component);
                if self.type_check_assign(&component, expected)? {
                    return Ok(());
                }
            }
        }
        if matches!(
            actual.structural_type(),
            LuaType::Unknown | LuaType::Any | LuaType::Never | LuaType::Nil
        ) || self.type_check_assign(&actual, expected)?
        {
            return Ok(());
        }
        self.assignment_mismatch(context, value.get_range(), expected, &actual)
    }

    fn assignment_index_key_type(&mut self, key: &LuaIndexKey) -> QueryResult<OwnedValue, E> {
        self.checkpoint()?;
        match key {
            LuaIndexKey::Name(_) | LuaIndexKey::String(_) => Ok(OwnedValue::plain(LuaType::String)),
            LuaIndexKey::Integer(_) | LuaIndexKey::Idx(_) => {
                Ok(OwnedValue::plain(LuaType::Integer))
            }
            LuaIndexKey::Expr(expression) => self.type_of_expr(expression.get_syntax_id()),
        }
    }

    fn assignment_signature_matches(
        &mut self,
        signature: &str,
        key: &LuaIndexKey,
    ) -> QueryResult<bool, E> {
        // Query always runs, even string/any/unknown signature branches.
        let value = self.assignment_index_key_type(key)?;
        match signature {
            "integer" | "number" | "int" => {
                Ok(matches!(key, LuaIndexKey::Integer(_) | LuaIndexKey::Idx(_))
                    || self.type_check(&value, &OwnedValue::plain(LuaType::Integer))?)
            }
            "string" => Ok(matches!(key, LuaIndexKey::Name(_) | LuaIndexKey::String(_))),
            "any" | "unknown" => Ok(true),
            _ => Ok(false),
        }
    }

    fn assignment_collect_members(
        &mut self,
        def: &TypeDef,
        visited: &mut Vec<SemanticId>,
        out: &mut Vec<(String, OwnedValue)>,
        required: &mut Vec<(String, OwnedValue)>,
        signatures: &mut Vec<(String, OwnedValue)>,
    ) -> QueryResult<(), E> {
        self.checkpoint()?;
        if visited.contains(&def.id) {
            return Ok(());
        }
        visited.push(def.id.clone());
        for reference in self.model.members_of_owner(&def.id).iter() {
            self.checkpoint()?;
            let Some(facts) = self.model.file_facts_of(reference.file_id) else {
                continue;
            };
            let Some(member) = facts.member_by_id(&reference.id) else {
                continue;
            };
            let name = reference.name.to_string();
            let value = self
                .type_of_member(&reference.id)?
                .unwrap_or_else(|| OwnedValue::plain(LuaType::Unknown));
            if member.is_index_signature {
                if !signatures.iter().any(|(current, _)| current == &name) {
                    signatures.push((name, value));
                }
            } else {
                if !out.iter().any(|(current, _)| current == &name) {
                    out.push((name.clone(), value.clone()));
                }
                if !member.is_nullable && !required.iter().any(|(current, _)| current == &name) {
                    required.push((name, value));
                }
            }
        }
        for name in &def.super_names {
            self.checkpoint()?;
            if let Some(parent) = self
                .model
                .type_defs_in_scope(TypeScope::Global, name.as_str())
                .into_iter()
                .next()
            {
                self.assignment_collect_members(&parent, visited, out, required, signatures)?;
            }
        }
        Ok(())
    }
}
