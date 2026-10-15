//! Iteration-variable inference adapted from fixed SemanticModel/types.rs.
//! Query order, current-model cross-file expression behavior and constant kinds
//! match the old producer; selected members stay in their owned carriers.
use super::{OwnedQuery, QueryResult, call_unify::Bindings};
use crate::session::{OwnedValue, SelectedProjectionBackend};
use emmylua_code_analysis::{
    Decl, GenericTplId, LuaMemberKey, LuaType, LuaTypeDeclId, SemanticId, VariadicType,
};
use emmylua_parser::{LuaAstNode, LuaCallExpr, LuaDocType, LuaExpr, LuaForRangeStat};

impl<E> OwnedQuery<'_, '_, E> {
    pub(super) fn infer_owned_iteration(
        &mut self,
        decl: &Decl,
    ) -> QueryResult<Option<OwnedValue>, E> {
        self.checkpoint()?;
        let Some(owner) = decl.owner_syntax else {
            return Ok(None);
        };
        let Some(stat) = self
            .model
            .syntax_tree()
            .and_then(|tree| owner.to_node_from_root(&tree.get_red_root()))
            .and_then(LuaForRangeStat::cast)
        else {
            return Ok(None);
        };
        let vars = stat.get_var_name_list().collect::<Vec<_>>();
        let Some(index) = vars
            .iter()
            .position(|var| var.get_name_text() == decl.name.as_str())
        else {
            return Ok(None);
        };
        let Some(expr) = stat.get_expr_list().next() else {
            return Ok(None);
        };
        if let LuaExpr::CallExpr(call) = &expr {
            let Some(prefix) = call.get_prefix_expr() else {
                return Ok(None);
            };
            if let LuaExpr::NameExpr(name) = &prefix
                && let Some(name) = name.get_name_text()
                && matches!(name.as_str(), "pairs" | "ipairs" | "next")
            {
                let Some(arg) = call.get_args_list().and_then(|list| list.get_args().next()) else {
                    return Ok(None);
                };
                let value = self.type_of_expr(arg.get_syntax_id())?;
                let key = LuaMemberKey::Name(
                    if name.as_str() == "ipairs" {
                        "__ipairs"
                    } else {
                        "__pairs"
                    }
                    .into(),
                );
                if let Some(generator) = self.member_type(&value, &key)?
                    && matches!(generator.structural_type(), LuaType::DocFunction(_))
                    && let Some(slot) = generator
                        .function_return()
                        .expect("generator return")
                        .result_slot(index)
                {
                    return Ok(Some(slot));
                }
                return self.standard_owned_iteration(prefix, &value, index);
            }
        }
        let mut iterator = if let LuaExpr::CallExpr(call) = &expr {
            match self.infer_owned_call_bindings(call.get_syntax_id())? {
                Some((value, _)) => value,
                None => self.type_of_expr(expr.get_syntax_id())?,
            }
        } else {
            self.type_of_expr(expr.get_syntax_id())?
        };
        if let LuaType::Ref(id) | LuaType::Def(id) = iterator.structural_type()
            && let Some(def) = self.model.type_def_of(id)
        {
            if let Some(alias) = self.model.alias_target(&def) {
                iterator = OwnedValue::plain(alias);
            } else if let Some(syntax) = def.call_overloads.first() {
                self.checkpoint()?;
                let overload =
                    self.model
                        .doc_type_lua_in(def.file_id, *syntax, &def.generic_params);
                if !matches!(overload, LuaType::Unknown) {
                    iterator = OwnedValue::plain(overload);
                }
            }
        }
        if let LuaType::Variadic(v) = iterator.structural_type()
            && let VariadicType::Multi(values) = v.as_ref()
        {
            if let Some((i, value)) = values
                .iter()
                .enumerate()
                .find(|(_, value)| matches!(value, LuaType::DocFunction(_)))
            {
                iterator = self.owned_child(&iterator, i, value);
            }
        }
        if !matches!(iterator.structural_type(), LuaType::DocFunction(_)) {
            return Ok(None);
        }
        let ret = if let LuaExpr::CallExpr(call) = &expr {
            let mut args = Vec::new();
            if let Some(list) = call.get_args_list() {
                for arg in list.get_args() {
                    args.push(self.type_of_expr(arg.get_syntax_id())?);
                }
            }
            let mut bindings = Bindings::new();
            for (param, arg) in iterator
                .function_parameters()
                .expect("function params")
                .iter()
                .zip(&args)
            {
                if let Some(param) = param {
                    let _ = self.unify_structural(param, arg, &mut bindings)?;
                }
            }
            self.substitute_owned(
                iterator.function_return().expect("function return"),
                &bindings,
            )?
        } else {
            iterator.function_return().expect("function return")
        };
        let Some(slot) = ret.result_slot(index) else {
            return Ok(None);
        };
        if index == 0 {
            self.iteration_remove_nil(slot).map(Some)
        } else {
            Ok(Some(slot))
        }
    }

