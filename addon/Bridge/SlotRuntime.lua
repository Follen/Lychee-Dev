local _, ns = ...

local engine,receiving,inputLease,expectedSlot,skipLoaded,mailbox,runtimeToken
local LIMIT=ns.SlotProtocol.Count
local zero=string.rep("0",32)
-- Latest private callback outcome only. This is diagnostic data, never a
-- receipt, receive capability, owner decision or retry authorization.
local attemptSequence,attemptActive,attemptReceived,lastAttempt=0,false,false,nil
local attemptReasons={slot_input_not_ready=true,input_protection_not_ready=true,input_protection_busy=true,
    input_protection_unavailable=true,input_protection_timer_unavailable=true,slot_exhausted=true,
    slot_loader_no_dispatch=true,slot_inventory_incomplete=true,slot_load_failed=true,slot_wake_failed=true}
local function noteAttempt(stage,reason,slot)
    if not attemptActive or not runtimeToken or not engine then return end
    if reason and not attemptReasons[reason] then
        reason=type(reason)=="string" and reason:match("^slot_inventory_incomplete:") and "slot_inventory_incomplete" or "slot_wake_rejected"
    end
    lastAttempt={schema="lycheedev.input-attempt.v1",runtime=runtimeToken,attemptSeq=attemptSequence,
        slot=slot or engine.NextSlot(),stage=stage,reason=reason or "",received=attemptReceived}
end
local function attemptSnapshot()
    if not lastAttempt then return nil end
    return {schema=lastAttempt.schema,runtime=lastAttempt.runtime,attemptSeq=lastAttempt.attemptSeq,
        slot=lastAttempt.slot,stage=lastAttempt.stage,reason=lastAttempt.reason,received=lastAttempt.received}
end
local function releaseInput()
    receiving=false
    if inputLease then ns.InputProtection.Release(inputLease);inputLease=nil end
end
local function protectInput()
    if receiving then return true end
    local reason
    inputLease,reason=ns.InputProtection.Acquire(2,releaseInput,true)
    if not inputLease then return nil,reason end
    receiving=true
    return true
end
local function activity(state,ticket)
    if not ns.ActivityView then return end
    if state=="idle" then ns.ActivityView.Finish(ticket)
    elseif state=="collecting" and ns.ActivityView.Collecting then ns.ActivityView.Collecting(ticket)
    else ns.ActivityView.Begin(ticket) end
end
local function publishMailbox(runtime)
    if not mailbox then
        mailbox={schema="lycheedev.mailbox.v1",release=ns.Release,runtime=runtime,receipts={},bodies={}}
        engine.AttachMailbox(mailbox)
    end
    ns.Mailbox=mailbox
    return mailbox
end
local function start()
    if engine then
        local state=ns.Persistence.Current()
        if state and state.options.bridgeEnabled~=false then
            local publication=publishMailbox(runtimeToken)
            if ns.InputState then ns.InputState.Start(engine.InputIdentity,publication) end
        end
        return true
    end
    -- A new engine owns a new publication table. Never keep an older engine's
    -- public mailbox visible while rebuilding or when startup fails.
    ns.Mailbox=nil
    local state,reason=ns.Persistence.Bridge();if not state then return nil,reason end
    local build;build,reason=ns.Platform.ObserveBuild();if not build then return nil,reason end
    local actor;actor,reason=ns.Platform.ObserveActor();if not actor then return nil,reason end
    local epoch=state.memoryEpoch or 0
    if (issecretvalue and issecretvalue(epoch)) or type(epoch)~="number" or epoch<0 or epoch%1~=0 or epoch>=4294967295 then return nil,"slot_epoch_invalid" end
    local clock=ns.Compat.MonotonicSeconds();if not clock or clock>=4294967295 then return nil,"slot_clock_unavailable" end
    -- A reload-persisted generation plus runtime clock and random suffix. The
    -- CLI additionally binds the OS process creation identity and fresh nonce.
    -- This token is an accidental-replay discriminator, not a secret/password.
    state.memoryEpoch=epoch+1
    local runtime=string.format("%08x%08x%04x%04x%04x%04x",epoch+1,math.floor(clock),math.random(0,65535),math.random(0,65535),math.random(0,65535),math.random(0,65535))
    runtimeToken=runtime
    local available=0
    for i=1,LIMIT do
        local name=string.format("Lychee Dev Slot %02d",i)
        if ns.Compat.GetAddOnMetadata(name,"X-Lychee-Slot")==tostring(i)
            and ns.Compat.GetAddOnMetadata(name,"Version")==ns.Release
            and ns.Compat.GetAddOnMetadata(name,"X-Lychee-Transport")=="memory-slot-v3" then available=available+1 end
    end
    ns.AutomationHistory.Begin(runtime,{character=actor.character,realm=actor.realm,guid=actor.guid,build=build.build,product=build.product})
    engine=ns.SlotProtocol.Create({runtime=runtime,build=build.build,product=build.product,release=ns.Release,inventory=available,
        inputState=ns.InputState and "lycheedev.input.hybrid.v2" or nil,
        actor=function()
            local current=ns.Platform.ObserveActor()
            if current and current.guid==actor.guid then return current end
        end,encode=ns.CaptureWriter.Encode,
        compile=function(code)return loadstring(code,"=LycheeMemoryProbe")end,
        execute=ns.ProbeExecution.Run,releaseInput=releaseInput,activity=activity,observe=ns.AutomationHistory.Observe})
    engine.Describe()
    local current=ns.Persistence.Current()
    if current and current.options.bridgeEnabled~=false then
        local publication=publishMailbox(runtime)
        if ns.InputState then ns.InputState.Start(engine.InputIdentity,publication) end
    end
    return true
