local root=assert(arg[1])
local ns={Release="3.0.2",Startup={ready=true,identity={product="retail",build="12.1.0.69933"}},
    Platform={ObserveActor=function() return {character="Paladin",realm="Realm",guid="Player-1-123"} end,
        ObserveInputState=function() return true end},
    ReceiptView={Show=function() return true end}}
issecretvalue=function() return false end
GetTime=function() return 0 end
CreateFrame=function() error("investigation created idle machinery") end
for _,file in ipairs({"Core/Persistence.lua","Core/Compat.lua","Bridge/Session.lua","Bridge/CaptureWriter.lua",
    "Bridge/ReportStore.lua","Bridge/Investigation.lua","Bridge/ProbeRunner.lua"}) do
    assert(loadfile(root.."/"..file))("Lychee Dev",ns)
end
assert(ns.Persistence.Load());LycheeToolkitDB.options.bridgeEnabled=true
assert(ns.Session.Bind(string.rep("a",32)))
local entry={codeSHA256=string.rep("b",64)}
local id="REQ-investigation"
assert(ns.Investigation.Reserve(id,entry))
assert(not ns.Investigation.Reserve(id,entry))
assert(ns.Investigation.Busy())
assert(not ns.Investigation.Finish(id,string.rep("c",32)))
local code="return {answer=42}"
assert(ns.ProbeRunner.Load(id,code,string.rep("d",32),10,"finished"))
local loadedAgain=assert(ns.Investigation.ReadyReceipt(id))
assert(loadedAgain:find('"kind":"loaded"',1,true) and loadedAgain:find('"inputReady":true',1,true))
local reported=assert(ns.ProbeRunner.Dispatch(id))
assert(not ns.ProbeRunner.Dispatch(id))
local terminal=LycheeToolkitBridgeDB.terminals[id]
assert(terminal.phase=="reported" and terminal.receipt==reported)
local observed=assert(ns.Investigation.Observe(id,string.rep("c",32)))
assert(not ns.Investigation.Finish(id,string.rep("c",32)))
-- Exact receipt fields, as supplied by the queue's ACK endpoint.
local _,body=ns.ReportStore.Read(id)
local sequence=tonumber(reported:match('"sequence":(%d+)'))
local acknowledgement=assert(ns.ReportStore.Acknowledge({schema="lycheedev.signal.v1",release=ns.Release,
    kind="reported",sessionNonce=string.rep("a",32),requestId=id,character="Paladin",realm="Realm",
    product="retail",build="12.1.0.69933",sequence=sequence,inputReady=false,
    codeBytes=#code,codeAdler32=ns.CaptureWriter.DigestBytes(code),reportBytes=#body,reportAdler32=ns.CaptureWriter.DigestBytes(body)}))
assert(ns.Investigation.Busy(),"ACK released a complete investigation early")
assert(not ns.Investigation.Finish(id,"invalid"))
local finished=assert(ns.Investigation.Finish(id,string.rep("e",32)))
assert(ns.Investigation.Busy()==false)
assert(ns.Investigation.Finish(id,string.rep("f",32)))
assert(not ns.Investigation.Mark(id,"running"))
-- Retained proof survives reload without inheriting execution permission.
assert(loadfile(root.."/Bridge/Session.lua"))("Lychee Dev",ns)
assert(ns.Session.Bind(string.rep("a",32)))
assert(ns.Investigation.Observe(id,string.rep("c",32)))
assert(LycheeToolkitBridgeDB.terminals[id].receipt==acknowledgement)
-- Invalid result values still finish as ordinary, verifiable failure reports.
local cycleId="REQ-cycle"
assert(ns.Investigation.Reserve(cycleId,entry))
assert(ns.ProbeRunner.Load(cycleId,"local t={};t.self=t;return t",string.rep("d",32),10,"finished"))
assert(ns.ProbeRunner.Dispatch(cycleId))
local _,failedBody=ns.ReportStore.Read(cycleId)
assert(failedBody:find('"probeStatus":"failed"',1,true) and failedBody:find('report_cycle',1,true))
assert(LycheeToolkitBridgeDB.terminals[cycleId].phase=="reported")
-- Reserve the failure budget before user code can run; no report is evicted.
local originalReports=LycheeToolkitBridgeDB.reports
LycheeToolkitBridgeDB.reports={}
for i=1,100 do LycheeToolkitBridgeDB.reports["full"..i]={receipt="x",body="x"} end
local admitted,capacity=ns.ReportStore.Reserve("too-full")
assert(not admitted and capacity=="report_count_limit")
assert(LycheeToolkitBridgeDB.reports.full100.body=="x")
LycheeToolkitBridgeDB.reports=originalReports
-- No capacity is stolen from another unarchived execution.
local store=LycheeToolkitBridgeDB.terminals
store[id]=nil;store[cycleId]=nil
for i=1,16 do store["other"..i]={sessionNonce=string.rep("a",32),guid="other"..i,epoch=1,
    phase="running",codeSHA256=string.rep("b",64)} end
assert(not ns.Investigation.Reserve("capacity",entry))
assert(store.other16.phase=="running")
store.other16.epoch="invalid"
assert(not ns.Investigation.Observe("other16",string.rep("c",32)))
io.write(assert(ns.CaptureWriter.Encode({observed=observed,finished=finished})))
