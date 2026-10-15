local Env,client=...
local ns=Env.LoadWorkbench()
local originalPrint,notices=print,{}
print=function(text)notices[#notices+1]=text end
ns.Release="test";ns.Startup={ready=true}
local saved={wake="ALT-CTRL-F1",submit="ALT-CTRL-F2",close="ALT-CTRL-F3"}
local state={options={bridgeEnabled=true,receiverBindings=saved}}
ns.Persistence={Current=function()return state end,Bridge=function()return state end}
IsPlayerInWorld=function()return true end
IsLoggedIn=function()return true end
local base,override,foreign={},{},{}
local combat,active=false,false
InCombatLockdown=function()return combat end
SaveBindings=function()error("must not save account bindings")end
local createFrame=CreateFrame
local frameCount,eventCalls,scriptCalls,overrideCalls=0,0,0,0
CreateFrame=function(...)
    local frame=createFrame(...);frameCount=frameCount+1
    local registerEvent,setScript=frame.RegisterEvent,frame.SetScript
    frame.RegisterEvent=function(self,...)eventCalls=eventCalls+1;return registerEvent(self,...)end
    frame.SetScript=function(self,...)scriptCalls=scriptCalls+1;return setScript(self,...)end
    return frame
end
GetBindingAction=function(chord,checkOverride)return checkOverride and (foreign[chord] or override[chord] or base[chord] or "") or (base[chord] or "")end
ClearOverrideBindings=function()overrideCalls=overrideCalls+1;override={}end
SetOverrideBindingClick=function(_,_,chord,name)overrideCalls=overrideCalls+1;override[chord]="CLICK "..name..":LeftButton";return true end
ns.Platform={ObserveBuild=function()return {build="70000",product=client}end,
    ObserveActor=function()return {guid="Player-1",character="Tester",realm="Realm"}end,
    ObserveInputState=function()return true end}
ns.Compat.GetAddOnMetadata=function(_,key)return key=="Version" and "test" or nil end
Env.LoadAddon("Modules/AutomationHistory.lua",ns)
for _,name in ipairs({"CaptureWriter","MemoryProtocol","SlotProtocol","ProbeExecution","ReceiverBindings","InputSignal","InputState","SlotRuntime"})do Env.LoadAddon("Bridge/"..name..".lua",ns)end
Env.LoadAddon("Core/Controls.lua",ns)
ns.InputProtection={IsActive=function()return active end}
local function repeatRegistration(expected,reason)
    local frames,events,scripts,overrides=frameCount,eventCalls,scriptCalls,overrideCalls
    local ok,failure=ns.SlotRuntime.Register()
    assert(ok==expected and failure==reason,"repeated registration concealed ineffective bindings")
    assert(frameCount==frames and eventCalls==events and scriptCalls==scripts and overrideCalls==overrides,
        "repeated registration created frames, events, scripts or changed overrides")
end
local function preserved()
    assert(state.options.receiverBindings==saved and saved.wake=="ALT-CTRL-F1"
        and saved.submit=="ALT-CTRL-F2" and saved.close=="ALT-CTRL-F3",
        "native reset changed saved legacy bindings")
end
local function resetRejected(reason)
    local calls=overrideCalls
    local ok,failure=ns.Controls.Handle("receiver reset")
    assert(ok==nil and failure==reason,"native reset bypassed guard")
    assert(overrideCalls==calls,"rejected reset changed overrides")
    preserved()
end
local function blocked()
    local packet=assert(ns.InputState.Snapshot())
    assert(packet:find('"inputBlocked":true',1,true) and packet:find('"reason":"input_binding_unavailable"',1,true),"invalid native binding advertised ready")
    assert(packet:find('"schema":"lycheedev.input.v3"',1,true)
        and packet:find('"bindings":{"close":"","submit":"","wake":""}',1,true),"unavailable binding profile not explicit")
    local cells=ns.InputSignal.Frame().children
    assert(cells[1].colorTexture[1]==1 and cells[1].colorTexture[2]==1 and cells[1].colorTexture[3]==1)
    assert(cells[2].colorTexture[1]==0 and cells[2].colorTexture[2]==0 and cells[2].colorTexture[3]==0)
end
local function ready()assert(ns.InputState.Snapshot():find('"inputBlocked":false',1,true))end
assert(ns.SlotRuntime.Start());blocked() -- No registration yet.
base["ALT-CTRL-F12"]="TOGGLELYCHEE"
base["ALT-CTRL-F11"]="PLAYER_FALLBACK_ACTION"
assert(not ns.SlotRuntime.Register());ns.InputState.Refresh();blocked()
local expectedWarning=string.format(ns.L.NATIVE_RECEIVER_BINDING_BLOCKED,
    string.format(ns.L.NATIVE_RECEIVER_BINDING_CONFLICT,ns.L.NATIVE_RECEIVER_BINDING_WAKE,"Ctrl+Alt+F12"),"TOGGLELYCHEE",
    string.format(ns.L.NATIVE_RECEIVER_BINDING_CONFLICT,ns.L.NATIVE_RECEIVER_BINDING_WAKE,"Ctrl+Alt+F11"),"PLAYER_FALLBACK_ACTION")
assert(#notices==1 and notices[1]=="|cffd83b4eLychee Dev:|r "..expectedWarning,"bootstrap warning not localized or missing actual binding")
assert(base["ALT-CTRL-F12"]=="TOGGLELYCHEE","sampler changed player binding")
repeatRegistration(nil,"receiver_bindings_unavailable")
resetRejected("receiver_binding_conflict:wake:ALT-CTRL-F11")
assert(base["ALT-CTRL-F12"]=="TOGGLELYCHEE" and next(override)==nil,"reset stole player binding")
-- External conflict resolution and explicit registration reset, not sampler repair.
base["ALT-CTRL-F12"]=nil
base["ALT-CTRL-F11"]=nil
combat=true;resetRejected("receiver_combat");combat=false
active=true;resetRejected("receiver_active");active=false
assert(ns.Controls.Handle("receiver reset")=="receiver bindings reset");ready();preserved()
local profile=assert(ns.ReceiverBindings.Current())
assert(profile.wake=="ALT-CTRL-F12" and profile.submit=="ALT-CTRL-SHIFT-F12" and profile.close=="ALT-CTRL-[")
repeatRegistration(true,nil)
combat=true;resetRejected("receiver_combat");combat=false
active=true;resetRejected("receiver_active");active=false
assert(not ns.Controls.Handle("receiver bind wake ALT-CTRL-F1"),"native reset enabled configurable keys")
foreign["ALT-CTRL-F12"]="TOGGLELYCHEE"
foreign["ALT-CTRL-F11"]="PLAYER_FALLBACK_ACTION"
ns.InputState.Refresh();blocked()
repeatRegistration(nil,"receiver_binding_ineffective:wake:ALT-CTRL-F12")
resetRejected("receiver_binding_conflict:wake:ALT-CTRL-F11")
assert(foreign["ALT-CTRL-F12"]=="TOGGLELYCHEE","reset stole foreign override")
foreign["ALT-CTRL-F11"]=nil
assert(ns.Controls.Handle("receiver reset")=="receiver bindings reset");ready();preserved()
assert(ns.ReceiverBindings.Current().wake=="ALT-CTRL-F11" and foreign["ALT-CTRL-F12"]=="TOGGLELYCHEE",
    "fallback reset changed foreign primary binding")
assert(ns.InputState.Snapshot():find('"bindings":{"close":"ALT-CTRL-[","submit":"ALT-CTRL-SHIFT-F11","wake":"ALT-CTRL-F11"}',1,true),
    "effective fallback missing from INPUT")
foreign["ALT-CTRL-F12"]=nil
assert(ns.Controls.Handle("receiver reset")=="receiver bindings reset");ready();preserved()
local current=ns.ReceiverBindings.Current
ns.ReceiverBindings.Current=function()return {wake="ALT-CTRL-F11",submit="ALT-CTRL-SHIFT-F12",close="ALT-CTRL-["}end
ns.InputState.Refresh();blocked()
ns.ReceiverBindings.Current=function()error("binding lookup failed")end
ns.InputState.Refresh();blocked()
ns.ReceiverBindings.Current=current
ns.InputState.Refresh();ready()
ns.InputState.Stop()
state.options.bridgeEnabled=false
local sampler=ns.InputSignal.Frame()
local frames,events=frameCount,eventCalls
assert(ns.Controls.Handle("receiver reset")=="receiver bindings reset")
assert(ns.InputState.Snapshot()==nil and not sampler:GetScript("OnUpdate")
    and frameCount==frames and eventCalls==events,"disabled reset started recurring input work")
preserved()
state.options.bridgeEnabled=true
assert(ns.SlotRuntime.Start())
ns.SlotRuntime=nil;ns.ReceiverBindings.Current=function()return nil end
ns.InputState.Refresh();blocked() -- v3 never advertises readiness without keys.
ns.InputState.Stop()
assert(#notices==1,"native recovery repeated the bootstrap warning")
print=originalPrint
print("native binding gate: bounded profiles, takeover, recovery and disabled isolation")
