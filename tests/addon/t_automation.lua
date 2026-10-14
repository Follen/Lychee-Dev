local Env=...
local ns=Env.LoadWorkbench()
local root={}
ns.Persistence={Bridge=function()return root end}
Env.LoadAddon("Modules/AutomationHistory.lua",ns)
Env.LoadAddon("Modules/AutomationView.lua",ns)
Env.LoadAddon("UI/Pages/Automation.lua",ns)
local view=ns.AutomationView
assert(view.Execute==nil and view.ShowNotice==nil and view.HideNotice==nil,"retired execution API remains")
local history=ns.AutomationHistory
history.Begin(string.rep("a",32),{character="角色",realm="服务器",guid="Player-1",build="5.5.4",product="classic"})
local function op(i,state) return {state=state,payload=state=="reported" and "report" or nil,ok=true,envelope={ticket=string.format("%032x",i),nonce=string.rep("b",32),codeBytes=9,codeChecksum=42}} end
history.Observe(op(1,"released"));history.Observe(op(2,"reported"))
view.Collect()
assert(view.GetOrder()[1]=="MEM-"..string.format("%032x",2))
local page=ns.CreateAutomationPage(UIParent)
assert(page.executeButton==nil and page.showNoticeButton==nil and page.hideNoticeButton==nil)
assert(view.ClearRecords()==1 and view.GetCount()==1,"clear discarded pending report")
local pending=view.GetRecord(view.GetOrder()[1]);pending.pending=false;pending.hasReport=false;pending.protected=true
assert(view.ClearRecords()==0,"clear discarded protected record")
assert(view.GetReportText(pending)=="report")
return true
