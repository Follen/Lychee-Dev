// Syntax identity is independent of the bounded abstract evaluator.
#[derive(Clone, Debug, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all="snake_case")]
pub enum ReferenceRole { Read, Write, Capture, Call, Declaration }

#[derive(Clone, Debug, Serialize)]
pub struct SourceDeclaration {
    pub id:String, pub name:String, pub site:Site, pub kind:String,
    pub owner:Option<Site>, pub implicit_self:bool,
}
#[derive(Clone, Debug, Serialize)]
pub struct SyntaxReference {
    pub id:String, pub site:Site, pub declaration:Option<String>,
    pub roles:Vec<ReferenceRole>, pub closure:Option<Site>, pub call:Option<Site>,
    pub member:bool,
}
#[derive(Clone, Debug, Serialize)]
pub struct SyntaxCall { pub id:String, pub site:Site, pub callee:Site, pub closure:Option<Site> }

// The owning SourceInputs already retains each path. Keep original-source
// offsets here and materialize public Site values only for query results.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct SyntaxSite { file:usize, start:u32, end:u32 }
impl SyntaxSite {
    fn at(file:usize,range:TextRange)->Self {Self{file,start:range.start().into(),end:range.end().into()}}
    fn from_site(inputs:&SourceInputs,site:&Site)->Option<Self> {Some(Self{file:*inputs.corpus.index.get(&site.file)?,start:site.start,end:site.end})}
    fn public(self,inputs:&SourceInputs)->Site {Site{file:inputs.corpus.files[self.file].path.clone(),start:self.start,end:self.end}}
}
#[derive(Clone, Debug)]
struct IndexedDeclaration {
    id:String, name:String, site:SyntaxSite, kind:String,
    owner:Option<SyntaxSite>, implicit_self:bool,
}
impl IndexedDeclaration {
    fn public(&self,inputs:&SourceInputs)->SourceDeclaration {
        SourceDeclaration{id:self.id.clone(),name:self.name.clone(),site:self.site.public(inputs),kind:self.kind.clone(),owner:self.owner.map(|site|site.public(inputs)),implicit_self:self.implicit_self}
    }
}
#[derive(Clone, Debug)]
struct IndexedReference {
    id:String, site:SyntaxSite, declaration:Option<usize>,
    roles:Vec<ReferenceRole>, closure:Option<SyntaxSite>,
}
#[derive(Clone, Debug)]
struct IndexedCall { id:String, site:SyntaxSite, callee:SyntaxSite }
#[derive(Clone, Debug, Default)]
struct SourceSyntaxIndex {
    declarations:Vec<IndexedDeclaration>, declaration_ranges:Vec<std::ops::Range<usize>>,
    declarations_by_id:BTreeMap<String,usize>, declarations_by_site:BTreeMap<SyntaxSite,usize>,
    reference_count:usize, call_count:usize,
}
impl SourceSyntaxIndex {
    fn declaration_at(&self,inputs:&SourceInputs,site:&Site)->Option<usize> {
        self.declarations_by_site.get(&SyntaxSite::from_site(inputs,site)?).copied()
    }
}
// Queries own at most one of these projections. Emmy retains the whole
// admitted workspace; reference/call wrappers do not duplicate it in memory.
#[derive(Default)]
struct SourceFileSyntax {
    references:Vec<IndexedReference>, calls:Vec<IndexedCall>,
    references_by_site:BTreeMap<SyntaxSite,usize>,
    // Query-local syntax owners, never a second session-wide AST or graph.
    callable_owners:BTreeMap<SyntaxSite,Option<SyntaxSite>>,
}
impl SourceFileSyntax {
    fn reference_at(&self,inputs:&SourceInputs,site:&Site)->Option<&IndexedReference> {
        self.references_by_site.get(&SyntaxSite::from_site(inputs,site)?).map(|id|&self.references[*id])
    }
}
fn source_decl_id(inputs:&SourceInputs,site:&Site)->String {digest(&(inputs.identity_sha256(),"declaration",site))}
fn syntax_owner(node:&emmylua_parser::LuaSyntaxNode,inputs:&SourceInputs,file:usize)->Option<SyntaxSite> {
    node.ancestors().skip(1).find_map(LuaClosureExpr::cast).map(|closure|SyntaxSite::at(file,inputs.original_range(&inputs.corpus.files[file].path,closure.get_range()).expect("admitted parser range")))
}
fn unparen(mut expr:LuaExpr)->LuaExpr {
    while let LuaExpr::ParenExpr(ref paren)=expr {let Some(inner)=paren.get_expr()else{break};expr=inner;}expr
}
impl PreparedSnapshot {
    fn build_syntax_index(&self)->Result<SourceSyntaxIndex> {
        let mut index=SourceSyntaxIndex::default();let mut count=0usize;
        for (file_index,source) in self.inputs.corpus.files.iter().enumerate() {
            self.control.check().map_err(|e|e.to_string())?;
            let file=source.path.as_str();let model=self.analysis.semantic_model(self.files[file]);
            let facts=model.file_facts().ok_or("syntax facts missing")?;
            let root=model.syntax_tree().ok_or("syntax tree missing")?.get_red_root();
            let source_site=|range|SyntaxSite::at(file_index,self.inputs.original_range(file,range).expect("admitted parser range"));
            let declaration_start=index.declarations.len();
            for decl in &facts.decls {
                self.control.check().map_err(|e|e.to_string())?;
                if !decl.kind.is_local(){continue}
                let site=source_site(decl.name_range);
                let owner=decl.owner_syntax.and_then(|syntax|syntax.to_node_from_root(&root)).and_then(|node|{
                    if let Some(closure)=LuaClosureExpr::cast(node.clone()){Some(source_site(closure.get_range()))}else{syntax_owner(&node,&self.inputs,file_index)}
                });
                let kind=match decl.kind {emmylua_code_analysis::DeclKind::Param=>"parameter",emmylua_code_analysis::DeclKind::Local{is_iter:true,..}=>"loop_local",_=>"local"}.into();
                index.declarations.push(IndexedDeclaration{id:source_decl_id(&self.inputs,&site.public(&self.inputs)),name:decl.name.to_string(),site,kind,owner,implicit_self:decl.is_implicit_self});
            }
            index.declaration_ranges.push(declaration_start..index.declarations.len());
            // Admit the same complete row count without retaining or hashing
            // every reference and call before any query needs them.
            for node in root.descendants(){
                self.control.check().map_err(|e|e.to_string())?;count+=1;if count>self.inputs.corpus.limits.ast_nodes{return Err("syntax index AST bound".into())}
                if matches!(LuaExpr::cast(node.clone()),Some(LuaExpr::NameExpr(_)|LuaExpr::IndexExpr(_))){index.reference_count+=1;}
                if let Some(call)=emmylua_parser::LuaCallExpr::cast(node) && call.get_prefix_expr().is_some(){index.call_count+=1;}
            }
        }
        let rows=index.declarations.len()+index.reference_count+index.call_count;
        if rows>if self.inputs.corpus.limits.research{5000000}else{12000}{return Err("syntax index row bound".into())}
        for (i,decl) in index.declarations.iter().enumerate(){index.declarations_by_id.insert(decl.id.clone(),i);index.declarations_by_site.insert(decl.site,i);}
        self.inputs.charge(rows).map_err(|e|e.to_string())?;Ok(index)
    }
    fn file_syntax(&self,file_index:usize)->Result<SourceFileSyntax> {
        let file=self.inputs.corpus.files[file_index].path.as_str();
        let model=self.analysis.semantic_model(self.files[file]);
        let facts=model.file_facts().ok_or("syntax facts missing")?;
        let tree=model.syntax_tree().ok_or("syntax tree missing")?;
        let root=tree.get_red_root();
        // Errors can be reported out of source order. Retain only distinct
        // syntax-error positions for bounded declaration checks, not messages.
        let mut syntax_errors=std::collections::BTreeSet::new();
        for error in tree.get_errors() {
            self.control.check().map_err(|e|e.to_string())?;
            if error.kind==emmylua_parser::LuaParseErrorKind::SyntaxError{syntax_errors.insert(error.range.start());}
        }
        let source_site=|range|SyntaxSite::at(file_index,self.inputs.original_range(file,range).expect("admitted parser range"));
        let mut local_bindings=BTreeMap::new();
        // Use the same Emmy NameUse records and visibility rule as admission.
        for use_ in &facts.name_uses {
            self.control.check().map_err(|e|e.to_string())?;
            if let Some(decl)=facts.find_visible_decl_before_offset(&use_.name,use_.syntax.get_range().start()).filter(|decl|decl.kind.is_local()){
                local_bindings.insert(source_site(use_.syntax.get_range()),self.syntax.declarations_by_site[&source_site(decl.name_range)]);
            }
        }
        let mut index=SourceFileSyntax::default();let mut writes=HashSet::new();let mut callees=BTreeMap::new();
        for node in root.descendants(){
            self.control.check().map_err(|e|e.to_string())?;
            self.index_research_callables(file_index,&node,&syntax_errors,&mut index.callable_owners)?;
            if let Some(stat)=emmylua_parser::LuaAssignStat::cast(node.clone()) {for expr in stat.get_var_and_expr_list().0 {writes.insert(source_site(expr.to_expr().get_range()));}}
            if let Some(stat)=emmylua_parser::LuaFuncStat::cast(node.clone()) && let Some(name)=stat.get_func_name(){writes.insert(source_site(name.to_expr().get_range()));}
            if let Some(call)=emmylua_parser::LuaCallExpr::cast(node.clone()) && let Some(prefix)=call.get_prefix_expr(){
                let site=source_site(call.get_range());let callee=source_site(unparen(prefix).get_range());
                callees.insert(callee,site);index.calls.push(IndexedCall{id:digest(&(self.inputs.identity_sha256(),"syntax_call",site.public(&self.inputs))),site,callee});
            }
        }
        for node in root.descendants(){
            self.control.check().map_err(|e|e.to_string())?;
            let Some(expr)=LuaExpr::cast(node.clone())else{continue};
            let member=matches!(expr,LuaExpr::IndexExpr(_));if !member&&!matches!(expr,LuaExpr::NameExpr(_)){continue}
            let site=source_site(expr.get_range());
            let declaration=if matches!(expr,LuaExpr::NameExpr(_)){local_bindings.get(&site).copied()}else{None};
            let closure=syntax_owner(&node,&self.inputs,file_index);let call=callees.get(&site).copied();
            let mut roles=vec![if writes.contains(&site){ReferenceRole::Write}else{ReferenceRole::Read}];
            if call.is_some(){roles.push(ReferenceRole::Call)}
            if let(Some(id),Some(owner))=(declaration,closure) {
                let site=self.syntax.declarations[id].site;
                if !(site.file==owner.file&&site.start>=owner.start&&site.end<=owner.end){roles.push(ReferenceRole::Capture)}
            }
            index.references_by_site.insert(site,index.references.len());
            index.references.push(IndexedReference{id:digest(&(self.inputs.identity_sha256(),"syntax_reference",site.public(&self.inputs))),site,declaration,roles,closure});
        }
        Ok(index)
    }
    fn lexical_declaration_at(&self,site:&Site)->Option<&IndexedDeclaration> {
        if let Some(binding)=self.expression_binding_site(site) {
            match binding.syntax.get_kind() {
                emmylua_parser::LuaSyntaxKind::NameExpr=>{
                    let model=self.analysis.semantic_model(binding.file_id);let facts=model.file_facts()?;
                    let use_=facts.name_use_at_offset(binding.syntax.get_range().start()).filter(|use_|use_.syntax==binding.syntax)?;
                    let decl=facts.find_visible_decl_before_offset(&use_.name,use_.syntax.get_range().start()).filter(|decl|decl.kind.is_local())?;
                    let key=SyntaxSite::at(self.inputs.corpus.index[&site.file],self.inputs.original_range(&site.file,decl.name_range).expect("admitted parser range"));
                    return self.syntax.declarations_by_site.get(&key).map(|id|&self.syntax.declarations[*id]);
                },
                emmylua_parser::LuaSyntaxKind::IndexExpr=>return None,
                _=>{},
            }
        }
        self.syntax.declaration_at(&self.inputs,site).map(|id|&self.syntax.declarations[id])
    }
    fn syntax_callee_at(&self,site:&Site)->Option<Site> {
        let binding=self.expression_binding_site(site)?;
        let model=self.analysis.semantic_model(binding.file_id);let root=model.syntax_tree()?.get_red_root();
        let call=emmylua_parser::LuaCallExpr::cast(binding.syntax.to_node_from_root(&root)?)?;
        Some(self.inputs.site(&site.file,unparen(call.get_prefix_expr()?).get_range()))
    }
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(tag="kind",rename_all="snake_case")]
pub enum StorageOwner {
    Local { declaration:Site, load_occurrence:usize, frame:Option<usize> },
    Global { name:String },
    ObjectField { object:usize, field:String },
    ObjectBehavior { object:usize },
}
#[derive(Clone, Debug, Serialize)]
pub struct StorageFact { pub cell:String, pub owner:StorageOwner }
#[derive(Clone, Debug, Serialize)]
pub struct CaptureFact { pub closure:usize, pub declaration:Site, pub cell:String, pub creation_context:String }
#[derive(Clone, Debug, Serialize)]
pub struct UseStorageFact { pub use_fact:usize, pub cell:String }
