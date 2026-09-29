local Env,client=...
local ns=Env.LoadWorkbench()
ns.Release="test"
local validatedLoadInputSlot=ns.Compat.LoadInputSlot
local state={options={bridgeEnabled=false}}
local bridge={}
ns.Persistence={Current=function()return state end,Bridge=function()return bridge end}
local ready=true
ns.ReceiverBindings={Current=function()return {wake="ALT-CTRL-F12",submit="ALT-CTRL-SHIFT-F12",close="ALT-CTRL-["}end}
IsPlayerInWorld=function()return true end
ns.Platform={ObserveBuild=function()return {build="70000",product=client}end,
    ObserveActor=function()return {guid="Player-1-1",character="Tester",realm="Realm"}end,
    ObserveInputState=function(allowFocused)
        if not allowFocused and ns.InputProtection then assert(not ns.InputProtection.IsActive(),"sampled while input protected") end
        return ready,not ready and "input_keyboard_focus" or nil
    end}
ns.ActivityView={Receiving=function()end,Begin=function()end,Finish=function()end,Collecting=function()end}
ns.StartupBeacon={Stop=function()end}
C_Timer.NewTimer=function(_,callback)return {Cancel=function()end}end
ns.Compat.GetAddOnMetadata=function(name,key)
    if key=="Version" then return ns.Release end
    if key=="X-Lychee-Transport" then return "memory-slot-v2" end
    return tostring(tonumber(name:match("(%d+)$")))
end
Env.LoadAddon("Modules/AutomationHistory.lua",ns)
for _,file in ipairs({"MemoryProtocol","CaptureWriter","SlotProtocol","InputProtection","ProbeExecution","InputSignal","InputState","SlotRuntime"}) do
    Env.LoadAddon("Bridge/"..file..".lua",ns)
end
local frames=Env.framesCreated
assert(ns.SlotRuntime.Start())
assert(Env.framesCreated==frames,"passive identity created a frame")
local ok,reason=ns.SlotRuntime.Wake()
assert(not ok and reason=="slot_disabled" and Env.framesCreated==frames)
state.options.bridgeEnabled=true
ready=false
assert(not ns.SlotRuntime.Wake() and Env.framesCreated==frames+1)
assert(ns.InputState.Snapshot():find('"inputBlocked":true',1,true))
local rejected=ns.InputState.Snapshot()
ready=true
ns.Compat.LoadInputSlot=function()
    assert(ns.SlotRuntime.IsActive(),"connecting did not shield input")
    error("injected loader failure")
