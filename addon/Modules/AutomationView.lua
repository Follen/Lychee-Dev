local _, ns = ...
-- Read-only workbench projection of the current mailbox history.
local MAX_RECORDS, REPORT_DISPLAY_BYTES = 200, 48*1024
local records, recordIndex, observedSequence = {}, {}, 0
local changeHandler
local function Restricted(v) return issecretvalue and issecretvalue(v) end
local function RequestId(v) return not Restricted(v) and type(v)=="string" and #v>0 and #v<=128 end
local function PruneRecordSlot()
    if #records<MAX_RECORDS then return true end
    for i,r in ipairs(records) do if not r.protected and not r.pending and not r.hasReport then
        recordIndex[r.requestId]=nil;table.remove(records,i);return true
    end end
    return false
end
local KIND_LUA="lua"
local function Observe(incoming)
    if type(incoming) ~= "table" or not RequestId(incoming.requestId) then
        return nil, "auto_invalid_request"
    end
    local requestId = incoming.requestId
    local record = recordIndex[requestId]
    if not record then
        if not PruneRecordSlot() then
            return nil, "auto_record_limit"
        end
        observedSequence = observedSequence + 1
        record = { requestId = requestId, observedSeq = observedSequence }
        records[#records + 1] = record
        recordIndex[requestId] = record
    end
    for key, value in pairs(incoming) do
        if key ~= "requestId" and key ~= "observedSeq" then
            record[key] = value
        end
    end
    if type(record.observedAt) ~= "number" then
        record.observedAt = (time and time()) or 0
    end
    if type(record.kind) ~= "string" then
        record.kind = KIND_LUA
    end
    return record
end


local function Collect()
    local current={}
    for _,stored in ipairs(ns.AutomationHistory.List()) do
        local incoming={transport="memory-slot",kind="lua"}
        for key,value in pairs(stored) do incoming[key]=value end
        current[incoming.requestId]=true
        local old=recordIndex[incoming.requestId]
        -- Clear fields removed by the current persisted record.
        if old then for key in pairs(old) do if key~="requestId" and key~="observedSeq" and key~="protected" then old[key]=nil end end end
        Observe(incoming)
    end
    for i=#records,1,-1 do local r=records[i];if not current[r.requestId] then recordIndex[r.requestId]=nil;table.remove(records,i) end end
    return #records
end
local function GetRecord(target) return type(target)=="table" and target or recordIndex[target] end
local function GetReportText(target)
    local r=GetRecord(target);if not r then return nil,"auto_execution_unknown" end
    local body=r.reportBody
    if Restricted(body) then return "<secret>" end
    if type(body)~="string" then return nil,ns.L.AUTO_NO_REPORT end
    if r.reportTruncated then return body.."\n... "..string.format(ns.L.AUTO_REPORT_HISTORY_LIMIT,ns.AutomationHistory.REPORT_BYTES/1024) end
    if #body>REPORT_DISPLAY_BYTES then return body:sub(1,REPORT_DISPLAY_BYTES).."\n... "..string.format(ns.L.AUTO_REPORT_DISPLAY_LIMIT,REPORT_DISPLAY_BYTES/1024) end
    return body
end
ns.AutomationView={
    MAX_RECORDS=MAX_RECORDS,REPORT_DISPLAY_BYTES=REPORT_DISPLAY_BYTES,
    SetChangeHandler=function(handler)changeHandler=handler end,
    Changed=function()if changeHandler then pcall(changeHandler) end end,
    Observe=Observe,Collect=Collect,GetRecord=GetRecord,GetReportText=GetReportText,
    GetCount=function()return #records end,
    GetOrder=function()local out={};for i=#records,1,-1 do out[#out+1]=records[i].requestId end;return out end,
    RefreshRecord=function(target)local r=GetRecord(target);return r,r==nil and "auto_execution_unknown" or nil end,
    ClearRecords=function()
        local removed=0
        for i=#records,1,-1 do local r=records[i]
            if not r.protected and not r.pending and not r.hasReport then
                ns.AutomationHistory.Remove(r.requestId);recordIndex[r.requestId]=nil;table.remove(records,i);removed=removed+1
            end
        end
        return removed
    end,
}
