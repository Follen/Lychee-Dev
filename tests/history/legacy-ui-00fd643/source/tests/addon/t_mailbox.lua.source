local Env,client,root=...
local ns={}
for _,name in ipairs({"CaptureWriter","MemoryProtocol","SlotProtocol"}) do
    assert(loadfile(root.."/Bridge/"..name..".lua"))("Lychee Dev",ns)
end

local runtime,owner,ticket=string.rep("1",32),string.rep("2",32),string.rep("3",32)
local mailbox={schema="lycheedev.mailbox.v1",release="test",runtime=runtime}
local actor={guid="Player-1-1",character="Tester",realm="Realm"}
local executions=0
local engine=ns.SlotProtocol.Create({runtime=runtime,release="test",build="70000",product=client,mailbox=mailbox,
    actor=function()return actor end,encode=ns.CaptureWriter.Encode,compile=loadstring,
    execute=function(fn,budget,done)executions=executions+1;done(true,fn())end})
assert(engine.Describe()==mailbox.identity and type(mailbox.identity)=="string")
assert(mailbox.receipts~=engine.Snapshot().receipts and mailbox.operations==nil and mailbox.fn==nil and mailbox.code==nil)
local serial=0
local function envelope(action)
    serial=serial+1
    return {schema="lycheedev.slot.v3",index=engine.NextSlot(),runtime=runtime,owner=owner,fence=1,
        nonce=string.format("%032x",serial),ticket=ticket,action=action,guid=actor.guid,build="70000"}
end
local function send(e)
    local record,value=engine.Receive(e.index,e)
    assert(record,tostring(value));assert(mailbox.receipts[e.nonce]==record)
    assert(engine.Snapshot().receipts[e.nonce]==record)
    return record,value
end
-- Public data cannot create a nonce receipt, change ownership, or authorize work.
local bind=envelope("bind")
mailbox.receipts[bind.nonce]="public forged receipt"
local binding,value=send(bind);assert(value.state=="bound")
mailbox.runtime=string.rep("a",32);mailbox.identity="public forged identity"
local intruder=envelope("bind");intruder.owner=string.rep("b",32)
assert(select(2,send(intruder)).reason=="slot_owner_busy" and executions==0)
local changedActor=envelope("prepare");changedActor.guid="Player-changed"
assert(select(2,send(changedActor)).reason=="slot_target_changed" and executions==0)
local changedFence=envelope("prepare");changedFence.fence=2
assert(select(2,send(changedFence)).reason=="slot_owner_mismatch" and executions==0)
local prepare=envelope("prepare");prepare.code="return {answer=42}";prepare.codeBytes=#prepare.code
prepare.codeChecksum=ns.MemoryProtocol.Checksum(prepare.code);prepare.budget=10
local prepared,p=send(prepare);assert(p.state=="prepared" and executions==0 and mailbox.bodies[ticket]==nil)
local wrongCommit=envelope("commit");wrongCommit.preparedNonce=prepare.nonce;wrongCommit.challenge=string.rep("0",32)
mailbox.receipts[prepare.nonce]="public forged challenge"
assert(select(2,send(wrongCommit)).reason=="slot_commit_mismatch" and executions==0 and mailbox.bodies[ticket]==nil)
local commit=envelope("commit");commit.preparedNonce=prepare.nonce;commit.challenge=p.challenge
assert(select(2,send(commit)).state=="accepted" and executions==1)
local op=engine.Snapshot().operations[ticket]
assert(type(mailbox.bodies[ticket])=="string" and mailbox.bodies[ticket]==op.body)
assert(mailbox.receipts[prepare.nonce]==engine.Snapshot().receipts[prepare.nonce] and mailbox.receipts[prepare.nonce]~=prepared)
assert(mailbox.bodies[ticket]:find('"answer":42',1,true))
-- Mutating public strings/maps never mutates private result or nonce state.
local body=op.body
mailbox.bodies[ticket]="public forged body"
mailbox.receipts[bind.nonce]="public forged receipt"
local duplicate=envelope("confirm");duplicate.nonce=bind.nonce
local record,reason=engine.Receive(duplicate.index,duplicate)
assert(record==binding and reason=="slot_nonce_reused" and op.body==body)
local confirm=envelope("confirm");local _,proof=send(confirm)
assert(proof.state=="reported" and proof.reportBytes==op.bytes and proof.reportChecksum==op.checksum)
local replacement={schema="lycheedev.mailbox.v1",release="test",runtime=runtime}
engine.AttachMailbox(replacement);mailbox=replacement
assert(mailbox.identity==engine.Snapshot().descriptor and mailbox.bodies[ticket]==body and mailbox.receipts[bind.nonce]==binding)
local invalid=envelope("release");invalid.reportBytes=1;invalid.reportChecksum=0
assert(select(2,send(invalid)).reason=="slot_release_mismatch" and mailbox.bodies[ticket]==body)
local release=envelope("release");release.reportBytes=proof.reportBytes;release.reportChecksum=proof.reportChecksum
assert(select(2,send(release)).state=="released" and mailbox.bodies[ticket]==nil and engine.Snapshot().operations[ticket].body==nil and op.body==body)
assert(select(2,send(envelope("unbind"))).state=="unbound")
assert(mailbox.identity==engine.Snapshot().descriptor and engine.Snapshot().owner==nil)
for key,value in pairs(mailbox) do
    assert(key=="schema" or key=="release" or key=="runtime" or key=="identity" or key=="input" or key=="receipts" or key=="bodies")
    assert(type(value)=="string" or type(value)=="table")
