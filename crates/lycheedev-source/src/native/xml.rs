#[derive(Clone,Debug,Serialize)]
pub struct XmlObjectFact {
    pub instance:String,pub object:usize,pub value:usize,pub origin:crate::xml_load::XmlOrigin,
    pub tag:String,pub name:Option<String>,pub parent:Option<String>,
}
#[derive(Clone,Debug,Serialize)]
pub struct XmlCopyFact {
    pub id:usize,pub origin:crate::xml_load::XmlOrigin,pub reference_use:usize,pub field_use:usize,
    pub source_object:usize,pub source_field_write:usize,pub destination_object:usize,pub destination_field_write:usize,pub field:String,pub selected_value:Decision,
}
#[derive(Clone,Debug,Serialize)]
pub struct XmlReferenceFact {pub use_fact:usize,pub name:String,pub role:String,pub origin:crate::xml_load::XmlOrigin}
#[derive(Clone,Debug,Serialize)]
pub struct XmlHandlerFact {
    pub id:usize,pub action:usize,pub instance:String,pub object:usize,pub self_value:usize,
    pub handler:String,pub target:crate::xml_load::HandlerTarget,pub origin:crate::xml_load::XmlOrigin,
    pub selected:Decision,pub selected_use:Option<usize>,pub closure:Option<usize>,pub resolution_phase:&'static str,
}
#[derive(Clone,Debug,Default,Serialize)]
pub struct XmlRuntimeFacts {
    pub objects:Vec<XmlObjectFact>,pub copies:Vec<XmlCopyFact>,pub references:Vec<XmlReferenceFact>,pub handlers:Vec<XmlHandlerFact>,pub gaps:Vec<Value>,
}
fn xml_site(origin:&crate::xml_load::XmlOrigin)->Site{Site{file:origin.path.clone(),start:origin.bytes.start as u32,end:origin.bytes.end as u32}}
impl SourceInputs {
    fn origin_at(&self,site:&Site)->ValueOrigin{
        match self.xml.as_ref().and_then(|program|program.documents.get(&site.file)){
            Some(sha256) if !self.corpus.index.contains_key(&site.file)=>ValueOrigin::Xml{xml:crate::xml_load::XmlOrigin{path:site.file.clone(),sha256:sha256.clone(),bytes:crate::xml_inline::source_map::Span::new(site.start as usize,site.end as usize)}},
            _=>ValueOrigin::Source(site.clone()),
        }
    }
}
impl Builder {
    fn xml_context(&mut self,action:usize,site:&Site)->Context{
        let ctx=Context{file:site.file.clone(),occurrence:0,name:format!("xml:{action}:{}",site.file),deferred:false,parameters:BTreeMap::new(),conditional_branch:None};
        self.native.facts.query_views.insert(ctx.name.clone(),NativeQueryView::Xml{action});ctx
    }
    fn xml_reference(&mut self,reference:&crate::xml_load::XmlReference,receiver:Option<usize>,ctx:&Context,role:&str)->Result<usize>{
        self.budget()?;
        let names=reference.name.split('.').collect::<Vec<_>>();
        let valid=names.iter().all(|name|!name.is_empty()&&name.bytes().enumerate().all(|(index,byte)|byte==b'_'||byte.is_ascii_alphabetic()||(index>0&&byte.is_ascii_digit())));
        let site=xml_site(&reference.origin);
        if !valid{
            let fact=self.usage(site,ctx,Decision::Unavailable("xml_reference_expression_not_static".into()),vec![]);
            self.graph.xml.as_mut().expect("XML facts").references.push(XmlReferenceFact{use_fact:fact,name:reference.name.clone(),role:role.into(),origin:reference.origin.clone()});return Ok(fact);
        }
        let mut selected=receiver.map(|object|Decision::Bound(self.xml_objects.values().find(|(id,_)|*id==object).expect("XML receiver identity").1));
        let mut last=0;
        for name in names {
            let (decision,writes,cell,owner)=if let Some(selected)=selected{
                if let Some(object)=self.object(&selected){let (decision,writes)=self.field(object,name);(decision,writes,Some(format!("object:{object}:field:{name}")),Some(StorageOwner::ObjectField{object,field:name.into()}))}
                else{(Decision::Unavailable("xml_reference_receiver_unavailable".into()),vec![],None,None)}
            }else{let cell=format!("global:{name}");let (decision,writes)=self.native_read(&cell,ctx);(decision,writes,Some(cell),Some(StorageOwner::Global{name:name.into()}))};
            selected=Some(decision.clone());last=self.usage(site.clone(),ctx,decision,writes);
            if let (Some(cell),Some(owner))=(cell,owner){self.native.typed_cells.borrow_mut().entry(cell.clone()).or_insert(owner);self.native.facts.use_storages.push(UseStorageFact{use_fact:last,cell});}
        }
        self.graph.xml.as_mut().expect("XML facts").references.push(XmlReferenceFact{use_fact:last,name:reference.name.clone(),role:role.into(),origin:reference.origin.clone()});Ok(last)
    }
    fn xml_mixin(&mut self,instance:&str,reference:&crate::xml_load::XmlReference,ctx:&Context)->Result<()>{
        let destination=self.xml_objects[instance].0;let reference_use=self.xml_reference(reference,None,ctx,"mixin")?;
        let selected=self.graph.uses[reference_use].decision.clone();let site=xml_site(&reference.origin);
        let Some(source)=self.object(&selected)else{
            self.unknown_object(destination,"xml_mixin_source_unavailable".into(),site);
            self.graph.xml.as_mut().expect("XML facts").gaps.push(json!({"code":"xml_mixin_source_unavailable","reference":reference,"use":reference_use}));return Ok(());
        };
        let fields=self.objects[source].fields.clone();
        if self.objects[source].unknown.is_some(){self.unknown_object(destination,"xml_mixin_enumeration_unavailable".into(),site.clone());}
        for field in fields.keys(){
            self.budget()?;let (decision,writes)=self.field(source,field);
            if matches!(&decision,Decision::KnownAbsent(_))||matches!(&decision,Decision::Bound(id) if matches!(&self.graph.values[*id].kind,ValueKind::Primitive(kind) if kind=="nil")){continue;}
            let source_write=writes.first().copied();
            let field_use=self.usage(site.clone(),ctx,decision.clone(),writes);
            let cell=format!("object:{source}:field:{field}");self.native.facts.use_storages.push(UseStorageFact{use_fact:field_use,cell});
            let uncertain=matches!(&decision,Decision::Unavailable(_)|Decision::NotManaged)||matches!(&decision,Decision::Bound(value) if matches!(&self.graph.values[*value].kind,ValueKind::CallResult(_)|ValueKind::NativeParameter(_)|ValueKind::HostReturn{..}));
            if uncertain{
                let selected=match decision{Decision::Unavailable(_)=>decision,_=>Decision::Unavailable("xml_mixin_source_field_presence_unavailable".into())};
                self.put_field(destination,field.clone(),site.clone(),ctx,selected.clone(),vec![reference_use,field_use]);
                self.graph.xml.as_mut().expect("XML facts").gaps.push(json!({"code":"xml_mixin_source_field_unavailable","reference":reference,"field":field,"fieldUse":field_use,"selected":selected}));
                continue;
            }
            let source_write=source_write.expect("known present field has an observed write");
            let destination_write=self.put_field(destination,field.clone(),site.clone(),ctx,decision.clone(),vec![reference_use,field_use]);
            let facts=self.graph.xml.as_mut().expect("XML facts");let id=facts.copies.len();
            facts.copies.push(XmlCopyFact{id,origin:reference.origin.clone(),reference_use,field_use,source_object:source,source_field_write:source_write,destination_object:destination,destination_field_write:destination_write,field:field.clone(),selected_value:decision});
        }
        Ok(())
    }
    fn xml_load(&mut self,program:&crate::xml_load::Program)->Result<()>{
        self.graph.xml=Some(XmlRuntimeFacts{gaps:program.gaps.clone(),..Default::default()});
        let mut occurrence=0;
        for (action_id,action) in program.actions().iter().enumerate(){
            self.budget()?;
            match action{
                crate::xml_load::Action::Lua{file}=>{
                    self.load_file(file,occurrence)?;occurrence+=1;
                    if !self.native.facts.control.complete{return Ok(());}
                },
                crate::xml_load::Action::Object{instance,origin,tag,name,parent}=>{
                    let site=xml_site(origin);let ctx=self.xml_context(action_id,&site);let object=self.allocate_object(&site)?;
                    // Native members remain open; subsequently copied or written Lua
                    // fields have their own normal write/observation versions.
                    self.unknown_object(object,"xml_native_members_unavailable".into(),site.clone());
                    let value=self.graph.values.len();self.graph.values.push(AbstractValue{id:value,origin:ValueOrigin::Xml{xml:origin.clone()},kind:ValueKind::Object(object)});
                    self.xml_objects.insert(instance.clone(),(object,value));
                    if let Some(name)=name{let cell=format!("global:{name}");self.native.typed_cells.borrow_mut().insert(cell.clone(),StorageOwner::Global{name:name.clone()});self.write(cell,site,&ctx,Decision::Bound(value),vec![]);}
                    self.graph.xml.as_mut().expect("XML facts").objects.push(XmlObjectFact{instance:instance.clone(),object,value,origin:origin.clone(),tag:tag.clone(),name:name.clone(),parent:parent.clone()});
                },
                crate::xml_load::Action::Mixin{instance,reference}=>{let ctx=self.xml_context(action_id,&xml_site(&reference.origin));self.xml_mixin(instance,reference,&ctx)?;},
                crate::xml_load::Action::ParentField{parent,parent_reference,child,reference}=>{
                    let site=xml_site(&reference.origin);let ctx=self.xml_context(action_id,&site);
                    let mut inputs=vec![];
                    let parent=if let Some(reference)=parent_reference{let fact=self.xml_reference(reference,None,&ctx,"parent")?;inputs.push(fact);self.object(&self.graph.uses[fact].decision)}else{parent.as_ref().and_then(|parent|self.xml_objects.get(parent).map(|(object,_)|*object))};
                    if let Some(parent)=parent{self.put_field(parent,reference.name.clone(),site,&ctx,Decision::Bound(self.xml_objects[child].1),inputs);}
                    else{self.graph.xml.as_mut().expect("XML facts").gaps.push(json!({"code":"xml_parent_key_receiver_unavailable","reference":reference}));}
                },
                crate::xml_load::Action::Handler{instance,handler,target,origin}=>{
                    let (object,self_value)=self.xml_objects[instance];
                    let selected=if let crate::xml_load::HandlerTarget::Inline{file}=target{
                        self.native.facts.closures.iter().rev().find(|closure|closure.load_occurrence==occurrence.saturating_sub(1)&&self.graph.functions[closure.function].closure.file==*file)
                            .map(|closure|Decision::Bound(closure.value)).unwrap_or_else(||Decision::Unavailable("xml_inline_closure_unavailable".into()))
                    }else{Decision::Unavailable("xml_handler_not_observed".into())};
                    let facts=self.graph.xml.as_mut().expect("XML facts");let id=facts.handlers.len();
                    facts.handlers.push(XmlHandlerFact{id,action:action_id,instance:instance.clone(),object,self_value,handler:handler.clone(),target:target.clone(),origin:origin.clone(),selected,selected_use:None,closure:None,resolution_phase:"hypothetical_after_load"});
                },
                crate::xml_load::Action::Definition{..}=>{},
            }
        }
        // Resolution updates only the current handler and appends references/uses;
        // later handlers remain unchanged. Clone one item, not the whole vector.
        for index in 0..self.graph.xml.as_ref().expect("XML facts").handlers.len(){
            let handler=self.graph.xml.as_ref().expect("XML facts").handlers[index].clone();
            let ctx=self.xml_context(handler.action,&xml_site(&handler.origin));
            let fact=match &handler.target{
                crate::xml_load::HandlerTarget::Method{reference}=>Some(self.xml_reference(reference,Some(handler.object),&ctx,"handler_method")?),
                crate::xml_load::HandlerTarget::Function{reference}=>Some(self.xml_reference(reference,None,&ctx,"handler_function")?),
                _=>None,
            };
            let selected=fact.map(|fact|self.graph.uses[fact].decision.clone()).unwrap_or(handler.selected);
            let closure=match selected{Decision::Bound(value)=>self.native.closure_values.get(&value).copied(),_=>None};
            let destination=&mut self.graph.xml.as_mut().expect("XML facts").handlers[handler.id];destination.selected=selected;destination.selected_use=fact;destination.closure=closure;
        }
        Ok(())
    }
    fn load_file(&mut self,file:&str,occurrence:usize)->Result<()>{
        let body=self.harvest.blocks[file].clone();let ctx=Context{file:file.into(),occurrence,name:format!("load:{occurrence}:{file}"),deferred:false,parameters:BTreeMap::new(),conditional_branch:None};
        match body {
            ScriptBody::Block(block)=>self.block(block,&ctx),
            ScriptBody::Empty(range) if self.native_profile=>{
                let paths=self.native_block_body(None,range,&ctx,0)?;self.native_load_terminals(paths)
            },
            ScriptBody::Empty(_)=>Ok(()),
        }
    }
}
