//! Shared normal/recovery lifecycle driver. Isolated, prepared, not compiled.
//! No input API exists here; PrepareTransport is never a sending permission.
#[path = "../native-v1-prototype/gate.rs"]
mod gate;
use std::{cell::RefCell,collections::BTreeMap,fs::{self,OpenOptions},io::{Read,Write},path::{Path,PathBuf}};
use serde::{Deserialize,Serialize};
use sha2::{Digest,Sha256};
const MAX_BODY:usize=524_288;
const MAX_INTENTS:usize=32;
const MAX_RECEIVED:usize=4;
pub const MAX_CODE:usize=262_144;
pub const MAX_COMMAND_WIRE:usize=34+MAX_CODE*2;
pub const MAX_STAGE_WIRE:usize=176+MAX_COMMAND_WIRE*2;
pub const MAX_SLOT_SOURCE:usize=10+MAX_STAGE_WIRE;
fn sha(bytes:&[u8])->String{hex::encode(Sha256::digest(bytes))}
fn is_hex(s:&str,n:usize)->bool{s.len()==n&&s.bytes().all(|b|b.is_ascii_digit()||(b'a'..=b'f').contains(&b))}
fn read_bounded(path:&Path,max:usize)->Result<Vec<u8>,String>{let mut bytes=Vec::new();fs::File::open(path).map_err(|e|e.to_string())?.take((max+1)as u64).read_to_end(&mut bytes).map_err(|e|e.to_string())?;if bytes.len()>max{return Err("journal_size".into());}Ok(bytes)}
fn immutable(path:&Path,bytes:&[u8])->Result<(),String>{
    if path.exists(){if read_bounded(path,1<<20)?==bytes{return Ok(());}return Err("protected_record_changed".into());}
    let temp=path.with_extension(format!("sealed-{}",lycheedev_windows::random::token().map_err(|e|e.to_string())?));
    let mut file=OpenOptions::new().write(true).create_new(true).open(&temp).map_err(|e|e.to_string())?;
    file.write_all(bytes).and_then(|_|file.sync_all()).map_err(|e|e.to_string())?;
    fs::hard_link(&temp,path).map_err(|e|e.to_string())?;
    fs::remove_file(&temp).map_err(|e|e.to_string())
    // Directory crash durability/path-reparse races are not proved here.
}
#[derive(Clone,Debug,Serialize,Deserialize,PartialEq,Eq)]
#[serde(tag="op",deny_unknown_fields)]
pub enum Command{
    #[serde(rename="RUN")] Run{number:u32,code:String,budget_millis:u32},
    #[serde(rename="RECEIVED")] Received{number:u32,task:u32,task_digest:String,result_ref:String},
    #[serde(rename="CANCEL")] Cancel{number:u32,task:u32,task_digest:String},
    #[serde(rename="CLOSE")] Close{number:u32},
}
impl Command{
    pub fn number(&self)->u32{match self{Self::Run{number,..}|Self::Received{number,..}|Self::Cancel{number,..}|Self::Close{number}=>*number}}
    pub fn wire(&self)->Result<String,String>{
        let n=self.number();if n==0{return Err("command_number".into());}
        Ok(match self{
            Self::Run{code,budget_millis,..}=>{if code.len()>MAX_CODE||!(1000..=120000).contains(budget_millis){return Err("run_fields".into());}format!("K1|{n:08x}|RUN|{n:08x}|{budget_millis:08x}|{}",hex::encode(code))},
            Self::Received{task,task_digest,result_ref,..}=>{if *task==0||!is_hex(task_digest,64)||!is_hex(result_ref,64){return Err("received_fields".into());}format!("K1|{n:08x}|RECEIVED|{task:08x}|{task_digest}|{result_ref}")},
            Self::Cancel{task,task_digest,..}=>{if *task==0||!is_hex(task_digest,64){return Err("cancel_fields".into());}format!("K1|{n:08x}|CANCEL|{task:08x}|{task_digest}")},
            Self::Close{..}=>format!("K1|{n:08x}|CLOSE|00000000"),
        })
    }
    pub fn digest(&self)->Result<String,String>{Ok(sha(self.wire()?.as_bytes()))}
}
#[derive(Clone,Debug,Serialize,Deserialize,PartialEq,Eq)]
#[serde(deny_unknown_fields)]
pub struct Binding{pub binding:String,pub runtime:String,pub session:String}
impl Binding{fn valid(&self)->bool{[&self.binding,&self.runtime,&self.session].iter().all(|s|is_hex(s,32))}}
#[derive(Debug)]
struct VerifiedState{
    high_water:u32,closed:bool,command_number:u32,command_digest:String,command_outcome:String,
    task:u32,task_digest:String,phase:String,result_state:String,result_ref:String,body_present:bool,body_bytes:usize,
    completion_task:u32,completion_ref:String,completion_received_number:u32,completion_received_digest:String,completion_result_state:String,
    received_number:u32,received_digest:String,received_outcome:String,
}
fn decode(wire:&str,binding:&Binding,key:&[u8])->Result<VerifiedState,String>{
    if wire.len()>8192||!wire.is_ascii(){return Err("state_size".into());}let f:Vec<_>=wire.split('|').collect();
    if f.len()!=35||f[0]!="LDBC2"{return Err("state_fields".into());}
    let raw=wire.rsplit_once('|').unwrap().0;let expected=gate::hmac(key,&[b"lycheedev.lifecycle.state.v2\0".as_slice(),raw.as_bytes()].concat());
    if !is_hex(f[34],64)||expected.bytes().zip(f[34].bytes()).fold(0,|v,(a,b)|v|(a^b))!=0{return Err("state_mac".into());}
    if f[1]!=binding.binding||f[2]!=binding.runtime||f[3]!=binding.session{return Err("state_binding".into());}
    let number=|i:usize|->Result<u32,String>{if !is_hex(f[i],8){return Err("state_number".into());}u32::from_str_radix(f[i],16).map_err(|e|e.to_string())};
    let boolean=|i:usize|->Result<bool,String>{match f[i]{"0"=>Ok(false),"1"=>Ok(true),_=>Err("state_boolean".into())}};
    for i in [8,11,13,16,25,27,30]{if !is_hex(f[i],64){return Err("state_digest".into());}}
    if !matches!(f[14],"none"|"running"|"running_unknown"|"terminal")||!matches!(f[15],"none"|"verified"|"unavailable")||!matches!(f[28],"none"|"verified"|"unavailable"){return Err("state_enum".into());}
    if f[9].is_empty()||f[9].len()>48||!f[9].bytes().all(|b|b==b'_'||b.is_ascii_lowercase()){return Err("state_outcome".into());}
    if !matches!(f[20],"none"|"pending"|"unknown"|"blocked"|"verified"|"release_pending"|"release_blocked"){return Err("state_cleanup".into());}
    for i in [21,22,33]{if !is_hex(f[i],2)||u8::from_str_radix(f[i],16).unwrap()>4{return Err("state_budget".into());}}
    for i in [4,10,19,23,32]{number(i)?;}
    if (f[21]!="00"||f[22]!="00")&&number(23)?==0||f[33]!="00"&&number(32)?==0{return Err("state_phase_budget_combination".into());}
    let x=VerifiedState{high_water:number(5)?,closed:boolean(6)?,command_number:number(7)?,command_digest:f[8].into(),command_outcome:f[9].into(),
        task:number(12)?,task_digest:f[13].into(),phase:f[14].into(),result_state:f[15].into(),result_ref:f[16].into(),body_present:boolean(17)?,body_bytes:number(18)?as usize,
        completion_task:number(24)?,completion_ref:f[25].into(),completion_received_number:number(26)?,completion_received_digest:f[27].into(),completion_result_state:f[28].into(),
        received_number:number(29)?,received_digest:f[30].into(),received_outcome:f[31].into()};
    if x.body_bytes>MAX_BODY||x.body_present!=(x.phase=="terminal")||(!x.body_present&&x.body_bytes!=0){return Err("state_body_combination".into());}
    if (x.task==0)!=(x.phase=="none")||(x.phase=="none"&&x.result_state!="none"){return Err("state_task_combination".into());}
    if x.command_number>x.high_water||(x.phase=="terminal"&&!matches!(x.result_state.as_str(),"verified"|"unavailable")){
        return Err("state_terminal_combination".into());
    }
    if (x.completion_task==0)!=(x.completion_received_number==0)
        ||(x.completion_task==0&&x.completion_result_state!="none")
        ||(x.completion_task!=0&&!matches!(x.completion_result_state.as_str(),"verified"|"unavailable")){
        return Err("state_completion_combination".into());
    }
    let zero="0".repeat(64);
    if (x.command_number==0)!=(x.high_water==0)
        ||(x.command_number==0&&(x.command_digest!=zero||x.command_outcome!="none")){return Err("state_command_combination".into());}
    if x.received_number==0 {
        if x.received_digest!=zero||x.received_outcome!="none"||x.completion_task!=0{return Err("state_received_combination".into());}
    }else{
        if x.received_number>x.high_water||x.received_digest==zero
            ||!matches!(x.received_outcome.as_str(),"cleanup_unknown"|"cleanup_pending"|"body_release_pending"|"cleanup_complete"|"clock_unavailable"|"cleanup_budget_exhausted")
            ||(x.command_number==x.received_number&&(x.command_digest!=x.received_digest||x.command_outcome!=x.received_outcome)){
            return Err("state_received_combination".into());
        }
        if x.received_outcome=="cleanup_complete" {
            if x.completion_received_number!=x.received_number||x.completion_received_digest!=x.received_digest||x.task!=0||x.body_present{return Err("state_received_completion".into());}
        }else if x.phase!="terminal"||!x.body_present||x.completion_task!=0{return Err("state_received_terminal".into());}
        let consistent=match x.received_outcome.as_str(){
            "cleanup_unknown"=>f[20]=="unknown","cleanup_pending"|"clock_unavailable"=>f[20]=="pending",
            "body_release_pending"=>f[20]=="release_pending","cleanup_complete"=>f[20]=="none",
            "cleanup_budget_exhausted"=>matches!(f[20],"blocked"|"release_blocked"),_=>false,
        };if !consistent{return Err("state_received_cleanup_combination".into());}
    }
    Ok(x)
}
#[derive(Serialize,Deserialize)]
#[serde(deny_unknown_fields)]
struct Intent{schema:String,binding:Binding,command:Command,digest:String}
#[derive(Serialize,Deserialize)]
#[serde(deny_unknown_fields)]
struct ResultMeta{schema:String,binding:Binding,task:u32,task_digest:String,result_ref:String,result_state:String,bytes:usize}
#[derive(Debug)]
pub enum Action{
    PrepareTransport(Command), // Intent is synced; transport must still obtain
                               // real fresh authentication before any input.
    FetchBody{result_ref:String,bytes:usize},
    Wait(&'static str),
    CleanupBlocked,
    Complete{result_state:String,result_ref:String},
    Quarantined(String),
}
#[derive(Clone)]
enum Preparation{Ready(Command),Consumed(Command)}
impl Preparation{fn command(&self)->&Command{match self{Self::Ready(c)|Self::Consumed(c)=>c}}}
pub struct Driver{directory:PathBuf,binding:Binding,key:Vec<u8>,run:Command,run_digest:String,preparations:RefCell<BTreeMap<u32,Preparation>>}
impl Driver{
    pub fn create(directory:PathBuf,binding:Binding,key:Vec<u8>,run:Command)->Result<Self,String>{
        if !binding.valid()||key.len()!=32||!matches!(run,Command::Run{..}){return Err("driver_context".into());}
        let run_digest=run.digest()?;
        fs::create_dir(&directory).map_err(|e|if e.kind()==std::io::ErrorKind::AlreadyExists{"journal_exists_use_restore".into()}else{e.to_string()})?;
        let x=Self{directory,binding,key,run,run_digest,preparations:RefCell::new(BTreeMap::new())};x.save_intent(&x.run)?;Ok(x)
    }
    pub fn restore(directory:PathBuf,binding:Binding,key:Vec<u8>,run_number:u32)->Result<Self,String>{
        if !binding.valid()||key.len()!=32{ return Err("driver_context".into());}let path=directory.join(format!("intent-{run_number:08x}.json"));
        if !path.exists(){return Err("missing_run_intent_quarantine".into());}
        let i:Intent=serde_json::from_slice(&read_bounded(&path,1<<20)?).map_err(|e|e.to_string())?;
        if i.schema!="lifecycle.intent.v1"||i.binding!=binding||i.command.number()!=run_number||!matches!(i.command,Command::Run{..})||i.digest!=i.command.digest()?{return Err("protected_record_changed".into());}
        let x=Self{directory,binding,key,run:i.command,run_digest:i.digest,preparations:RefCell::new(BTreeMap::new())};
        let intents=x.intents()?;
        *x.preparations.borrow_mut()=intents.into_iter().map(|(n,c)|(n,Preparation::Consumed(c))).collect();Ok(x)
    }
    fn save_intent(&self,c:&Command)->Result<(),String>{
        let path=self.directory.join(format!("intent-{:08x}.json",c.number()));let existed=path.exists();
        let i=Intent{schema:"lifecycle.intent.v1".into(),binding:self.binding.clone(),command:c.clone(),digest:c.digest()?};
        immutable(&path,&serde_json::to_vec(&i).unwrap())?;
        // Only this exact instance's successful first publication gives a
        // preparation capability. Reopening immutable bytes grants none.
        if !existed{self.preparations.borrow_mut().insert(c.number(),Preparation::Ready(c.clone()));}Ok(())
    }
    fn intents(&self)->Result<BTreeMap<u32,Command>,String>{
        let mut map=BTreeMap::new();let mut entries=0;
        for item in fs::read_dir(&self.directory).map_err(|e|e.to_string())?{let item=item.map_err(|e|e.to_string())?;entries+=1;if entries>128{return Err("journal_entry_budget".into());}
            let name=item.file_name();let Some(name)=name.to_str()else{continue};if !name.starts_with("intent-")||!name.ends_with(".json"){continue;}
            let id=name.strip_prefix("intent-").unwrap().strip_suffix(".json").unwrap();if !is_hex(id,8){return Err("journal_name".into());}let n=u32::from_str_radix(id,16).unwrap();
            let i:Intent=serde_json::from_slice(&read_bounded(&item.path(),1<<20)?).map_err(|e|e.to_string())?;
            if i.schema!="lifecycle.intent.v1"||i.binding!=self.binding||i.command.number()!=n||i.digest!=i.command.digest()?{return Err("protected_record_changed".into());}
            map.insert(n,i.command);if map.len()>MAX_INTENTS{return Err("journal_intent_budget".into());}
        }
        if map.get(&self.run.number())!=Some(&self.run){return Err("missing_run_intent_quarantine".into());}
        for (index,number) in map.keys().enumerate(){if self.run.number().checked_add(index as u32)!=Some(*number){return Err("journal_intent_gap_quarantine".into());}}
        for (n,p) in self.preparations.borrow().iter(){if map.get(n)!=Some(p.command()){return Err("known_intent_lost_or_changed_quarantine".into());}}
        // Seeing an intent written by another restored instance makes its
        // identity known, but cannot give this instance a fresh capability.
        for (n,c) in &map{self.preparations.borrow_mut().entry(*n).or_insert_with(||Preparation::Consumed(c.clone()));}
        Ok(map)
    }
    fn pending_bytes(&self,c:&Command)->Result<Vec<u8>,String>{Ok(serde_json::to_vec(&serde_json::json!({"number":c.number(),"digest":c.digest()?,"binding":self.binding,"state":"input_unknown_before_attempt"})).unwrap())}
    fn pending(&self,c:&Command)->Result<bool,String>{
        let path=self.directory.join(format!("input-pending-{:08x}.json",c.number()));
        if !path.exists(){return Ok(false);}
        if read_bounded(&path,4096)?!=self.pending_bytes(c)?{return Err("input_witness_changed_quarantine".into());}Ok(true)
    }
    fn unobserved(&self,c:&Command,reason:&'static str)->Result<Action,String>{
        if self.pending(c)?{return Ok(Action::Wait(reason));}
        if matches!(self.preparations.borrow().get(&c.number()),Some(Preparation::Ready(exact))if exact==c){return Ok(Action::PrepareTransport(c.clone()));}
        Err("unobserved_input_quarantine".into())
    }
    pub fn mark_business_input_pending(&self,c:&Command,wire:&str)->Result<(),String>{
        // Consume before every fallible check/write. An exception never grants
        // this instance another preparation capability for the same command.
        let ready={let mut p=self.preparations.borrow_mut();let ready=matches!(p.get(&c.number()),Some(Preparation::Ready(exact))if exact==c);
            if ready{p.insert(c.number(),Preparation::Consumed(c.clone()));}ready};
        let intents=self.intents()?;if intents.get(&c.number())!=Some(c){return Err("unknown_intent".into());}
        let s=decode(wire,&self.binding,&self.key)?;
        if !ready{
            // Tests may explicitly observe accepted duplicates. Neither a
            // pending witness nor a high-water mark proves exact acceptance.
            let accepted=match c{
                Command::Run{..}=>s.task==c.number()&&s.task_digest==c.digest()?,
                Command::Received{..}=>s.received_number==c.number()&&s.received_digest==c.digest()?,
                Command::Cancel{..}|Command::Close{..}=>s.command_number==c.number()&&s.command_digest==c.digest()?,
            };
            if !accepted||!self.pending(c)?{return Err("input_preparation_unavailable_quarantine".into());}
        }
        if let Command::Received{task,task_digest,result_ref,..}=c{
            let saved=self.saved_result()?.ok_or("durable_result_required")?;
            if *task!=saved.task||*task_digest!=saved.task_digest||*result_ref!=saved.result_ref{return Err("received_not_bound_to_saved_result".into());}
        }
        immutable(&self.directory.join(format!("input-pending-{:08x}.json",c.number())),&self.pending_bytes(c)?)
        // This records uncertainty BEFORE exposure. It does not claim delivery
        // or drain, and restoration does not resend unknown business input.
    }
    fn saved_result(&self)->Result<Option<ResultMeta>,String>{
        let meta_path=self.directory.join("result-meta.json");let body_path=self.directory.join("result.bin");
        if !meta_path.exists(){if body_path.exists(){return Err("body_without_meta_quarantine".into());}return Ok(None);}if !body_path.exists(){return Err("lost_result_bytes_quarantine".into());}
        let m:ResultMeta=serde_json::from_slice(&read_bounded(&meta_path,4096)?).map_err(|e|e.to_string())?;
        if m.schema!="lifecycle.result.v1"||m.binding!=self.binding||m.task!=self.run.number()||m.task_digest!=self.run_digest||!is_hex(&m.result_ref,64)||!matches!(m.result_state.as_str(),"verified"|"unavailable"){return Err("protected_result_changed".into());}
        let bytes=read_bounded(&body_path,MAX_BODY)?;if bytes.len()!=m.bytes||sha(&bytes)!=m.result_ref{return Err("protected_result_changed".into());}Ok(Some(m))
    }
    pub fn save_result(&self,wire:&str,body:&[u8])->Result<(),String>{
        let s=decode(wire,&self.binding,&self.key)?;
        if s.task!=self.run.number()||s.task_digest!=self.run_digest||s.phase!="terminal"||!s.body_present||body.len()!=s.body_bytes||sha(body)!=s.result_ref{return Err("body_identity_or_hash".into());}
        if self.directory.join("result-meta.json").exists(){self.saved_result()?;}
        immutable(&self.directory.join("result.bin"),body)?;
        let m=ResultMeta{schema:"lifecycle.result.v1".into(),binding:self.binding.clone(),task:s.task,task_digest:s.task_digest,result_ref:s.result_ref,result_state:s.result_state,bytes:body.len()};
        immutable(&self.directory.join("result-meta.json"),&serde_json::to_vec(&m).unwrap())
        // Real fsync'ed bytes precede metadata and every RECEIVED intent. The
        // fixture does not prove volume/dir durability under power loss.
    }
    pub fn prepare_cancel(&self)->Result<Command,String>{
        let intents=self.intents()?;if intents.len()>=MAX_INTENTS{return Err("journal_intent_budget".into());}let number=intents.keys().next_back().copied().unwrap_or(self.run.number()).checked_add(1).ok_or("command_exhausted")?;
        let c=Command::Cancel{number,task:self.run.number(),task_digest:self.run_digest.clone()};self.save_intent(&c)?;Ok(c)
    }
    pub fn prepare_close(&self)->Result<Command,String>{
        let intents=self.intents()?;if intents.len()>=MAX_INTENTS{return Err("journal_intent_budget".into());}
        let number=intents.keys().next_back().copied().unwrap_or(self.run.number()).checked_add(1).ok_or("command_exhausted")?;
        let c=Command::Close{number};self.save_intent(&c)?;Ok(c)
    }
    pub fn advance(&self,wire:&str)->Action{match self.advance_inner(wire){Ok(x)=>x,Err(error)=>Action::Quarantined(error)}}
    fn advance_inner(&self,wire:&str)->Result<Action,String>{
        let s=decode(wire,&self.binding,&self.key)?;let intents=self.intents()?;let max=intents.keys().next_back().copied().ok_or("missing_journal_quarantine")?;
        if s.high_water>max{return Err("missing_accepted_intent_quarantine".into());}
        if s.received_number!=0{
            let c=intents.get(&s.received_number).ok_or("received_intent_missing_quarantine")?;
            let observed_ref=if s.completion_received_number==s.received_number&&s.completion_task!=0{&s.completion_ref}else{&s.result_ref};
            if !matches!(c,Command::Received{task,task_digest,result_ref,..}if *task==self.run.number()&&*task_digest==self.run_digest&&result_ref==observed_ref)||c.digest()?!=s.received_digest{return Err("received_identity_quarantine".into());}
        }
        if s.completion_task==self.run.number(){
            let result=self.saved_result()?.ok_or("completion_without_durable_result")?;
            let c=intents.get(&s.completion_received_number).ok_or("completion_intent_missing")?;
            if !matches!(c,Command::Received{task,task_digest,result_ref,..}if *task==self.run.number()&&*task_digest==self.run_digest&&*result_ref==result.result_ref)
                ||c.digest()!=Ok(s.completion_received_digest.clone())||s.completion_ref!=result.result_ref||s.completion_result_state!=result.result_state{return Err("completion_identity".into());}
            // Store the exact completion identity, not unrelated mutable
            // command/revision fields in the current authenticated publication.
            immutable(&self.directory.join("completion.json"),&serde_json::to_vec(&serde_json::json!({
                "schema":"lifecycle.completion.v1","binding":self.binding,"task":self.run.number(),
                "taskDigest":self.run_digest,"receivedNumber":s.completion_received_number,
                "receivedDigest":s.completion_received_digest,"resultRef":result.result_ref,
                "resultState":result.result_state})).unwrap())?;
            return Ok(Action::Complete{result_state:result.result_state,result_ref:result.result_ref});
        }
        if s.closed{return Ok(Action::Wait("connection_closed_without_completion"));}
        if s.task==0{
            if s.high_water>=self.run.number(){return Ok(Action::Wait("accepted_task_or_result_unavailable"));}
            return self.unobserved(&self.run,"run_input_unknown_no_replay");
        }
        if s.task!=self.run.number()||s.task_digest!=self.run_digest{return Err("foreign_task_quarantine".into());}
        if s.phase!="terminal"{return Ok(Action::Wait("executing_or_unknown"));}
        let Some(result)=self.saved_result()?else{return Ok(Action::FetchBody{result_ref:s.result_ref,bytes:s.body_bytes})};
        if result.result_ref!=s.result_ref||result.result_state!=s.result_state{return Err("result_changed_after_save".into());}
        let received:Vec<_>=intents.values().filter(|c|matches!(c,Command::Received{..})).collect();
        if let Some(c)=received.last(){
            if s.received_number<c.number(){return self.unobserved(c,"received_input_unknown_no_replay");}
            if s.received_number!=c.number()||s.received_digest!=c.digest()?{return Err("accepted_content_conflict".into());}
            if !matches!(s.received_outcome.as_str(),"cleanup_unknown"|"cleanup_pending"|"body_release_pending"){return Ok(Action::CleanupBlocked);}
        }
        if received.len()>=MAX_RECEIVED||intents.len()>=MAX_INTENTS{return Ok(Action::CleanupBlocked);}
        let number=max.checked_add(1).ok_or("command_exhausted")?;
        let c=Command::Received{number,task:self.run.number(),task_digest:self.run_digest.clone(),result_ref:result.result_ref};self.save_intent(&c)?;self.unobserved(&c,"received_input_unknown_no_replay")
    }
}
