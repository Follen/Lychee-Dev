local root, queue = assert(arg[1]), assert(arg[2])
local ns = { Release = "2.0.6", Startup = {ready=true,identity={product="retail",build="12.1.0.12345"}} }
local actor = {character="Paladin",realm="Realm",guid="Player-1-123"}
ns.Platform = {ObserveActor=function() return actor end}
local secret = {}
issecretvalue = function(value) return rawequal(value,secret) end
CreateFrame = function() error("queue must not create frames") end
for _, name in ipairs({"Core/Persistence.lua","Bridge/Session.lua","Bridge/CaptureWriter.lua","Bridge/ReportStore.lua","Bridge/ProbeRunner.lua","Core/Controls.lua"}) do
    assert(loadfile(root.."/"..name))("Lychee Dev",ns)
end
assert(loadfile(queue))("Lychee Dev",ns)
local definitions = ns.ProbeDefinitions
assert(loadfile(root.."/Bridge/ProbeQueue.lua"))("Lychee Dev",ns)
assert(ns.ProbeDefinitions==nil)
assert(ns.Persistence.Load())
local displayed, reply, showFailure
ns.ReceiptView = {
    Hide = function() displayed = nil end,
    Show = function(receipt)
        if showFailure then return nil, "fixture_display_failed" end
        displayed = receipt; return true
    end,
}
SlashCmdList, SLASH_LYCHEETOOLKIT1 = {}, nil
assert(ns.Controls.Register() and SLASH_LYCHEETOOLKIT1 == "/dev")
local savedPrint = print
print = function(value) reply = value end
assert(ns.ProbeQueue.Load("OP-target")==nil)
LycheeToolkitDB.options.bridgeEnabled=true
assert(ns.Session.Bind(string.rep("a",32)))
local entry=definitions.entries["OP-target"]
local retired, retirementFailure = ns.ProbeQueue.VerifyRetired("OP-target", string.rep("c",32))
assert(retired == nil and retirementFailure == "queue_request_retained")
assert(definitions.entries["OP-target"] == entry and ns.Session.Current().sequence == 0)
local function fails(expected)
    local value,reason=ns.ProbeQueue.Load("OP-target")
    assert(value==nil and reason==expected,tostring(reason).." expected "..expected)
end
for _,field in ipairs({"character","realm","guid","product","build","release","sessionNonce"}) do
    local held=entry[field]
    entry[field]=field=="sessionNonce" and string.rep("c",32) or "Wrong"
    fails("queue_identity_mismatch")
    entry[field]=held
end
entry.extra=true; fails("queue_invalid_entry"); entry.extra=nil
local digest=entry.codeAdler32
entry.codeAdler32="00000000"; fails("queue_code_mismatch"); entry.codeAdler32=digest
local code=entry.code
entry.code=secret; fails("queue_code_mismatch"); entry.code=code
entry.character=secret; fails("queue_invalid_identity"); entry.character="Paladin"
queueExecuted=nil
SlashCmdList.LYCHEETOOLKIT("  BRIDGE LOAD OP-target  ")
local loaded=assert(displayed)
assert(reply == "Lychee Dev: " .. loaded, "receipt was double-encoded")
assert(queueExecuted==nil,"queue load executed probe")
entry.code="error('changed after load')"
SlashCmdList.LYCHEETOOLKIT("bridge RUN OP-target")
local receipt=assert(displayed)
assert(reply == "Lychee Dev: " .. receipt)
assert(queueExecuted==1,"frozen selected code was not executed")
local _,body=ns.ReportStore.Read("OP-target")
SlashCmdList.LYCHEETOOLKIT("bridge run OP-target")
assert(reply == "Lychee Dev: probe_already_dispatched" and displayed == nil and queueExecuted == 1)
assert(ns.Controls.Handle("bridge load OP-target extra") == nil)
assert(ns.Controls.Handle("bridge run " .. string.rep("a",129)) == nil)
assert(ns.Controls.Handle(string.rep("a",257)) == nil)
assert(ns.Controls.Handle(secret) == nil)
-- A display failure after execution must not make an executed request retryable.
assert(ns.ProbeRunner.Load("OP-display", "queueExecuted = queueExecuted + 1; return 42"))
showFailure = true
SlashCmdList.LYCHEETOOLKIT("bridge run OP-display")
assert(reply == "Lychee Dev: fixture_display_failed" and queueExecuted == 2)
assert(ns.ReportStore.Read("OP-display"))
showFailure = false
SlashCmdList.LYCHEETOOLKIT("bridge run OP-display")
assert(reply == "Lychee Dev: probe_already_dispatched" and queueExecuted == 2)
assert(ns.Controls.Handle("bridge off"))
SlashCmdList.LYCHEETOOLKIT("bridge run OP-target")
assert(reply == "Lychee Dev: bridge_disabled" and displayed == nil)
print = savedPrint
io.write(assert(ns.CaptureWriter.Encode({loaded=loaded,receipt=receipt,body=body,code=code})))