end
local receiptCount=0
for key,record in pairs(mailbox.receipts) do
    assert(#key==32 and key:match("^[0-9a-f]+$") and type(record)=="string")
    receiptCount=receiptCount+1
end
assert(receiptCount<=200)
-- One fresh table for another runtime, with no retained result/receipt history.
local fresh={schema="lycheedev.mailbox.v1",release="test",runtime=string.rep("f",32)}
local nextEngine=ns.SlotProtocol.Create({runtime=fresh.runtime,release="test",build="70000",product=client,mailbox=fresh,
    actor=function()return actor end,encode=ns.CaptureWriter.Encode})
assert(nextEngine.Describe()==fresh.identity and fresh.identity~=mailbox.identity)
assert(next(fresh.receipts)==nil and next(fresh.bodies)==nil and fresh.input==nil)
assert(fresh.identity:sub(1,8)=="LYCMEM06" and fresh.identity:find("LYCEND06",1,true))
assert(fresh.identity:find('"schema":"lycheedev.slot.identity.v2"',1,true))
-- Prior generations consume the physical slot but cannot bind or publish a receipt.
for i,schema in ipairs({"lycheedev.slot.v1","lycheedev.slot.v2"}) do
    local old=envelope("bind");old.schema=schema;old.runtime=fresh.runtime;old.index=i
    local record,reason=nextEngine.Receive(i,old)
    assert(not record and reason=="slot_envelope_invalid" and not nextEngine.Snapshot().owner)
    assert(next(fresh.receipts)==nil and next(fresh.bodies)==nil and executions==1)
end

-- Public snapshots and caller-owned prepare tables must not retain authority.
for _,source in ipairs({"snapshot","prepare"}) do
    local public={schema="lycheedev.mailbox.v1",release="test",runtime=runtime}
    local ran,forged=0,0
    local isolated=ns.SlotProtocol.Create({runtime=runtime,release="test",build="70000",product=client,mailbox=public,
        actor=function()return actor end,encode=ns.CaptureWriter.Encode,compile=loadstring,
        execute=function(fn,budget,done)ran=ran+1;assert(budget==10);done(true,fn())end})
    local function request(index,action,nonce)
        return {schema="lycheedev.slot.v3",index=index,runtime=runtime,owner=owner,fence=1,
            nonce=nonce,ticket=ticket,action=action,guid=actor.guid,build="70000"}
    end
    local binding=request(1,"bind",string.rep("4",32));assert(isolated.Receive(1,binding))
    local original=request(2,"prepare",string.rep("5",32))
    original.code="return {answer=42}";original.codeBytes=#original.code
    original.codeChecksum=ns.MemoryProtocol.Checksum(original.code);original.budget=10
    original.extra={mutable=true}
    local _,prepared=isolated.Receive(2,original);assert(prepared.state=="prepared")
    local snapshot=isolated.Snapshot()
    local op=snapshot.operations[ticket]
    local mutation=source=="snapshot" and op.envelope or original
    mutation.nonce=string.rep("a",32);mutation.code="return {answer=99}";mutation.budget=120
    mutation.codeChecksum=0;mutation.guid="changed";mutation.owner=string.rep("b",32)
    if source=="snapshot" then
        op.challenge=string.rep("c",32);op.fn=function()forged=forged+1;return {answer=99}end
        snapshot.owner=string.rep("b",32);snapshot.fence=2
        snapshot.consumed[1]=nil;snapshot.receipts[binding.nonce]="forged receipt"
    end
    local wrong=request(3,"commit",string.rep("6",32))
    wrong.preparedNonce=mutation.nonce;wrong.challenge=string.rep("c",32)
    local _,rejection=isolated.Receive(3,wrong)
    assert(rejection.reason=="slot_commit_mismatch" and ran==0 and forged==0,"public mutation authorized commit")
    local commit=request(4,"commit",string.rep("7",32))
    commit.preparedNonce=string.rep("5",32);commit.challenge=prepared.challenge
    local _,accepted=isolated.Receive(4,commit)
    assert(accepted.state=="accepted" and ran==1 and forged==0,"private prepared identity changed through public alias")
    assert(public.bodies[ticket]:find('"answer":42',1,true) and not public.bodies[ticket]:find('"answer":99',1,true))
    assert(not isolated.Receive(4,commit) and ran==1 and forged==0,"valid commit replay executed again")
    assert(not isolated.Receive(1,binding),"snapshot reopened consumed input")
    local clean=isolated.Snapshot()
    assert(clean.owner==owner and clean.fence==1 and clean.receipts[binding.nonce]~="forged receipt")
    assert(clean.operations[ticket].fn==nil and clean.operations[ticket].envelope.code==nil
        and clean.operations[ticket].envelope.extra==nil,"snapshot exposed execution data or external tables")
end
