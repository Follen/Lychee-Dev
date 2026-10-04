local _, ns = ...

-- Read-only projection of the one current duplex request. It retains no source,
-- compiled closure, result body or historical queue, and cannot dispatch work.
local current, hiddenReleased, changeHandler
local phases={receiving="queued",prepared="loaded",running="running",settling="finalizing",terminal="reported",released="acknowledged"}
local function idOK(id)
    return not (issecretvalue and issecretvalue(id)) and type(id)=="string" and #id==32 and not id:find("[^0-9a-f]")
end
local function collect()
    if not ns.DuplexRuntime then current=nil;return 0 end
    local ok,snapshot=pcall(ns.DuplexRuntime.Snapshot)
    local state=ok and snapshot and snapshot.protocol
    if not state then current=nil;return 0 end
    local request=state.request or state.released
    local id=request and request.requestId
    if not idOK(id) or id==hiddenReleased and not state.request then current=nil;return 0 end
    local terminal=state.terminal
    current={requestId=id,transport="memory-duplex",kind="lua",status=phases[state.phase] or "unavailable",
        codeBytes=request.totalBytes,codeSHA256=request.requestSHA256,sequence=request.requestSeq,
        pending=state.request~=nil,observedAt=current and current.requestId==id and current.observedAt or (time and time()) or 0,
        errorCode=state.lastFailure,probeStatus=terminal and (terminal.outcome=="success" and "completed" or terminal.outcome),
        terminal=terminal}
    if state.phase=="terminal" and not terminal then current.status="unavailable" end
    if not state.request then current.status="acknowledged" end
    return 1
end
local function get(target)
    local id=type(target)=="table" and target.requestId or target
    return current and current.requestId==id and current or nil
end
ns.AutomationView={
    SetChangeHandler=function(handler)changeHandler=handler end,
    Changed=function()if changeHandler then pcall(changeHandler)end end,
    MAX_RECORDS=1,REPORT_DISPLAY_BYTES=49152,Collect=collect,
    GetOrder=function()return current and {current.requestId} or {}end,
    GetRecord=get,GetCount=function()return current and 1 or 0 end,
    RefreshRecord=function(target)collect();return get(target)end,
    DeriveStatus=function(id)local record=get(id);return record and record.status or "unavailable" end,
    GetReportText=function(target)
        local record=get(target)
        if not record then return nil,"auto_execution_unknown" end
        if not record.terminal then return nil,ns.L.AUTO_NO_REPORT end
        return ns.CaptureWriter.Encode(record.terminal,49152)
    end,
    ClearRecords=function()
        if not current or current.pending then return 0 end
        hiddenReleased=current.requestId;current=nil;return 1
    end,
}
