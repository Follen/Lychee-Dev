//! Current-model signature producer. Its parameter fallbacks and lack of a raw
//! closure-return retry intentionally differ from the cross-file producer.
use super::*;
use emmylua_code_analysis::LuaTypeNode;

impl<E> OwnedQuery<'_, '_, E> {
    pub(in crate::session::backend) fn parameter_type(
        &mut self,
        closure: LuaSyntaxId,
        index: usize,
    ) -> QueryResult<Option<OwnedValue>, E> {
        self.checkpoint()?;
        let Some(signature) = self
            .model
            .file_facts()
            .and_then(|facts| facts.signature_by_closure(closure))
        else {
            return Ok(None);
        };
        let syntax = signature.docs.as_ref().and_then(|docs| {
            let name = signature.param_names.get(index)?;
            docs.param_types
                .iter()
                .find(|(param, _)| param == name)
                .map(|(_, syntax)| *syntax)
        });
        if let Some(syntax) = syntax {
            let generics = signature
                .docs
                .as_ref()
                .map(|docs| docs.generic_params.as_slice())
                .unwrap_or(&[]);
            let value = self
                .model
                .doc_type_lua_in(self.model.file_id(), syntax, generics);
            self.checkpoint()?;
            if !value.is_unknown() {
                return Ok(Some(OwnedValue::plain(value)));
            }
        }
        self.expected_member_param_for_closure(closure, index)
    }

    pub(in crate::session::backend) fn signature_local(
        &mut self,
        closure: LuaSyntaxId,
    ) -> QueryResult<Option<OwnedValue>, E> {
        self.checkpoint()?;
        let Some(signature) = self
            .model
            .file_facts()
            .and_then(|facts| facts.signature_by_closure(closure))
        else {
            return Ok(None);
        };
        let file = self.model.file_id();
        let generic_docs = signature
            .docs
            .as_ref()
            .map(|docs| docs.generic_params.as_slice())
            .unwrap_or(&[]);
        let generic_params = self.model.generic_tpls_with_metadata(file, generic_docs);
        self.checkpoint()?;
        let mut params = Vec::new();
        let mut carried = Vec::new();
        for (index, name) in signature.param_names.iter().enumerate() {
            self.checkpoint()?;
            let mut value = self
                .parameter_type(closure, index)?
                .unwrap_or_else(|| OwnedValue::plain(LuaType::Unknown));
            if matches!(value.structural_type(), LuaType::Unknown | LuaType::Table)
                && let Some(syntax) = signature.docs.as_ref().and_then(|docs| {
                    docs.param_types
                        .iter()
                        .find(|(param, _)| param == name)
                        .map(|(_, syntax)| *syntax)
                })
            {
                let rich = self.model.doc_type_lua_rich_in(file, syntax);
                self.checkpoint()?;
                if !rich.is_unknown() {
                    value = OwnedValue::plain(rich);
                }
            }
            if value.structural_type().is_unknown()
                && let Some(expected) = self.expected_member_param_for_closure(closure, index)?
            {
                value = expected;
            }
            value = self.expand_aliases(value)?;
            if signature
                .docs
                .as_ref()
                .is_some_and(|docs| docs.nullable_params.iter().any(|param| param == name))
                && !value.structural_type().is_nullable()
            {
                value = OwnedValue::raw_union(vec![value, OwnedValue::plain(LuaType::Nil)]);
            }
            params.push((name.to_string(), Some(value.structural_type().clone())));
            carried.push(Some(value));
        }
        if signature.is_variadic {
            let functions = if let Some(docs) = signature
                .docs
                .as_ref()
                .filter(|docs| !docs.overloads.is_empty())
            {
                let mut functions = Vec::new();
                for syntax in &docs.overloads {
                    self.checkpoint()?;
                    let ty = self.model.doc_type_lua_rich_in(file, *syntax);
                    self.checkpoint()?;
                    if matches!(ty, LuaType::DocFunction(_)) {
                        functions.push(OwnedValue::plain(ty));
                    }
                }
                functions
            } else {
                self.expected_member_signatures_for_closure(closure)?
                    .unwrap_or_default()
            };
            if !functions.is_empty() {
                let has_self = functions.iter().any(|value| match value.structural_type() {
                    LuaType::DocFunction(function) => {
                        function.get_params().first().is_some_and(|(name, ty)| {
                            name == "self" || matches!(ty, Some(LuaType::SelfInfer))
                        })
                    }
                    _ => false,
                });
                let start = usize::from(has_self);
                let max_len = functions
                    .iter()
                    .filter_map(|value| match value.structural_type() {
                        LuaType::DocFunction(function) => {
                            Some(function.get_params().len().saturating_sub(start))
                        }
                        _ => None,
                    })
                    .max()
                    .unwrap_or(0);
                let mut slots = Vec::new();
                for slot in 0..max_len {
                    let mut parts = Vec::new();
                    for function in &functions {
                        self.checkpoint()?;
                        if let Some(value) = function
                            .function_parameters()
                            .and_then(|params| params.get(start + slot).cloned().flatten())
                        {
                            let value = self.expand_aliases(value)?;
                            let value = if value
                                .structural_type()
                                .any_type(|ty| matches!(ty, LuaType::Any))
                            {
                                OwnedValue::plain(LuaType::Any)
                            } else {
                                value
                            };
                            if !parts.contains(&value) {
                                parts.push(value);
                            }
                        }
                    }
                    if parts
                        .iter()
                        .any(|value| matches!(value.structural_type(), LuaType::Any))
                    {
                        parts = vec![OwnedValue::plain(LuaType::Any)];
                    }
                    slots.push(OwnedValue::union_checked(parts, || self.checkpoint())?);
                }
                if let Some(index) = params.iter().position(|(name, _)| name == "...") {
                    let value = OwnedValue::pack(slots);
                    params[index].1 = Some(value.structural_type().clone());
                    carried[index] = Some(value);
                }
            }
        }
        let ret = self.contextual_signature_return(file, signature)?;
        let ret = self.expand_aliases(ret)?;
        let metadata = LuaFunctionType::new(
            if signature.docs.as_ref().is_some_and(|docs| docs.is_async) {
                AsyncState::Async
            } else {
                AsyncState::None
            },
            signature.is_method,
            signature.is_variadic,
            params,
            ret.structural_type().clone(),
            Some(generic_params),
        );
        self.checkpoint()?;
        Ok(Some(OwnedValue::function(&metadata, carried, ret)))
    }
}
