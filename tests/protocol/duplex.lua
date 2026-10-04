local F=assert(loadfile(arg[1].."/tests/protocol/duplex_fixture.lua"))()
local P,S=F.P,F.S
-- SHA contexts may span many blocks and interleave without sharing state.
assert(S.Hex(S.Digest(""))=="e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855")
local vector="abcdefghbcdefghicdefghijdefghijkefghijklfghijklmghijklmnhijklmnoijklmnopjklmnopqklmnopqrlmnopqrsmnopqrstnopqrstu"
local a,b=S.New(),S.New()
assert(S.Update(a,vector:sub(1,65)));assert(S.Update(b,"a"))
assert(S.Update(a,vector:sub(66)));assert(S.Update(b,"bc"))
assert(S.Hex(S.Final(a))=="cf5b16a778af8380036ce59e7b0492370b249b11e8f07a51afac45037afee9d1")
assert(S.Hex(S.Final(b))=="ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad")
assert(not S.Final(a) and not S.Update(b,"late"),"finalized SHA context reused")
local function read(path)local f=assert(io.open(path,"rb"));local b=f:read("*a");f:close();return b end
if arg[2] then
    local wire=read(arg[2]);local h=assert(P.DecodeHeader(wire:sub(1,320),P.Kind.frame))
    assert(h.requestSeqHi==2097152 and h.requestSeqLo==1,"u64 request sequence lost precision")
    assert(P.VerifyFrame(wire:sub(1,320),wire:sub(321)))
    local cells={};for n=1,262224 do cells[n]=0 end;F.setRow(cells,wire)
    assert(P.ReadHeader(cells)==wire:sub(1,320))
end
if arg[3] then
    local wire=read(arg[3]);local cells={};for n=1,336 do cells[n]=0 end;F.setRow(cells,wire)
    assert(P.ReadControl(cells)==wire:sub(1,320))
