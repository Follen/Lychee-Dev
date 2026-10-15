local Env=...
local ns=Env.LoadWorkbench()
local root={}
ns.Persistence={Bridge=function()return root end}
local function load()
    Env.LoadAddon("Modules/AutomationHistory.lua",ns)
    Env.LoadAddon("Modules/AutomationView.lua",ns)
end
load()
local history,view=ns.AutomationHistory,ns.AutomationView
local actor={character="角色",realm="服务器",guid="Player-1",build="5.5.4",product="classic"}
local run=string.rep("a",32)
local frames=Env.framesCreated
history.Begin(run,actor)
assert(root.automationHistory==nil and #history.List()==0)
assert(Env.framesCreated==frames,"history created UI while closed")
local function operation(id,state,payload,ok)
    return {state=state or "prepared",ok=ok,payload=payload,envelope={ticket=string.format("%032x",id),
        nonce=string.rep("b",32),codeBytes=9,codeChecksum=42}}
end
local first=operation(1)
assert(history.Observe(first))
view.Collect()
local id="MEM-"..first.envelope.ticket
assert(view.GetCount()==1 and view.GetRecord(id).status=="loaded")
assert(select(2,view.Execute(id))=="auto_history_read_only")
assert(view.ClearRecords()==0,"prepared history was cleared")
first.state="reported";first.ok=true;first.payload='{"ok":true,"result":"测试结果"}'
assert(history.Observe(first));view.Collect()
assert(view.RefreshRecord(id).status=="reported")
assert(view.GetReportText(id)==first.payload)
assert(view.ClearRecords()==0,"unacknowledged history was cleared")
local body=first.payload
first.state="released";first.payload=nil
assert(history.Observe(first));view.Collect()
assert(view.GetRecord(id).status=="acknowledged" and view.GetReportText(id)==body)
assert(select(2,view.ShowNotice(id))=="auto_notice_unavailable")
local unfinished=operation(2,"running")
assert(history.Observe(unfinished))
local pendingReport=operation(3,"reported",'{"ok":false}',false)
assert(history.Observe(pendingReport))
-- Simulate reload: fresh module locals, same SavedVariables, new runtime.
load();history,view=ns.AutomationHistory,ns.AutomationView
history.Begin(string.rep("c",32),actor);view.Collect()
assert(view.GetCount()==3 and view.GetReportText(id)==body)
assert(view.GetRecord(id).status=="acknowledged")
assert(view.GetRecord("MEM-"..unfinished.envelope.ticket).status=="interrupted")
local reported=view.GetRecord("MEM-"..pendingReport.envelope.ticket)
assert(reported.status=="reported" and not reported.pending and reported.probeStatus=="failed")
assert(view.ClearRecords()==3)
load();ns.AutomationView.Collect()
assert(ns.AutomationView.GetCount()==0,"cleared records returned after reload")
history,view=ns.AutomationHistory,ns.AutomationView
history.Begin(run,actor)
-- Fixed count and per-report bounds. A pending record cannot be evicted.
local retained=operation(4,"running");assert(history.Observe(retained))
for i=5,110 do
    local op=operation(i,"reported",string.rep("x",history.REPORT_BYTES+1),false)
    assert(history.Observe(op));op.state="released";op.payload=nil;assert(history.Observe(op))
end
assert(#history.List()==history.MAX_RECORDS)
view.Collect()
assert(view.GetRecord("MEM-"..retained.envelope.ticket))
local last=view.GetRecord("MEM-"..string.format("%032x",110))
assert(last.reportTruncated and #last.reportBody==history.REPORT_BYTES)
assert(view.GetReportText(last):find(".lycheedev/live/",1,true))
local unicode=operation(111,"reported",string.rep("x",history.REPORT_BYTES-1).."中",true)
assert(history.Observe(unicode));view.Collect()
local clipped=view.GetRecord("MEM-"..unicode.envelope.ticket)
assert(clipped.reportBody==string.rep("x",history.REPORT_BYTES-1) and clipped.reportTruncated,
    "history persisted half of a UTF-8 character")
-- A different per-character root never inherits the previous display records.
local saved=root
root={};view.Collect();assert(view.GetCount()==0)
root={automationHistory={schema=999,records={}}}
assert(select(2,history.Observe(operation(120)))=="automation_history_invalid")
assert(root.automationHistory.schema==999,"future history overwritten")
root={automationHistory={schema=1,records={operation(1)}}}
assert(select(2,history.List())=="automation_history_invalid")
root=saved
assert(Env.framesCreated==frames)
print("native automation persistence, interruption, retention and isolation ok")
