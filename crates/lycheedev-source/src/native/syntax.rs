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
    id:DeclarationId, name:String, site:SyntaxSite, kind:&'static str,
    owner:Option<SyntaxSite>, implicit_self:bool,
}
impl IndexedDeclaration {
    fn public(&self,inputs:&SourceInputs)->SourceDeclaration {
        SourceDeclaration{id:self.id.public(),name:self.name.clone(),site:self.site.public(inputs),kind:self.kind.into(),owner:self.owner.map(|site|site.public(inputs)),implicit_self:self.implicit_self}
    }
}
#[derive(Clone, Debug)]
struct IndexedReference {
    id:String, site:SyntaxSite, declaration:Option<usize>,
    roles:Vec<ReferenceRole>, closure:Option<SyntaxSite>,
}
#[derive(Clone, Debug)]
struct IndexedCall { id:String, site:SyntaxSite, callee:SyntaxSite }
// Temporary counts from the already-required identity walk of the same immutable
// admitted trees. No syntax nodes, source bytes, or public result rows are copied.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct SourceSyntaxCounts { nodes:usize, references:usize, calls:usize }
impl SourceSyntaxCounts {
    fn for_files(files:usize,admitted:usize)->Result<Box<[Self]>> {
        // The existing admitted file ceiling is the bound, not a new quota.
        // Payload is size_of::<Self>() * files (24 * files on Windows x64),
        // at most 480000 bytes for the 20000-file research ceiling. Allocator
        // overhead is additional. This fixed slice dies when syntax indexing ends.
        if files>admitted{return Err("syntax counts exceed admitted file count".into());}
        let _payload_bytes=files.checked_mul(std::mem::size_of::<Self>()).ok_or("syntax count payload overflow")?;
        Ok(vec![Self::default();files].into_boxed_slice())
    }
    fn observe(&mut self,node:&emmylua_parser::LuaSyntaxNode) {
        self.nodes+=1;
        if matches!(LuaExpr::cast(node.clone()),Some(LuaExpr::NameExpr(_)|LuaExpr::IndexExpr(_))){self.references+=1;}
        if let Some(call)=emmylua_parser::LuaCallExpr::cast(node.clone()) && call.get_prefix_expr().is_some(){self.calls+=1;}
    }
    fn replay(&self,count:&mut usize,limit:usize,mut check:impl FnMut()->Result<()>)->Result<()> {
        // Retain every original syntax-stage control-before-node-limit check,
        // after this file's declarations. Only repeat Rowan traversal is removed;
        // construction row charging remains at the original final boundary.
        for _ in 0..self.nodes {
            check()?;*count+=1;if *count>limit{return Err("syntax index AST bound".into());}
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Default)]
struct SourceSyntaxIndex {
    declarations:Vec<IndexedDeclaration>, declaration_ranges:Vec<std::ops::Range<usize>>,
    // Immutable lookup permutations borrow keys from declarations. No second
    // digest allocation, site copy, or B-tree node is retained for either key.
    declarations_by_id:Box<[usize]>, declarations_by_site:Box<[usize]>,
    reference_count:usize, call_count:usize,
}
impl SourceSyntaxIndex {
    fn finish_declaration_lookups(&mut self) {
        let mut by_id:Vec<_>=(0..self.declarations.len()).collect();
        let mut by_site=by_id.clone();
        // Index tie-breaking retains original insertion order without allocating
        // stable-sort scratch. Equal keys retain the last insertion, exactly as
        // the former BTreeMap::insert did, including recovered duplicate sites.
        by_id.sort_unstable_by(|&a,&b|self.declarations[a].id.cmp(&self.declarations[b].id).then(a.cmp(&b)));
        by_id.dedup_by(|later,earlier|{
            if self.declarations[*later].id==self.declarations[*earlier].id{*earlier=*later;true}else{false}
        });
        by_site.sort_unstable_by(|&a,&b|self.declarations[a].site.cmp(&self.declarations[b].site).then(a.cmp(&b)));
        by_site.dedup_by(|later,earlier|{
            if self.declarations[*later].site==self.declarations[*earlier].site{*earlier=*later;true}else{false}
        });
        self.declarations_by_id=by_id.into_boxed_slice();
        self.declarations_by_site=by_site.into_boxed_slice();
        // The former insertion loop had no control or step charge. Preserve
        // that ordering: the original shared row charge follows this freeze.
    }
    #[cfg(test)]
    fn declaration_by_id(&self,id:&str)->Option<usize> {
        self.declaration_by_key(&DeclarationId::parse(id)?)
    }
    fn declaration_by_key(&self,id:&DeclarationId)->Option<usize> {
        self.declarations_by_id.binary_search_by(|&index|self.declarations[index].id.cmp(id))
            .ok().map(|index|self.declarations_by_id[index])
    }
    fn declaration_by_site(&self,site:SyntaxSite)->Option<usize> {
        self.declarations_by_site.binary_search_by_key(&site,|&index|self.declarations[index].site)
            .ok().map(|index|self.declarations_by_site[index])
    }
    fn declaration_at(&self,inputs:&SourceInputs,site:&Site)->Option<usize> {
        self.declaration_by_site(SyntaxSite::from_site(inputs,site)?)
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
#[cfg(test)]
fn source_decl_id(inputs:&SourceInputs,site:&Site)->String {digest(&(inputs.identity_sha256(),"declaration",site))}
// Binary SHA256 order is identical to the public fixed-width lowercase hex
// order. Keep one inline digest per retained declaration; public rows alone
// allocate the text representation.
#[derive(Clone,Copy,Debug,PartialEq,Eq,PartialOrd,Ord)]
struct DeclarationId([u8;32]);
impl DeclarationId {
    fn parse(public:&str)->Option<Self> {
        if public.len()!=64{return None;}
        let nibble=|byte|match byte{b'0'..=b'9'=>Some(byte-b'0'),b'a'..=b'f'=>Some(byte-b'a'+10),_=>None};
        let mut bytes=[0;32];
        for (byte,pair) in bytes.iter_mut().zip(public.as_bytes().chunks_exact(2)) {
            *byte=nibble(pair[0])?<<4|nibble(pair[1])?;
        }
        Some(Self(bytes))
    }
    fn public(self)->String {
        const HEX:&[u8;16]=b"0123456789abcdef";
        let mut public=String::with_capacity(64);
        for byte in self.0{public.push(HEX[(byte>>4)as usize]as char);public.push(HEX[(byte&15)as usize]as char);}
        public
    }
}
fn source_decl_key(inputs:&SourceInputs,site:&Site)->DeclarationId {
    let mut writer=JsonDigestWriter(Sha256::new());
    serde_json::to_writer(&mut writer,&(inputs.identity_sha256(),"declaration",site)).expect("serializable bounded facts");
    DeclarationId(writer.0.finalize().into())
}
fn syntax_owner(node:&emmylua_parser::LuaSyntaxNode,inputs:&SourceInputs,file:usize)->Option<SyntaxSite> {
    node.ancestors().skip(1).find_map(LuaClosureExpr::cast).map(|closure|SyntaxSite::at(file,inputs.original_range(&inputs.corpus.files[file].path,closure.get_range()).expect("admitted parser range")))
}
fn unparen(mut expr:LuaExpr)->LuaExpr {
    while let LuaExpr::ParenExpr(ref paren)=expr {let Some(inner)=paren.get_expr()else{break};expr=inner;}expr
}
impl PreparedSnapshot {
    fn build_syntax_index(&self,counts:&[SourceSyntaxCounts])->Result<SourceSyntaxIndex> {
        assert_eq!(counts.len(),self.inputs.corpus.files.len(),"same immutable admitted file set");
        // The immutable Emmy facts already give the exact declaration count.
        // Avoid growing and reallocating this large native collection beside
        // the fully loaded workspace: the previous and new capacities overlap.
        let mut declarations=0usize;
        for source in &self.inputs.corpus.files {
            self.control.check().map_err(|e|e.to_string())?;
            let model=self.analysis.semantic_model(self.files[&source.path]);
            declarations+=model.file_facts().ok_or("syntax facts missing")?.decls.iter()
                .filter(|decl|decl.kind.is_local()).count();
        }
        self.control.check().map_err(|e|e.to_string())?;
        let mut index=SourceSyntaxIndex{
            declarations:Vec::with_capacity(declarations),
            declaration_ranges:Vec::with_capacity(counts.len()),
            ..SourceSyntaxIndex::default()
        };let mut count=0usize;
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
                index.declarations.push(IndexedDeclaration{id:source_decl_key(&self.inputs,&site.public(&self.inputs)),name:decl.name.to_string(),site,kind,owner,implicit_self:decl.is_implicit_self});
            }
            index.declaration_ranges.push(declaration_start..index.declarations.len());
            // Admit the same complete row count without retaining or hashing
            // every reference and call before any query needs them.
            counts[file_index].replay(&mut count,self.inputs.corpus.limits.ast_nodes,||self.control.check().map_err(|e|e.to_string()))?;
            index.reference_count+=counts[file_index].references;
            index.call_count+=counts[file_index].calls;
        }
        let rows=index.declarations.len()+index.reference_count+index.call_count;
        if rows>if self.inputs.corpus.limits.research{5000000}else{12000}{return Err("syntax index row bound".into())}
        index.finish_declaration_lookups();
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
                local_bindings.insert(source_site(use_.syntax.get_range()),self.syntax.declaration_by_site(source_site(decl.name_range)).expect("admitted local declaration site"));
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
                    return self.syntax.declaration_by_site(key).map(|id|&self.syntax.declarations[id]);
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

#[cfg(test)]
mod syntax_count_tests {
    use super::*;
    include!("syntax_count_tests.rs");
}

#[cfg(test)]
#[path = "syntax_index_tests.rs"]
mod syntax_index_tests;
