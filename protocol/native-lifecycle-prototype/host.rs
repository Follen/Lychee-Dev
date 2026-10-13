//! Prepared shared Rust/real-Lua51 comparison. Not a product or OS input path.
//! Import into an integration-test wrapper only in an approved build window.
#[path="driver.rs"]
mod driver;
#[path="../native-v1-prototype/gate.rs"]
mod gate;

#[cfg(test)]
mod tests {
    use super::{driver::{Action,Binding,Command,Driver},gate};
    use sha2::{Digest,Sha256};
    use std::{fs::{self,OpenOptions},io::{BufRead,BufReader,Read,Write},path::{Path,PathBuf},
        process::{Child,ChildStdin,Command as ProcessCommand,Stdio},sync::mpsc,time::{Duration,Instant}};
    const KEY:[u8;32]=[0x19;32];
    const MAX_LINE:u64=1_048_583;
    fn digest(bytes:&[u8])->String{hex::encode(Sha256::digest(bytes))}
    fn record(path:&Path,bytes:&[u8]) {
        let mut file=OpenOptions::new().create_new(true).write(true).open(path).unwrap();
        file.write_all(bytes).unwrap();file.sync_all().unwrap();
    }
    fn binding()->Binding{Binding{binding:"1".repeat(32),runtime:"2".repeat(32),session:"3".repeat(32)}}
    fn run(code:&str,budget:u32)->Command{Command::Run{number:1,code:code.into(),budget_millis:budget}}
    fn prepare(action:Action)->Command{match action{Action::PrepareTransport(c)=>c,x=>panic!("expected prepared, got {x:?}")}}
    fn complete(action:Action,state:&str){assert!(matches!(action,Action::Complete{result_state,..}if result_state==state));}
    fn wait(action:Action,reason:&str){assert!(matches!(action,Action::Wait(x)if x==reason));}
    fn quarantine(action:Action,reason:&str){assert!(matches!(action,Action::Quarantined(x)if x==reason));}

