//! Declaration priority and documented types. Selected initializer probes occur
//! before annotation/parameter/module/class handling, as in the fixed producer.
use super::*;
use emmylua_code_analysis::{GenericTpl, GenericTplId, LuaAliasCallKind, TypeDefKind};
use emmylua_parser::{LuaAssignStat, LuaDocType, LuaVarExpr};

impl<E> OwnedQuery<'_, '_, E> {
    pub(super) fn declaration_body(
        &mut self,
        declaration: &SemanticId,
    ) -> QueryResult<Option<OwnedValue>, E> {
        self.checkpoint()?;
        if let Some(facts) = self.model.file_facts()
            && let Some(info) = facts.decl_by_id(declaration)
        {
            if let Some(syntax) = info.value_expr_syntax
                && self.manages_initializer(syntax)?
            {
                let ty = self.type_of_expr(syntax)?;
                return Ok(Some(Self::initializer_slot(ty, info.multi_return_index)));
            }
            if info.name == "self"
                && info.kind == DeclKind::Param
                && let Some(closure) = info.owner_syntax.filter(|closure| {
                    facts
                        .signature_by_closure(*closure)
                        .is_some_and(|signature| signature.is_method)
                })
                && let Some(value) = self.method_owner_type(closure)?
            {
                return Ok(Some(value));
            }
            if matches!(info.kind, DeclKind::Local { is_iter: true, .. })
                && let Some(value) = self.infer_owned_iteration(info)?
                && !matches!(value.structural_type(), LuaType::Unknown)
            {
                return Ok(Some(value));
            }
            if let Some(syntax) = info.doc_type_syntax {
                if let Some(value) = self.declaration_annotation(info, syntax)? {
                    return Ok(Some(value));
                }
            }
            if let Some(path) = &info.module_path
                && let Some(file) = self.model.module_file_of(path)
            {
                return Ok(Some(OwnedValue::plain(LuaType::ModuleRef(file))));
            }
            if info.delayed_definition
                && let Some(value) = self.delayed_definition_type(info)?
            {
                return Ok(Some(value));
            }
            if let Some(def) = info
                .owner_syntax
                .and_then(|owner| facts.type_def_by_owner_syntax(owner))
                .filter(|def| matches!(def.kind, TypeDefKind::Class | TypeDefKind::Enum))
            {
                return Ok(Some(OwnedValue::plain(self.model.type_def_ref(def))));
            }
            if info.kind == DeclKind::Param
                && let Some((signature, index)) = facts.signature_and_param_index_of_decl(info)
            {
                // closure_param_vm consults the wrapping call before falling
                // back to the declared parameter; that producer remains owned.
                let closure = signature.closure_syntax;
                if let Some(value) = self.closure_parameter(closure, index)? {
                    return self.attach_generic_constraints(value, closure).map(Some);
                }
                if let Some(value) = self.expected_member_param_for_closure(closure, index)? {
                    let value = if let Some(owner) = self.method_owner_type(closure)? {
                        self.replace_owned_self(value, &owner)?
                    } else {
                        value
                    };
                    return self.attach_generic_constraints(value, closure).map(Some);
                }
            }
            if let Some(syntax) = info.value_expr_syntax {
                let node = self
                    .model
                    .syntax_tree()
                    .and_then(|tree| syntax.to_node_from_root(&tree.get_red_root()));
                if let Some(closure) = node.clone().and_then(LuaClosureExpr::cast)
                    && let Some(value) = self.signature_local(closure.get_syntax_id())?
                {
                    return Ok(Some(value));
                }
                let value = if node.and_then(LuaIndexExpr::cast).is_some() {
                    self.type_of_expr_at(syntax, syntax.get_range().start().into())?
                } else {
                    self.type_of_expr(syntax)?
                };
                return Ok(Some(Self::initializer_slot(value, info.multi_return_index)));
            }
            // With a selected session, every initializer returned above. The
            // remaining shell has no executable expression to infer. Param
            // docs use signature generics and nullable exactly as the shell.
            return self.uninitialized_declaration_shell(info).map(Some);
        }
        let file = match declaration {
            SemanticId::Decl(key) => key.file_id,
            _ => self.model.file_id(),
        };
        if file != self.model.file_id() {
            let mut foreign = OwnedQuery::new(self.analysis, self.session, file, &mut *self.check);
            foreign.contribution_order = self.contribution_order.clone();
            foreign.workspace_facts = Arc::clone(&self.workspace_facts);
            foreign.flow_trees = Rc::clone(&self.flow_trees);
            foreign.initializer_selection = std::rc::Rc::clone(&self.initializer_selection);
            if let Some(value) = foreign.type_of_decl(declaration)? {
                return Ok(Some(value));
            }
        }
        // Missing file/fact behavior belongs to the public immutable projection;
        // there is no declaration body left which could read selected context.
        self.checkpoint()?;
        let value = self.model.type_of_decl(declaration).map(OwnedValue::plain);
        self.checkpoint()?;
        Ok(value)
    }

