local root, framePath, controlPath, sendboxInput, sendboxOutput, bundle = arg[1], arg[2], arg[3], arg[4], arg[5], arg[6]
local ns = {CaptureWriter={}}
assert(loadfile(root.."/addon/Bridge/SHA256.lua"))("Lychee Dev",ns)
assert(loadfile(root.."/addon/Bridge/DuplexProtocol.lua"))("Lychee Dev",ns)
local function read(path)local f=assert(io.open(path,"rb"));local b=f:read("*a");f:close();return b end
local function words(bytes,count)
    local out={};for i=1,count do
        local p=(i-1)*4+1;local a,b,c,d=bytes:byte(p,p+3)
        out[i]=(a or 0)+((b or 0)*256)+((c or 0)*65536)+((d or 0)*16777216)
    end;return out
end
local frame=read(framePath)
local header=frame:sub(1,320)
local h,err=ns.DuplexProtocol.DecodeHeader(header,ns.DuplexProtocol.Kind.frame);assert(h,err)
assert(h.requestSeqHi==2097152 and h.requestSeqLo==1,"u64 request sequence lost precision")
local cells=words(frame,1104)
local parsedHeader=assert(ns.DuplexProtocol.ReadHeader(cells))
local payload=frame:sub(321)
assert(ns.DuplexProtocol.VerifyFrame(parsedHeader,payload,h))
local expected=assert(ns.DuplexProtocol.RequestDigest(h.requestId,h.actorBinding,h.budget,h.utcHi,h.utcLo,h.totalBytes,payload))
assert(expected==h.requestSHA)
local control=read(controlPath)
local controlWords=words(control,336)
local controlHeader=assert(ns.DuplexProtocol.ReadControl(controlWords))
local c=assert(ns.DuplexProtocol.DecodeHeader(controlHeader,ns.DuplexProtocol.Kind.reload))
assert(c.laneName=="reload" and c.publicationSeqHi==0 and c.publicationSeqLo==9)
local json=read(sendboxInput)
ns.CaptureWriter.Encode=function()return json end
local function writeRow(row,wire)
    for i=1,#row do row[i]=0 end
    local source=words(wire,#row)
    for i=1,#source do row[i]=source[i] end
end
local identity={runtime="00112233445566778899aabbccddeeff",arenaGeneration="10112233445566778899aabbccddeeff",
    actorBindingId="40112233445566778899aabbccddeeff",actorGUID="Player-1-1",character="Paladin",realm="Realm",build="12.1.0.12345",product="retail",release="3.1.1"}
local arena=assert(ns.DuplexProtocol.NewArena(identity.arenaGeneration))
local executions,pages=0,{}
local resultBody='{"ok":false,"error":"closed_before_execution","resourcesReleased":true}'
local engine=assert(ns.DuplexProtocol.Create({identity=identity,publish=function()return true end,
    compile=function()return function()end end,execute=function()executions=executions+1 end,
    clock=function()return 100 end,challenge=function()return string.rep(string.char(187),16)end,
    encode=function()return resultBody end,actor=function()return {guid=identity.actorGUID,character=identity.character,realm=identity.realm}end,
    publishPage=function(index,body)pages[index]=body end}))
assert(engine.BindIdentity(identity));assert(engine.Enable())
writeRow(arena.control.bindResume,read(bundle.."/bind.bin"));assert(engine.Poll(arena,8))
writeRow(arena.request.frames[1],frame);assert(engine.Poll(arena,8))
assert(engine.Snapshot().phase=="prepared","Go frame was not staged for commit")
writeRow(arena.control.close,read(bundle.."/close.bin"))
writeRow(arena.control.cancel,read(bundle.."/cancel.bin"))
writeRow(arena.control.commit,read(bundle.."/commit.bin"))
assert(engine.Poll(arena,8))
local state=engine.Snapshot()
assert(executions==0,"close/cancel did not win the same-tick commit race")
assert(state.terminal and state.terminal.outcome=="cancelled" and state.terminal.resourcesReleased==true)
assert(pages[1]==resultBody and state.terminal.resultBytes==#resultBody and #state.terminal.pageSHA256==1)
assert(state.receipts.close.state=="closed" and state.receipts.cancel.state=="cancel_too_late")
local forbidden,why=engine.BeginRepair("90112233445566778899aabbccddeeff",string.rep(string.char(171),16))
assert(not forbidden and why=="duplex_repair_unavailable","unacknowledged result entered idle repair")
writeRow(arena.control.resultAck,read(bundle.."/ack.bin"));assert(engine.Poll(arena,8))
assert(engine.Snapshot().request==nil,"result ACK did not retire the exact request")
assert(engine.Poll(arena,8));assert(engine.Snapshot().request==nil,"retired frame replay resurrected an old request")
local cancelArena=assert(ns.DuplexProtocol.NewArena(identity.arenaGeneration))
local completion,finished= nil,0
local cancelPages={}
local cancelEngine=assert(ns.DuplexProtocol.Create({identity=identity,publish=function()return true end,
    compile=function()return function()end end,execute=function(_,_,done)
        completion=done
        return {RequestCancel=function(self)completion(false,"probe_cancelled",{cancelled=true,resourcesReleased=false});return true,"cancelled"end}
    end,clock=function()return 100 end,challenge=function()return string.rep(string.char(187),16)end,
    encode=function()return resultBody end,actor=function()return {guid=identity.actorGUID,character=identity.character,realm=identity.realm}end,
    publishPage=function(index,body)cancelPages[index]=body end,finished=function()finished=finished+1 end}))
assert(cancelEngine.BindIdentity(identity));assert(cancelEngine.Enable())
writeRow(cancelArena.control.bindResume,read(bundle.."/bind.bin"));assert(cancelEngine.Poll(cancelArena,8))
writeRow(cancelArena.request.frames[1],frame);assert(cancelEngine.Poll(cancelArena,8))
writeRow(cancelArena.control.commit,read(bundle.."/commit.bin"));assert(cancelEngine.Poll(cancelArena,8))
assert(cancelEngine.Snapshot().phase=="running" and completion,"async request did not enter running state")
assert(cancelEngine.Disable())
local disabled=cancelEngine.Snapshot()
assert(disabled.disabled and disabled.terminal and disabled.terminal.outcome=="cancelled"
    and disabled.terminal.resourcesReleased==false and cancelPages[1]==resultBody and finished==1,
    "disable lost the cancellation terminal or claimed resources were released")
local nextGeneration="90112233445566778899aabbccddeeff"
local idleArena=assert(ns.DuplexProtocol.NewArena(identity.arenaGeneration))
local nextArena=assert(ns.DuplexProtocol.NewArena(nextGeneration))
local idleEngine=assert(ns.DuplexProtocol.Create({identity=identity,publish=function()return true end,
    compile=function(source)return function()end end,execute=function()end,clock=function()return 100 end,
    challenge=function()return string.rep(string.char(187),16)end,encode=function()return resultBody end,
    actor=function()return {guid=identity.actorGUID,character=identity.character,realm=identity.realm}end,
    publishPage=function()end}))
assert(idleEngine.BindIdentity(identity));assert(idleEngine.Enable())
writeRow(idleArena.control.bindResume,read(bundle.."/bind.bin"));assert(idleEngine.Poll(idleArena,8))
assert(idleEngine.BeginRepair(nextGeneration,string.rep(string.char(171),16)))
writeRow(nextArena.control.bindResume,read(bundle.."/repair.bin"));assert(idleEngine.Poll(nextArena,8))
local repaired=idleEngine.Snapshot()
assert(repaired.repaired and repaired.repair and repaired.repair.idle and repaired.repair.noPendingRequest
    and repaired.repair.resourcesReleased and repaired.repair.ledgerRetained and repaired.repair.previousWriterDrained,
    "repair proof missing: repaired="..tostring(repaired.repaired).." repair="..tostring(repaired.repair and repaired.repair.idle).." noPending="..tostring(repaired.repair and repaired.repair.noPendingRequest).." released="..tostring(repaired.repair and repaired.repair.resourcesReleased).." ledger="..tostring(repaired.repair and repaired.repair.ledgerRetained).." drained="..tostring(repaired.repair and repaired.repair.previousWriterDrained).." receipt="..tostring(repaired.receipts.bindResume and repaired.receipts.bindResume.state).." failure="..tostring(repaired.lastFailure))
assert(repaired.repair.requestId==string.rep("0",32) and repaired.repair.requestSHA256==string.rep("0",64))
assert(idleEngine.ReleaseRetiredArena())
local reconnectArena=assert(ns.DuplexProtocol.NewArena(identity.arenaGeneration))
local reconnectIdentity={runtime=identity.runtime,arenaGeneration=identity.arenaGeneration,
    actorBindingId=identity.actorBindingId,actorGUID=identity.actorGUID,character=identity.character,realm=identity.realm,
    build=identity.build,product=identity.product,release=identity.release}
local reconnectEngine=assert(ns.DuplexProtocol.Create({identity=reconnectIdentity,publish=function()return true end,
    compile=function()return function()end end,execute=function(_,_,done)
        done(true,true,{resourcesReleased=true});return {}
    end,clock=function()return 100 end,challenge=function()return string.rep(string.char(187),16)end,
    encode=function()return '{"ok":true,"result":true,"resourcesReleased":true}' end,
    actor=function()return {guid=identity.actorGUID,character=identity.character,realm=identity.realm}end}))
assert(reconnectEngine.BindIdentity(reconnectIdentity));assert(reconnectEngine.Enable())
writeRow(reconnectArena.control.bindResume,read(bundle.."/bind.bin"));assert(reconnectEngine.Poll(reconnectArena,8))
writeRow(reconnectArena.request.frames[1],read(bundle.."/fresh-frame.bin"));assert(reconnectEngine.Poll(reconnectArena,8))
writeRow(reconnectArena.control.commit,read(bundle.."/fresh-commit.bin"));assert(reconnectEngine.Poll(reconnectArena,8))
assert(reconnectEngine.Snapshot().terminal and reconnectEngine.Snapshot().terminal.outcome=="success")
writeRow(reconnectArena.control.resultAck,read(bundle.."/fresh-ack.bin"));assert(reconnectEngine.Poll(reconnectArena,8))
writeRow(reconnectArena.control.close,read(bundle.."/fresh-close.bin"));assert(reconnectEngine.Poll(reconnectArena,8))
assert(reconnectEngine.Snapshot().closing and reconnectEngine.Snapshot().phase=="closed")
writeRow(reconnectArena.control.bindResume,read(bundle.."/fresh-bind.bin"));assert(reconnectEngine.Poll(reconnectArena,8))
assert(reconnectEngine.Snapshot().receipts.bindResume.state=="bound","closed owner proof did not admit fresh owner")
for i=1,#reconnectArena.request.frames[1] do reconnectArena.request.frames[1][i]=0 end
writeRow(reconnectArena.control.cancel,read(bundle.."/stale-control.bin"));assert(reconnectEngine.Poll(reconnectArena,8))
assert(reconnectEngine.Snapshot().request==nil and reconnectEngine.Snapshot().lastFailure=="duplex_control_identity",
    "old owner control was admitted after fresh bind: request="..tostring(reconnectEngine.Snapshot().request).." failure="..tostring(reconnectEngine.Snapshot().lastFailure).." cancel="..tostring(reconnectEngine.Snapshot().receipts.cancel and reconnectEngine.Snapshot().receipts.cancel.state))
for i=1,#reconnectArena.control.cancel do reconnectArena.control.cancel[i]=0 end
writeRow(reconnectArena.request.frames[1],read(bundle.."/stale-frame.bin"));assert(reconnectEngine.Poll(reconnectArena,8))
assert(reconnectEngine.Snapshot().request==nil and reconnectEngine.Snapshot().lastFailure=="duplex_frame_identity",
    "old owner frame was admitted after fresh bind")
for i=1,#reconnectArena.request.frames[1] do reconnectArena.request.frames[1][i]=0 end
for _,lane in ipairs({"close","cancel","commit","resultAck","reload","lease"}) do
    for i=1,#reconnectArena.control[lane] do reconnectArena.control[lane][i]=0 end
end
writeRow(reconnectArena.control.bindResume,read(bundle.."/bad-fence-bind.bin"));assert(reconnectEngine.Poll(reconnectArena,8))
assert(reconnectEngine.Snapshot().identity and reconnectEngine.Snapshot().identity.runtime==identity.runtime
    and reconnectEngine.Snapshot().lastFailure=="duplex_bind_owner_conflict",
    "same owner/session bind changed the fence: failure="..tostring(reconnectEngine.Snapshot().lastFailure).." owner="..tostring(reconnectEngine.Snapshot().receipts.bindResume and reconnectEngine.Snapshot().receipts.bindResume.state))
for i=1,#reconnectArena.control.bindResume do reconnectArena.control.bindResume[i]=0 end
writeRow(reconnectArena.request.frames[1],read(bundle.."/second-frame.bin"));assert(reconnectEngine.Poll(reconnectArena,8))
assert(reconnectEngine.Snapshot().phase=="prepared","fresh owner request sequence 1 was not accepted")
writeRow(reconnectArena.control.commit,read(bundle.."/second-commit.bin"));assert(reconnectEngine.Poll(reconnectArena,8))
assert(reconnectEngine.Snapshot().terminal and reconnectEngine.Snapshot().terminal.outcome=="success")
writeRow(reconnectArena.control.resultAck,read(bundle.."/second-ack.bin"));assert(reconnectEngine.Poll(reconnectArena,8))
local sendbox=assert(ns.DuplexProtocol.Sendbox({},65536))
local f=assert(io.open(sendboxOutput,"wb"));assert(f:write(sendbox));f:close()
-- Exercise the actual encoder at the execution callback boundary. A constant
-- fixture encoder cannot detect a false result being dropped or an error
-- field incorrectly appearing in a successful result.
assert(loadfile(root.."/addon/Bridge/CaptureWriter.lua"))("Lychee Dev",ns)
for _,case in ipairs({{ok=true,value=false},{ok=true,value=true},{ok=true,value=0},
    {ok=true,value="answer"},{ok=false,value="failure"}}) do
    local realArena=assert(ns.DuplexProtocol.NewArena(identity.arenaGeneration))
    local actualPages={}
    local realEngine=assert(ns.DuplexProtocol.Create({identity=identity,publish=function()return true end,
        compile=function()return function()end end,
        execute=function(_,_,done)done(case.ok,case.value,{resourcesReleased=true,logs={}});return {} end,
        clock=function()return 100 end,challenge=function()return string.rep(string.char(187),16)end,
        encode=ns.CaptureWriter.Encode,
        actor=function()return {guid=identity.actorGUID,character=identity.character,realm=identity.realm}end,
        publishPage=function(index,body)actualPages[index]=body end}))
    assert(realEngine.BindIdentity(identity));assert(realEngine.Enable())
    writeRow(realArena.control.bindResume,read(bundle.."/bind.bin"));assert(realEngine.Poll(realArena,8))
    writeRow(realArena.request.frames[1],read(bundle.."/fresh-frame.bin"));assert(realEngine.Poll(realArena,8))
    writeRow(realArena.control.commit,read(bundle.."/fresh-commit.bin"));assert(realEngine.Poll(realArena,8))
    local expectedRecord={ok=case.ok,resourcesReleased=true,logs={}}
    if case.ok then expectedRecord.result=case.value else expectedRecord.error=case.value end
    local expectedBody=assert(ns.CaptureWriter.Encode(expectedRecord,524288))
    assert(actualPages[1]==expectedBody,"execution envelope changed success/false/error: "..tostring(actualPages[1]))
end
print("duplex: Go wire, exact u64, ACK, repair, close and fresh-owner fencing passed")
