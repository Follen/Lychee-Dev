local Env,client=...
local ns=Env.LoadWorkbench()
ns.Release="test"
local state={options={bridgeEnabled=false}}
local bridge={}
ns.Persistence={Current=function()return state end,Bridge=function()return bridge end}
local ready=true
ns.Platform={ObserveBuild=function()return {build="70000",product=client}end,
    ObserveActor=function()return {guid="Player-1-1",character="Tester",realm="Realm"}end,
    ObserveInputState=function()return ready end}
ns.ActivityView={Receiving=function()end,Begin=function()end,Finish=function()end,Collecting=function()end}
ns.StartupBeacon={Stop=function()end}
C_Timer.NewTimer=function(_,callback)return {Cancel=function()end}end
ns.Compat.GetAddOnMetadata=function(name,key)
    if key=="Version" then return ns.Release end
    return tostring(tonumber(name:match("(%d+)$")))
end
Env.LoadAddon("Modules/AutomationHistory.lua",ns)
for _,file in ipairs({"MemoryProtocol","CaptureWriter","SlotProtocol","InputProtection","ProbeExecution","SlotRuntime"}) do
    Env.LoadAddon("Bridge/"..file..".lua",ns)
end
local frames=Env.framesCreated
assert(ns.SlotRuntime.Start())
assert(Env.framesCreated==frames,"passive identity created a frame")
local ok,reason=ns.SlotRuntime.Wake()
assert(not ok and reason=="slot_disabled" and Env.framesCreated==frames)
state.options.bridgeEnabled=true
ready=false
assert(not ns.SlotRuntime.Wake() and Env.framesCreated==frames)
ready=true
ns.Compat.LoadInputSlot=function()
    assert(ns.SlotRuntime.IsActive(),"connecting did not shield input")
    error("injected loader failure")
end
assert(not ns.SlotRuntime.Wake())
assert(not ns.SlotRuntime.IsActive(),"exception retained input")
local descriptor=ns.SlotRuntime.Snapshot().descriptor
local runtime=descriptor:sub(25,40):gsub(".",function(c)return string.format("%02x",c:byte())end)
local e={schema="lycheedev.slot.v1",index=1,runtime=runtime,owner=string.rep("2",32),fence=1,
    nonce=string.rep("3",32),ticket=string.rep("4",32),action="bind",guid="Player-1-1",build="70000"}
assert(not ns.SlotRuntime.Receive(1,e),"failed load left a receive capability armed")
ns.Compat.LoadInputSlot=function()
    assert(ns.SlotRuntime.IsActive())
    local record,value=ns.SlotRuntime.Receive(1,e)
    assert(record and value.state=="bound")
    return true
end
assert(ns.SlotRuntime.Wake())
assert(not ns.SlotRuntime.IsActive(),"control receipt retained input")
assert(not ns.SlotRuntime.Receive(2,e),"standalone slot load was accepted")
-- The production input adapter must feed the workbench, even while it is closed.
Env.LoadAddon("Modules/AutomationView.lua",ns)
local function dispatch(action,index)
    e.action=action;e.index=index;e.nonce=string.format("%032x",index)
    ns.Compat.LoadInputSlot=function()
        local envelope={};for key,value in pairs(e) do envelope[key]=value end
        local record,value=ns.SlotRuntime.Receive(index,envelope)
        assert(record and value.state~="rejected")
        return true
    end
    assert(ns.SlotRuntime.Wake())
end
e.code="return {answer=42}";e.codeBytes=#e.code;e.codeChecksum=ns.MemoryProtocol.Checksum(e.code);e.budget=10
dispatch("prepare",2)
local op=ns.SlotRuntime.Snapshot().operations[e.ticket]
e.preparedNonce=e.nonce;e.challenge=op.challenge
dispatch("commit",3)
ns.AutomationView.Collect()
assert(ns.AutomationView.GetCount()==1,"memory probe missing from Automation history")
local history=ns.AutomationView.GetRecord("MEM-"..e.ticket)
assert(history.status=="reported" and history.reportBody:find('"answer":42',1,true))
local body=history.reportBody
e.reportBytes=op.bytes;e.reportChecksum=op.checksum
dispatch("release",4)
ns.AutomationView.Collect()
assert(history.status=="acknowledged" and history.reportBody==body,"release removed history")
print("slot runtime input and disabled-state invariants ok")
