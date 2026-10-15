-- Full page fixture: capture remains independent of hidden UI refresh scheduling.
local Env,client=...
local ns=Env.LoadWorkbench()
assert(ns.Persistence.Load())
local catalogs={retail="Mainline",classic="Mists",titan="Titan",forever="Forever"}
Env.LoadAddon("Modules/Events/CatalogData_"..catalogs[client]..".lua",ns)
Env.LoadAddon("Modules/Events/Catalog.lua",ns)
local observedRecords=0
ns.EventMonitor={GetCount=function()return 0 end,IsRunning=function()return false end,Stop=function()end,Clear=function()end}
Env.LoadAddon("UI/Pages/Events.lua",ns)
assert(Env.framesCreated==0,"optional page registration allocated frames")
assert(ns.Workbench.Toggle())
local page=ns.CreateEventsPage(UIParent)
page:Hide();Env.pendingTimers={}
for i=1,100 do
    observedRecords=observedRecords+1 -- monitor records independently before its notification
    page.QueueRefresh()
    local pending=Env.pendingTimers;Env.pendingTimers={}
    for _,callback in ipairs(pending)do callback()end
end
local hiddenTimers=0
local oldAfter=C_Timer.After
C_Timer.After=function(seconds,callback)hiddenTimers=hiddenTimers+1;oldAfter(seconds,callback)end
for i=1,100 do
    observedRecords=observedRecords+1
    page.QueueRefresh()
    local pending=Env.pendingTimers;Env.pendingTimers={}
    for _,callback in ipairs(pending)do callback()end
end
local expected=tonumber(os.getenv("LYCHEEDEV_EXPECT_HIDDEN_REFRESH_TIMERS") or "0")
assert(hiddenTimers==expected,"hidden scheduling count="..hiddenTimers.." expected="..expected)
assert(observedRecords==200,"refresh optimization dropped monitor records")
C_Timer.After=oldAfter
local refreshes=0
local actualRefresh=page.Refresh
page.Refresh=function(self)refreshes=refreshes+1;return actualRefresh(self)end
page:Show();page:RefreshIfDirty()
assert(refreshes==1,"activation failed to refresh dirty hidden data")
Env.pendingTimers={}
for i=1,100 do page.QueueRefresh()end
assert(#Env.pendingTimers==1,"visible event burst did not coalesce")
local pending=Env.pendingTimers;Env.pendingTimers={};pending[1]()
assert(refreshes==2,"visible flush did not render")
print("refresh-cost: hiddenTimers="..hiddenTimers.." visibleBurstTimers=1 records="..observedRecords.." refreshes="..refreshes)
local traceDefinition
local register=ns.Workbench.RegisterPage
ns.Workbench.RegisterPage=function(value)traceDefinition=value;return true end
local traceChanged
local tracing=false
ns.FunctionTrace={GetCount=function()return 0 end,IsRunning=function()return tracing end,Stop=function()tracing=false end,Clear=function()end,
    Start=function(_,callback)traceChanged=callback;tracing=true;return true end,GetActivePath=function()return "fixture"end}
Env.LoadAddon("UI/Pages/Trace.lua",ns)
ns.Workbench.RegisterPage=register
local trace=traceDefinition.build(UIParent)
local function findButton(frame)
    if frame.label and frame.label:GetText()==ns.L.START_TRACE then return frame end
    for _,child in ipairs(frame.children or {})do local found=findButton(child);if found then return found end end
end
assert(findButton(trace)):Click()
assert(traceChanged and tracing)
trace:Hide();Env.pendingTimers={};hiddenTimers=0
C_Timer.After=function(seconds,callback)hiddenTimers=hiddenTimers+1;oldAfter(seconds,callback)end
for i=1,100 do traceChanged();local waiting=Env.pendingTimers;Env.pendingTimers={};for _,cb in ipairs(waiting)do cb()end end
assert(hiddenTimers==expected,"hidden trace scheduling count="..hiddenTimers.." expected="..expected)
assert(tracing,"hidden trace notification stopped data capture")
C_Timer.After=oldAfter;trace:Show();trace.HandleActivate();Env.pendingTimers={}
for i=1,100 do traceChanged()end
assert(#Env.pendingTimers==1,"visible trace burst did not coalesce")
print("trace-cost: hiddenTimers="..hiddenTimers.." visibleBurstTimers=1 captureActive=true")
