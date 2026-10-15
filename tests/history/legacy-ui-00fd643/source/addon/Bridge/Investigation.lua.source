local _, ns = ...

-- Small character-owned execution facts. Slots are reserved before execution;
-- only an acknowledged, finished slot may be reclaimed by a later admission.
local LIMIT, SLOT_BYTES = 16, 2048
local function secret(value) return issecretvalue and issecretvalue(value) end
local function plain(value) return not secret(value) and type(value)=="table" and getmetatable(value)==nil end
local function hex(value, size)
    return not secret(value) and type(value)=="string" and #value==size and value:match("^[0-9a-f]+$")~=nil
end
local phases={loaded=true,running=true,reported=true,report_error=true,acknowledged=true,finished=true}
local fields={sessionNonce=true,guid=true,epoch=true,phase=true,codeSHA256=true,receipt=true}
local function valid(item)
    if not plain(item) or not ns.CaptureWriter.Encode(item,SLOT_BYTES) then return false end
    for key in pairs(item) do if not fields[key] then return false end end
    return hex(item.sessionNonce,32) and hex(item.codeSHA256,64) and type(item.guid)=="string"
        and #item.guid>0 and #item.guid<=128 and type(item.epoch)=="number" and item.epoch>=1
        and item.epoch<9007199254740991 and item.epoch%1==0 and phases[item.phase]==true
        and (item.receipt==nil and (item.phase=="loaded" or item.phase=="running")
            or type(item.receipt)=="string" and #item.receipt>0 and #item.receipt<=1400)
end
local function context()
    local state, reason = ns.Persistence.Bridge()
    if not state then return nil, reason end
    local session, failure = ns.Session.Current()
    if not session then return nil, failure end
    if state.terminals == nil then state.terminals = {} end
    if not plain(state.terminals) then return nil, "terminal_invalid_store" end
    local count = 0
    for id, item in pairs(state.terminals) do
        count = count+1
        if count>LIMIT or secret(id) or type(id)~="string" or #id>80 or not id:match("^[%w_%-]+$")
            or not valid(item) then return nil,"terminal_invalid_store" end
    end
    return state.terminals, session
end
local function selected(requestId)
    if secret(requestId) or type(requestId)~="string" then return nil,"terminal_invalid_request" end
    local records, session = context()
    if not records then return nil, session end
    local item = records[requestId]
    if not plain(item) or item.sessionNonce~=session.sessionNonce or item.guid~=session.guid then
        return nil,"terminal_unavailable"
    end
    return item, session, records
end
local function store(records, requestId, item)
    if not valid(item) then return nil,"terminal_slot_limit" end
    records[requestId]=item
    return true
end
local function copy(item)
    local result={}
    for k,v in pairs(item) do result[k]=v end
    return result
end
local function released(requestId, item, session)
    -- A different validated Lua runtime has destroyed the previous managed
    -- timers/frames. Historical cleanup failures remain in the archived report.
    if item.epoch~=session.runtimeEpoch then return true end
    return ns.ProbeRunner.ResourcesReleased(requestId)==true
end
local function proof(requestId, nonce)
    if not hex(nonce,32) then return nil,"terminal_invalid_challenge" end
    local item, session = selected(requestId)
    if not item then return nil, session end
    local identity, reason=ns.Session.NextIdentity()
    if not identity then return nil,reason end
    return ns.CaptureWriter.EncodeSignal({
        schema="lycheedev.signal.v1",kind="checkpoint",release=ns.Release,
        product=ns.Startup.identity.product,build=ns.Startup.identity.build,
        sessionNonce=session.sessionNonce,requestId=requestId,probeNonce=nonce,
        character=session.character,realm=session.realm,guid=session.guid,
        sequence=identity.sequence,runtimeEpoch=session.runtimeEpoch,inputReady=false,
        workState=item.phase,codeSHA256=item.codeSHA256,receipt=item.receipt,
        resourcesReleased=released(requestId,item,session),
    },4096)
end

ns.Investigation = {
    ActivityRequest=function()
        local records,session=context()
        if not records then return nil end
        for id,item in pairs(records) do
            if item.guid==session.guid and item.sessionNonce==session.sessionNonce
                and (item.phase=="running" or item.phase=="reported" or item.phase=="acknowledged") then return id end
        end
    end,
    Tracked=function(requestId)
        local state=ns.Persistence.Bridge()
        return state and plain(state.terminals) and state.terminals[requestId]~=nil or false
    end,
    Reserve=function(requestId,entry)
        local records,session=context()
        if not records then return nil,session end
        local prior=records[requestId]
        if prior~=nil then return nil,"terminal_request_exists" end
        local count,retired=0,nil
        for id,item in pairs(records) do
            count=count+1
            if item.phase=="finished" and (not retired or id<retired) then retired=id end
            if item.guid==session.guid and item.epoch==session.runtimeEpoch and item.phase~="finished" then return nil,"investigation_busy" end
        end
        if count>=LIMIT and not retired then return nil,"terminal_capacity" end
        local item={sessionNonce=session.sessionNonce,guid=session.guid,epoch=session.runtimeEpoch,
            phase="loaded",codeSHA256=entry.codeSHA256}
        -- Leave bounded room for the longest minimal failure receipt.
        if not ns.CaptureWriter.Encode(item,512) then return nil,"terminal_identity_limit" end
        local capacity,reason=ns.ReportStore.Reserve(requestId)
        if not capacity then return nil,reason end
        if count>=LIMIT then records[retired]=nil end
        return store(records,requestId,item)
    end,
    Mark=function(requestId,phase,receipt)
        local item,session,records=selected(requestId)
        if not item then return nil,session end
        local nextItem=copy(item)
        if phase=="running" and item.phase~="loaded" then return nil,"terminal_already_dispatched" end
        if item.phase=="finished" then return nil,"terminal_finished" end
        nextItem.phase,nextItem.receipt=phase,receipt or item.receipt
        if phase=="acknowledged" then nextItem.epoch=session.runtimeEpoch end
        return store(records,requestId,nextItem)
    end,
    Busy=function()
        local state=ns.Persistence.Bridge()
        if not state then return nil,"terminal_state_unavailable" end
        if state.terminals==nil then return false end
        if not plain(state.terminals) then return nil,"terminal_invalid_store" end
        local actor,reason=ns.Platform.ObserveActor()
        if not actor then return nil,reason end
        local count=0
        for _,item in pairs(state.terminals) do
            count=count+1
            if count>LIMIT or not valid(item) then return nil,"terminal_invalid_store" end
            if item.guid==actor.guid and item.epoch==state.runtimeEpoch and item.phase~="finished" then return true end
        end
        return false
    end,
    Observe=proof,
    ReadyReceipt=function(requestId)
        local item,session=selected(requestId)
        if not item then return nil,session end
        if item.phase=="loaded" and item.epoch==session.runtimeEpoch then
            return ns.ProbeRunner.RefreshLoaded(requestId)
        end
        return ns.Session.ReadyReceipt()
    end,
    Finish=function(requestId,nonce)
        local item,session,records=selected(requestId)
        if not item then return nil,session end
        if not hex(nonce,32) then return nil,"terminal_invalid_challenge" end
        if item.phase~="acknowledged" and item.phase~="finished" then return nil,"terminal_ack_required" end
        if not released(requestId,item,session) then return nil,"probe_resources_pending" end
        local nextItem=copy(item); nextItem.phase="finished"
        if not store(records,requestId,nextItem) then return nil,"terminal_slot_limit" end
        if ns.ActivityView then ns.ActivityView.Finish(requestId) end
        return proof(requestId,nonce)
    end,
}
