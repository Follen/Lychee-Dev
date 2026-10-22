// Incoming call labels are syntax ownership, independent of selected callee
// values and their proof/coverage. Never derive a caller from the query target.
impl PreparedSnapshot {
    fn research_static_name(control:&OperationControl,mut expression:LuaExpr)->Result<bool> {
        loop {
            control.check().map_err(|error|error.to_string())?;
            match expression {
                LuaExpr::NameExpr(name)=>return Ok(name.get_name_token().is_some()),
                LuaExpr::IndexExpr(index) if matches!(index.get_index_key(),Some(LuaIndexKey::Name(_)|LuaIndexKey::String(_)))=>{
                    let Some(prefix)=index.get_prefix_expr()else{return Ok(false)};
                    expression=prefix;
                },
                _=>return Ok(false),
            }
        }
    }

    // Recovery nodes are not proof of a binding. The parser can retain a
    // FuncStat/LocalFuncStat after a malformed name or parameter/header suffix.
    // A syntax error anywhere in this declaration conservatively withholds its
    // name; nearest closure identity and every semantic call row stay intact.
    fn research_function_binding(control:&OperationControl,stat:LuaStat,syntax_errors:&std::collections::BTreeSet<rowan::TextSize>)->Result<Option<(LuaClosureExpr,TextRange)>> {
        control.check().map_err(|error|error.to_string())?;
        let range=stat.get_range();
        if syntax_errors.range(range.start()..=range.end()).next().is_some(){return Ok(None)}
        let(closure,name)=match stat {
            LuaStat::FuncStat(stat)=>{
                let(Some(closure),Some(name))=(stat.get_closure(),stat.get_func_name())else{return Ok(None)};
                let name=name.to_expr();
                if !Self::research_static_name(control,name.clone())?{return Ok(None)}
                (closure,name.get_range())
            },
            LuaStat::LocalFuncStat(stat)=>{
                let(Some(closure),Some(name))=(stat.get_closure(),stat.get_local_name().and_then(|name|name.get_name_token()))else{return Ok(None)};
                (closure,name.get_range())
            },
            _=>return Ok(None),
        };
        let Some(params)=closure.get_params_list()else{return Ok(None)};
        let parens=params.syntax().first_token().is_some_and(|token|token.kind()==emmylua_parser::LuaTokenKind::TkLeftParen.into())
            &&params.syntax().last_token().is_some_and(|token|token.kind()==emmylua_parser::LuaTokenKind::TkRightParen.into());
        Ok(parens.then_some((closure,name)))
    }

    fn research_assigned_closure(&self,mut expression:LuaExpr)->Result<Option<LuaClosureExpr>> {
        loop {
            self.control.check().map_err(|error|error.to_string())?;
            match expression {
                LuaExpr::ClosureExpr(closure)=>return Ok(Some(closure)),
                LuaExpr::ParenExpr(paren)=>match paren.get_expr(){Some(inner)=>expression=inner,None=>return Ok(None)},
                _=>return Ok(None),
            }
        }
    }