    struct World {
        child:Child,input:Option<ChildStdin>,job:Option<lycheedev_windows::JobGuard>,
        receiver:Option<mpsc::Receiver<String>>,reader:Option<std::thread::JoinHandle<()>>,
        directory:PathBuf,transport:&'static str,next_slot:u16,transcript:Vec<(String,String)>,
        published_slots:u16,token_bytes:usize,semantic_keys:u16,
        cost_events:Vec<serde_json::Value>,
    }
    impl World {
        fn start(root:&Path,lua:&Path,directory:PathBuf,transport:&'static str)->Self {
            fs::create_dir_all(directory.join("slots")).unwrap();
            let mut child=ProcessCommand::new(lua).arg(root.join("protocol/native-lifecycle-prototype/receiver.lua"))
                .arg(root).arg(directory.join("slots")).arg(transport).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::inherit()).spawn().unwrap();
            let job=match lycheedev_windows::JobGuard::attach(&child){Ok(job)=>job,Err(e)=>{let _=child.kill();let _=child.wait();panic!("Lua job attach: {e}");}};
            let input=child.stdin.take();let output=child.stdout.take().unwrap();let(tx,receiver)=mpsc::sync_channel(4);
            let reader=std::thread::spawn(move||{
                let mut output=BufReader::new(output);
                for _ in 0..4096 {
                    let mut bytes=Vec::new();
                    match output.by_ref().take(MAX_LINE+1).read_until(b'\n',&mut bytes) {
                        Ok(0)=>break,
                        Ok(_)if bytes.len()<=MAX_LINE as usize&&bytes.last()==Some(&b'\n')=>{
                            let Ok(line)=String::from_utf8(bytes)else{break};if tx.send(line.trim_end_matches(['\r','\n']).into()).is_err(){break;}
                        },_=>break,
                    }
                }
            });
            let mut world=Self{child,input,job:Some(job),receiver:Some(receiver),reader:Some(reader),directory,transport,
                next_slot:1,transcript:vec![],published_slots:0,token_bytes:0,semantic_keys:0,cost_events:vec![]};
            assert_eq!(world.read(),format!("READY|Lua51|shared-business|transport-{transport}"));world
        }
        fn read(&mut self)->String{self.receiver.as_ref().unwrap().recv_timeout(Duration::from_secs(60)).expect("bounded Lua response")}
        fn send(&mut self,text:&str)->String {
            assert!(text.len()<=8192&&!text.contains('\n'));
            let input=self.input.as_mut().unwrap();writeln!(input,"{text}").unwrap();input.flush().unwrap();
            let answer=self.read();self.transcript.push((text.into(),answer.clone()));answer
        }
        fn mailbox(&mut self)->String{self.send("MAILBOX")}
        fn private_status(&mut self)->Vec<String>{self.send("STATUS").split('|').map(str::to_owned).collect()}
        fn fault(&mut self,name:&str){assert_eq!(self.send(&format!("FAULT|{name}")),"OK|fault");}
        fn publish_slot(&mut self,index:u16,wire:&str) {
            assert!((1..=500).contains(&index));
            let path=self.directory.join("slots").join(format!("slot-{index:03}.lua"));
            // A bounded exact string returned by a once-loader, not executable
            // host-dispatched business code. wire only contains ASCII hex/|.
            let source=format!("return \"{wire}\"\n");assert!(source.len()<=1_048_800);
            record(&path,&source.as_bytes());self.published_slots+=1;
        }
        fn execute(&mut self,driver:&Driver,c:&Command)->String {
            let before=(self.published_slots,self.token_bytes,self.semantic_keys);
            let result=self.execute_inner(driver,c,false).unwrap();
            self.cost_events.push(serde_json::json!({"commandNumber":c.number(),"commandDigest":c.digest().unwrap(),
                "operation":c.wire().unwrap().split('|').nth(2).unwrap(),"publishedSlots":self.published_slots-before.0,
                "tokenBytes":self.token_bytes-before.1,"finalCallbackKeys":self.semantic_keys-before.2,"answer":result}));
            result
        }
        fn execute_inner(&mut self,driver:&Driver,c:&Command,omit_final_key:bool)->Result<String,String> {
            let started=Instant::now();
            let nonce=format!("{}{}",lycheedev_windows::random::token().unwrap(),lycheedev_windows::random::token().unwrap());
            let stage=self.next_slot;let commit=if self.transport=="four"{stage+1}else{0};
            self.next_slot+=if self.transport=="four"{2}else{1};
            let logical=c.wire()?;let digest=c.digest()?;
            let mut attempt=gate::Attempt::new("1".repeat(32),"2".repeat(32),"3".repeat(32),c.number(),nonce.clone(),digest.clone(),started).map_err(str::to_owned)?;
            self.publish_slot(stage,&format!("P3S1|{}|{nonce}|{stage:03}|{commit:03}|{digest}|{}","1".repeat(32),hex::encode(logical)));
            let answer=self.send("STAGE");let observation=answer.strip_prefix("STATE|").ok_or("stage_rejected")?;
            attempt.qualify(observation,&KEY,Instant::now()).map_err(str::to_owned)?;
            let state=gate::decode(observation,&KEY).map_err(str::to_owned)?;
            // Exact fsync'ed uncertainty witness exists before file/token
            // exposure. This is shared for normal and restored driver calls.
            driver.mark_business_input_pending(c)?;
            if started.elapsed()>gate::DELTA{return Err("publication_expired".into());}
            if self.transport=="four"{
                self.publish_slot(commit,&format!("P3C1|{}|{nonce}|{stage:03}|{commit:03}|{digest}","1".repeat(32)));
            }else{
                let deadline=state.game_clock.checked_add(750).ok_or("clock_overflow")?;
                let raw=format!("{}:{nonce}:{:08x}:{digest}:{stage:03}:{deadline:08x}","1".repeat(32),c.number());
                let mac=gate::hmac(&KEY,&[b"lycheedev.lifecycle.token\0".as_slice(),raw.as_bytes()].concat());
                let token=format!("P3T1:{raw}:{mac}");assert_eq!(token.len(),254);self.token_bytes+=token.len();
                assert_eq!(self.send(&format!("TEXT|{token}")),"TEXT|inert");
            }
            if started.elapsed()>gate::DELTA{return Err("publication_exposed_but_expired".into());}
            if omit_final_key{return Ok("WITHHELD|final_key".into());}
            self.semantic_keys+=1;Ok(self.send(if self.transport=="four"{"COMMIT"}else{"ENTER"}))
        }
        fn save(&mut self,driver:&Driver) {
            let state=self.mailbox();assert!(matches!(driver.advance(&state),Action::FetchBody{..}));
            let bytes=self.send("BODY");let bytes=hex::decode(bytes.strip_prefix("BODY|").unwrap()).unwrap();
            driver.save_result(&state,&bytes).unwrap();
        }
        fn finish(&mut self,driver:&Driver,state:&str) {
            self.save(driver);let c=prepare(driver.advance(&self.mailbox()));
            assert_eq!(self.execute(driver,&c),"DISPATCH|cleanup_complete|published");
            complete(driver.advance(&self.mailbox()),state);
        }
        fn restore(&self)->Driver{Driver::restore(self.directory.join("journal"),binding(),KEY.to_vec(),1).unwrap()}
        fn stop(mut self) {
            assert_eq!(self.send("EXIT"),"EXIT");self.input.take();let deadline=Instant::now()+Duration::from_secs(5);
            while self.child.try_wait().unwrap().is_none(){assert!(Instant::now()<deadline);std::thread::sleep(Duration::from_millis(10));}
            assert!(self.child.wait().unwrap().success());self.receiver.take();self.reader.take().unwrap().join().unwrap();self.job.take();
            record(&self.directory.join("transcript.json"),&serde_json::to_vec_pretty(&self.transcript).unwrap());
            record(&self.directory.join("cost.json"),&serde_json::to_vec_pretty(&serde_json::json!({
                "transport":self.transport,"publishedSlots":self.published_slots,"tokenBytes":self.token_bytes,"finalCallbackKeys":self.semantic_keys,
                "perCompletedTransportAttempt":self.cost_events,
                "scope":"fixture counters only; no actual OS input/performance proof"})).unwrap());
        }
    }
    impl Drop for World {
        fn drop(&mut self) {
            self.input.take();if self.child.try_wait().ok().flatten().is_none(){if let Some(job)=&self.job{let _=job.terminate();}let _=self.child.kill();let _=self.child.wait();}
            self.receiver.take();if let Some(reader)=self.reader.take(){let _=reader.join();}self.job.take();
            if !self.directory.join("transcript.json").exists(){let _=fs::write(self.directory.join("failed-transcript.json"),serde_json::to_vec_pretty(&self.transcript).unwrap());}
        }
    }
    fn scenario(root:&Path,lua:&Path,evidence:&Path,transport:&'static str,name:&str,code:&str,f:impl FnOnce(&mut World,&Driver)) {
        let mut world=World::start(root,lua,evidence.join(format!("{transport}-{name}")),transport);
        let driver=Driver::create(world.directory.join("journal"),binding(),KEY.to_vec(),run(code,1000)).unwrap();
        f(&mut world,&driver);world.stop();
    }
    #[test]
    fn shared_lifecycle_fault_matrix_with_real_lua51() {
        let root=std::env::var_os("LYCHEEDEV_LIFECYCLE_PROTOTYPE_ROOT").map(PathBuf::from).expect("explicit root; test is not silently skipped");
        let lua=std::env::var_os("LYCHEEDEV_LUA51").map(PathBuf::from).unwrap_or_else(||PathBuf::from("C:/Program Files (x86)/Lua/5.1/lua.exe"));
        let evidence=root.join(".tmp/rust-supervision").join(format!("native-shared-lifecycle-{}",lycheedev_windows::random::token().unwrap()));
        fs::create_dir_all(&evidence).unwrap();
        let paths=[root.join("protocol/native-lifecycle-prototype/host.rs"),root.join("protocol/native-lifecycle-prototype/driver.rs"),
            root.join("protocol/native-lifecycle-prototype/business.lua"),root.join("protocol/native-lifecycle-prototype/receiver.lua"),root.join("protocol/native-lifecycle-prototype/commands.lua"),
            root.join("protocol/native-v1-prototype/gate.rs"),root.join("protocol/native-v1-prototype/crypto.lua"),root.join("protocol/native-v1-prototype/observation.lua"),lua,std::env::current_exe().unwrap(),
            root.join("crates/lycheedev-live/tests/native_shared_lifecycle_prototype.rs")];
        let inputs:Vec<_>=paths.iter().map(|p|serde_json::json!({"path":p,"sha256":digest(&fs::read(p).unwrap())})).collect();
        record(&evidence.join("inputs.json"),&serde_json::to_vec_pretty(&inputs).unwrap());
        // Emit before the first scenario so an actual test failure can still
        // seal the exact executable, matching symbols and partial transcripts.
        println!("SHARED_LIFECYCLE_EVIDENCE={}",evidence.display());
        for transport in ["two","four"] {
            scenario(&root,&paths[8],&evidence,transport,"normal-restore","return 'body-中文'",|w,d|{
                let c=prepare(d.advance(&w.mailbox()));assert_eq!(w.execute(d,&c),"DISPATCH|accepted|published");w.save(d);
                let restored=w.restore();let received=prepare(restored.advance(&w.mailbox()));
                assert_eq!(w.execute(&restored,&received),"DISPATCH|cleanup_complete|published");complete(restored.advance(&w.mailbox()),"verified");
                let before=fs::read(w.directory.join("journal/completion.json")).unwrap();
                // A later CLOSE has its own exact intent and cannot erase the
                // separately authenticated previous completion.
                let close=restored.prepare_close().unwrap();assert_eq!(w.execute(&restored,&close),"DISPATCH|closed|published");
                complete(restored.advance(&w.mailbox()),"verified");assert_eq!(fs::read(w.directory.join("journal/completion.json")).unwrap(),before);
                assert_eq!(w.execute(&restored,&close),"DISPATCH|closed|published");
                assert_eq!(w.private_status()[6],"1");
            });
            for effect in ["cleanup","clear"] {for return_kind in ["false","nil","throw"] {for boundary in ["before","after"] {
                let fault=format!("{effect}_{return_kind}_{boundary}");
                scenario(&root,&paths[8],&evidence,transport,&fault,"return 'owned-body'",|w,d|{
                    let c=prepare(d.advance(&w.mailbox()));w.execute(d,&c);w.save(d);w.fault(&fault);
                    let c=prepare(d.advance(&w.mailbox()));let outcome=w.execute(d,&c);
                    assert_eq!(outcome,if effect=="cleanup"{"DISPATCH|cleanup_unknown|published"}else{"DISPATCH|body_release_pending|published"});
                    assert_eq!(w.private_status()[2],"1");
                    let original_deadline=w.private_status()[5].clone();
                    // Same number is an immutable history read, never a retry
                    // that can perform cleanup twice or reset its deadline.
                    assert_eq!(w.execute(d,&c),outcome);assert_eq!(w.private_status()[5],original_deadline);
                    let restored=w.restore();let retry=prepare(restored.advance(&w.mailbox()));
                    assert_eq!(w.execute(&restored,&retry),"DISPATCH|cleanup_complete|published");complete(restored.advance(&w.mailbox()),"verified");
                    let status=w.private_status();assert_eq!(status[8],"1");
                    if effect=="cleanup"&&boundary=="after"{assert_eq!(status[7],"1");}
                });
            }}}
            for effect in ["body","state"] {for return_kind in ["false","nil","throw"] {for boundary in ["before","after"] {
                let fault=format!("{effect}_{return_kind}_{boundary}");
                scenario(&root,&paths[8],&evidence,transport,&fault,"return 'publication-body'",|w,d|{
                    let c=prepare(d.advance(&w.mailbox()));w.fault(&fault);assert_eq!(w.execute(d,&c),"DISPATCH|accepted|unconfirmed");
                    assert_eq!(w.private_status()[2],"1");assert_eq!(w.send("REPUBLISH"),"OK|republished");
                    assert_eq!(w.private_status()[6],"1");w.finish(d,"verified");
                });
            }}}
            scenario(&root,&paths[8],&evidence,transport,"unknown-final-key","return 'late-owned'",|w,d|{
                let c=prepare(d.advance(&w.mailbox()));assert_eq!(w.execute_inner(d,&c,true).unwrap(),"WITHHELD|final_key");
                wait(w.restore().advance(&w.mailbox()),"run_input_unknown_no_replay");assert_eq!(w.private_status()[6],"0");
                // The queued original key is released exactly once by the
                // fixture. Recovery itself did not resend any semantic input.
                assert_eq!(w.send(if transport=="four"{"COMMIT"}else{"ENTER"}),"DISPATCH|accepted|published");w.finish(d,"verified");
            });
            scenario(&root,&paths[8],&evidence,transport,"lost-input-witness","return 'must-not-replay'",|w,d|{
                let c=prepare(d.advance(&w.mailbox()));w.execute_inner(d,&c,true).unwrap();
                fs::remove_file(w.directory.join("journal/input-pending-00000001.json")).unwrap();
                quarantine(w.restore().advance(&w.mailbox()),"restored_unobserved_input_quarantine");assert_eq!(w.private_status()[6],"0");
            });
            for fault in ["execute_false_before","execute_nil_before","execute_throw_before","execute_false_after","execute_nil_after","execute_launch_throw"] {
                scenario(&root,&paths[8],&evidence,transport,fault,"return 'eventual-owned-result'",|w,d|{
                    let c=prepare(d.advance(&w.mailbox()));w.fault(fault);assert_eq!(w.execute(d,&c),"DISPATCH|accepted_execution_unknown|published");
                    wait(d.advance(&w.mailbox()),"executing_or_unknown");assert_eq!(w.private_status()[1],"running_unknown");
                    let done=w.send("DONE");if fault.ends_with("before"){assert_eq!(done,"OK|no_execution");
                        assert_eq!(w.send("CLOCK|11001"),"OK|clock_model");wait(d.advance(&w.mailbox()),"executing_or_unknown");
                    }else{assert_eq!(done,"OK|completed");w.finish(d,"verified");assert_eq!(w.private_status()[6],"1");}
                });
            }
            scenario(&root,&paths[8],&evidence,transport,"timeout-confirmed-stop","return 'not-yet-executed'",|w,d|{
                assert_eq!(w.send("ASYNC_ON"),"OK|async");let c=prepare(d.advance(&w.mailbox()));w.execute(d,&c);
                assert_eq!(w.send("CLOCK|11001"),"OK|clock_model");w.finish(d,"unavailable");assert_eq!(w.private_status()[6],"0");
            });
            scenario(&root,&paths[8],&evidence,transport,"timeout-unconfirmed-stop","return 'actual-late-result'",|w,d|{
                w.send("ASYNC_ON");let c=prepare(d.advance(&w.mailbox()));w.execute(d,&c);w.fault("cancel_false_before");
                w.send("CLOCK|11001");wait(w.restore().advance(&w.mailbox()),"executing_or_unknown");
                assert_eq!(w.private_status()[1],"running_unknown");assert_eq!(w.private_status()[9],"1");
                w.send("CLOCK|11002");assert_eq!(w.private_status()[9],"1");assert_eq!(w.private_status()[6],"0");
                assert_eq!(w.send("DONE"),"OK|completed");w.finish(d,"verified");assert_eq!(w.private_status()[6],"1");
            });
            for fault in ["cancel_false_before","cancel_nil_before","cancel_throw_before","cancel_false_after","cancel_nil_after","cancel_throw_after"] {
                scenario(&root,&paths[8],&evidence,transport,fault,"return 'cancel-race'",|w,d|{
                    w.send("ASYNC_ON");let c=prepare(d.advance(&w.mailbox()));w.execute(d,&c);w.fault(fault);
                    let cancel=d.prepare_cancel().unwrap();assert_eq!(w.execute(d,&cancel),"DISPATCH|cancel_unconfirmed|published");
                    wait(d.advance(&w.mailbox()),"executing_or_unknown");let second=d.prepare_cancel().unwrap();
                    assert_eq!(w.execute(d,&second),"DISPATCH|cancel_stop_confirmed|published");w.finish(d,"unavailable");
                    assert_eq!(w.private_status()[10],"1");
                });
            }
            scenario(&root,&paths[8],&evidence,transport,"tamper-and-lost-body","return 'exact-private-body'",|w,d|{
                let c=prepare(d.advance(&w.mailbox()));w.execute(d,&c);let state=w.mailbox();w.send("PUBLIC_BODY_TAMPER");
                assert_eq!(d.save_result(&state,b"forged-body"),Err("body_identity_or_hash".into()));
                w.send("PUBLIC_STATE_TAMPER");quarantine(d.advance(&w.mailbox()),"state_fields");w.send("REPUBLISH");w.save(d);
                fs::remove_file(w.directory.join("journal/result.bin")).unwrap();quarantine(w.restore().advance(&w.mailbox()),"lost_result_bytes_quarantine");
                assert_eq!(w.private_status()[2],"1");assert_eq!(w.private_status()[8],"0");
            });
            scenario(&root,&paths[8],&evidence,transport,"cleanup-budget","return 'body-remains'",|w,d|{
                let c=prepare(d.advance(&w.mailbox()));w.execute(d,&c);w.save(d);
                for _ in 0..4 {w.fault("cleanup_false_before");let c=prepare(d.advance(&w.mailbox()));assert_eq!(w.execute(d,&c),"DISPATCH|cleanup_unknown|published");}
                assert!(matches!(w.restore().advance(&w.mailbox()),Action::CleanupBlocked));let s=w.private_status();assert_eq!(s[2],"1");assert_eq!(s[4],"4");assert_eq!(s[7],"4");assert_eq!(s[8],"0");
            });
            scenario(&root,&paths[8],&evidence,transport,"clear-budget","return 'private-body-remains'",|w,d|{
                let c=prepare(d.advance(&w.mailbox()));w.execute(d,&c);w.save(d);
                for _ in 0..4 {w.fault("clear_false_before");let c=prepare(d.advance(&w.mailbox()));assert_eq!(w.execute(d,&c),"DISPATCH|body_release_pending|published");}
                assert!(matches!(w.restore().advance(&w.mailbox()),Action::CleanupBlocked));let s=w.private_status();assert_eq!(s[2],"1");assert_eq!(s[7],"1");assert_eq!(s[8],"1");assert_eq!(s[11],"0");
            });
            scenario(&root,&paths[8],&evidence,transport,"producer-wins-cancel","return 'already-produced'",|w,d|{
                w.send("ASYNC_ON");let c=prepare(d.advance(&w.mailbox()));w.execute(d,&c);w.send("DONE");
                let cancel=d.prepare_cancel().unwrap();assert_eq!(w.execute(d,&cancel),"DISPATCH|task_already_terminal|published");
                w.finish(d,"verified");assert_eq!(w.private_status()[9],"0");assert_eq!(w.private_status()[6],"1");
            });
            scenario(&root,&paths[8],&evidence,transport,"history-eviction","return 'history-body'",|w,d|{
                let original=prepare(d.advance(&w.mailbox()));w.execute(d,&original);
                for _ in 0..18 {let c=d.prepare_cancel().unwrap();assert_eq!(w.execute(d,&c),"DISPATCH|task_already_terminal|published");}
                w.finish(d,"verified");assert_eq!(w.execute_inner(d,&original,false),Err("input_blocked".into()));
                let s=w.private_status();assert_eq!(s[6],"1");assert_eq!(s[8],"1");
            });
            scenario(&root,&paths[8],&evidence,transport,"body-limit-exact","return string.rep('x',524288)",|w,d|{
                let c=prepare(d.advance(&w.mailbox()));w.execute(d,&c);w.finish(d,"verified");
                assert_eq!(fs::metadata(w.directory.join("journal/result.bin")).unwrap().len(),524288);
            });
            scenario(&root,&paths[8],&evidence,transport,"body-limit-over","return string.rep('x',524289)",|w,d|{
                let c=prepare(d.advance(&w.mailbox()));w.execute(d,&c);w.finish(d,"unavailable");
                assert_eq!(fs::read(w.directory.join("journal/result.bin")).unwrap(),b"result_encoding_or_limit");
            });
            scenario(&root,&paths[8],&evidence,transport,"original-cleanup-deadline","return 'deadline-body'",|w,d|{
                let c=prepare(d.advance(&w.mailbox()));w.execute(d,&c);w.save(d);w.fault("cleanup_false_before");
                let c=prepare(d.advance(&w.mailbox()));w.execute(d,&c);assert_eq!(w.private_status()[5],"15000");
                w.send("CLOCK|15001");let restored=w.restore();let c=prepare(restored.advance(&w.mailbox()));
                assert_eq!(w.execute(&restored,&c),"DISPATCH|cleanup_budget_exhausted|published");assert!(matches!(restored.advance(&w.mailbox()),Action::CleanupBlocked));
                let s=w.private_status();assert_eq!(s[2],"1");assert_eq!(s[4],"1");assert_eq!(s[5],"15000");
            });
        }
        record(&evidence.join("result.json"),&serde_json::to_vec_pretty(&serde_json::json!({
            "schema":"lycheedev.protocol-prototype.shared-lifecycle.v1","state":"passed_scoped_shared_matrix",
            "scope":"one shared Rust advance/real Lua51 business receiver; fixture once-loader, model clocks/owned resources, fixed trusted binding",
            "finalProtocolPassed":false,"excluded":["independent audit","clock/pause/B1","cold binding/private-key isolation","OS carrier/game input","500 shared allocator/maxinstances","power-loss directory durability","full product mapping/performance"]})).unwrap());
    }
}