end
if arg[6] and arg[7] then
    local reload,lease=read(arg[6]),read(arg[7])
    local rh=assert(P.DecodeHeader(reload:sub(1,320),P.Kind.reload))
    local lh=assert(P.DecodeHeader(lease:sub(1,320),P.Kind.lease))
    assert(#reload==320 and #lease==336 and lease:sub(321)==rh.messageId)
    assert(lh.challenge~=F.Z16 and lh.runtime==rh.runtime and lh.owner==rh.owner and lh.session==rh.session)
    assert(S.Digest("LYCMBX/frame/v1\0"..reload:sub(1,232))==rh.frameSHA)
    assert(S.Digest("LYCMBX/frame/v1\0"..lease:sub(1,232)..lease:sub(321))==lh.frameSHA)
end
-- First command binds and executes in the same callback; no control write.
local f=F.engine();local nonce=f.engine.Snapshot().readyChallenge
local first=f.command("return false")
assert(f.calls()==1 and f.engine.Snapshot().phase=="result_pending")
assert(f.engine.Snapshot().terminal.executionStarted and f.engine.Snapshot().terminal.resourcesReleased)
assert(f.pages[1]:find('"result":false',1,true),"false result disappeared")
assert(f.engine.Snapshot().readyChallenge~=nonce,"single-use nonce repeated")
assert(f.engine.Poll(f.arena,8));assert(f.calls()==1,"same row executed twice")
-- Changed replay and checksum/padding failures preserve the retained result.
local ack=f.ack();local state=f.engine.Snapshot();local secondNonce=state.readyChallenge
f.command("return true",{seq=2,ack=string.rep("x",92)})
assert(f.calls()==1 and f.engine.Snapshot().request.requestId==state.request.requestId and f.engine.Snapshot().readyChallenge==secondNonce)
local replacement=F.header(P.Kind.frame,"return true",{seq=2,ack=ack,challenge=secondNonce})
F.setRow(f.arena.command,replacement);f.arena.command[100]=1;assert(f.engine.Poll(f.arena,8))
assert(f.calls()==1 and not f.engine.Snapshot().repair,"mixed row triggered repair or execution")
f.arena.command[100]=0;F.setRow(f.arena.command,replacement);assert(f.engine.Poll(f.arena,8))
assert(f.calls()==2 and f.engine.Snapshot().request.requestId==string.format("%032x",2),"valid completion with same stamp did not retry")
assert(f.engine.Snapshot().released.requestId==state.request.requestId)
assert(f.engine.Snapshot().released.outcome=="success" and f.engine.Snapshot().released.executionStarted
    and f.engine.Snapshot().released.totalBytes==#"return false" and f.engine.Snapshot().released.requestSeq=="1")
-- Final ACK+close is one stop message and duplicate observation is idempotent.
state=f.engine.Snapshot();local closeOpts={seq=2,pub=50,id=state.request.requestId,digest=state.request.requestSHA256,challenge=state.request.challenge}
assert(f.writeStop(P.Kind.close,f.ack(),closeOpts))
assert(f.engine.Snapshot().phase=="closed" and not f.engine.Snapshot().request and not f.pages[1])
assert(f.engine.Poll(f.arena,8));assert(f.calls()==2)
local rearmed=f.engine.Snapshot()
assert(rearmed.phase=="ready_unbound" and rearmed.closedAdmission and rearmed.readyChallenge~="")
f.command("return true",{seq=1,fence=12,owner="60112233445566778899aabbccddeeff",session="70112233445566778899aabbccddeeff"})
assert(f.calls()==3 and not f.engine.Snapshot().closedAdmission,"precisely retired owner did not rearm")
-- Stop wins the same tick against a not-yet-accepted command and retains an
-- exact terminal record that can be acknowledged by the next command.
local c=F.engine();local late=F.header(P.Kind.frame,"return true",{challenge=c.engine.Snapshot().readyChallenge})
local lateHeader=assert(P.DecodeHeader(late:sub(1,320)))
F.setRow(c.arena.command,late)
assert(c.writeStop(P.Kind.cancel,"",{seq=1,pub=1,id=S.Hex(lateHeader.requestId),digest=S.Hex(lateHeader.requestSHA),challenge=S.Hex(lateHeader.challenge)}))
state=c.engine.Snapshot()
assert(c.calls()==0 and state.terminal and state.terminal.outcome=="cancelled" and state.terminal.executionStarted==false and state.receipts.stop.state=="not_started")
assert(state.terminal.effects=="none_started")
assert(c.engine.Poll(c.arena,8));assert(c.calls()==0,"late write escaped revoked challenge")
c.command("return true",{seq=2,ack=c.ack()});assert(c.calls()==1)
-- A later unknown write can still expose the previous retained result.
-- Exact prior ACK + cancellation revoke the next admission atomically.
local uncertain=F.engine();uncertain.command("return true")
local old=uncertain.engine.Snapshot();local oldAck=uncertain.ack()
local unknown=F.header(P.Kind.frame,"return false",{seq=2,ack=oldAck,challenge=old.readyChallenge})
local unknownHeader=assert(P.DecodeHeader(unknown:sub(1,320)))
local cancelOpts={seq=2,pub=1,id=S.Hex(unknownHeader.requestId),digest=S.Hex(unknownHeader.requestSHA),challenge=S.Hex(unknownHeader.challenge)}
assert(uncertain.writeStop(P.Kind.cancel,string.rep("x",92),cancelOpts))
assert(uncertain.calls()==1 and uncertain.engine.Snapshot().request.requestId==old.request.requestId and uncertain.engine.Snapshot().readyChallenge==old.readyChallenge,
    "wrong prior ACK retired result or nonce")
F.setRow(uncertain.arena.command,unknown)
assert(uncertain.writeStop(P.Kind.cancel,oldAck,cancelOpts))
state=uncertain.engine.Snapshot()
assert(uncertain.calls()==1 and state.request.requestId==cancelOpts.id and state.terminal.outcome=="cancelled" and not state.terminal.executionStarted)
assert(state.released.requestId==old.request.requestId and state.receipts.stop.state=="not_started")
assert(uncertain.engine.Poll(uncertain.arena,8));assert(uncertain.calls()==1,"late second command executed after revocation")
uncertain.command("return true",{seq=3,ack=uncertain.ack()});assert(uncertain.calls()==2)
-- The same stop payload is legal if the second command won first: cancel
-- the current execution and retain its effects, never restore the old result.
local executions,signals=0,0
local accepted=F.engine({execute=function(_,_,completion)
    executions=executions+1
    if executions==1 then completion(true,true,{resourcesReleased=true});return {} end
    return {RequestCancel=function()signals=signals+1;completion(false,"cancelled",{cancelled=true,resourcesReleased=true});return true end}
end})
accepted.command("return true");old=accepted.engine.Snapshot();oldAck=accepted.ack()
accepted.command("return false",{seq=2,ack=oldAck});state=accepted.engine.Snapshot()
local currentId=state.request.requestId
cancelOpts={seq=2,pub=1,id=currentId,digest=state.request.requestSHA256,challenge=state.request.challenge}
assert(accepted.writeStop(P.Kind.cancel,string.rep("x",92),cancelOpts));assert(signals==0)
assert(accepted.writeStop(P.Kind.cancel,oldAck,cancelOpts))
state=accepted.engine.Snapshot()
assert(executions==2 and signals==1 and state.request.requestId==currentId and state.terminal.outcome=="cancelled" and state.terminal.executionStarted
    and state.terminal.effects=="may_have_occurred" and state.released.requestId==old.request.requestId,
    "accepted replacement cancellation rolled execution back")
-- Incremental validation keeps its source private and consumes nothing before
-- completion. Cancel wins after copying starts, including a prior-result ACK.
local startedCount=0
local validating=F.engine({started=function()startedCount=startedCount+1 end})
validating.command("return true");old=validating.engine.Snapshot();oldAck=validating.ack()
local larger="return false"..string.rep(" ",1012)
unknown=F.header(P.Kind.frame,larger,{seq=2,ack=oldAck,challenge=old.readyChallenge})
unknownHeader=assert(P.DecodeHeader(unknown:sub(1,320)))
F.setRow(validating.arena.command,unknown);assert(validating.engine.Poll(validating.arena,8));assert(validating.engine.Poll(validating.arena,8))
state=validating.engine.Snapshot()
assert(state.phase=="validating" and state.validation.copiedBytes>0 and not state.ready and state.readyChallenge==old.readyChallenge
    and state.request.requestId==old.request.requestId and startedCount==1)
cancelOpts={seq=2,pub=1,id=S.Hex(unknownHeader.requestId),digest=S.Hex(unknownHeader.requestSHA),challenge=S.Hex(unknownHeader.challenge),total=#larger}
assert(validating.writeStop(P.Kind.cancel,oldAck,cancelOpts))
state=validating.engine.Snapshot()
assert(not state.validation and state.phase=="result_pending" and state.terminal.outcome=="cancelled" and not state.terminal.executionStarted and startedCount==1)
assert(P.ParseSendbox(validating.publications.status):find('"phase":"result_pending"',1,true))
-- A precise clock batches several private validation steps, yet yields on
-- its time budget. A stop published between polls still wins before admission.
local workMs=0
local timed=F.engine({workClockMillis=function()workMs=workMs+0.05;return workMs end})
local timedSource="return true"..string.rep(" ",65536)
nonce=timed.engine.Snapshot().readyChallenge
local timedWire=F.header(P.Kind.frame,timedSource,{challenge=nonce})
local timedHeader=assert(P.DecodeHeader(timedWire:sub(1,320)))
F.setRow(timed.arena.command,timedWire)
assert(timed.engine.Poll(timed.arena,8) and timed.engine.HasPendingValidation())
assert(timed.engine.Poll(timed.arena,8) and timed.engine.HasPendingValidation())
state=timed.engine.Snapshot()
assert(state.validation.copiedBytes>8192,
    "time-bounded validator did not batch and yield")
assert(timed.writeStop(P.Kind.cancel,"",{seq=1,pub=1,id=S.Hex(timedHeader.requestId),
    digest=S.Hex(timedHeader.requestSHA),challenge=S.Hex(timedHeader.challenge),total=#timedSource}))
assert(timed.calls()==0 and not timed.engine.HasPendingValidation()
    and timed.engine.Snapshot().terminal.outcome=="cancelled", "stop lost to batched validation")
-- A changed header discards a partly copied row without consuming its nonce.
local changed=F.engine({started=function()startedCount=startedCount+1 end})
nonce=changed.engine.Snapshot().readyChallenge
unknown=F.header(P.Kind.frame,larger,{challenge=nonce})
F.setRow(changed.arena.command,unknown);assert(changed.engine.Poll(changed.arena,8));assert(changed.engine.HasPendingValidation())
local firstCell=changed.arena.command[1];changed.arena.command[1]=0;assert(changed.engine.Poll(changed.arena,8))
assert(not changed.engine.HasPendingValidation() and not changed.engine.Snapshot().request and changed.engine.Snapshot().readyChallenge==nonce)
assert(P.ParseSendbox(changed.publications.status):find('"phase":"ready_unbound"',1,true))
changed.arena.command[1]=firstCell
local count=0
repeat assert(changed.engine.Poll(changed.arena,8));count=count+1;assert(count<100) until not changed.engine.HasPendingValidation()
assert(changed.calls()==1 and startedCount==2)
local actorOptions={noActor=false}
local actorChange=F.engine(actorOptions);nonce=actorChange.engine.Snapshot().readyChallenge
F.setRow(actorChange.arena.command,F.header(P.Kind.frame,larger,{challenge=nonce}));assert(actorChange.engine.Poll(actorChange.arena,8))
actorOptions.noActor=true;assert(actorChange.engine.Poll(actorChange.arena,8))
assert(not actorChange.engine.HasPendingValidation() and actorChange.calls()==0 and actorChange.engine.Snapshot().readyChallenge~=nonce)
actorOptions.noActor=false;assert(actorChange.engine.Poll(actorChange.arena,8));assert(actorChange.calls()==0,"actor recovery revived the old admission")
-- Actor and competing owner changes execute zero times and preserve nonce.
local contender=F.engine();nonce=contender.engine.Snapshot().readyChallenge
contender.command("return true");ack=contender.ack();nonce=contender.engine.Snapshot().readyChallenge
contender.command("return true",{seq=2,ack=ack,owner="50112233445566778899aabbccddeeff"})
assert(contender.calls()==1 and contender.engine.Snapshot().readyChallenge==nonce)
local absent=F.engine({noActor=true});absent.command("return true");assert(absent.calls()==0)
local idle=F.engine();nonce=idle.engine.Snapshot().readyChallenge;idle.clock(100000);idle.command("return true")
assert(idle.calls()==1 and idle.engine.Snapshot().request.challenge==nonce,"long idle made ready admission unusable")
-- Compile failure is retained as failed/no-execution; execution failure and
-- cancellation have distinct explicit terminal outcomes.
local bad=F.engine();bad.command("this is not lua")
assert(bad.calls()==0 and bad.engine.Snapshot().terminal.outcome=="failed" and bad.engine.Snapshot().terminal.executionStarted==false)
local thrown=F.engine();thrown.command("error('broken')")
assert(thrown.calls()==1 and thrown.engine.Snapshot().terminal.outcome=="failed" and thrown.engine.Snapshot().terminal.executionStarted)
local done,cancelCalls
cancelCalls=0
local running=F.engine({execute=function(_,_,completion)done=completion;return {RequestCancel=function()cancelCalls=cancelCalls+1;done(false,"cancelled",{cancelled=true,resourcesReleased=false});return true end}end})
running.command("return true");state=running.engine.Snapshot()
assert(state.phase=="running" and state.readyChallenge=="")
assert(running.writeStop(P.Kind.close,"",{seq=1,id=state.request.requestId,digest=state.request.requestSHA256,challenge=state.request.challenge}))
state=running.engine.Snapshot();assert(cancelCalls==1 and state.phase=="closing" and not state.terminal.resourcesReleased and state.readyChallenge=="")
assert(running.writeStop(P.Kind.close,running.ack(),{seq=1,pub=2,id=state.request.requestId,digest=state.request.requestSHA256,challenge=state.request.challenge}))
assert(running.engine.Snapshot().request,"unreleased resource ACK retired evidence")
-- Repeated mixed-image failures back off but retry the completed body with
-- the same header/marker. No repair or nonce consumption occurs.
local partial=F.engine();local valid=F.header(P.Kind.frame,"return true",{challenge=partial.engine.Snapshot().readyChallenge})
F.setRow(partial.arena.command,valid);partial.arena.command[100]=1
for _=1,10 do assert(partial.engine.Poll(partial.arena,8)) end
assert(partial.calls()==0 and not partial.engine.Snapshot().repair)
partial.arena.command[100]=0;assert(partial.engine.Poll(partial.arena,8));assert(partial.calls()==0)
partial.clock(100.5);assert(partial.engine.Poll(partial.arena,8));assert(partial.calls()==1)
-- Activity ends when execution finishes, before result encoding/pagination.
local active,encodedWhileActive=false,false
local activity=F.engine({started=function()active=true end,finished=function()active=false end,
    encode=function(record,limit)if record.ok~=nil and active then encodedWhileActive=true end;return F.ns.CaptureWriter.Encode(record,limit)end})
activity.command("return true");assert(not active and not encodedWhileActive)
local startEvents,finishEvents=0,0
local function beganActivity()startEvents=startEvents+1;active=true end
local function endedActivity()finishEvents=finishEvents+1;active=false end
local compileActivity=F.engine({started=beganActivity,finished=endedActivity})
compileActivity.command("this is not lua");assert(startEvents==0 and finishEvents==0 and not active)
local exceptionActivity=F.engine({started=beganActivity,finished=endedActivity,execute=function()error("executor_exception")end})
exceptionActivity.command("return true");assert(startEvents==1 and finishEvents==1 and not active)
local quarantineActivity=F.engine({started=beganActivity,finished=endedActivity,execute=function(_,_,done)
    return {RequestCancel=function()done(false,"cancelled",{cancelled=true,resourcesReleased=true});return true end}
end})
quarantineActivity.command("return true");assert(active)
assert(quarantineActivity.engine.Quarantine("topology_fault"))
assert(not active and startEvents==2 and finishEvents==2 and quarantineActivity.engine.Snapshot().terminal.outcome=="cancelled")
assert(quarantineActivity.engine.Quarantine("still_faulted"));assert(finishEvents==2)
local finishUncancelled
local uncancellable=F.engine({started=beganActivity,finished=endedActivity,execute=function(_,_,done)
    finishUncancelled=done;return {RequestCancel=function()return false end}
end})
uncancellable.command("return true");assert(active)
assert(uncancellable.engine.Quarantine("topology_fault"))
assert(active and finishEvents==2,"quarantine pretended running work ended")
finishUncancelled(false,"budget_elapsed",{resourcesReleased=true})
assert(not active and startEvents==3 and finishEvents==3,"actual completion left activity visible")
local disabledActivity=F.engine({started=beganActivity,finished=endedActivity,execute=function(_,_,done)
    return {RequestCancel=function()done(false,"cancelled",{cancelled=true,resourcesReleased=true});return true end}
end})
disabledActivity.command("return true");assert(active);assert(disabledActivity.engine.Disable())
assert(not active and startEvents==4 and finishEvents==4)
-- Reload is a separate new stop intent and needs the exact prepared lease.
local reload=F.engine({reload=function()return true end})
reload.command("return true");state=reload.engine.Snapshot()
assert(reload.writeStop(P.Kind.close,reload.ack(),{seq=1,pub=1,id=state.request.requestId,digest=state.request.requestSHA256,challenge=state.request.challenge}))
assert(reload.writeStop(P.Kind.reload,"",{seq=0,pub=2,mid=string.format("%032x",19000),id=string.rep("0",32),digest=string.rep("0",64),budget=0,total=0}))
local receipt=reload.engine.Snapshot().receipts.stop
assert(receipt.state=="accepted" and receipt.challenge and not reload.engine.TakeReload())
assert(reload.writeStop(P.Kind.lease,F.raw(receipt.messageId),{seq=0,pub=3,mid=string.format("%032x",20000),id=string.rep("0",32),digest=string.rep("0",64),challenge=receipt.challenge,budget=0,total=0}))
local reloadStatus=assert(P.ParseSendbox(reload.publications.status))
assert(reloadStatus:find(string.format("%032x",20000),1,true) and reload.publications.status==reload.publications.receipts,"lease receipt was invisible before reload")
assert(reload.engine.TakeReload() and not reload.engine.TakeReload())
-- Repair stays bounded and requires the exact stop challenge before dropping
-- private references to the retired arena.
local repair=F.engine();local nextArena=assert(P.NewArena("90112233445566778899aabbccddeeff"))
repair.command("return true");state=repair.engine.Snapshot()
assert(repair.writeStop(P.Kind.close,repair.ack(),{seq=1,pub=1,id=state.request.requestId,digest=state.request.requestSHA256,challenge=state.request.challenge}))
assert(repair.engine.Poll(repair.arena,8))
assert(repair.engine.BeginRepair("90112233445566778899aabbccddeeff",string.rep(string.char(171),16)))
assert(not repair.engine.ReleaseRetiredArena())
F.setRow(nextArena.stop,F.header(P.Kind.repair,"",{seq=0,pub=2,mid=string.format("%032x",30000),id=string.rep("0",32),digest=string.rep("0",64),challenge=string.rep("ab",16),arena="90112233445566778899aabbccddeeff"}))
assert(repair.engine.Poll(nextArena,8));assert(repair.engine.Snapshot().repaired)
assert(repair.engine.ReleaseRetiredArena())
assert(not repair.engine.BeginRepair("90112233445566778899aabbccddeeff",string.rep(string.char(171),16)))
-- Exercise the actual JSON encoder for values that conditional expressions
-- can accidentally discard.
for _,case in ipairs({{ok=true,value=false},{ok=true,value=0},{ok=true,value="answer"},{ok=false,value="failure"}}) do
    local e=F.engine({execute=function(_,_,completion)completion(case.ok,case.value,{resourcesReleased=true,logs={}});return {}end})
    e.command("return true")
    local expected={ok=case.ok,resourcesReleased=true,logs={}}
    if case.ok then expected.result=case.value else expected.error=case.value end
    assert(e.pages[1]==F.ns.CaptureWriter.Encode(expected,524288))
end
if arg[4] and arg[5] then
    local json=read(arg[4]);F.ns.CaptureWriter.Encode=function()return json end
    local out=assert(io.open(arg[5],"wb"));assert(out:write(P.Sendbox({},65536)));out:close()
end
print("duplex: Go wire, one write, exact u64, ACK, repair, close and fresh-owner fencing passed")
