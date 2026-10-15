local Env,client,root=...
local ns={}
for _,path in ipairs({"Bridge/CaptureWriter.lua","Bridge/MemoryProtocol.lua","Bridge/SlotProtocol.lua"}) do
    assert(loadfile(root.."/"..path))("Lychee Dev",ns)
end
local executions,released=0,false
local runtime=string.rep("1",32)
local actor={character="测试角色",realm="测试服",guid="Player-1-1"}
local mailbox
local function create(run)
    mailbox={schema="Lycheedev mailbox protocol v1",runtime=run,release="test"}
    return ns.SlotProtocol.Create({runtime=run,build="1.2.3.4",product=client,release="test",
        mailbox=mailbox,
        actor=function()return actor end,encode=ns.CaptureWriter.Encode,
        compile=function(code)return loadstring(code)end,
        releaseInput=function()released=true end,
        execute=function(fn,budget,done)assert(released,"execution retained input");executions=executions+1;local ok,result=pcall(fn);done(ok,result)end})
end
local engine=create(runtime)
local serial=0
local function message(action,ticket)
    serial=serial+1
    return {schema="Lycheedev mailbox protocol v1",index=engine.NextSlot(),runtime=runtime,
        owner=string.rep("2",32),fence=1,nonce=string.format("%032x",serial),ticket=ticket or string.rep("3",32),
        action=action,guid=actor.guid,build="1.2.3.4"}
end
local function send(e)
    local record,value=engine.Receive(e.index,e)
    assert(record,tostring(value));return value
end
assert(type(engine.Describe())=="string")
local binding=message("bind");assert(send(binding).state=="bound")
local prepare=message("prepare");prepare.code="return false";prepare.budget=10;prepare.codeBytes=#prepare.code;prepare.codeChecksum=ns.MemoryProtocol.Checksum(prepare.code)
local p=send(prepare);assert(p.state=="prepared");assert(executions==0)
local wrong=message("commit");wrong.preparedNonce=prepare.nonce;wrong.challenge=string.rep("0",32)
assert(send(wrong).reason=="slot_commit_mismatch");assert(executions==0)
local commit=message("commit");commit.preparedNonce=prepare.nonce;commit.challenge=p.challenge
assert(send(commit).state=="accepted");assert(executions==1)
local proof=send(message("confirm"));assert(proof.state=="reported" and proof.reportBytes>0)
local snapshot=engine.Snapshot();assert(snapshot.operations[prepare.ticket].body)
assert(snapshot.operations[prepare.ticket].body:find('"result":false',1,true),"false result lost")
local release=message("release");release.reportBytes=proof.reportBytes;release.reportChecksum=proof.reportChecksum
assert(send(release).state=="released");assert(engine.Snapshot().operations[prepare.ticket].body==nil and snapshot.operations[prepare.ticket].body)
local repeated=message("prepare");repeated.code=prepare.code;repeated.budget=10;repeated.codeBytes=9;repeated.codeChecksum=0
assert(send(repeated).reason=="slot_operation_exists");assert(executions==1)
assert(engine.Receive(commit.index,commit)==nil,"consumed slot reused")
local other=message("bind");other.owner=string.rep("4",32);assert(send(other).reason=="slot_owner_busy")
local stale=message("confirm");stale.fence=2;assert(send(stale).reason=="slot_owner_mismatch")
local old=message("bind");old.runtime=string.rep("a",32)
local foreignRecord,foreignReason,skip=engine.Receive(old.index,old)
assert(not foreignRecord and foreignReason=="slot_skipped_foreign" and skip and not snapshot.receipts[old.nonce])
local oldRecord=snapshot.receipts[proof.nonce]
-- Repeated nonces cannot overwrite retained evidence, even in another slot.
local duplicate=message("confirm");duplicate.nonce=commit.nonce
local before=snapshot.receipts[commit.nonce];engine.Receive(duplicate.index,duplicate);assert(snapshot.receipts[commit.nonce]==before)
assert(send(message("unbind")).state=="unbound")
assert(engine.Snapshot().owner==nil)
-- New runtime rejects a full old handshake transcript without business effects.
local replacement=create(string.rep("f",32))
local record,rejection,skip=replacement.Receive(binding.index,binding)
assert(not record and rejection=="slot_skipped_foreign" and skip)
assert(executions==1)
-- Invalid-but-loaded files consume all remaining positions without reuse.
for i=engine.NextSlot(),500 do assert(engine.Receive(i,nil)==nil) end
assert(engine.NextSlot()==501)
assert(engine.Receive(501,{})==nil)


