local _, ns = ...

-- Read-only projection of the current mailbox request. It retains no source,
-- compiled closure, result body or historical queue, and cannot dispatch work.
local current, mailbox, hiddenReleased, changeHandler
local phases={ready_unbound="acknowledged",validating="queued",running="running",result_pending="reported",closing="finalizing",closed="acknowledged",quarantined="interrupted",execution_unknown="interrupted"}
local function idOK(id)
    return not (issecretvalue and issecretvalue(id)) and type(id)=="string" and #id==32 and not id:find("[^0-9a-f]")
end
local function collect()
    if not ns.DuplexRuntime then current=nil;mailbox={state="disconnected"};return 0 end
    local ok,snapshot=pcall(ns.DuplexRuntime.Snapshot)
    local state=ok and snapshot and snapshot.protocol
    if not state then current=nil;mailbox={state="disconnected"};return 0 end
    local active=snapshot.enabled and not state.disabled
    local mailboxState="unavailable"
    if not active then mailboxState="disconnected"
    elseif state.quarantined then mailboxState="unavailable"
    elseif state.ready then mailboxState="ready"
    elseif state.phase=="validating" or state.phase=="running" or state.phase=="result_pending" or state.phase=="closing" then mailboxState="busy" end
    mailbox={state=mailboxState,phase=state.phase,actorReady=state.actorReady==true,
        transportReady=state.transportReady==true,controlReady=state.controlReady==true}
    local request=state.validation or state.request or state.released
    local id=request and request.requestId
    if not idOK(id) or id==hiddenReleased and not state.request then current=nil;return 0 end
    local terminal
    if not state.validation and state.request then terminal=state.terminal end
    if terminal and terminal.requestId and terminal.requestId~=id then terminal=nil end
    current={requestId=id,transport="mailbox-v1",kind="lua",status=phases[state.phase] or "unavailable",
        codeBytes=request.totalBytes,codeSHA256=request.requestSHA256,sequence=request.requestSeq,
        pending=state.validation~=nil or state.request~=nil,observedAt=current and current.requestId==id and current.observedAt or (time and time()) or 0,
        errorCode=state.lastFailure or terminal and terminal.failureCode,
        probeStatus=terminal and (terminal.outcome=="success" and "completed" or terminal.outcome),
        terminal=terminal}
    if state.phase=="result_pending" and not terminal then current.status="unavailable" end
    if not state.request and not state.validation then current.status="acknowledged" end
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
    GetMailboxStatus=function()return mailbox or {state="disconnected"}end,
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
