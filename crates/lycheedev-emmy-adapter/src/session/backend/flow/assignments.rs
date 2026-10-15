use super::*;
use emmylua_code_analysis::{LuaMemberKey, LuaType};
use emmylua_parser::{LuaNameExpr, LuaVarExpr};
impl<E> OwnedQuery<'_, '_, E> {
    pub(super) fn assigned_value(
        &mut self,
        decl: &SemanticId,
        tree: &FlowTree,
        flow: FlowId,
        position: TextSize,
        syntax: LuaSyntaxId,
        target: &OwnedValue,
    ) -> QueryResult<OwnedValue, E> {
        self.checkpoint()?;
        let multi = tree.get_decl_multi_return_ref_on_flow(decl, position, flow);
        if let Some(reference) = multi.and_then(|at| at.reference.as_ref()) {
            if let Some(call) = self
                .model
                .chunk()
                .and_then(|chunk| reference.call_expr.to_node(&chunk))
            {
                let value = self.type_of_expr(call.get_syntax_id())?;
                if let Some(slot) = value.result_slot(reference.return_index) {
                    return Ok(slot);
                }
                if reference.return_index > 0 {
                    return Ok(OwnedValue::plain(LuaType::Nil));
                }
            } else {
                let value = self.type_of_expr(syntax)?;
                return self.self_widen_assignment(decl, syntax, target, value);
            }
        } else if multi.is_some()
            && let Some(node) = tree.get_flow_node(flow)
            && let FlowNodeKind::Assignment(ptr) = &node.kind
            && let Some(assign) = self.model.chunk().and_then(|chunk| ptr.to_node(&chunk))
        {
            let (vars, values) = assign.get_var_and_expr_list();
            if vars
                .iter()
                .enumerate()
                .find_map(|(index, var)| match var {
                    LuaVarExpr::NameExpr(name)
                        if self.model.resolve_name(name.get_position()).as_ref() == Some(decl) =>
                    {
                        Some(index)
                    }
                    _ => None,
                })
                .is_some_and(|index| index >= values.len())
            {
                return Ok(OwnedValue::plain(LuaType::Nil));
            }
        }
        let value = self.type_of_expr_at(syntax, position.into())?;
        self.self_widen_assignment(decl, syntax, target, value)
    }
    fn self_widen_assignment(
        &mut self,
        decl: &SemanticId,
        syntax: LuaSyntaxId,
        target: &OwnedValue,
        value: OwnedValue,
    ) -> QueryResult<OwnedValue, E> {
        let value = self.widen_for_target(value, target)?;
        let expr = self
            .model
            .syntax_tree()
            .and_then(|tree| syntax.to_node_from_root(&tree.get_red_root()))
            .and_then(LuaExpr::cast);
        let check_expr = match expr {
            Some(LuaExpr::ParenExpr(paren)) => paren.get_expr(),
            Some(expr @ (LuaExpr::BinaryExpr(_) | LuaExpr::UnaryExpr(_))) => Some(expr),
            _ => None,
        };
        if let Some(expr) = check_expr {
            for name in expr.descendants::<LuaNameExpr>() {
                self.checkpoint()?;
                if self.model.resolve_name(name.get_position()).as_ref() == Some(decl)
                    && matches!(
                        value.structural_type(),
                        LuaType::IntegerConst(_)
                            | LuaType::DocIntegerConst(_)
                            | LuaType::FloatConst(_)
                            | LuaType::StringConst(_)
                            | LuaType::DocStringConst(_)
                            | LuaType::BooleanConst(_)
                            | LuaType::DocBooleanConst(_)
                    )
                {
                    return Ok(algebra::widen(value));
                }
            }
        }
        Ok(value)
    }
    pub(super) fn coerce_flow_assignment(
        &mut self,
        decl: &SemanticId,
        value: OwnedValue,
        before: &OwnedValue,
        base: &OwnedValue,
    ) -> QueryResult<OwnedValue, E> {
        self.checkpoint()?;
        let empty = if let LuaType::TableConst(table) = value.structural_type() {
            self.model
                .members_of_owner(&SemanticId::member(table.file_id, table.value))
                .is_empty()
        } else {
            false
        };
        if empty && !matches!(before.structural_type(), LuaType::Ref(_) | LuaType::Def(_)) {
            let mut candidates = vec![before.clone()];
            if before != base {
                candidates.push(base.clone());
            }
            if let Some(syntax) = self
                .model
                .file_facts()
                .and_then(|facts| facts.decl_by_id(decl))
                .and_then(|decl| decl.value_expr_syntax)
            {
                let init = self.type_of_expr(syntax)?;
                if !matches!(init.structural_type(), LuaType::Unknown | LuaType::Any) {
                    candidates.push(init);
                } else if let Some(init) = self.flow_initializer_index(syntax)? {
                    candidates.push(init);
                }
            }
            for candidate in candidates {
                for component in algebra::components(candidate) {
                    self.checkpoint()?;
                    if matches!(
                        component.structural_type(),
                        LuaType::Nil
                            | LuaType::BooleanConst(false)
                            | LuaType::DocBooleanConst(false)
                    ) {
                        continue;
                    }
                    if self.empty_table_covered_by(&component)? {
                        return Ok(component);
                    }
                }
            }
        }
        if before == base
            || !matches!(
                value.structural_type(),
                LuaType::TableConst(_) | LuaType::Object(_)
            )
            || !matches!(before.structural_type(), LuaType::Ref(_) | LuaType::Def(_))
        {
            return Ok(value);
        }
        for field in ["kind", "type"] {
            let key = LuaMemberKey::Name(field.into());
            if let (Some(literal), Some(narrowed)) = (
                self.flow_table_member(&value, &key)?,
                self.member_type(before, &key)?,
            ) && !self.flow_overlap(literal, narrowed)?
            {
                return Ok(base.clone());
            }
        }
        Ok(before.clone())
    }
    fn flow_initializer_index(
        &mut self,
        syntax: LuaSyntaxId,
    ) -> QueryResult<Option<OwnedValue>, E> {
        let Some(LuaExpr::IndexExpr(index)) = self
            .model
            .syntax_tree()
            .and_then(|tree| syntax.to_node_from_root(&tree.get_red_root()))
            .and_then(LuaExpr::cast)
        else {
            return Ok(None);
        };
        let Some(LuaExpr::NameExpr(name)) = index.get_prefix_expr() else {
            return Ok(None);
        };
        let Some(decl) = self.model.resolve_name(name.get_position()) else {
            return Ok(None);
        };
        let value = self.declared_or_unknown(&decl)?;
        if matches!(value.structural_type(), LuaType::Unknown | LuaType::Any) {
            return Ok(None);
        }
        let Some(key) = static_member_key(&index) else {
            return Ok(None);
        };
        if let Some(value) = self.member_type(&value, &key)? {
            return Ok(Some(value));
        }
        Ok(match value.structural_type() {
            LuaType::Generic(generic)
                if generic.get_base_type_id().get_name() == "table"
                    && generic.get_params().len() == 2 =>
            {
                Some(OwnedValue::plain(generic.get_params()[1].clone()))
            }
            LuaType::TableGeneric(params) if params.len() == 2 => {
                Some(OwnedValue::plain(params[1].clone()))
            }
            _ => None,
        })
    }
    pub(super) fn flow_table_member(
        &mut self,
        value: &OwnedValue,
        key: &LuaMemberKey,
    ) -> QueryResult<Option<OwnedValue>, E> {
        let LuaType::TableConst(table) = value.structural_type() else {
            return self.member_type(value, key);
        };
        let owner = SemanticId::member(table.file_id, table.value);
        let member = self
            .model
            .members_of_owner(&owner)
            .iter()
            .find(|member| match key {
                LuaMemberKey::Name(name) => member.name.as_str() == name.as_str(),
                _ => false,
            })
            .cloned();
        let Some(member) = member else {
            return Ok(None);
        };
        let Some(syntax) = self
            .model
            .file_facts_of(member.file_id)
            .and_then(|facts| facts.member_by_id(&member.id))
            .and_then(|def| def.value_syntax)
        else {
            return Ok(None);
        };
        self.checkpoint()?;
        if let Some(LuaExpr::LiteralExpr(literal)) = self
            .model
            .syntax_tree_of(member.file_id)
            .and_then(|tree| syntax.to_node_from_root(&tree.get_red_root()))
            .and_then(LuaExpr::cast)
            && let Some(value) = super::guards::literal_type(&literal)
        {
            return Ok(Some(OwnedValue::plain(value)));
        }
        match self.type_of_member(&member.id)? {
            Some(value) => Ok(Some(value)),
            None => self.type_of_expr(syntax).map(Some),
        }
    }
    pub(super) fn flow_overlap(
        &mut self,
        left: OwnedValue,
        right: OwnedValue,
    ) -> QueryResult<bool, E> {
        if matches!(left.structural_type(), LuaType::Unknown | LuaType::Any)
            || matches!(right.structural_type(), LuaType::Unknown | LuaType::Any)
        {
            return Ok(true);
        }
        let value = self.flow_intersect(left.clone(), right.clone())?;
        if !value.structural_type().is_unknown() {
            return Ok(!value.structural_type().is_never());
        }
        Ok(matches!(
            (left.structural_type(), right.structural_type()),
            (LuaType::Integer, LuaType::Number)
                | (LuaType::Number, LuaType::Integer)
                | (
                    LuaType::IntegerConst(_) | LuaType::DocIntegerConst(_),
                    LuaType::Integer | LuaType::Number
                )
                | (
                    LuaType::Integer | LuaType::Number,
                    LuaType::IntegerConst(_) | LuaType::DocIntegerConst(_)
                )
                | (
                    LuaType::StringConst(_) | LuaType::DocStringConst(_),
                    LuaType::String
                )
                | (
                    LuaType::String,
                    LuaType::StringConst(_) | LuaType::DocStringConst(_)
                )
                | (
                    LuaType::BooleanConst(_) | LuaType::DocBooleanConst(_),
                    LuaType::Boolean
                )
                | (
                    LuaType::Boolean,
                    LuaType::BooleanConst(_) | LuaType::DocBooleanConst(_)
                )
        ))
    }
}
pub(super) fn static_member_key(index: &emmylua_parser::LuaIndexExpr) -> Option<LuaMemberKey> {
    Some(match index.get_index_key()? {
        emmylua_parser::LuaIndexKey::Name(name) => LuaMemberKey::Name(name.get_name_text().into()),
        emmylua_parser::LuaIndexKey::String(value) => LuaMemberKey::Name(value.get_value().into()),
        emmylua_parser::LuaIndexKey::Integer(number) => {
            LuaMemberKey::Integer(match number.get_number_value() {
                emmylua_parser::NumberResult::Int(v) => v,
                emmylua_parser::NumberResult::Uint(v) => v as i64,
                _ => return None,
            })
        }
        emmylua_parser::LuaIndexKey::Idx(index) => LuaMemberKey::Integer(index as i64),
        _ => return None,
    })
}
