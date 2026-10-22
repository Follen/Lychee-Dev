use super::*;
use emmylua_code_analysis::{
    AsyncState, GenericTpl, GenericTplId, InFiled, LuaAliasCallKind, LuaAliasCallType,
    LuaArrayType, LuaFunctionType, LuaGenericType, LuaObjectType, LuaStringTplType, LuaTupleStatus,
    LuaTupleType, LuaTypeDeclId, LuaUnionType, TypeVisibility, VariadicType,
};
use emmylua_parser::LuaTypeBinaryOperator;
use rowan::{TextRange, TextSize};
use std::sync::Arc;

impl<E> OwnedQuery<'_, '_, E> {
    pub(super) fn shell_doc(
        &mut self,
        file: FileId,
        syntax: LuaSyntaxId,
        generics: &[DocGenericParam],
    ) -> QueryResult<TypeShell, E> {
        self.checkpoint()?;
        let doc = self
            .model
            .syntax_tree_of(file)
            .and_then(|tree| syntax.to_node_from_root(&tree.get_red_root()))
            .and_then(LuaDocType::cast);
        match doc {
            Some(doc) => self.shell_doc_node(file, &doc, generics),
            None => Ok(TypeShell::unknown()),
        }
    }
    pub(super) fn shell_doc_node(
        &mut self,
        file: FileId,
        doc: &LuaDocType,
        generics: &[DocGenericParam],
    ) -> QueryResult<TypeShell, E> {
        self.checkpoint()?;
        Ok(match doc {
            LuaDocType::Name(name) => {
                if let Some(name) = name.get_name_text() {
                    if generics.iter().any(|g| g.name == name) {
                        TypeShell::from_generic(&name)
                    } else if let Some(shell) = primitive_named(&name) {
                        shell
                    } else if let Some(def) = self.model.resolve_type_def_in(file, &name) {
                        TypeShell::from_name(&def.full_name)
                    } else {
                        TypeShell::from_name(&name)
                    }
                } else {
                    TypeShell::unknown()
                }
            }
            LuaDocType::Literal(literal) => match literal.get_literal() {
                Some(LuaLiteralToken::String(s)) => {
                    TypeShell::from_literal(LiteralShell::String(s.get_value().into()))
                }
                Some(LuaLiteralToken::Number(n)) => match n.get_number_value() {
                    NumberResult::Int(i) => TypeShell::from_literal(LiteralShell::Integer(i)),
                    NumberResult::Uint(u) => {
                        TypeShell::from_literal(LiteralShell::Integer(u as i64))
                    }
                    NumberResult::Float(f) => {
                        TypeShell::from_literal(LiteralShell::Float(f.to_bits()))
                    }
                    NumberResult::Number => TypeShell::from_primitive(PrimitiveType::Number),
                },
                Some(LuaLiteralToken::Bool(b)) => {
                    TypeShell::from_literal(LiteralShell::Boolean(b.is_true()))
                }
                Some(LuaLiteralToken::Nil(_)) => TypeShell::from_literal(LiteralShell::Nil),
                _ => TypeShell::unknown(),
            },
            LuaDocType::Array(array) => match array.get_type() {
                Some(base) => TypeShell::from_array(self.shell_doc_node(file, &base, generics)?),
                None => TypeShell::from_primitive(PrimitiveType::Table),
            },
            LuaDocType::Variadic(pack) => match pack.get_type() {
                Some(base) => TypeShell::from_variadic(self.shell_doc_node(file, &base, generics)?),
                None => TypeShell::unknown(),
            },
            LuaDocType::Tuple(tuple) => {
                let mut values = Vec::new();
                for ty in tuple.get_types() {
                    values.push(self.shell_doc_node(file, &ty, generics)?);
                }
                TypeShell::from_tuple(values)
            }
            LuaDocType::Object(object) => {
                TypeShell::from_primitive(if object.get_fields().next().is_none() {
                    PrimitiveType::EmptyObject
                } else {
                    PrimitiveType::Table
                })
            }
            LuaDocType::Generic(generic) => {
                if let Some(name) = generic.get_name_type().and_then(|n| n.get_name_text()) {
                    let mut args = Vec::new();
                    if let Some(list) = generic.get_generic_types() {
                        for arg in list.get_types() {
                            args.push(self.shell_doc_node(file, &arg, generics)?);
                        }
                    }
                    TypeShell::from_generic_instance(&name, args)
                } else {
                    TypeShell::unknown()
                }
            }
            LuaDocType::StrTpl(tpl) => {
                let (prefix, name, suffix) = tpl.get_name();
                let index = name
                    .as_deref()
                    .and_then(|n| generics.iter().position(|g| g.name == n))
                    .map(|i| i as u32);
                TypeShell::from_str_tpl(
                    &prefix.unwrap_or_default(),
                    &name.unwrap_or_default(),
                    index,
                    &suffix.unwrap_or_default(),
                )
            }
            LuaDocType::Func(fun) => {
                let mut local = generics.to_vec();
                let mut declared = Vec::new();
                if let Some(list) = fun.get_generic_decl_list() {
                    for decl in list.get_generic_decl() {
                        self.checkpoint()?;
                        if let Some(token) = decl.get_name_token() {
                            let name = SmolStr::new(token.get_name_text());
                            local.push(DocGenericParam::new(
                                name.clone(),
                                None,
                                None,
                                false,
                                false,
                            ));
                            declared.push(name);
                        }
                    }
                }
                let mut params = Vec::new();
                let mut names = Vec::new();
                let mut variadic = false;
                for param in fun.get_params() {
                    self.checkpoint()?;
                    variadic |= param.is_dots();
                    names.push(
                        param
                            .get_name_token()
                            .map(|t| SmolStr::new(t.get_name_text()))
                            .or_else(|| param.is_dots().then(|| SmolStr::new("...")))
                            .unwrap_or_default(),
                    );
                    let shell = if let Some(ty) = param.get_type() {
                        let mut shell = self.shell_doc_node(file, &ty, &local)?;
                        if param.is_nullable() {
                            shell.merge(&TypeShell::from_primitive(PrimitiveType::Nil));
                        }
                        shell
                    } else {
                        TypeShell::unknown()
                    };
                    params.push(shell);
                }
                let mut returns = TypeShell::unknown();
                let mut slots = Vec::new();
                if let Some(list) = fun.get_return_type_list() {
                    for ret in list.get_return_type_list() {
                        if let (_, Some(ty)) = ret.get_name_and_type() {
                            let shell = self.shell_doc_node(file, &ty, &local)?;
                            returns.merge(&shell);
                            slots.push(shell);
                        }
                    }
                }
                TypeShell::from_function(
                    params,
                    names,
                    returns,
                    slots,
                    declared,
                    if fun.is_async() {
                        1
                    } else if fun.is_sync() {
                        2
                    } else {
                        0
                    },
                    false,
                    variadic,
                )
            }
            LuaDocType::Binary(binary) => {
                if binary.get_op_token().map(|t| t.get_op()) == Some(LuaTypeBinaryOperator::Union) {
                    if let Some((left, right)) = binary.get_types() {
                        let mut shell = self.shell_doc_node(file, &left, generics)?;
                        shell.merge(&self.shell_doc_node(file, &right, generics)?);
                        shell
                    } else {
                        TypeShell::unknown()
                    }
                } else {
                    TypeShell::unknown()
                }
            }
            LuaDocType::Nullable(nullable) => {
                if let Some(inner) = nullable.get_type() {
                    let mut shell = self.shell_doc_node(file, &inner, generics)?;
                    shell.merge(&TypeShell::from_primitive(PrimitiveType::Nil));
                    shell
                } else {
                    TypeShell::unknown()
                }
            }
            LuaDocType::MultiLineUnion(union) => {
                let mut shell = TypeShell::unknown();
                for field in union.get_fields() {
                    if let Some(ty) = field.get_type() {
                        shell.merge(&self.shell_doc_node(file, &ty, generics)?);
                    }
                }
                shell
            }
            _ => TypeShell::unknown(),
        })
    }
    pub(super) fn project_owned_shell(
        &mut self,
        file: FileId,
        shell: &TypeShell,
        generics: &[SmolStr],
    ) -> QueryResult<LuaType, E> {
        let mut types = Vec::new();
        for candidate in &shell.candidates {
            self.checkpoint()?;
            let ty = match candidate {
                TypeCandidate::Primitive(p) => primitive_type(*p),
                TypeCandidate::Named(name) => self.shell_resolve_named(file, name),
                TypeCandidate::Generic(name) => {
                    if let Some(index) = generics.iter().position(|n| n == name) {
                        LuaType::TplRef(Arc::new(GenericTpl::new(
                            GenericTplId::Type(index as u32),
                            name.clone(),
                            None,
                            None,
                            false,
                            None,
                        )))
                    } else {
                        LuaType::Ref(LuaTypeDeclId::global(name))
                    }
                }
                TypeCandidate::Function(fun) => {
                    let context = if fun.generic_params.is_empty() {
                        generics
                    } else {
                        &fun.generic_params
                    };
                    let mut params = Vec::new();
                    for (index, param) in fun.params.iter().enumerate() {
                        params.push((
                            fun.param_names
                                .get(index)
                                .map(ToString::to_string)
                                .unwrap_or_default(),
                            Some(self.project_owned_shell(file, param, context)?),
                        ));
                    }
                    let ret = if fun.returns_multi.len() > 1 {
                        let mut slots = Vec::new();
                        for ret in &fun.returns_multi {
                            slots.push(self.project_owned_shell(file, ret, context)?);
                        }
                        LuaType::Variadic(Arc::new(VariadicType::Multi(slots)))
                    } else {
                        self.project_owned_shell(file, &fun.returns, context)?
                    };
                    let templates = fun
                        .generic_params
                        .iter()
                        .enumerate()
                        .map(|(i, n)| {
                            GenericTpl::new(
                                GenericTplId::Type(i as u32),
                                n.clone(),
                                None,
                                None,
                                false,
                                None,
                            )
                        })
                        .collect();
                    LuaType::DocFunction(Arc::new(LuaFunctionType::new(
                        match fun.async_state {
                            1 => AsyncState::Async,
                            2 => AsyncState::Sync,
                            _ => AsyncState::None,
                        },
                        fun.is_colon_define,
                        fun.is_variadic,
                        params,
                        ret,
                        Some(templates),
                    )))
                }
                TypeCandidate::Table(id) => LuaType::TableConst(InFiled::new(
                    FileId::new(id.file_id),
                    TextRange::new(TextSize::from(id.start), TextSize::from(id.end)),
                )),
                TypeCandidate::Array(base) => LuaType::Array(Arc::new(
                    LuaArrayType::from_base_type(self.project_owned_shell(file, base, generics)?),
                )),
                TypeCandidate::Variadic(base) => LuaType::Variadic(Arc::new(VariadicType::Base(
                    self.project_owned_shell(file, base, generics)?,
                ))),
                TypeCandidate::Tuple(items) => {
                    let mut types = Vec::new();
                    for item in items {
                        types.push(self.project_owned_shell(file, item, generics)?);
                    }
                    LuaType::Tuple(Arc::new(LuaTupleType::new(
                        types,
                        LuaTupleStatus::DocResolve,
                    )))
                }
                TypeCandidate::Literal(literal) => match literal {
                    LiteralShell::String(s) => LuaType::StringConst(s.clone().into()),
                    LiteralShell::Integer(i) => LuaType::IntegerConst(*i),
                    LiteralShell::Float(bits) => LuaType::FloatConst(f64::from_bits(*bits)),
                    LiteralShell::Boolean(b) => LuaType::BooleanConst(*b),
                    LiteralShell::Nil => LuaType::Nil,
                },
                TypeCandidate::GenericInstance(instance) => {
                    let id = self.shell_named_id(file, &instance.name);
                    let mut params = Vec::new();
                    for arg in &instance.args {
                        params.push(self.project_owned_shell(file, arg, generics)?);
                    }
                    if instance.name == "TypeGuard" && !params.is_empty() {
                        LuaType::TypeGuard(Arc::new(params[0].clone()))
                    } else if instance.name == "std.Unpack" {
                        LuaType::Call(Arc::new(LuaAliasCallType::new(
                            LuaAliasCallKind::Unpack,
                            params,
                        )))
                    } else {
                        LuaType::Generic(Arc::new(LuaGenericType::new(id, params)))
                    }
                }
                TypeCandidate::StrTpl(tpl) => LuaType::StrTplRef(Arc::new(LuaStringTplType::new(
                    &tpl.prefix,
                    &tpl.name,
                    tpl.tpl_index
                        .map(GenericTplId::Type)
                        .unwrap_or(GenericTplId::ConditionalInfer(0)),
                    &tpl.suffix,
                    None,
                ))),
                TypeCandidate::ModuleRef(file) => LuaType::ModuleRef(*file),
                TypeCandidate::Recursive => LuaType::Unknown,
            };
            if !types.contains(&ty) {
                types.push(ty);
            }
        }
        Ok(match types.len() {
            0 => LuaType::Unknown,
            1 => types.pop().expect("one candidate"),
            _ => LuaType::Union(Arc::new(LuaUnionType::from_vec(types))),
        })
    }
    fn shell_named_id(&self, file: FileId, name: &str) -> LuaTypeDeclId {
        match self.model.resolve_type_def_in(file, name) {
            Some(def) => match def.visibility {
                TypeVisibility::Public => LuaTypeDeclId::global(&def.full_name),
                _ => LuaTypeDeclId::file(def.file_id, &def.full_name),
            },
            None => LuaTypeDeclId::global(name),
        }
    }
    fn shell_resolve_named(&self, file: FileId, name: &str) -> LuaType {
        match name {
            "any" => LuaType::Any,
            "unknown" => LuaType::Unknown,
            "never" => LuaType::Never,
            "userdata" => LuaType::Userdata,
            "thread" => LuaType::Thread,
            "io" => LuaType::Io,
            "self" => LuaType::SelfInfer,
            "global" => LuaType::Global,
            "int" | "integer" => LuaType::Integer,
            "number" | "float" => LuaType::Number,
            "string" => LuaType::String,
            "table" => LuaType::Table,
            "function" => LuaType::Function,
            "boolean" => LuaType::Boolean,
            "nil" => LuaType::Nil,
            _ => LuaType::Ref(self.shell_named_id(file, name)),
        }
    }
}
fn primitive_named(name: &str) -> Option<TypeShell> {
    Some(TypeShell::from_primitive(match name {
        "string" => PrimitiveType::String,
        "number" => PrimitiveType::Number,
        "integer" | "int" => PrimitiveType::Integer,
        "boolean" | "bool" => PrimitiveType::Boolean,
        "nil" | "void" => PrimitiveType::Nil,
        "table" => PrimitiveType::Table,
        "function" => PrimitiveType::Function,
        _ => return None,
    }))
}
fn primitive_type(p: PrimitiveType) -> LuaType {
    match p {
        PrimitiveType::Nil => LuaType::Nil,
        PrimitiveType::Boolean => LuaType::Boolean,
        PrimitiveType::Integer => LuaType::Integer,
        PrimitiveType::Number => LuaType::Number,
        PrimitiveType::String => LuaType::String,
        PrimitiveType::Table => LuaType::Table,
        PrimitiveType::Function => LuaType::Function,
        PrimitiveType::EmptyObject => LuaType::Object(Arc::new(LuaObjectType::new_with_fields(
            Default::default(),
            Vec::new(),
        ))),
    }
}
