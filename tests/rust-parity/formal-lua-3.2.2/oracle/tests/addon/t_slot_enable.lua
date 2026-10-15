local Env,client=...
local guid="Player-1-current"
local protocol="Lycheedev mailbox protocol v1"
local enabled,queries,writes,saves,loads={},0,{},0,0
local failRead,failEnable,failVerify,failSave,foreign

local function index(name)
    local value=tonumber(name:match("^Lychee Dev Slot (%d+)$"))
    assert(value and value>=1 and value<=500 and name==string.format("Lychee Dev Slot %02d",value),"touched another addon")
    return value
end
C_AddOns.GetAddOnMetadata=function(name,key)
    local i=index(name)
    if key=="X-Lychee-Slot" then return tostring(i) end
    if key=="Version" then return foreign==i and "foreign" or "test" end
    if key=="X-Lychee-Transport" then return protocol end
end
C_AddOns.GetAddOnEnableState=function(name,character)
    local i=index(name)
    assert(character==guid,"enable state used a character name or another actor")
    assert(i~=foreign,"queried a slot with foreign metadata")
    queries=queries+1
    if failRead==i then error("read failed") end
    return enabled[i]==false and 0 or 2
end
C_AddOns.EnableAddOn=function(name,character)
    local i=index(name)
    assert(character==guid,"enabled globally or for another actor")
    writes[#writes+1]=i
    if failEnable==i then error("enable failed") end
    if failVerify~=i then enabled[i]=true end
end
C_AddOns.SaveAddOns=function()
    saves=saves+1
    if failSave then error("save failed") end
end
C_AddOns.LoadAddOn=function()loads=loads+1;error("startup loaded a slot")end
IsPlayerInWorld=function()return true end

local function start(bridgeEnabled,expectedFailure)
    local ns=Env.LoadWorkbench()
    ns.Release="test"
    local state={options={bridgeEnabled=bridgeEnabled}}
    local bridge={}
    ns.Persistence={Current=function()return state end,Bridge=function()return bridge end}
    ns.Platform={ObserveBuild=function()return {build="70000",product=client}end,
        ObserveActor=function()return {guid=guid,character="NotAGuid",realm="Realm"}end,
        ObserveInputState=function()return true end}
    ns.ReceiverBindings={Current=function()return {wake="ALT-CTRL-F12",reload="ALT-CTRL-F11"}end}
    Env.LoadAddon("Modules/AutomationHistory.lua",ns)
    for _,file in ipairs({"MemoryProtocol","CaptureWriter","SlotProtocol","InputProtection","ProbeExecution","InputState","SlotRuntime"}) do
        Env.LoadAddon("Bridge/"..file..".lua",ns)
    end
    local ok,reason=ns.SlotRuntime.Start()
    if expectedFailure then
        assert(not ok and reason==expectedFailure,"startup hid an enable failure: "..tostring(reason))
        assert(ns.Mailbox==nil,"failed repair published ready input")
    else assert(ok,reason) end
    return ns,state
end
local function flag(ns,expected)
    local input=assert(ns.Mailbox.input)
    assert((input:find('"slotReloadRequired":true',1,true)~=nil)==expected,"wrong reload request")
    assert(input:find('"inputBlocked":false',1,true),"reload request changed input safety")
    assert(input:find('"nextSlot":1',1,true),"startup consumed a slot")
end
local function reset()
    enabled,queries,writes,saves,loads={},0,{},0,0
    failRead,failEnable,failVerify,failSave,foreign=nil,nil,nil,nil,nil
end

enabled[1],enabled[200],enabled[500]=false,false,false
local ns=start(true)
assert(#writes==3 and writes[1]==1 and writes[2]==200 and writes[3]==500,"startup did not enable disabled managed slots")
assert(queries==503 and saves==1 and loads==0,"startup must check 500 slots, verify changes and save once without loading")
flag(ns,true)
local reads=queries
assert(ns.SlotRuntime.Start());ns.InputState.Refresh();flag(ns,true)
assert(queries==reads and #writes==3 and saves==1,"startup check repeated within a runtime")
ns.InputState.Stop()

-- The persisted settings survive reload; the runtime-local request does not.
writes,saves={},0
ns=start(true);flag(ns,false)
assert(#writes==0 and saves==0 and loads==0,"enabled inventory wrote settings")
ns.InputState.Stop()

reset();enabled[1]=false;foreign=1
ns=start(true);flag(ns,false)
assert(queries==499 and #writes==0 and saves==0,"foreign slot was modified")
ns.InputState.Stop()

for _,failure in ipairs({"read","enable","verify","save","unavailable"}) do
    reset();enabled[1]=false
    if failure=="read" then failRead=1 elseif failure=="enable" then failEnable=1
    elseif failure=="verify" then failVerify=1 elseif failure=="save" then failSave=true end
    local enable=C_AddOns.EnableAddOn
    if failure=="unavailable" then C_AddOns.EnableAddOn=nil end
    local expected={read="slot_enable_state_failed",enable="slot_enable_failed",verify="slot_enable_failed",
        save="slot_enable_save_failed",unavailable="slot_enable_unavailable"}
    ns=start(true,expected[failure])
    assert(loads==0,"failed enable loaded a slot")
    if failure~="save" then assert(saves==0,"saved without a verified enable") end
    if failure=="save" then
        failSave=false
        assert(ns.SlotRuntime.Start());flag(ns,true)
        assert(saves==2 and #writes==1,"failed save was lost when enable state became enabled")
    end
    ns.InputState.Stop();C_AddOns.EnableAddOn=enable
end

reset();enabled[1],enabled[2]=false,false;failEnable=2
ns=start(true,"slot_enable_failed")
assert(enabled[1] and enabled[2]==false and saves==1,"partial success was discarded or claimed an unsuccessful enable")
failEnable=nil
assert(ns.SlotRuntime.Start());flag(ns,true)
ns.InputState.Stop()

reset();enabled[1]=false
local frames=Env.framesCreated
local disabledState
ns,disabledState=start(false)
assert(Env.framesCreated==frames and ns.Mailbox==nil,"disabled bridge created input sampling")
assert(#writes==1 and saves==1 and loads==0,"disabled bridge did not prepare managed slots")
disabledState.options.bridgeEnabled=true
assert(ns.SlotRuntime.Start());flag(ns,true)
ns.InputState.Stop()
print("slot startup: current-character enable/save, sticky reload request, metadata isolation and failures")