-- The 500-slot profile reserves control capacity and admits 121 complete jobs.
engine=create(runtime)
assert(send(message("bind")).state=="bound")
for i=1,121 do
    local ticket=string.format("%032x",10000+i)
    local prepare=message("prepare",ticket)
    prepare.code="return 1";prepare.budget=1;prepare.codeBytes=#prepare.code;prepare.codeChecksum=ns.MemoryProtocol.Checksum(prepare.code)
    local prepared=send(prepare);assert(prepared.state=="prepared","operation capacity was not expanded")
    local commit=message("commit",ticket);commit.preparedNonce=prepare.nonce;commit.challenge=prepared.challenge
    assert(send(commit).state=="accepted")
    local report=send(message("confirm",ticket));assert(report.state=="reported")
    local release=message("release",ticket);release.reportBytes=report.reportBytes;release.reportChecksum=report.reportChecksum
    assert(send(release).state=="released")
    assert(mailbox.bodies[ticket]==nil)
end
assert(engine.NextSlot()==486)
assert(send(message("prepare",string.rep("e",32))).reason=="slot_capacity")
-- A malformed foreign public header must stop rather than authorize skipping.
local malformed=message("bind");malformed.runtime=string.rep("a",32);malformed.fence=0
local record,reason,skip=engine.Receive(malformed.index,malformed)
assert(not record and reason=="slot_envelope_invalid" and not skip)
local retained=0
for nonce,record in pairs(mailbox.receipts)do
    if nonce=="schema" then assert(record==mailbox.schema and engine.Snapshot().receipts.schema==nil)
    else retained=retained+1;assert(engine.Snapshot().receipts[nonce]==record) end
end
assert(retained<=500 and next(mailbox.bodies)=="schema" and next(mailbox.bodies,"schema")==nil,
    "mailbox exceeded record retention or removed its metadata")

-- Compilation failure consumes three slots but still consumes one operation.
engine=create(runtime)
assert(send(message("bind")).state=="bound")
for i=1,121 do
    local ticket=string.format("%032x",20000+i)
    local prepare=message("prepare",ticket)
    prepare.code=i==121 and "local =" or "return 1"
    prepare.budget=1;prepare.codeBytes=#prepare.code;prepare.codeChecksum=ns.MemoryProtocol.Checksum(prepare.code)
    local prepared=send(prepare)
    if i<121 then
        assert(prepared.state=="prepared")
        local commit=message("commit",ticket);commit.preparedNonce=prepare.nonce;commit.challenge=prepared.challenge
        assert(send(commit).state=="accepted")
    else assert(prepared.state=="reported") end
    local report=send(message("confirm",ticket))
    local release=message("release",ticket);release.reportBytes=report.reportBytes;release.reportChecksum=report.reportChecksum
    assert(send(release).state=="released")
end
assert(engine.NextSlot()==485)
assert(send(message("prepare",string.rep("d",32))).reason=="slot_capacity")
-- Asynchronous false results must remain JSON false as well.
local complete
engine=ns.SlotProtocol.Create({runtime=runtime,build="1.2.3.4",product=client,release="test",actor=function()return actor end,
    encode=ns.CaptureWriter.Encode,compile=loadstring,execute=function(fn,budget,done)complete=done end})
assert(send(message("bind")).state=="bound")
local prepare=message("prepare");prepare.code="return false";prepare.budget=1;prepare.codeBytes=#prepare.code;prepare.codeChecksum=ns.MemoryProtocol.Checksum(prepare.code)
local prepared=send(prepare);local commit=message("commit");commit.preparedNonce=prepare.nonce;commit.challenge=prepared.challenge
assert(send(commit).state=="accepted")
complete(true,false)
assert(engine.Snapshot().operations[prepare.ticket].body:find('"result":false',1,true))