end
assert(not ns.SlotRuntime.Wake())
assert(not ns.SlotRuntime.IsActive(),"exception retained input")
assert(ns.InputState.Snapshot()~=rejected and ns.InputState.Snapshot():find('"inputBlocked":false',1,true),"failed wake did not refresh after release")
local descriptor=ns.SlotRuntime.Snapshot().descriptor
local runtime=descriptor:sub(25,40):gsub(".",function(c)return string.format("%02x",c:byte())end)
local e={schema="lycheedev.slot.v2",index=1,runtime=runtime,owner=string.rep("2",32),fence=1,
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
assert(ns.InputState.Snapshot():find('"nextSlot":2',1,true) and ns.InputState.Snapshot():find('"owner":"'..e.owner..'"',1,true),"bind did not immediately refresh identity")
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
    assert(ns.InputState.Snapshot():find('"nextSlot":'..(index+1),1,true),"dispatch sample has old slot")
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
dispatch("unbind",5)
assert(ns.InputState.Snapshot():find('"owner":""',1,true),"unbind sample retained owner")
-- One wake traverses only empty/foreign slots and stops at the first own envelope.
local loads={}
local function envelope(index,action,run)
    return {schema="lycheedev.slot.v2",index=index,runtime=run or runtime,owner=e.owner,fence=1,
        nonce=string.format("%032x",index+1000),ticket=string.rep("0",32),action=action or "bind",guid=e.guid,build=e.build}
end
ns.Compat.LoadInputSlot=function(name)
    local index=tonumber(name:match("(%d+)$"));loads[#loads+1]=index
    local payload=index==6 and nil or envelope(index)
    if index==6 then payload=nil end
    if index==7 then payload.runtime=string.rep("a",32);payload.code="error('foreign code must never run')" end
    ns.SlotRuntime.Receive(index,payload);return true
end
assert(ns.SlotRuntime.Wake() and #loads==3 and loads[1]==6 and loads[3]==8)
assert(ns.SlotRuntime.Snapshot().receipts[string.format("%032x",1007)]==nil,"foreign receipt fabricated")
assert(ns.InputState.Snapshot():find('"nextSlot":9',1,true))
local rejectedOwn=envelope(9);rejectedOwn.owner=string.rep("b",32)
loads={};ns.Compat.LoadInputSlot=function(name)
    local index=tonumber(name:match("(%d+)$"));loads[#loads+1]=index
    local record,value=ns.SlotRuntime.Receive(index,rejectedOwn)
    assert(record and value.reason=="slot_owner_busy");return true
end
assert(ns.SlotRuntime.Wake() and #loads==1,"own rejected envelope scanned past")
-- A late wake hitting an old own nonce must also stop after one loaded slot.
local duplicate=envelope(10);duplicate.nonce=string.format("%032x",1008)
loads={};ns.Compat.LoadInputSlot=function(name)
    local index=tonumber(name:match("(%d+)$"));loads[#loads+1]=index
    ns.SlotRuntime.Receive(index,duplicate);return true
end
assert(ns.SlotRuntime.Wake() and #loads==1,"own reused nonce scanned past")
loads={};ns.Compat.LoadInputSlot=function(name)
    local index=tonumber(name:match("(%d+)$"));loads[#loads+1]=index
    ns.SlotRuntime.Receive(index,{runtime=string.rep("a",32)});return true
end
assert(ns.SlotRuntime.Wake() and #loads==1,"malformed foreign was skipped")
ns.Compat.LoadInputSlot=function()return true end
local ok,reason=ns.SlotRuntime.Wake()
assert(not ok and reason=="slot_loader_no_dispatch" and not ns.SlotRuntime.IsActive())
local metadata=ns.Compat.GetAddOnMetadata
ns.Compat.GetAddOnMetadata=function(name,key)
    if name=="Lychee Dev Slot 200" and key=="X-Lychee-Transport" then return "memory-slot-v1" end
    return metadata(name,key)
end
local ok,reason=ns.SlotRuntime.Wake();assert(not ok and reason=="slot_inventory_incomplete:200")
ns.Compat.GetAddOnMetadata=metadata
loads={};ns.Compat.LoadInputSlot=function(name)
    local index=tonumber(name:match("(%d+)$"));loads[#loads+1]=index
    ns.SlotRuntime.Receive(index,index==200 and envelope(index) or nil);return true
end
assert(ns.SlotRuntime.Wake() and #loads==189 and loads[1]==12 and loads[#loads]==200)
assert(ns.InputState.Snapshot():find('"nextSlot":201',1,true))
local ok,reason=ns.SlotRuntime.Wake();assert(not ok and reason=="slot_exhausted" and #loads==189)
assert(not ns.SlotRuntime.IsActive(),"exhaustion retained input")
-- Canonical two/three-digit names only; numeric expansion is bounded at 200.
local addonLoader=C_AddOns.LoadAddOn;local named={}
C_AddOns.LoadAddOn=function(name)named[#named+1]=name;return true end
for _,name in ipairs({"Lychee Dev Slot 01","Lychee Dev Slot 99","Lychee Dev Slot 100","Lychee Dev Slot 200"}) do assert(validatedLoadInputSlot(name)) end
for _,name in ipairs({"Lychee Dev Slot 00","Lychee Dev Slot 201","Lychee Dev Slot 001","Lychee Dev Slot 1","Lychee Dev Slot -1"}) do assert(not validatedLoadInputSlot(name)) end
assert(#named==4);C_AddOns.LoadAddOn=addonLoader
ready=false
ns.SlotRuntime.Close()
assert(ns.InputState.Snapshot():find('"inputBlocked":true',1,true),"close did not refresh")
ns.InputState.Stop()
state.options.bridgeEnabled=false
ns.SlotRuntime.Close();assert(not ns.InputState.Snapshot(),"close revived stopped sampler")
assert(not ns.SlotRuntime.Wake() and not ns.InputState.Snapshot(),"disabled wake revived sampler")
print("slot runtime input and disabled-state invariants ok")


-- A freshly loaded runtime can burn all 200 empty slots in one bounded wake.
Env.LoadAddon("Bridge/SlotRuntime.lua",ns)
state.options.bridgeEnabled=true;ready=true
local emptyLoads=0
ns.Compat.LoadInputSlot=function(name)
    local index=tonumber(name:match("(%d+)$"));emptyLoads=emptyLoads+1
    ns.SlotRuntime.Receive(index,nil);return true
end
local ok,reason=ns.SlotRuntime.Wake()
assert(not ok and reason=="slot_exhausted" and emptyLoads==200)
assert(not ns.SlotRuntime.IsActive() and ns.InputState.Snapshot():find('"nextSlot":201',1,true))