    fn declaration_annotation(
        &mut self,
        info: &Decl,
        syntax: LuaSyntaxId,
    ) -> QueryResult<Option<OwnedValue>, E> {
        self.checkpoint()?;
        let ty = self
            .raw_declaration_type(self.model.file_id(), &info.id)?
            .unwrap_or_else(|| OwnedValue::plain(LuaType::Unknown))
            .structural_type()
            .clone();
        self.checkpoint()?;
        if !ty.is_unknown() {
            let bare = self
                .model
                .syntax_tree()
                .and_then(|tree| syntax.to_node_from_root(&tree.get_red_root()))
                .and_then(LuaDocType::cast)
                .is_some_and(|doc| matches!(doc, LuaDocType::Name(_)));
            if let LuaType::Ref(id) | LuaType::Def(id) = &ty
                && bare
                && self.model.type_def_of(id).is_some_and(|def| {
                    !def.generic_params.is_empty()
                        && def
                            .generic_params
                            .iter()
                            .any(|param| param.default.is_none())
                })
            {
                return Ok(Some(OwnedValue::plain(LuaType::Any)));
            }
            if self.alias_cycle(&ty)? {
                return Ok(Some(OwnedValue::plain(LuaType::Any)));
            }
            let expanded = self.expand_aliases(OwnedValue::plain(ty.clone()))?;
            if matches!(expanded.structural_type(), LuaType::Unknown | LuaType::Any) {
                return Ok(Some(expanded));
            }
            let rich = self
                .model
                .doc_type_lua_rich_in(self.model.file_id(), syntax);
            self.checkpoint()?;
            if !rich.is_unknown() && rich != ty {
                let imprecise = matches!(&rich, LuaType::Call(call) if call.get_call_kind() == LuaAliasCallKind::Index);
                if !imprecise || matches!(ty, LuaType::Table | LuaType::Unknown) {
                    return Ok(Some(OwnedValue::plain(rich)));
                }
            }
            if ty == LuaType::Table && !rich.is_unknown() {
                return Ok(Some(OwnedValue::plain(rich)));
            }
            if matches!(
                ty,
                LuaType::String | LuaType::Number | LuaType::Integer | LuaType::Boolean
            ) {
                let rich = self
                    .model
                    .doc_type_lua_rich_in(self.model.file_id(), syntax);
                if matches!(
                    rich,
                    LuaType::Union(_)
                        | LuaType::StringConst(_)
                        | LuaType::DocStringConst(_)
                        | LuaType::IntegerConst(_)
                        | LuaType::DocIntegerConst(_)
                        | LuaType::BooleanConst(_)
                        | LuaType::DocBooleanConst(_)
                ) {
                    return Ok(Some(OwnedValue::plain(rich)));
                }
            }
            return Ok(Some(OwnedValue::plain(ty)));
        }
        let rich = self
            .model
            .doc_type_lua_rich_in(self.model.file_id(), syntax);
        self.checkpoint()?;
        Ok((!rich.is_unknown()).then(|| OwnedValue::plain(rich)))
    }

