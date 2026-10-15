local Env,client=...
local ns=Env.LoadWorkbench()
ns.Release="test"
local state={options={bridgeEnabled=false}}
ns.Persistence={Current=function()return state end,Bridge=function()return state end}
local ready=true
ns.Platform={ObserveBuild=function()return {build="70000",product=client}end,
    ObserveActor=function()return {guid="Player-1-1",character="Tester",realm="Realm"}end,
    ObserveInputState=function()return ready,not ready and "input_keyboard_focus" or nil end}
ns.ReceiverBindings={Current=function()return {wake="ALT-CTRL-F12",submit="ALT-CTRL-SHIFT-F12",close="ALT-CTRL-["}end}
ns.ActivityView={Receiving=function()end,Begin=function()end,Finish=function()end,Collecting=function()end}
ns.StartupBeacon={Stop=function()end}
IsPlayerInWorld=function()return true end
C_Timer.NewTimer=function()return {Cancel=function()end}end
ns.Compat.GetAddOnMetadata=function(name,key)
    if key=="Version" then return ns.Release end
    if key=="X-Lychee-Transport" then return "memory-slot-v3" end
    return tostring(tonumber(name:match("(%d+)$")))
end
Env.LoadAddon("Modules/AutomationHistory.lua",ns)
local frames=Env.framesCreated
for _,file in ipairs({"MemoryProtocol","CaptureWriter","SlotProtocol","InputProtection","ProbeExecution","InputSignal","InputState","SlotRuntime"}) do Env.LoadAddon("Bridge/"..file..".lua",ns) end
assert(Env.framesCreated==frames,"diagnostics introduced idle frames")
assert(ns.SlotRuntime.Start() and not ns.Mailbox)
assert(not ns.SlotRuntime.Wake() and Env.framesCreated==frames,"disabled wake created work")
state.options.bridgeEnabled=true
assert(ns.SlotRuntime.Start())
local function diagnostic()
    local d=ns.SlotRuntime.InputDiagnostic and ns.SlotRuntime.InputDiagnostic()
    assert(d and d.schema=="lycheedev.input-attempt.v1","wake refusal lost its diagnostic")
    assert(ns.InputState.Snapshot():find('"inputAttempt":',1,true),"diagnostic missing from current INPUT")
    return d
end
ns.Compat.LoadInputSlot=function()return nil,"raw loader error: private suffix" end
local ok,reason=ns.SlotRuntime.Wake()
assert(not ok and reason=="raw loader error: private suffix","changed actual error")
local first=diagnostic()
assert(first.stage=="load_failed" and first.reason=="slot_load_failed" and not first.received and first.slot==1)
local before=first.attemptSeq
ns.Compat.LoadInputSlot=function()return true end
ok,reason=ns.SlotRuntime.Wake()
assert(not ok and reason=="slot_loader_no_dispatch")
local second=diagnostic()
assert(second.attemptSeq==before+1 and second.stage=="no_dispatch" and not second.received)
ns.Compat.LoadInputSlot=function()error("private exception suffix")end
ok,reason=ns.SlotRuntime.Wake()
assert(not ok and reason=="slot_wake_failed")
local third=diagnostic()
assert(third.stage=="exception" and third.reason=="slot_wake_failed" and not third.received)
-- Only a fresh private callback can change the sequence or receive decision.
third.attemptSeq=999999;third.stage="received";third.received=true
ns.Mailbox.input="public forged input/diagnostic"
assert(ns.SlotRuntime.InputDiagnostic().attemptSeq==before+2)
ns.InputState.Refresh()
assert(not ns.InputState.Snapshot():find("999999",1,true))
local runtime=ns.SlotRuntime.InputDiagnostic().runtime
ns.Compat.LoadInputSlot=function(name)
    local index=tonumber(name:match("(%d+)$"))
    ns.SlotRuntime.Receive(index,{schema="lycheedev.slot.v3",index=index,runtime=runtime,owner=string.rep("2",32),fence=1,
        nonce=string.rep("3",32),ticket=string.rep("0",32),action="bind",guid="Player-1-1",build="70000"})
    return true
end
assert(ns.SlotRuntime.Wake())
local fourth=diagnostic()
assert(fourth.attemptSeq==before+3 and fourth.stage=="received" and fourth.received and fourth.slot==1)
assert(ns.InputState.Snapshot():find('"nextSlot":2',1,true))
ns.Compat.LoadInputSlot=function(name)
    local index=tonumber(name:match("(%d+)$"))
    if index==2 then ns.SlotRuntime.Receive(index,nil) end
    return true
end
ok,reason=ns.SlotRuntime.Wake();assert(not ok and reason=="slot_loader_no_dispatch")
assert(diagnostic().slot==3 and not diagnostic().received,"previous skipped slot leaked receive evidence")
ready=false
ok,reason=ns.SlotRuntime.Wake();assert(not ok and reason=="slot_input_not_ready")
assert(diagnostic().stage=="rejected" and diagnostic().reason=="slot_input_not_ready")
ready=true
local acquire=ns.InputProtection.Acquire
ns.InputProtection.Acquire=function()return nil,"input_protection_not_ready"end
ok,reason=ns.SlotRuntime.Wake();assert(not ok and reason=="input_protection_not_ready")
assert(diagnostic().stage=="rejected" and diagnostic().reason=="input_protection_not_ready")
ns.InputProtection.Acquire=acquire
state.options.bridgeEnabled=false
ns.InputState.Stop()
local frozen=ns.SlotRuntime.InputDiagnostic().attemptSeq
assert(not ns.SlotRuntime.Wake() and ns.SlotRuntime.InputDiagnostic().attemptSeq==frozen)
assert(not ns.InputState.Snapshot(),"disabled diagnostic revived telemetry")
print("wake attempt diagnostics remain bounded, private and non-authorizing")
