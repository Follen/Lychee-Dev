local root, queue = assert(arg[1]), assert(arg[2])
local receiptPath, sequence = assert(arg[3]), assert(arg[4])
local ns = {Release="2.0.3",Startup={ready=true,identity={product="retail",build="12.1.0.12345"}}}
ns.Platform = {ObserveActor=function() return {character="Paladin",realm="Realm",guid="Player-1-123"} end}
issecretvalue = function() return false end
CreateFrame = function() error("unexpected frame") end
for _, name in ipairs({"Core/Persistence.lua","Bridge/Session.lua","Bridge/CaptureWriter.lua","Bridge/ReportStore.lua","Bridge/ProbeRunner.lua","Core/Controls.lua"}) do
    assert(loadfile(root.."/"..name))("Lychee Dev",ns)
end
assert(loadfile(queue))("Lychee Dev",ns)
local entry = ns.ProbeDefinitions.entries["OP-target"]
assert(loadfile(root.."/Bridge/ProbeQueue.lua"))("Lychee Dev",ns)
assert(ns.Persistence.Load())
LycheeToolkitDB.options.bridgeEnabled = true
assert(ns.Session.Bind(string.rep("a",32)))
assert(ns.ProbeRunner.Load("OP-target",entry.code,entry.reloadNonce))
local original = assert(ns.ProbeRunner.Dispatch("OP-target"))
local receiptFile = assert(io.open(receiptPath, "rb"))
assert(original == receiptFile:read("*a"), "host archived receipt differs")
receiptFile:close()
assert(entry.acknowledgement == nil)
-- New private runtime state after reload; the original queue and saved report
-- remain unchanged. Acknowledgement must not depend on the previous runner.
assert(loadfile(root.."/Bridge/Session.lua"))("Lychee Dev",ns)
assert(loadfile(root.."/Bridge/ProbeRunner.lua"))("Lychee Dev",ns)
assert(ns.Session.Bind(string.rep("a",32)))
local foreign = {receipt="foreign receipt",body="foreign body"}
LycheeToolkitDB.reports["OP-foreign"] = foreign
local displayed, reply, readiness
ns.Platform.ObserveInputState = function() return true end
ns.ReceiptView = {Hide=function() displayed=nil;readiness=nil end, Show=function(value, ready) displayed=value;readiness=ready; return true end}
SlashCmdList, SLASH_LYCHEETOOLKIT1 = {}, nil
assert(ns.Controls.Register())
print = function(value) reply=value end
for _, suffix in ipairs({"", " 0", " 01", " -1", " 1.5", " 1e3", " 9007199254740992", " "..(tonumber(sequence)+1)}) do
    assert(ns.Controls.Handle("bridge ack OP-target"..suffix) == nil)
    assert(ns.ReportStore.Read("OP-target"), "failed acknowledgement deleted report")
end
local stored = LycheeToolkitDB.reports["OP-target"]
assert(ns.ProbeQueue.Busy() == true, "unacknowledged queue must remain busy")
local body = stored.body
stored.body = body.." "
assert(ns.Controls.Handle("bridge ack OP-target "..sequence) == nil)
assert(ns.ReportStore.Read("OP-target"), "changed body was deleted")
stored.body = body
SlashCmdList.LYCHEETOOLKIT("BRIDGE ACK OP-target "..sequence)
local acknowledged = assert(displayed)
local ready = assert(readiness, "ACK did not expose separate readiness")
assert(reply == "Lychee Dev: "..acknowledged)
assert(ns.ReportStore.Read("OP-target") == nil)
assert(ns.ProbeQueue.Busy() == false, "ACK left the runtime queue busy")
assert(ns.ProbeQueue.Load("OP-target") == nil, "ACK allowed source to load again")
SlashCmdList.LYCHEETOOLKIT("bridge ack OP-target "..sequence)
assert(displayed == nil and reply == "Lychee Dev: report_unavailable")
assert(queueExecuted == 1, "acknowledgement re-executed code")
assert(LycheeToolkitDB.reports["OP-foreign"] == foreign)
-- Serialize the resulting live table as a fixture, not as evidence of a real
-- client disk flush. WoW's writer and source binding are separate acceptance.
local rows = {}
for key, record in pairs(LycheeToolkitDB.reports) do
    rows[#rows+1] = string.format("[%q]={receipt=%q,body=%q}",key,record.receipt,record.body)
end
local saved = "LycheeToolkitDB={schema=1,reports={"..table.concat(rows,",").."}}"
io.write(assert(ns.CaptureWriter.Encode({acknowledged=acknowledged,ready=ready,saved=saved})))