    // Each statement is inspected once in the existing AST pass. In particular,
    // a many-value assignment does not rescan every sibling for each closure.
    fn index_research_callables(&self,file:usize,node:&emmylua_parser::LuaSyntaxNode,syntax_errors:&std::collections::BTreeSet<rowan::TextSize>,owners:&mut BTreeMap<SyntaxSite,Option<SyntaxSite>>)->Result<()> {
        self.control.check().map_err(|error|error.to_string())?;
        let site=|range|self.inputs.original_range(&self.inputs.corpus.files[file].path,range)
            .map(|range|SyntaxSite::at(file,range)).map_err(|error|error.to_string());
        if let Some(closure)=LuaClosureExpr::cast(node.clone()) {owners.entry(site(closure.get_range())?).or_insert(None);}
        let mut bind=|closure:LuaClosureExpr,range:TextRange|->Result<()> {
            owners.insert(site(closure.get_range())?,Some(site(range)?));Ok(())
        };
        match LuaStat::cast(node.clone()) {
            Some(stat @ (LuaStat::FuncStat(_)|LuaStat::LocalFuncStat(_)))=>{
                if let Some((closure,name))=Self::research_function_binding(&self.control,stat,syntax_errors)?{bind(closure,name)?;}
            },
            Some(LuaStat::LocalStat(stat))=>{
                for(value,name)in stat.get_value_exprs().zip(stat.get_local_name_list()) {
                    self.control.check().map_err(|error|error.to_string())?;
                    if let (Some(closure),Some(name))=(self.research_assigned_closure(value)?,name.get_name_token()){bind(closure,name.get_range())?;}
                }
            },
            Some(LuaStat::AssignStat(stat))=>{
                let(vars,values)=stat.get_var_and_expr_list();
                for(value,name)in values.into_iter().zip(vars) {
                    self.control.check().map_err(|error|error.to_string())?;
                    if let Some(closure)=self.research_assigned_closure(value)? {
                        let name=name.to_expr();
                        if Self::research_static_name(&self.control,name.clone())? {bind(closure,name.get_range())?;}
                    }
                }
            },
            _=>{},
        }
        Ok(())
    }

    fn research_incoming_caller(&self,syntax:&SourceFileSyntax,reference:&IndexedReference,origins:&crate::native_research::XmlResearch)->lycheedev_core::Result<Value> {
        self.control.check()?;
        let reference_site=reference.site.public(&self.inputs);
        let Some(closure)=reference.closure else {
            let position=origins.position(self,&Site{file:reference_site.file,start:0,end:0})?;
            return Ok(json!({"kind":"file","position":position}));
        };
        let closure_site=closure.public(&self.inputs);
        if let Some(script)=origins.scripts.get(&closure_site.file)
            &&script.is_syntax_wrapper(closure.start as usize,closure.end as usize) {
            let position=origins.position(self,&Site{file:script.path.clone(),start:script.element.start as u32,end:script.element.end as u32})?;
            return Ok(json!({"kind":if script.kind=="handler"{"xml_handler"}else{"xml_binding"},
                "handler":script.handler,"position":position,"syntaxClosure":closure_site}));
        }
        let Some(declaration)=syntax.callable_owners.get(&closure)else {
            return Ok(json!({"kind":"unknown","reason":"enclosing_callable_identity_unavailable","syntaxClosure":closure_site}));
        };
        if let Some(declaration)=declaration {
            let site=declaration.public(&self.inputs);let name=site.text(&self.inputs)?;
            Ok(json!({"kind":"named","name":name,"position":origins.position(self,&site)?,"syntaxClosure":closure_site}))
        } else {
            // A point locates the closure without copying a potentially large
            // XML source map for its entire body into every call row.
            let point=Site{file:closure_site.file.clone(),start:closure.start,end:closure.start};
            Ok(json!({"kind":"anonymous","position":origins.position(self,&point)?,"syntaxClosure":closure_site}))
        }
    }

    fn project_incoming_caller(&self,row:&mut Value,syntax:&SourceFileSyntax,reference:&IndexedReference,origins:&crate::native_research::XmlResearch)->lycheedev_core::Result<()> {
        let caller=self.research_incoming_caller(syntax,reference,origins)?;
        row["referenceExpression"]=row["source"].clone();
        row["source"]=match caller["kind"].as_str() {
            Some("named")=>caller["name"].clone(),
            Some("anonymous")=>json!("<anonymous>"),
            Some("file")=>json!("<file>"),
            Some("xml_handler")=>json!(format!("<xml-handler:{}>",caller["handler"].as_str().unwrap_or("unknown"))),
            Some("xml_binding")=>json!(format!("<xml-binding:{}>",caller["handler"].as_str().unwrap_or("unknown"))),
            _=>json!("<unknown>"),
        };
        row["caller"]=caller;self.control.check()
    }
}