    fn alias_cycle(&mut self, ty: &LuaType) -> QueryResult<bool, E> {
        let mut current = ty.clone();
        let mut visited = Vec::new();
        loop {
            self.checkpoint()?;
            let (LuaType::Ref(id) | LuaType::Def(id)) = &current else {
                return Ok(false);
            };
            let Some(def) = self.model.type_def_of(id) else {
                return Ok(false);
            };
            if def.kind != TypeDefKind::Alias {
                return Ok(false);
            }
            if visited.contains(id) {
                return Ok(true);
            }
            visited.push(id.clone());
            let Some(target) = self.model.alias_target(&def) else {
                return Ok(false);
            };
            current = target;
        }
    }
    fn delayed_definition_type(&mut self, info: &Decl) -> QueryResult<Option<OwnedValue>, E> {
        let Some(chunk) = self.model.chunk() else {
            return Ok(None);
        };
        let mut values = Vec::new();
        for assignment in chunk.descendants::<LuaAssignStat>() {
            self.checkpoint()?;
            let (vars, exprs) = assignment.get_var_and_expr_list();
            for (var, expr) in vars.into_iter().zip(exprs) {
                self.checkpoint()?;
                let LuaVarExpr::NameExpr(name) = var else {
                    continue;
                };
                if name.get_name_text().as_deref() != Some(info.name.as_str())
                    || name.get_position() <= info.name_offset()
                    || self.model.resolve_name(name.get_position()).as_ref() != Some(&info.id)
                {
                    continue;
                }
                let value = self.type_of_expr(expr.get_syntax_id())?;
                let value = match value.structural_type() {
                    LuaType::StringConst(_) | LuaType::DocStringConst(_) => {
                        OwnedValue::plain(LuaType::String)
                    }
                    LuaType::IntegerConst(_) | LuaType::DocIntegerConst(_) => {
                        OwnedValue::plain(LuaType::Integer)
                    }
                    _ => value,
                };
                if !matches!(value.structural_type(), LuaType::Unknown | LuaType::Any)
                    && !values.contains(&value)
                {
                    values.push(value);
                }
            }
        }
        if values.is_empty() {
            Ok(None)
        } else {
            OwnedValue::union_checked(values, || self.checkpoint()).map(Some)
        }
    }
    fn uninitialized_declaration_shell(&mut self, info: &Decl) -> QueryResult<OwnedValue, E> {
        self.checkpoint()?;
        if info.kind == DeclKind::Param
            && let Some((signature, _)) = self
                .model
                .file_facts()
                .and_then(|facts| facts.signature_and_param_index_of_decl(info))
            && let Some(docs) = &signature.docs
            && let Some((_, syntax)) = docs.param_types.iter().find(|(name, _)| name == &info.name)
        {
            let ty =
                self.model
                    .doc_type_lua_in(self.model.file_id(), *syntax, &docs.generic_params);
            if !ty.is_unknown() {
                return Ok(
                    if docs.nullable_params.iter().any(|name| name == &info.name) {
                        OwnedValue::raw_union(vec![
                            OwnedValue::plain(ty),
                            OwnedValue::plain(LuaType::Nil),
                        ])
                    } else {
                        OwnedValue::plain(ty)
                    },
                );
            }
        }
        Ok(OwnedValue::plain(LuaType::Unknown))
    }
    pub(super) fn attach_generic_constraints(
        &mut self,
        value: OwnedValue,
        closure: LuaSyntaxId,
    ) -> QueryResult<OwnedValue, E> {
        self.checkpoint()?;
        let Some(docs) = self
            .model
            .file_facts()
            .and_then(|facts| facts.signature_by_closure(closure))
            .and_then(|signature| signature.docs.as_ref())
        else {
            return Ok(value);
        };
        let (id, name, default, is_const) = match value.structural_type() {
            LuaType::TplRef(tpl)
                if docs
                    .generic_params
                    .iter()
                    .any(|param| param.name.as_str() == tpl.get_name()) =>
            {
                (
                    tpl.get_tpl_id(),
                    tpl.get_name(),
                    tpl.get_default_type().cloned(),
                    tpl.is_const(),
                )
            }
            LuaType::Ref(id) => {
                let Some(index) = docs
                    .generic_params
                    .iter()
                    .position(|param| param.name.as_str() == id.get_name())
                else {
                    return Ok(value);
                };
                (GenericTplId::Type(index as u32), id.get_name(), None, false)
            }
            _ => return Ok(value),
        };
        let param = docs
            .generic_params
            .iter()
            .find(|param| param.name.as_str() == name)
            .expect("matched signature generic");
        let constraint = param
            .constraint
            .map(|syntax| self.model.doc_type_lua(syntax));
        let ty = LuaType::TplRef(Arc::new(GenericTpl::new(
            id,
            param.name.clone(),
            constraint,
            default,
            is_const,
            None,
        )));
        self.checkpoint()?;
        Ok(OwnedValue::plain(ty))
    }
}
