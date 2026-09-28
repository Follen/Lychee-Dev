local Env = ...
local ns = Env.LoadWorkbench()
local displayed
ns.ReceiptView={AnchorCompanion=function() error("activity must not depend on receipts") end}
local state={options={}}
ns.Persistence.Current=function() return state end
Env.LoadAddon("Bridge/ActivityView.lua",ns)
local view=ns.ActivityView
assert(Env.framesCreated==0 and view.Current()==nil,"disabled activity allocated UI")
local originalCreate=CreateFrame
CreateFrame=function(...) local f=originalCreate(...);displayed=displayed or f;return f end
view.Receiving(true)
CreateFrame=originalCreate
assert(view.Current()=="connecting" and displayed:IsShown())
assert(displayed.label:GetText()==ns.L.ACTIVITY_CONNECTING)
assert(displayed.mouseEnabled==false and not displayed:GetScript("OnKeyDown"),"activity took input authority")
assert(displayed:GetScript("OnUpdate"),"connection has no bounce")
view.Begin("request-one")
view.Receiving(false)
assert(view.Current()=="probe" and displayed:IsShown(),"input release removed running feedback")
assert(displayed.label:GetText()==ns.L.ACTIVITY_PROBE)
local allocated=Env.framesCreated
for i=1,100 do
    view.Receiving(true);view.Receiving(false)
    view.Finish("another-request")
end
assert(view.Current()=="probe" and Env.framesCreated==allocated,"unrelated input ended or reallocated activity")
state.options.reducedMotion=true;view.Refresh()
assert(not displayed:GetScript("OnUpdate") and displayed:IsShown(),"reduced motion hides status or leaves animation")
state.options.reducedMotion=false;view.Refresh()
assert(displayed:GetScript("OnUpdate"))
view.Finish("request-one")
assert(view.Current()==nil and not displayed:IsShown() and not displayed:GetScript("OnUpdate"),"finish left feedback running")
view.Begin("next");view.Stop()
assert(view.Current()==nil and not displayed:IsShown(),"disconnect retained activity")

-- A broken cosmetic API must not throw into an already consumed probe.
local before=Env.framesCreated
local oldCreate=CreateFrame
CreateFrame=function() error("injected UI failure") end
Env.LoadAddon("Bridge/ActivityView.lua",ns)
assert(pcall(ns.ActivityView.Begin,"failed-view"))
CreateFrame=oldCreate
ns.ActivityView.Begin("retry-view")
assert(Env.framesCreated==before,"failed renderer repeatedly allocated UI")
print("activity lifecycle ok")