end
local function inventory()
    for i=1,LIMIT do
        local name=string.format("Lychee Dev Slot %02d",i)
        if ns.Compat.GetAddOnMetadata(name,"X-Lychee-Slot")~=tostring(i)
            or ns.Compat.GetAddOnMetadata(name,"Version")~=ns.Release
            or ns.Compat.GetAddOnMetadata(name,"X-Lychee-Transport")~="memory-slot-v3" then return nil,"slot_inventory_incomplete:"..i end
    end
    return true
end
local function wake()
    local state=ns.Persistence.Current()
    if not state or state.options.bridgeEnabled==false then return nil,"slot_disabled" end
    attemptActive=attemptSequence<9007199254740991
    if attemptActive then attemptSequence=attemptSequence+1;attemptReceived=false;lastAttempt=nil end
    local ok,reason=start();if not ok then return nil,reason end
    noteAttempt("entered")
    if ns.Platform.ObserveInputState(true)~=true and not receiving then return nil,"slot_input_not_ready" end
    ok,reason=inventory();if not ok then return nil,reason end
    state.options.bridgeEnabled=true -- Explicit wake opts in; explicit off stays off.
    if ns.StartupBeacon then ns.StartupBeacon.Stop() end
    ok,reason=protectInput();if not ok then releaseInput();return nil,reason end
    -- One wake may traverse foreign/empty physical slots, but can process at
    -- most one own-runtime envelope. A rejected own envelope is still a stop.
    for attempt=1,LIMIT do
        local index=engine.NextSlot()
        if index>LIMIT then noteAttempt("rejected","slot_exhausted",index);releaseInput();engine.Describe();return nil,"slot_exhausted" end
        expectedSlot,skipLoaded=index,nil
        attemptReceived=false
        noteAttempt("loading",nil,index)
        local loaded,failure=ns.Compat.LoadInputSlot(string.format("Lychee Dev Slot %02d",index))
        expectedSlot=nil
        if not loaded then noteAttempt("load_failed","slot_load_failed",index);releaseInput();engine.Describe();return nil,failure end
        if skipLoaded==nil then noteAttempt("no_dispatch","slot_loader_no_dispatch",index);releaseInput();engine.Describe();return nil,"slot_loader_no_dispatch" end
        if not skipLoaded then break end
    end
    if skipLoaded and engine.NextSlot()>LIMIT then noteAttempt("rejected","slot_exhausted",engine.NextSlot());releaseInput();engine.Describe();return nil,"slot_exhausted" end
    -- Control transactions have no long-lived keyboard ownership. A fresh
    -- wake protects each input burst; business entry already released it.
    releaseInput()
    engine.Describe()
    return true
end
local function guardedWake()
    attemptActive=false
    local ok,result,reason=pcall(wake)
    if attemptActive then
        if not ok then noteAttempt("exception","slot_wake_failed",lastAttempt and lastAttempt.slot)
        elseif not result and (not lastAttempt or lastAttempt.stage=="entered" or lastAttempt.stage=="loading") then
            noteAttempt("rejected",reason,lastAttempt and lastAttempt.slot)
        end
    end
    attemptActive=false
    expectedSlot,skipLoaded=nil,nil
    releaseInput()
    if ns.InputState then ns.InputState.Refresh() end
    if not ok then return nil,"slot_wake_failed" end
    return result,reason
end
local function close()
    releaseInput()
    ns.InputProtection.Cancel()
    if ns.InputState then ns.InputState.Refresh() end
end
ns.SlotRuntime={
    Start=start,Wake=guardedWake,Close=close,
    IsActive=function()return ns.InputProtection.IsActive()end,
    InputDiagnostic=attemptSnapshot,
    Receive=function(index,envelope)
        if not engine or index~=expectedSlot then return nil,"slot_not_requested" end
        attemptReceived=true
        noteAttempt("received",nil,index)
        local record,value,skip=engine.Receive(index,envelope)
        skipLoaded=skip==true
        return record,value
    end,
    Snapshot=function()return engine and engine.Snapshot()end,
    Register=function()
        return ns.ReceiverBindings.Register({wake=guardedWake,submit=guardedWake,close=close})
    end,
}