    fn standard_owned_iteration(
        &mut self,
        callee: LuaExpr,
        argument: &OwnedValue,
        index: usize,
    ) -> QueryResult<Option<OwnedValue>, E> {
        self.checkpoint()?;
        let LuaExpr::NameExpr(name) = callee else {
            return Ok(None);
        };
        let Some(text) = name.get_name_text() else {
            return Ok(None);
        };
        if !matches!(text.as_str(), "pairs" | "ipairs" | "next") {
            return Ok(None);
        }
        if let LuaType::Ref(id) | LuaType::Def(id) = argument.structural_type()
            && let Some(value) = self.iteration_from_type(id, index)?
        {
            return Ok(Some(value));
        }
        let Some(decl) = self.model.resolve_name(name.get_position()) else {
            return Ok(None);
        };
        let file = match &decl {
            SemanticId::Decl(key) => key.file_id,
            _ => self.model.file_id(),
        };
        let Some(facts) = self.model.file_facts_of(file) else {
            return Ok(None);
        };
        let Some(syntax) = facts
            .decl_by_id(&decl)
            .and_then(|decl| decl.value_expr_syntax)
        else {
            return Ok(None);
        };
        let Some(docs) = facts
            .signature_by_closure(syntax)
            .and_then(|sig| sig.docs.as_ref())
        else {
            return Ok(None);
        };
        if docs.generic_params.len() < 2 {
            return Ok(None);
        }
        let mut value = argument.clone();
        let mut visited = Vec::new();
        loop {
            self.checkpoint()?;
            let (LuaType::Ref(id) | LuaType::Def(id)) = value.structural_type() else {
                break;
            };
            if visited.contains(id) {
                break;
            }
            visited.push(id.clone());
            let Some(def) = self.model.type_def_of(id) else {
                break;
            };
            let mut target = self.model.alias_target(&def);
            if target
                .as_ref()
                .is_none_or(|value| matches!(value, LuaType::Table))
                && let Some(syntax) = def.alias_type
            {
                let rich = self.model.doc_type_lua_rich_in(def.file_id, syntax);
                if !matches!(rich, LuaType::Unknown) {
                    target = Some(rich);
                }
            }
            let Some(target) = target else {
                break;
            };
            value = OwnedValue::plain(target);
        }
        let (key, element) = match value.structural_type() {
            LuaType::Any => (
                OwnedValue::plain(LuaType::Any),
                OwnedValue::plain(LuaType::Any),
            ),
            LuaType::TableConst(table) => {
                let owner = SemanticId::member(table.file_id, table.value);
                let mut keys = Vec::new();
                let mut values = Vec::new();
                for reference in self.model.members_of_owner(&owner) {
                    self.checkpoint()?;
                    let Some(member) = self
                        .model
                        .file_facts_of(reference.file_id)
                        .and_then(|facts| facts.member_by_id(&reference.id))
                    else {
                        continue;
                    };
                    let mut computed = None;
                    if matches!(member.key, LuaMemberKey::Name(_))
                        && let Some(range) = member.id.member_key_range()
                        && let Some(tree) = self.model.syntax_tree_of(reference.file_id)
                    {
                        for expr in tree.get_chunk_node().descendants::<LuaExpr>() {
                            self.checkpoint()?;
                            if expr.get_range() == range {
                                let ty = self.type_of_expr(expr.get_syntax_id())?;
                                if !matches!(ty.structural_type(), LuaType::Unknown) {
                                    computed = Some(ty);
                                }
                                break;
                            }
                        }
                    }
                    let key = if let Some(key) = computed {
                        key
                    } else {
                        OwnedValue::plain(match &member.key {
                            LuaMemberKey::Integer(i) => LuaType::IntegerConst(*i),
                            LuaMemberKey::Name(name) => LuaType::StringConst(name.clone().into()),
                            _ => continue,
                        })
                    };
                    let element = if let Some(syntax) = member.value_syntax {
                        self.type_of_expr(syntax)?
                    } else {
                        self.type_of_member(&reference.id)?
                            .unwrap_or_else(|| OwnedValue::plain(LuaType::Unknown))
                    };
                    let element = match element.structural_type() {
                        LuaType::IntegerConst(i) => OwnedValue::plain(LuaType::DocIntegerConst(*i)),
                        LuaType::StringConst(s) => {
                            OwnedValue::plain(LuaType::DocStringConst(s.clone()))
                        }
                        _ => element,
                    };
                    if !keys.contains(&key) {
                        keys.push(key);
                    }
                    if !values.contains(&element) {
                        values.push(element);
                    }
                }
                (self.iteration_union(keys)?, self.iteration_union(values)?)
            }
            LuaType::Ref(id) | LuaType::Def(id) => {
                let Some(def) = self.model.type_def_of(id) else {
                    return Ok(None);
                };
                let mut values = Vec::new();
                for reference in self.model.members_of_owner(&def.id) {
                    self.checkpoint()?;
                    let Some(member) = self
                        .model
                        .file_facts_of(reference.file_id)
                        .and_then(|facts| facts.member_by_id(&reference.id))
                    else {
                        continue;
                    };
                    if !matches!(member.key, LuaMemberKey::Integer(_)) && !member.is_index_signature
                    {
                        continue;
                    }
                    let value = self
                        .type_of_member(&reference.id)?
                        .unwrap_or_else(|| OwnedValue::plain(LuaType::Unknown));
                    if !values.contains(&value) {
                        values.push(value);
                    }
                }
                if values.is_empty() {
                    return Ok(None);
                }
                (
                    OwnedValue::plain(LuaType::Integer),
                    self.iteration_union(values)?,
                )
            }
            LuaType::Object(object) => {
                let element = if let Some((key, element)) = object.get_fields().iter().next() {
                    self.owned_field(&value, key, element)
                } else if let Some((_, element)) = object.get_index_access().first() {
                    self.owned_index(&value, 0, false, element)
                } else {
                    OwnedValue::plain(LuaType::Unknown)
                };
                (OwnedValue::plain(LuaType::String), element)
            }
            LuaType::Array(array) => (
                OwnedValue::plain(LuaType::Integer),
                self.owned_child(&value, 0, array.get_base()),
            ),
            LuaType::Generic(generic) if generic.get_base_type_id().get_name() == "table" => {
                let params = generic.get_params();
                (
                    params
                        .first()
                        .map(|v| self.owned_child(&value, 0, v))
                        .unwrap_or_else(|| OwnedValue::plain(LuaType::Unknown)),
                    params
                        .get(1)
                        .map(|v| self.owned_child(&value, 1, v))
                        .unwrap_or_else(|| OwnedValue::plain(LuaType::Unknown)),
                )
            }
            _ => return Ok(None),
        };
        let mut bindings = Bindings::new();
        bindings.insert(GenericTplId::Type(0), key);
        bindings.insert(GenericTplId::Type(1), element);
        let Some(syntax) = docs.returns.first() else {
            return Ok(None);
        };
        let Some(LuaDocType::Func(function)) = self
            .model
            .syntax_tree_of(file)
            .and_then(|tree| syntax.to_node_from_root(&tree.get_red_root()))
            .and_then(LuaDocType::cast)
        else {
            return Ok(None);
        };
        let Some(list) = function.get_return_type_list() else {
            return Ok(None);
        };
        let mut slots = Vec::new();
        for ret in list.get_return_type_list() {
            self.checkpoint()?;
            if let (_, Some(ty)) = ret.get_name_and_type() {
                slots.push(OwnedValue::plain(self.model.doc_type_lua_in(
                    file,
                    ty.get_syntax_id(),
                    &docs.generic_params,
                )));
            }
        }
        let Some(slot) = slots.get(index) else {
            return Ok(None);
        };
        let value = self.substitute_owned(slot.clone(), &bindings)?;
        Ok((!matches!(value.structural_type(), LuaType::Unknown)).then_some(value))
    }

