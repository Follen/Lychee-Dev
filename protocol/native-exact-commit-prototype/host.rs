//! Isolated exact-COMMIT host fixture, never imported by product modules.
//! Pending controlled compilation: no result is claimed from source alone.
#[path = "../native-v1-prototype/gate.rs"]
mod gate;
use std::{fs::{self,OpenOptions},io::{Read,Write},path::{Path,PathBuf},time::Instant};
use sha2::{Digest,Sha256};

#[derive(Clone,Debug,serde::Serialize)]
pub struct Request {
    binding:String,number:u32,nonce:String,digest:String,stage:u16,commit:u16,
    op:String,payload:String,raw:String,
}
impl Request {
    pub fn new(number:u32,stage:u16,op:&str,payload:&str)->Self {
        assert!(number>0 && stage>0 && stage<500 && stage%2==1);
        assert!(matches!(op,"RUN"|"RECEIVED") && payload.len()<=1024);
        let binding="1".repeat(32);
        let nonce=format!("{}{}",lycheedev_windows::random::token().unwrap(),lycheedev_windows::random::token().unwrap());
        let commit=stage+1;
        let payload=hex::encode(payload);
        let raw=format!("{binding}|{number:08x}|{nonce}|{stage:03}|{commit:03}|{op}|{payload}");
        let digest=hex::encode(Sha256::digest(raw.as_bytes()));
        Self {binding,number,nonce,digest,stage,commit,op:op.into(),payload,raw}
    }
    fn source(&self,kind:&str)->Vec<u8> {
        format!("return \"{kind}|{}|{:08x}|{}|{}|{:03}|{:03}|{}|{}\"\n",
            self.binding,self.number,self.nonce,self.digest,self.stage,self.commit,self.op,self.payload).into_bytes()
    }
}
fn record(path:&Path,bytes:&[u8])->Result<(),String> {
    let mut file=OpenOptions::new().write(true).create_new(true).open(path).map_err(|e|e.to_string())?;
    file.write_all(bytes).and_then(|_|file.sync_all()).map_err(|e|e.to_string())
}
/// Complete sealed file -> atomic no-replace fixture hard-link. This does not
/// prove directory/volume crash durability or managed-install coordination.
pub fn publish(path:&Path,bytes:&[u8])->Result<bool,String> {
    if bytes.len()>4096 {return Err("publication_size".into());}
    if path.exists(){
        let mut existing=Vec::new();fs::File::open(path).map_err(|e|e.to_string())?.take(4097).read_to_end(&mut existing).map_err(|e|e.to_string())?;
        if existing.len()>4096 {return Err("publication_existing_size".into());}
        return if existing==bytes {Ok(false)}else{Err("immutable_collision".into())};
    }
    let sealed=path.with_extension(format!("sealed-{}",lycheedev_windows::random::token().unwrap()));
    record(&sealed,bytes)?;
    fs::hard_link(&sealed,path).map_err(|e|e.to_string())?;
    Ok(true)
}
fn slot(dir:&Path,index:u16)->PathBuf {dir.join(format!("slot-{index:03}.lua"))}
pub struct Prepared {dir:PathBuf,request:Request,attempt:gate::Attempt}
pub struct Authorized {dir:PathBuf,request:Request,started:Instant}
#[derive(Debug)]
pub struct Receipt {pub outcome:String,pub consumed:bool,pub game_clock:u32,pub deadline:u32,pub executions:u32,pub loads:u32}
fn decode_receipt(wire:&str,x:&Request)->Result<Receipt,String> {
    if wire.len()>8192 || !wire.is_ascii(){return Err("receipt_size_or_encoding".into());}
    let f:Vec<_>=wire.split('|').collect();if f.len()!=16 || f[0]!="LDEC1" {return Err("receipt_fields".into());}
    let raw=wire.rsplit_once('|').unwrap().0;
    let expected=gate::hmac(&[0x19;32],&[b"lycheedev.exact.receipt\0".as_slice(),raw.as_bytes()].concat());
    if f[15].len()!=64 || expected.bytes().zip(f[15].bytes()).fold(0,|v,(a,b)|v|(a^b))!=0 {return Err("receipt_mac".into());}
    if f[1]!=x.binding || f[2]!="2".repeat(32) || f[3]!="3".repeat(32) || f[4]!=format!("{:08x}",x.number)
        || f[5]!=x.nonce || f[6]!=x.digest || f[7]!=format!("{:03}",x.stage) || f[8]!=format!("{:03}",x.commit){return Err("receipt_identity".into());}
    if !matches!(f[10],"0"|"1") {return Err("receipt_consumed".into());}
    let u32hex=|s:&str|->Result<u32,String>{if s.len()!=8 || !s.bytes().all(|b|b.is_ascii_digit()||(b'a'..=b'f').contains(&b)){return Err("receipt_clock".into());}u32::from_str_radix(s,16).map_err(|e|e.to_string())};
    let counter=|s:&str|->Result<u32,String>{if s.is_empty() || s.len()>3 || !s.bytes().all(|b|b.is_ascii_digit()){return Err("receipt_counter".into());}let v=s.parse::<u32>().map_err(|e|e.to_string())?;if v>500 {return Err("receipt_counter".into());}Ok(v)};
    Ok(Receipt{outcome:f[9].into(),consumed:f[10]=="1",game_clock:u32hex(f[11])?,deadline:u32hex(f[12])?,executions:counter(f[13])?,loads:counter(f[14])?})
}
pub fn prepare(dir:&Path,number:u32,stage:u16,op:&str,payload:&str)->Result<Prepared,String> {
    // Record time BEFORE acquiring the OS nonce, as required by the fresh gate.
    let started=Instant::now();
    let request=Request::new(number,stage,op,payload);
    let attempt=gate::Attempt::new(request.binding.clone(),"2".repeat(32),"3".repeat(32),number,
        request.nonce.clone(),request.digest.clone(),started).map_err(str::to_owned)?;
    record(&dir.join(format!("intent-{number}.json")),&serde_json::to_vec(&request).unwrap())?;
    publish(&slot(dir,request.stage),&request.source("STAGE"))?;
    Ok(Prepared{dir:dir.into(),request,attempt})
}
impl Prepared {
    /// Consuming transition; qualifier comes from actual authenticated Lua bytes.
    pub fn authorize(mut self,answer:&str)->Result<Authorized,String> {
        let observed=Instant::now();
        let wire=answer.strip_prefix("STATE|").ok_or("not_state")?;
        self.attempt.qualify(wire,&[0x19;32],observed).map_err(str::to_owned)?;
        record(&self.dir.join(format!("authorization-{}.json",self.request.number)),
            &serde_json::to_vec(&serde_json::json!({"request":self.request,"observation":wire,
                "observedElapsedMicros":observed.duration_since(self.attempt.started).as_micros()})).unwrap())?;
        Ok(Authorized{dir:self.dir,request:self.request,started:self.attempt.started})
    }
    pub fn number(&self)->u32 {self.request.number}
    pub fn receipt(&self,wire:&str)->Result<Receipt,String>{decode_receipt(wire,&self.request)}
}
impl Authorized {
    /// The pre/post publication checks do not make fsync/link atomic with time.
    /// If post-check fails, the file was EXPOSED and must remain uncertain.
    pub fn publish(&self)->Result<bool,String> {
        if self.started.elapsed()>gate::DELTA {return Err("publication_expired".into());}
        let published=publish(&slot(&self.dir,self.request.commit),&self.request.source("COMMIT"))?;
        if self.started.elapsed()>gate::DELTA {return Err("publication_exposed_but_expired".into());}
        Ok(published)
    }
    pub fn current_input_guard(&self)->Result<(),String>{if self.started.elapsed()>gate::DELTA {Err("new_input_expired".into())}else{Ok(())}}
    pub fn receipt(&self,wire:&str)->Result<Receipt,String>{decode_receipt(wire,&self.request)}
    pub fn accepted(&self,answer:&str)->bool {
        answer==format!("ACCEPTED|{:08x}|{}",self.request.number,self.request.digest)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{io::{BufRead,BufReader,Read},process::{Child,ChildStdin,Command,Stdio},sync::{mpsc,Arc,atomic::{AtomicBool,AtomicUsize,Ordering}},time::Duration};
    #[derive(Default)]
    struct DropProof{reader_lines:AtomicUsize,reader_joined:AtomicBool,child_terminal:AtomicBool,handles_dropped:AtomicBool}
    struct Lua {
        child:Child,input:Option<ChildStdin>,job:Option<lycheedev_windows::JobGuard>,
        receiver:Option<mpsc::Receiver<String>>,reader:Option<std::thread::JoinHandle<()>>,log:Vec<(String,String)>,dir:PathBuf,proof:Arc<DropProof>,
    }
    impl Lua {
        fn start(root:&Path,lua:&Path,dir:PathBuf)->Self {
            fs::create_dir_all(&dir).unwrap();
            let mut child=Command::new(lua).arg(root.join("protocol/native-exact-commit-prototype/receiver-auth-expiry.lua")).arg(root).arg(&dir)
                .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::inherit()).spawn().unwrap();
            let job=match lycheedev_windows::JobGuard::attach(&child){Ok(job)=>job,Err(error)=>{let _=child.kill();let _=child.wait();panic!("Lua job attach failed: {error}");}};
            let input=child.stdin.take();let output=child.stdout.take().unwrap();let(tx,receiver)=mpsc::sync_channel(16);
            let proof=Arc::new(DropProof::default());let reader_proof=proof.clone();
            let reader=std::thread::spawn(move||{
                let mut r=BufReader::new(output);
                for _ in 0..128 {let mut line=Vec::new();match r.by_ref().take(8193).read_until(b'\n',&mut line){
                    Ok(0)=>break,Ok(_) if line.len()<=8192 && line.last()==Some(&b'\n')=>{
                        reader_proof.reader_lines.fetch_add(1,Ordering::SeqCst);
                        if tx.send(String::from_utf8(line).unwrap().trim_end().into()).is_err(){break;}
                    },_=>break,
                }}
            });
            let mut x=Self{child,input,job:Some(job),receiver:Some(receiver),reader:Some(reader),log:Vec::new(),dir,proof};
            assert!(x.read().starts_with("READY|Lua51"));x
        }
        fn read(&mut self)->String {self.receiver.as_ref().unwrap().recv_timeout(Duration::from_secs(5)).expect("Lua response deadline")}
        fn send(&mut self,key:&str)->String {
            let input=self.input.as_mut().unwrap();writeln!(input,"{key}").unwrap();input.flush().unwrap();
            let answer=self.read();self.log.push((key.into(),answer.clone()));answer
        }
        fn send_new_commit(&mut self,a:&Authorized)->Result<String,String>{a.current_input_guard()?;Ok(self.send("COMMIT"))}
        fn mailbox(&mut self,a:&Authorized)->Receipt{a.receipt(&self.send("MAILBOX")).unwrap()}
        fn executions(&mut self)->u32 {self.send("STATUS").split('|').nth(1).unwrap().parse().unwrap()}
        fn stop(mut self){assert_eq!(self.send("EXIT"),"EXIT");self.input.take();
            let deadline=Instant::now()+Duration::from_secs(5);
            while self.child.try_wait().unwrap().is_none(){assert!(Instant::now()<deadline);std::thread::sleep(Duration::from_millis(10));}
            assert!(self.child.wait().unwrap().success());self.receiver.take();self.reader.take().unwrap().join().unwrap();self.job.take();
            record(&self.dir.join("transcript.json"),&serde_json::to_vec_pretty(&self.log).unwrap()).unwrap();
        }
    }
    impl Drop for Lua {fn drop(&mut self){
        self.input.take();if self.child.try_wait().ok().flatten().is_none(){if let Some(job)=&self.job{let _=job.terminate();}let _=self.child.kill();let _=self.child.wait();}
        // Drop the bounded receiver FIRST: a sender blocked on a full channel
        // then wakes with SendError. Reap reader even during fixture panics.
        self.receiver.take();if let Some(reader)=self.reader.take(){let _=reader.join();}self.proof.reader_joined.store(true,Ordering::SeqCst);
        self.proof.child_terminal.store(self.child.try_wait().ok().flatten().is_some(),Ordering::SeqCst);
        self.job.take();self.proof.handles_dropped.store(true,Ordering::SeqCst);
        let _=record(&self.dir.join("drop-transcript.json"),&serde_json::to_vec(&self.log).unwrap());
    }}
    #[test]
    fn exact_commit_publication_cuts_with_real_lua51(){
        let root=std::env::var_os("LYCHEEDEV_EXACT_PROTOTYPE_ROOT").map(PathBuf::from).expect("explicit fixture root");
        let lua=std::env::var_os("LYCHEEDEV_LUA51").map(PathBuf::from).unwrap_or_else(||PathBuf::from("C:/Program Files (x86)/Lua/5.1/lua.exe"));
        let evidence=root.join(".tmp/rust-supervision").join(format!("native-exact-commit-rust-{}",lycheedev_windows::random::token().unwrap()));
        fs::create_dir_all(&evidence).unwrap();
        let mut files=vec![root.join("protocol/native-exact-commit-prototype/host.rs"),root.join("protocol/native-exact-commit-prototype/receiver-auth-expiry.lua"),
            root.join("protocol/native-v1-prototype/gate.rs"),root.join("protocol/native-v1-prototype/crypto.lua"),root.join("protocol/native-v1-prototype/observation.lua"),lua.clone()];
        files.push(std::env::current_exe().unwrap());
        let inputs:Vec<_>=files.iter().map(|path|serde_json::json!({"path":path,"sha256":hex::encode(Sha256::digest(fs::read(path).unwrap()))})).collect();
        record(&evidence.join("inputs.json"),&serde_json::to_vec_pretty(&serde_json::json!({"inputs":inputs})).unwrap()).unwrap();
        let mut facts=Vec::new();
        for authorized_b in [false,true]{
            let dir=evidence.join(if authorized_b {"fifo-B-authorized"}else{"fifo-B-unpublished"});
            let mut w=Lua::start(&root,&lua,dir.clone());
            let a=prepare(&dir,7,1,"RECEIVED","A").unwrap().authorize(&w.send("STAGE")).unwrap();a.publish().unwrap();assert!(a.accepted(&w.send_new_commit(&a).unwrap()));assert_eq!(w.mailbox(&a).outcome,"accepted");
            let b=prepare(&dir,8,3,"RUN","return 7").unwrap();let observation=w.send("STAGE");
            if authorized_b {let b=b.authorize(&observation).unwrap();b.publish().unwrap();assert!(b.accepted(&w.send("COMMIT")));assert_eq!(w.mailbox(&b).outcome,"accepted");assert_eq!(w.executions(),2);}
            else {assert_eq!(w.send("COMMIT"),"ERR|empty_slot");let receipt=b.receipt(&w.send("MAILBOX")).unwrap();assert_eq!(receipt.outcome,"commit_empty_slot");assert!(receipt.consumed);assert_eq!(receipt.executions,1);assert_eq!(w.executions(),1);}
            facts.push(if authorized_b {"old key executes only already exact-authorized B"}else{"FIFO old key cannot execute unauthorized B"});w.stop();
        }
        for cut in ["prepared","observed","authorized","published"]{
            let dir=evidence.join(format!("cut-{cut}"));let mut w=Lua::start(&root,&lua,dir.clone());let p=prepare(&dir,1,1,"RUN","return 7").unwrap();assert_eq!(p.number(),1);
            if cut=="prepared" {assert_eq!(w.send("COMMIT"),"ERR|no_pending");}
            else {let observation=w.send("STAGE");if cut!="observed" {let a=p.authorize(&observation).unwrap();if cut=="published" {assert!(a.publish().unwrap());assert!(!a.publish().unwrap());assert!(a.accepted(&w.send_new_commit(&a).unwrap()));assert_eq!(w.mailbox(&a).outcome,"accepted");}else{assert_eq!(w.send("COMMIT"),"ERR|empty_slot");let receipt=w.mailbox(&a);assert_eq!(receipt.outcome,"commit_empty_slot");assert!(receipt.consumed);}}
                else{assert_eq!(w.send("COMMIT"),"ERR|empty_slot");}}
            assert_eq!(w.executions(),if cut=="published"{1}else{0});facts.push(cut);w.stop();
        }
        for clock_advances in [true,false]{
            let dir=evidence.join(if clock_advances {"advancing-game-clock"}else{"frozen-game-clock-counterexample"});let mut w=Lua::start(&root,&lua,dir.clone());
            let a=prepare(&dir,1,1,"RUN","return 7").unwrap().authorize(&w.send("STAGE")).unwrap();a.publish().unwrap();
            if clock_advances{assert_eq!(w.send("CLOCK|10751"),"OK|clock_model");assert_eq!(w.send("COMMIT"),"ERR|private_expired");let receipt=w.mailbox(&a);assert_eq!(receipt.outcome,"private_expired");assert!(!receipt.consumed);assert_eq!(receipt.game_clock,receipt.deadline+1);assert_eq!(receipt.executions,0);assert_eq!(receipt.loads,1);}
            else{std::thread::sleep(Duration::from_millis(800));assert_eq!(w.send_new_commit(&a),Err("new_input_expired".into()));assert_eq!(a.publish(),Err("publication_expired".into()));
                // Deliberate delivery of an OLD queued callback bypasses the
                // new-input path to demonstrate the frozen-clock counterexample.
                assert!(a.accepted(&w.send("COMMIT")));assert_eq!(w.mailbox(&a).executions,1);}
            facts.push(if clock_advances {"model advancing game clock rejects exposed expired COMMIT"}else{"expected frozen-game-clock counterexample; new-input guard rejects actual late path"});w.stop();
        }
        let mut proof=None;
        let caught=std::panic::catch_unwind(std::panic::AssertUnwindSafe(||{
            let mut w=Lua::start(&root,&lua,evidence.join("expected-host-panic-full-channel"));proof=Some(w.proof.clone());
            let input=w.input.as_mut().unwrap();for _ in 0..32 {writeln!(input,"STATUS").unwrap();}input.flush().unwrap();
            let deadline=Instant::now()+Duration::from_secs(5);
            while w.proof.reader_lines.load(Ordering::SeqCst)<18 {assert!(Instant::now()<deadline);std::thread::sleep(Duration::from_millis(1));}
            panic!("expected fixture abort with full bounded output channel");
        }));
        assert!(caught.is_err());let proof=proof.unwrap();assert!(proof.reader_joined.load(Ordering::SeqCst));assert!(proof.child_terminal.load(Ordering::SeqCst));assert!(proof.handles_dropped.load(Ordering::SeqCst));
        facts.push("expected host panic: child terminal, bounded reader unblocked/joined, JobGuard dropped");
        record(&evidence.join("result.json"),&serde_json::to_vec_pretty(&serde_json::json!({"state":"completed_scoped_rust_lua_checks","facts":facts,
            "scope":"Rust host transitions, real Lua51 button callbacks, once-loader model, fixed binding",
            "finalProtocolPassed":false,"unproven":["Windows/WoW inputs","clock across pause","B1","cold private key boundary","500 pool allocation","BODY cleanup","process crash/directory durability"]})).unwrap()).unwrap();
        println!("EXACT_COMMIT_EVIDENCE={}",evidence.display());
    }
}
