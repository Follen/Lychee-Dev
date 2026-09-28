local Env,client,root=...
local ns={}
for _,path in ipairs({"Bridge/CaptureWriter.lua","Bridge/MemoryProtocol.lua","Bridge/SlotProtocol.lua"}) do
    assert(loadfile(root.."/"..path))("Lychee Dev",ns)
end
local executions,released=0,false
local runtime=string.rep("1",32)
local actor={character="测试角色",realm="测试服",guid="Player-1-1"}
local function create(run)
    return ns.SlotProtocol.Create({runtime=run,build="1.2.3.4",product=client,release="test",
        actor=function()return actor end,encode=ns.CaptureWriter.Encode,
        compile=function(code)return loadstring(code)end,
        releaseInput=function()released=true end,
        execute=function(fn,budget,done)assert(released,"execution retained input");executions=executions+1;local ok,result=pcall(fn);done(ok,result)end})
end
local engine=create(runtime)
local serial=0
local function message(action,ticket)
    serial=serial+1
    return {schema="lycheedev.slot.v1",index=engine.NextSlot(),runtime=runtime,
        owner=string.rep("2",32),fence=1,nonce=string.format("%032x",serial),ticket=ticket or string.rep("3",32),
        action=action,guid=actor.guid,build="1.2.3.4"}
end
local function send(e)
    local record,value=engine.Receive(e.index,e)
    assert(record,tostring(value));return value
end
assert(type(engine.Describe())=="string")
local binding=message("bind");assert(send(binding).state=="bound")
local prepare=message("prepare");prepare.code="return 55";prepare.budget=10;prepare.codeBytes=#prepare.code;prepare.codeChecksum=ns.MemoryProtocol.Checksum(prepare.code)
local p=send(prepare);assert(p.state=="prepared");assert(executions==0)
local wrong=message("commit");wrong.preparedNonce=prepare.nonce;wrong.challenge=string.rep("0",32)
assert(send(wrong).reason=="slot_commit_mismatch");assert(executions==0)
local commit=message("commit");commit.preparedNonce=prepare.nonce;commit.challenge=p.challenge
assert(send(commit).state=="accepted");assert(executions==1)
local proof=send(message("confirm"));assert(proof.state=="reported" and proof.reportBytes>0)
local snapshot=engine.Snapshot();assert(snapshot.operations[prepare.ticket].body)
local release=message("release");release.reportBytes=proof.reportBytes;release.reportChecksum=proof.reportChecksum
assert(send(release).state=="released");assert(snapshot.operations[prepare.ticket].body==nil)
local repeated=message("prepare");repeated.code=prepare.code;repeated.budget=10;repeated.codeBytes=9;repeated.codeChecksum=0
assert(send(repeated).reason=="slot_operation_exists");assert(executions==1)
assert(engine.Receive(commit.index,commit)==nil,"consumed slot reused")
local other=message("bind");other.owner=string.rep("4",32);assert(send(other).reason=="slot_owner_busy")
local stale=message("confirm");stale.fence=2;assert(send(stale).reason=="slot_owner_mismatch")
local old=message("bind");old.runtime=string.rep("a",32);assert(send(old).reason=="slot_runtime_changed")
local oldRecord=snapshot.receipts[proof.nonce]
-- Repeated nonces cannot overwrite retained evidence, even in another slot.
local duplicate=message("confirm");duplicate.nonce=commit.nonce
local before=snapshot.receipts[commit.nonce];engine.Receive(duplicate.index,duplicate);assert(snapshot.receipts[commit.nonce]==before)
assert(send(message("unbind")).state=="unbound")
assert(engine.Snapshot().owner==nil)
-- New runtime rejects a full old handshake transcript without business effects.
local replacement=create(string.rep("f",32))
local _,rejection=replacement.Receive(binding.index,binding)
assert(rejection.reason=="slot_runtime_changed")
assert(executions==1)
-- Invalid-but-loaded files consume all remaining positions without reuse.
for i=engine.NextSlot(),64 do assert(engine.Receive(i,nil)==nil) end
assert(engine.NextSlot()==65)
assert(engine.Receive(65,{})==nil)
