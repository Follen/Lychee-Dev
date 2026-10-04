local Env = ...
local ns = Env.LoadWorkbench()
local displayed
ns.ReceiptView={AnchorCompanion=function() error("activity depended on a receipt") end}
local state={options={}}
ns.Persistence.Current=function() return state end
Env.LoadAddon("Bridge/ActivityView.lua",ns)
local view=ns.ActivityView
assert(Env.framesCreated==0 and view.Current()==nil)
for i=1,100 do view.Receiving(true);view.Begin("staged");view.Collecting("staged");view.Receiving(false) end
assert(Env.framesCreated==0 and view.Current()==nil,"transport staging animated the probe badge")
local originalCreate=CreateFrame
CreateFrame=function(...) local f=originalCreate(...);displayed=displayed or f;return f end
local id=string.rep("a",32)
view.RunStarted(id)
CreateFrame=originalCreate
assert(view.Current()=="probe" and displayed:IsShown())
assert(displayed.label:GetText()==ns.L.ACTIVITY_PROBE)
assert(displayed.mouseEnabled==false and not displayed:GetScript("OnKeyDown"))
assert(displayed:GetScript("OnUpdate"),"active probe has no animation")
view.RunFinished(string.rep("b",32))
assert(view.Current()=="probe","another request ended this badge")
state.options.reducedMotion=true;view.Refresh()
assert(not displayed:GetScript("OnUpdate") and displayed:IsShown())
state.options.reducedMotion=false;view.Refresh()
assert(displayed:GetScript("OnUpdate"))
view.RunFinished(id)
assert(view.Current()==nil and not displayed:IsShown() and not displayed:GetScript("OnUpdate"))
view.RunStarted(id);view.Stop()
assert(view.Current()==nil and not displayed:IsShown())
local before=Env.framesCreated
CreateFrame=function() error("injected cosmetic failure") end
Env.LoadAddon("Bridge/ActivityView.lua",ns)
assert(pcall(ns.ActivityView.RunStarted,id))
CreateFrame=originalCreate
ns.ActivityView.RunStarted(id)
assert(Env.framesCreated==before,"broken renderer repeatedly allocated frames")
print("activity shows only executing probes and releases its animation")
