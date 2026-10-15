local Env=...
local ns=Env.LoadWorkbench()
Env.LoadAddon("Bridge/CaptureWriter.lua",ns)
Env.LoadAddon("Bridge/MemoryProtocol.lua",ns)
Env.LoadAddon("Bridge/InputSignal.lua",ns)
IsPlayerInWorld=function()return true end
local now,ready,reason=100,true,nil
GetTime=function()return now end
ns.Platform={ObserveInputState=function()return ready,reason end}
ns.ReceiverBindings={Current=function()return {wake="ALT-CTRL-F12",submit="ALT-CTRL-SHIFT-F12",close="ALT-CTRL-["}end}
local created={}
local create=CreateFrame
CreateFrame=function(... )local f=create(...);created[#created+1]=f;return f end
Env.LoadAddon("Bridge/InputState.lua",ns)
assert(#created==0,"loading telemetry created idle work")
local provider=function()return {runtime=string.rep("1",32),owner="",fence=0,nextSlot=1,guid="g",build="b"}end
local mailbox={}
ns.InputState.Start(provider,mailbox)
assert(#created==1 and not created[1]:GetScript("OnKeyDown"))
assert(ns.InputState.Snapshot():find('"inputBlocked":false',1,true))
assert(mailbox.input==ns.InputState.Snapshot() and mailbox.input:find('"schema":"lycheedev.input.v3"',1,true))
ready,reason=false,"input_keyboard_focus"
now=100.2;created[1]:GetScript("OnUpdate")(created[1],.2)
assert(ns.InputState.Snapshot():find('"inputBlocked":false',1,true),"sampler ran before one second")
ns.InputState.Refresh()
assert(ns.InputState.Snapshot():find('"inputBlocked":true',1,true))
assert(ns.InputState.Snapshot():find('"reason":"input_keyboard_focus"',1,true))
ready,reason=true,nil
now=101;created[1]:GetScript("OnUpdate")(created[1],.8)
assert(ns.InputState.Snapshot():find('"inputBlocked":false',1,true),"refresh reset periodic cadence")
ns.InputState.Start(provider,mailbox);assert(#created==1,"repeated starts allocated a frame")
local prior=ns.InputState.Snapshot()
now=101.5;created[1]:GetScript("OnUpdate")(created[1],.5)
assert(ns.InputState.Snapshot()==prior)
now=nil;ns.InputState.Refresh();assert(not ns.InputState.Snapshot(),"invalid clock retained authority")
assert(mailbox.input==nil,"invalid clock retained public input")
now=100;ns.InputState.Refresh();assert(not ns.InputState.Snapshot(),"clock rollback retained authority")
now={};Env.secrets[now]=true
ns.InputState.Refresh();assert(not ns.InputState.Snapshot(),"secret clock retained authority")
now=102;ready,reason=nil,"input_focus_unavailable";ns.InputState.Refresh()
assert(ns.InputState.Snapshot():find('"inputBlocked":true',1,true))
local observe=ns.Platform.ObserveInputState
ns.Platform.ObserveInputState=function()error("restricted API")end
ns.InputState.Refresh();assert(not ns.InputState.Snapshot(),"failed observation retained authority")
assert(mailbox.input==nil,"failed observation retained public input")
ns.Platform.ObserveInputState=observe
ready,reason=true,nil
for i=1,3 do
    local before=ns.InputState.Snapshot()
    now=103+i;created[1]:GetScript("OnUpdate")(created[1],1)
    assert(ns.InputState.Snapshot() and ns.InputState.Snapshot()~=before,"enabled idle sampling stopped")
    assert(mailbox.input==ns.InputState.Snapshot())
end
ns.InputState.Stop();assert(not ns.InputState.Snapshot() and not created[1]:GetScript("OnUpdate"))
assert(mailbox.input==nil)
ns.InputState.Refresh();assert(not ns.InputState.Snapshot() and not created[1]:GetScript("OnUpdate"),"refresh revived stopped sampler")
ns.InputState.Start(provider,mailbox);assert(#created==1 and mailbox.input==ns.InputState.Snapshot())
ns.InputState.Stop()
-- Reload creates a separate runtime with no inherited record or callback.
Env.LoadAddon("Bridge/InputState.lua",ns)
assert(not ns.InputState.Snapshot() and #created==1)
ns.InputState.Refresh();assert(#created==1 and not ns.InputState.Snapshot())
Env.LoadAddon("Bridge/InputSignal.lua",ns)
local oldMailbox=mailbox;mailbox={}
ns.InputState.Start(provider,mailbox);assert(#created==2 and mailbox.input==ns.InputState.Snapshot() and oldMailbox.input==nil)
ns.InputState.Stop();assert(not created[2]:GetScript("OnUpdate"))
print("input telemetry: bounded sampler and explicit-off teardown")
