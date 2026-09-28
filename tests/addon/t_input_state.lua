local Env=...
local ns=Env.LoadWorkbench()
Env.LoadAddon("Bridge/CaptureWriter.lua",ns)
Env.LoadAddon("Bridge/MemoryProtocol.lua",ns)
local now,ready,reason=100,true,nil
ns.Compat.MonotonicSeconds=function()return now end
ns.Platform={ObserveInputState=function()return ready,reason end}
local created={}
local create=CreateFrame
CreateFrame=function(... )local f=create(...);created[#created+1]=f;return f end
Env.LoadAddon("Bridge/InputState.lua",ns)
assert(#created==0,"loading telemetry created idle work")
local provider=function()return {runtime=string.rep("1",32),owner="",fence=0,nextSlot=1,guid="g",build="b"}end
ns.InputState.Start(provider)
assert(#created==1 and not created[1]:GetScript("OnKeyDown"))
assert(ns.InputState.Snapshot():find('"inputBlocked":false',1,true))
ready,reason=false,"input_keyboard_focus"
now=100.2;created[1]:GetScript("OnUpdate")(created[1],.2)
assert(ns.InputState.Snapshot():find('"inputBlocked":true',1,true))
assert(ns.InputState.Snapshot():find('"reason":"input_keyboard_focus"',1,true))
ns.InputState.Start(provider);assert(#created==1,"repeated starts allocated a frame")
ns.InputState.Stop();assert(not ns.InputState.Snapshot() and not created[1]:GetScript("OnUpdate"))
ns.InputState.Start(provider);assert(#created==1 and ns.InputState.Snapshot())
ns.InputState.Stop()
print("input telemetry: bounded sampler and explicit-off teardown")
