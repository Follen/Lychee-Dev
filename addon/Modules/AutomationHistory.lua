local _, ns = ...

-- A bounded, per-character display history, not an execution/recovery journal.
-- SavedVariables flush on reload/logout. The CLI's project journal owns durable
-- evidence and retries; these records must never authorize execution.
local MAX_RECORDS, REPORT_BYTES = 100, 32 * 1024
local runtime, identity
local statuses = { loaded=true, running=true, reported=true, acknowledged=true, interrupted=true }
local phases = { prepared="loaded", running="running", reported="reported", released="acknowledged" }
local fields={requestId=true,runtime=true,nonce=true,observedAt=true,codeBytes=true,codeAdler32=true,
    character=true,realm=true,guid=true,build=true,product=true,status=true,pending=true,hasReport=true,
    reportBody=true,reportTruncated=true,probeStatus=true,errorCode=true}
local function safe(v) return not (issecretvalue and issecretvalue(v)) end
local function plain(v) return safe(v) and type(v)=="table" and getmetatable(v)==nil end
local function text(v,n) return safe(v) and type(v)=="string" and #v<=n end
local function number(v) return safe(v) and type(v)=="number" and v>=0 and v<math.huge and v%1==0 end
local function preview(body)
    local last=math.min(#body,REPORT_BYTES)
    -- CaptureWriter supplies UTF-8. Never persist half of its last codepoint.
    if #body>last then
        local nextByte=body:byte(last+1)
        if nextByte>=128 and nextByte<192 then
            while last>0 and body:byte(last)>=128 and body:byte(last)<192 do last=last-1 end
            last=last-1
        end
    end
    return body:sub(1,last)
end
local function valid(record)
    if not plain(record) or not text(record.requestId,68) or not record.requestId:match("^MEM%-%x+$")
        or not text(record.runtime,32) or not text(record.status,16) or not statuses[record.status]
        or not number(record.observedAt) or not number(record.codeBytes) then return false end
    for key in pairs(record) do if not safe(key) or not fields[key] then return false end end
    for key,limit in pairs({nonce=32,character=256,realm=256,guid=128,build=64,product=32,
        codeAdler32=16,reportBody=REPORT_BYTES,probeStatus=16,errorCode=128}) do
        if record[key]~=nil and not text(record[key],limit) then return false end
    end
    for _,key in ipairs({"pending","hasReport","reportTruncated"}) do
        if not safe(record[key]) or (record[key]~=nil and type(record[key])~="boolean") then return false end
    end
    return true
end
local function store(create)
    local root,reason=ns.Persistence.Bridge()
    if not root then return nil,reason end
    local saved=root.automationHistory
    if saved==nil and create then saved={schema=1,records={}};root.automationHistory=saved end
    if saved==nil then return nil end
    if not plain(saved) or not safe(saved.schema) or saved.schema~=1 or not plain(saved.records) then
        return nil,"automation_history_invalid"
    end
    local count=0
    for key,record in pairs(saved.records) do
        count=count+1
        if count>MAX_RECORDS or not number(key) or key<1 or key>MAX_RECORDS or not valid(record) then
            return nil,"automation_history_invalid"
        end
    end
    if count~=#saved.records then return nil,"automation_history_invalid" end
    return saved
end
local function changed()
    if ns.AutomationView then ns.AutomationView.Changed() end
end
local function begin(current,actor)
    runtime,identity=current,actor
    local saved=store(false)
    if not saved then return end
    for _,record in ipairs(saved.records) do
        if record.runtime~=runtime then
            record.pending=false;record.hasReport=false
            if record.status=="loaded" or record.status=="running" then
                record.status="interrupted";record.errorCode="runtime_changed"
            end
        end
    end
end
local function observe(op)
    if not runtime or not identity then return nil,"automation_history_not_started" end
    local saved,reason=store(true);if not saved then return nil,reason end
    local requestId="MEM-"..op.envelope.ticket
    local record
    for _,item in ipairs(saved.records) do if item.requestId==requestId and item.runtime==runtime then record=item;break end end
    if not record then
        if #saved.records>=MAX_RECORDS then
            local removed=false
            for i,item in ipairs(saved.records) do
                if not item.pending then table.remove(saved.records,i);removed=true;break end
            end
            if not removed then return nil,"automation_history_full" end
        end
        record={requestId=requestId,runtime=runtime,nonce=op.envelope.nonce,
            observedAt=(time and time()) or 0,codeBytes=op.envelope.codeBytes or 0,
            codeAdler32=string.format("%08x",op.envelope.codeChecksum or 0),
            character=identity.character,realm=identity.realm,guid=identity.guid,build=identity.build,product=identity.product}
        saved.records[#saved.records+1]=record
    end
    record.status=phases[op.state]
    record.pending=op.state~="released"
    record.hasReport=op.state=="reported"
    if op.payload then
        record.reportBody=preview(op.payload)
        record.reportTruncated=#op.payload>REPORT_BYTES
        record.probeStatus=op.ok and "completed" or "failed"
    end
    changed()
    return true
end
ns.AutomationHistory={
    MAX_RECORDS=MAX_RECORDS,REPORT_BYTES=REPORT_BYTES,Begin=begin,Observe=observe,
    List=function()local saved,reason=store(false);return saved and saved.records or {},reason end,
    Remove=function(id)
        local saved=store(false);if not saved then return false end
        for i,record in ipairs(saved.records) do
            if record.requestId==id and not record.pending then table.remove(saved.records,i);return true end
        end
        return false
    end,
}