    fn iteration_from_type(
        &mut self,
        id: &LuaTypeDeclId,
        index: usize,
    ) -> QueryResult<Option<OwnedValue>, E> {
        self.checkpoint()?;
        let Some(def) = self.model.type_def_of(id) else {
            return Ok(None);
        };
        let mut values = Vec::new();
        for reference in self.model.members_of_owner(&def.id) {
            self.checkpoint()?;
            let Some(member) = self
                .model
                .file_facts_of(reference.file_id)
                .and_then(|facts| facts.member_by_id(&reference.id))
            else {
                return Ok(None);
            };
            if !matches!(member.key, LuaMemberKey::Integer(_)) && !member.is_index_signature {
                continue;
            }
            let value = self
                .type_of_member(&reference.id)?
                .unwrap_or_else(|| OwnedValue::plain(LuaType::Unknown));
            if !values.contains(&value) {
                values.push(value);
            }
        }
        if values.is_empty() {
            return Ok(None);
        }
        let value = self.iteration_union(values)?;
        Ok(Some(if index == 0 {
            OwnedValue::plain(LuaType::Integer)
        } else {
            value
        }))
    }
    fn iteration_union(&mut self, values: Vec<OwnedValue>) -> QueryResult<OwnedValue, E> {
        if values.is_empty() {
            Ok(OwnedValue::plain(LuaType::Unknown))
        } else {
            OwnedValue::union_checked(values, || self.checkpoint())
        }
    }
    fn iteration_remove_nil(&mut self, value: OwnedValue) -> QueryResult<OwnedValue, E> {
        self.checkpoint()?;
        if let Some(parts) = value.union_components() {
            self.iteration_union(
                parts
                    .into_iter()
                    .filter(|v| !matches!(v.structural_type(), LuaType::Nil))
                    .collect(),
            )
        } else if matches!(value.structural_type(), LuaType::Nil) {
            Ok(OwnedValue::plain(LuaType::Unknown))
        } else {
            Ok(value)
        }
    }
}
