local _, ns = ...

local engine,receiving,inputLease,expectedSlot,skipLoaded
local LIMIT=ns.SlotProtocol.Count
local zero=string.rep("0",32)
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
local function start()
    if engine then
        local state=ns.Persistence.Current()
        if ns.InputState and state and state.options.bridgeEnabled~=false then ns.InputState.Start(engine.InputIdentity) end
        return true
    end
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
    local available=0
    for i=1,LIMIT do
        local name=string.format("Lychee Dev Slot %02d",i)
        if ns.Compat.GetAddOnMetadata(name,"X-Lychee-Slot")==tostring(i)
            and ns.Compat.GetAddOnMetadata(name,"Version")==ns.Release
            and ns.Compat.GetAddOnMetadata(name,"X-Lychee-Transport")=="memory-slot-v2" then available=available+1 end
    end
    ns.AutomationHistory.Begin(runtime,{character=actor.character,realm=actor.realm,guid=actor.guid,build=build.build,product=build.product})
    engine=ns.SlotProtocol.Create({runtime=runtime,build=build.build,product=build.product,release=ns.Release,inventory=available,
        inputState=ns.InputState and "lycheedev.input.hybrid.v1" or nil,
        actor=function()
            local current=ns.Platform.ObserveActor()
            if current and current.guid==actor.guid then return current end
        end,encode=ns.CaptureWriter.Encode,
        compile=function(code)return loadstring(code,"=LycheeMemoryProbe")end,
        execute=ns.ProbeExecution.Run,releaseInput=releaseInput,activity=activity,observe=ns.AutomationHistory.Observe})
    engine.Describe()
    local current=ns.Persistence.Current()
    if ns.InputState and current and current.options.bridgeEnabled~=false then ns.InputState.Start(engine.InputIdentity) end
    return true
end
local function inventory()
    for i=1,LIMIT do
        local name=string.format("Lychee Dev Slot %02d",i)
        if ns.Compat.GetAddOnMetadata(name,"X-Lychee-Slot")~=tostring(i)
            or ns.Compat.GetAddOnMetadata(name,"Version")~=ns.Release
            or ns.Compat.GetAddOnMetadata(name,"X-Lychee-Transport")~="memory-slot-v2" then return nil,"slot_inventory_incomplete:"..i end
    end
    return true
end
local function wake()
    local state=ns.Persistence.Current()
    if not state or state.options.bridgeEnabled==false then return nil,"slot_disabled" end
    local ok,reason=start();if not ok then return nil,reason end
    if ns.Platform.ObserveInputState(true)~=true and not receiving then return nil,"slot_input_not_ready" end
    ok,reason=inventory();if not ok then return nil,reason end
    state.options.bridgeEnabled=true -- Explicit wake opts in; explicit off stays off.
    if ns.StartupBeacon then ns.StartupBeacon.Stop() end
    ok,reason=protectInput();if not ok then releaseInput();return nil,reason end
    -- One wake may traverse foreign/empty physical slots, but can process at
    -- most one own-runtime envelope. A rejected own envelope is still a stop.
    for attempt=1,LIMIT do
        local index=engine.NextSlot()
        if index>LIMIT then releaseInput();engine.Describe();return nil,"slot_exhausted" end
        expectedSlot,skipLoaded=index,nil
        local loaded,failure=ns.Compat.LoadInputSlot(string.format("Lychee Dev Slot %02d",index))
        expectedSlot=nil
        if not loaded then releaseInput();engine.Describe();return nil,failure end
        if skipLoaded==nil then releaseInput();engine.Describe();return nil,"slot_loader_no_dispatch" end
        if not skipLoaded then break end
    end
    if skipLoaded and engine.NextSlot()>LIMIT then releaseInput();engine.Describe();return nil,"slot_exhausted" end
    -- Control transactions have no long-lived keyboard ownership. A fresh
    -- wake protects each input burst; business entry already released it.
    releaseInput()
    engine.Describe()
    return true
end
local function guardedWake()
    local ok,result,reason=pcall(wake)
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
    Receive=function(index,envelope)
        if not engine or index~=expectedSlot then return nil,"slot_not_requested" end
        local record,value,skip=engine.Receive(index,envelope)
        skipLoaded=skip==true
        return record,value
    end,
    Snapshot=function()return engine and engine.Snapshot()end,
    Register=function()
        return ns.ReceiverBindings.Register({wake=guardedWake,submit=guardedWake,close=close})
    end,
}
