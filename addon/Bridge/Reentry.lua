local ADDON_NAME, ns = ...

local waiting, ownedTicket, ownedRoot, ownedBytes, submitted
local function restricted(value) return issecretvalue and issecretvalue(value) end
local function plain(value)
    return not restricted(value) and type(value) == "table" and getmetatable(value) == nil
end
local fields = { schema=true, requestId=true, sessionNonce=true, reloadNonce=true,
    release=true, product=true, build=true, character=true, realm=true, guid=true,
    runtimeEpoch=true, receipt=true, bodyBytes=true, bodyAdler32=true,
    codeSHA256=true, codeAdler32=true, codeBytes=true, cleanupNonce=true, queueReload=true }
local function valid(ticket)
    if not plain(ticket) then return false end
    -- Encoding traverses all values before comparisons, rejecting secrets,
    -- cycles and metatables. Do not interpret unknown/newer ticket formats.
    if not ns.CaptureWriter.Encode(ticket, 8192) or ticket.schema ~= "lycheedev.reentry.v1" then return false end
    for field in pairs(ticket) do if not fields[field] then return false end end
    for _, field in ipairs({"requestId","release","product","build","character","realm","guid"}) do
        local value=ticket[field]
        if type(value)~="string" or #value==0 or #value>128 or string.find(value,"[%z\1-\31\127]") then return false end
    end
    if not string.match(ticket.requestId,"^[%w_%-]+$") then return false end
    for _, field in ipairs({"sessionNonce","reloadNonce"}) do
        local value=ticket[field]
        if type(value)~="string" or #value~=32 or not string.match(value,"^[0-9a-f]+$") then return false end
    end
    if type(ticket.runtimeEpoch)~="number" or ticket.runtimeEpoch<1
        or ticket.runtimeEpoch>=9007199254740991 or ticket.runtimeEpoch%1~=0 then return false end
    if ticket.queueReload~=nil then
        return ticket.queueReload==true and ticket.receipt==nil and ticket.bodyBytes==nil
            and ticket.bodyAdler32==nil and ticket.codeSHA256==nil and ticket.codeAdler32==nil
            and ticket.codeBytes==nil and ticket.cleanupNonce==nil
    end
    for field,size in pairs({codeSHA256=64,codeAdler32=8}) do
        local value=ticket[field]
        if type(value)~="string" or #value~=size or not string.match(value,"^[0-9a-f]+$") then return false end
    end
    if ticket.cleanupNonce~=nil then
        if type(ticket.cleanupNonce)~="string" or #ticket.cleanupNonce~=32
            or not string.match(ticket.cleanupNonce,"^[0-9a-f]+$")
            or ticket.bodyBytes~=nil or ticket.bodyAdler32~=nil then return false end
    elseif not (type(ticket.bodyBytes)=="number" and ticket.bodyBytes>=1 and ticket.bodyBytes<=512*1024 and ticket.bodyBytes%1==0
        and type(ticket.bodyAdler32)=="string" and #ticket.bodyAdler32==8 and string.match(ticket.bodyAdler32,"^[0-9a-f]+$")~=nil) then return false end
    return type(ticket.receipt)=="string" and #ticket.receipt>0 and #ticket.receipt<=4096
        and type(ticket.codeBytes)=="number" and ticket.codeBytes>=1 and ticket.codeBytes<=256*1024 and ticket.codeBytes%1==0
end
local function cancel(discard)
    if waiting then waiting:UnregisterAllEvents(); waiting:SetScript("OnEvent",nil); waiting=nil end
    local state=ns.Persistence.Current()
    if state and ((discard and valid(state.reentry)) or (state==ownedRoot and rawequal(state.reentry,ownedTicket)
        and ns.CaptureWriter.Encode(state.reentry,8192)==ownedBytes)) then state.reentry=nil end
    ownedTicket,ownedRoot,ownedBytes=nil,nil,nil
end
local function show(ticket)
    if ticket.cleanupNonce then
        local receipt,reason=ns.ProbeQueue.VerifyRetired(ticket.requestId,ticket.cleanupNonce)
        if not receipt then return nil,reason end
        return ns.ReceiptView.Show(receipt)
    end
    local identity,reason=ns.Session.NextIdentity()
    if not identity then return nil,reason end
    local ready=ns.Platform.ObserveInputState()==true
    local receipt,failure=ns.CaptureWriter.Encode({
        schema="lycheedev.signal.v1",kind="ready",release=ns.Release,
        sessionNonce=identity.sessionNonce,requestId=ticket.requestId,reloadNonce=ticket.reloadNonce,
        character=identity.character,realm=identity.realm,guid=identity.guid,
        product=ns.Startup.identity.product,build=ns.Startup.identity.build,
        sequence=identity.sequence,runtimeEpoch=identity.runtimeEpoch,inputReady=ready,
    },2048)
    if not receipt then return nil,failure end
    return ns.ReceiptView.Show(receipt)
end
local function submit(state,ticket)
    if not valid(ticket) then return nil,"reload_invalid_ticket" end
    if type(C_UI)~="table" or type(C_UI.Reload)~="function" or type(InCombatLockdown)~="function" then return nil,"reload_unavailable" end
    local ok,combat=pcall(InCombatLockdown)
    if not ok or restricted(combat) or combat~=false then return nil,"reload_combat_unavailable" end
    ns.Session.CancelInputWait(); ns.ReceiptView.Hide()
    -- Publish before the one-shot effect. An exception is unresolved, never
    -- permission to retry; opt-out can explicitly discard our own ticket.
    state.reentry=ticket
    ownedTicket,ownedRoot,ownedBytes,submitted=ticket,state,ns.CaptureWriter.Encode(ticket,8192),true
    if not pcall(C_UI.Reload) then return nil,"reload_submission_failed" end
    return "reload_requested"
end
ns.Reentry={
    Cancel=cancel,
    -- A pending, submitted or persisted reload ticket means a request-scoped
    -- operation is mid-flight and owns the displayed receipt.
    Busy=function()
        if waiting or submitted then return true end
        local state,failure=ns.Persistence.Current()
        if not state then return nil,failure end
        if restricted(state.reentry) then return nil,"reload_invalid_ticket" end
        return state.reentry~=nil
    end,
    LoadQueue=function(requestId,reloadNonce)
        if submitted then return nil,"reload_already_submitted" end
        local session,reason=ns.Session.Current()
        if not session then return nil,reason end
        local state,failure=ns.Persistence.Current()
        if not state then return nil,failure end
        if restricted(state.reentry) or state.reentry~=nil then return nil,"reload_ticket_exists" end
        local ticket={schema="lycheedev.reentry.v1",queueReload=true,requestId=requestId,
            sessionNonce=session.sessionNonce,reloadNonce=reloadNonce,runtimeEpoch=session.runtimeEpoch,
            character=session.character,realm=session.realm,guid=session.guid,
            release=ns.Release,product=ns.Startup.identity.product,build=ns.Startup.identity.build}
        if not valid(ticket) then return nil,"reload_invalid_ticket" end
        local receipt,reportFailure=ns.ReportStore.Read(requestId)
        if receipt or reportFailure~="report_unavailable" then return nil,"report_still_retained" end
        local absent,probeFailure=ns.ProbeRunner.VerifyAbsent(requestId)
        if not absent then return nil,probeFailure end
        return submit(state,ticket)
    end,
    Reload=function(requestId,cleanupNonce)
        if submitted then return nil,"reload_already_submitted" end
        local session,reason=ns.Session.Current()
        if not session then return nil,reason end
        local state,failure=ns.Persistence.Current()
        if not state then return nil,failure end
        if restricted(state.reentry) or state.reentry~=nil then return nil,"reload_ticket_exists" end
        local ticket,scopeFailure=ns.ProbeQueue.ReloadScope(requestId)
        if not ticket then return nil,scopeFailure end
        local receipt,body,code
        if cleanupNonce~=nil then
            receipt,body=ns.ReportStore.Acknowledged(requestId)
            ticket.cleanupNonce=cleanupNonce
        else
            receipt,body,code=ns.ProbeRunner.Reported(requestId)
            if receipt then
                if #code~=ticket.codeBytes or ns.CaptureWriter.DigestBytes(code)~=ticket.codeAdler32 then return nil,"reload_code_changed" end
                ticket.bodyBytes,ticket.bodyAdler32=#body,ns.CaptureWriter.DigestBytes(body)
            end
        end
        if not receipt then return nil,body end
        ticket.schema,ticket.runtimeEpoch,ticket.receipt="lycheedev.reentry.v1",session.runtimeEpoch,receipt
        return submit(state,ticket)
    end,
    Start=function(frame)
        local state=ns.Persistence.Current()
        if not state or not state.options or state.options.bridgeEnabled~=true then return true end
        if restricted(state.reentry) then return nil,"reload_invalid_ticket" end
        if state.reentry==nil then return true end
        local ticket=state.reentry
        if not valid(ticket) then return nil,"reload_invalid_ticket" end
        if waiting or submitted then return nil,"reload_already_submitted" end
        ownedTicket,ownedRoot,ownedBytes=ticket,state,ns.CaptureWriter.Encode(ticket,8192)
        local copy={}
        for key,value in pairs(ticket) do copy[key]=value end
        ticket=copy
        waiting=frame
        frame:RegisterEvent("PLAYER_ENTERING_WORLD")
        frame:SetScript("OnEvent",function(_,event,initialLogin,reloading)
            if event~="PLAYER_ENTERING_WORLD" or waiting~=frame then return end
            local unchanged=rawequal(state.reentry,ownedTicket) and ns.CaptureWriter.Encode(state.reentry,8192)==ownedBytes
            cancel()
            if not unchanged then return end
            local current=ns.Persistence.Current()
            if current~=state or not current.options or current.options.bridgeEnabled~=true then return end
            if restricted(initialLogin) or restricted(reloading) or initialLogin~=false or reloading~=true then return end
            if restricted(state.runtimeEpoch) or state.runtimeEpoch~=ticket.runtimeEpoch then return end
            local actor=ns.Platform.ObserveActor()
            if not actor or actor.character~=ticket.character or actor.realm~=ticket.realm or actor.guid~=ticket.guid
                or ns.Release~=ticket.release or ns.Startup.identity.product~=ticket.product or ns.Startup.identity.build~=ticket.build then return end
            local receipt,body=ns.ReportStore.Read(ticket.requestId)
            if ticket.cleanupNonce or ticket.queueReload then
                if receipt~=nil or body~="report_unavailable" then return end
            elseif receipt~=ticket.receipt or type(body)~="string" or #body~=ticket.bodyBytes or ns.CaptureWriter.DigestBytes(body)~=ticket.bodyAdler32 then return end
            local bound=ns.Session.Bind(ticket.sessionNonce)
            if not bound then return end
            if bound.runtimeEpoch~=ticket.runtimeEpoch+1 then ns.Session.Release(); return end
            if not ticket.cleanupNonce then
                local scope=ns.ProbeQueue.ReloadScope(ticket.requestId)
                if not scope then ns.Session.Release(); return end
                for key,value in pairs(scope) do
                    if not (ticket.queueReload and (key=="codeSHA256" or key=="codeAdler32" or key=="codeBytes"))
                        and ticket[key]~=value then ns.Session.Release(); return end
                end
            end
            local visible=show(ticket)
            if not visible then ns.Session.Release(); ns.ReceiptView.Hide(); return end
            if ticket.cleanupNonce then return end
            ns.Session.WhenInputReady(function(ready)
                if ready then
                    local shown=show(ticket)
                    if not shown then ns.ReceiptView.Hide() end
                end
            end)
        end)
        return true
    end,
}
