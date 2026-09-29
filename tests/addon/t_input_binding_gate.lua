local Env,client=...
local ns=Env.LoadWorkbench()
ns.Release="test";ns.Startup={ready=true}
local state={options={bridgeEnabled=true}}
ns.Persistence={Current=function()return state end,Bridge=function()return state end}
IsPlayerInWorld=function()return true end
IsLoggedIn=function()return true end
local base,override={},{}
GetBindingAction=function(chord,checkOverride)return checkOverride and (override[chord] or base[chord] or "") or (base[chord] or "")end
ClearOverrideBindings=function()override={}end
SetOverrideBindingClick=function(_,_,chord,name)override[chord]="CLICK "..name..":LeftButton";return true end
ns.Platform={ObserveBuild=function()return {build="70000",product=client}end,
    ObserveActor=function()return {guid="Player-1",character="Tester",realm="Realm"}end,
    ObserveInputState=function()return true end}
ns.Compat.GetAddOnMetadata=function(_,key)return key=="Version" and "test" or nil end
Env.LoadAddon("Modules/AutomationHistory.lua",ns)
for _,name in ipairs({"CaptureWriter","MemoryProtocol","SlotProtocol","ProbeExecution","ReceiverBindings","InputSignal","InputState","SlotRuntime"})do Env.LoadAddon("Bridge/"..name..".lua",ns)end
local function blocked()
    local packet=assert(ns.InputState.Snapshot())
    assert(packet:find('"inputBlocked":true',1,true) and packet:find('"reason":"input_binding_unavailable"',1,true),"invalid native binding advertised ready")
    local cells=ns.InputSignal.Frame().children
    assert(cells[1].colorTexture[1]==1 and cells[1].colorTexture[2]==1 and cells[1].colorTexture[3]==1)
    assert(cells[2].colorTexture[1]==0 and cells[2].colorTexture[2]==0 and cells[2].colorTexture[3]==0)
end
local function ready()assert(ns.InputState.Snapshot():find('"inputBlocked":false',1,true))end
assert(ns.SlotRuntime.Start());blocked() -- No registration yet.
base["ALT-CTRL-F12"]="TOGGLELYCHEE"
assert(not ns.SlotRuntime.Register());ns.InputState.Refresh();blocked()
assert(base["ALT-CTRL-F12"]=="TOGGLELYCHEE","sampler changed player binding")
-- External conflict resolution and explicit registration reset, not sampler repair.
base["ALT-CTRL-F12"]=nil
assert(ns.ReceiverBindings.Reset());ns.InputState.Refresh();ready()
override["ALT-CTRL-F12"]="TOGGLELYCHEE"
ns.InputState.Refresh();blocked()
override["ALT-CTRL-F12"]=nil
assert(ns.ReceiverBindings.Reset());ns.InputState.Refresh();ready()
local current=ns.ReceiverBindings.Current
ns.ReceiverBindings.Current=function()return {wake="ALT-CTRL-F11",submit="ALT-CTRL-SHIFT-F12",close="ALT-CTRL-["}end
ns.InputState.Refresh();blocked()
ns.ReceiverBindings.Current=function()error("binding lookup failed")end
ns.InputState.Refresh();blocked()
ns.ReceiverBindings.Current=current
ns.InputState.Refresh();ready()
ns.SlotRuntime=nil;ns.ReceiverBindings.Current=function()return nil end
ns.InputState.Refresh();ready() -- Legacy/non-native producer is unchanged.
ns.InputState.Stop()
print("native binding gate: initial conflict, takeover, recovery and legacy isolation")
