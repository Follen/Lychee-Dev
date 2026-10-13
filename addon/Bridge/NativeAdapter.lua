local _,ns=...

-- Candidate production closure. The three fixed keys invoke private phase
-- gates; Mailbox is a detached read-only publication, never command input.
local factory=ns.NativeRuntime.Create
local runtime,mailbox,lease,expected,bootstrap,received
local openedOnce,recoveryAttempted=false,false
local bootstrapPublication
local function release()
    if lease then local token=lease;lease=nil;ns.InputProtection.Release(token) end
end
local function metadata(slot)
    local name=string.format("Lychee Dev Slot %02d",slot)
    if ns.Compat.GetAddOnMetadata(name,"X-Lychee-Slot")~=tostring(slot)
        or ns.Compat.GetAddOnMetadata(name,"Version")~=ns.Release
        or ns.Compat.GetAddOnMetadata(name,"X-Lychee-Transport")~="memory-slot-v4" then return nil,"native_inventory_changed" end
    return name
end
local function load(slot,isBootstrap)
    local name,reason=metadata(slot);if not name then return nil,reason end
    expected,bootstrap,received=slot,isBootstrap,false
    local ok,result,failure=pcall(ns.Compat.LoadNativeInputSlot,name)
    expected,bootstrap=nil,nil
    if not ok then return nil,"native_load_exception" end
    if not result then return nil,failure end
    return true
end
local function passive()
    if bootstrapPublication then ns.Mailbox=bootstrapPublication;return true end
    local actor,reason=ns.Platform.ObserveActor();if not actor then return nil,reason end
    local build;build,reason=ns.Platform.ObserveBuild();if not build then return nil,reason end
    local bindings;bindings,reason=ns.ReceiverBindings.Current();if not bindings then return nil,reason end
    local clock=ns.Compat.MonotonicSeconds();if not clock or clock>=4294967295 then return nil,"native_clock_unavailable" end
    local identity=runtime and runtime.Protocol.InputIdentity() or {runtime=string.format("%08x%08x%04x%04x%04x%04x",0,math.floor(clock),math.random(0,65535),math.random(0,65535),math.random(0,65535),math.random(0,65535)),
        owner="",fence=0,sessionEpoch="",nextSlot=0,guid=actor.guid,character=actor.character,realm=actor.realm,product=build.product,build=build.build,release=ns.Release,inventory=0,slots=500,challenge=""}
    if not identity then return nil,"native_actor_unavailable" end
    identity.schema="lycheedev.slot.identity.v3";identity.activationOnly=true
    local payload=ns.CaptureWriter.Encode(identity,2048);if not payload then return nil,"native_bootstrap_encoding" end
    local zero=string.rep("0",32)
    local publication={schema="lycheedev.mailbox.v2",release=ns.Release,runtime=identity.runtime,receipts={},bodies={}}
    publication.identity=ns.NativeWire.Encode(zero,identity.runtime,zero,1,1,1,payload)
    identity.schema="lycheedev.input.v4";identity.bindings=bindings;identity.inputBlocked=true;identity.reason="native_activation_required";identity.sampleMillis=math.floor(clock*1000)
    payload=ns.CaptureWriter.Encode(identity,2048);if not payload then return nil,"native_bootstrap_encoding" end
    publication.input=ns.NativeWire.Encode(identity.runtime,identity.runtime,zero,5,1,1,payload)
    bootstrapPublication=publication;ns.Mailbox=publication
    -- Bootstrap infrastructure data only: no inventory loop, frame, events,
    -- timer or OnUpdate. A physical Wake performs explicit Enable below.
    return true
end
local function start()
    if not ns.Persistence.BridgeEnabled() then return passive() end
    bootstrapPublication=nil
    if runtime then ns.InputState.Start(runtime.Protocol.InputIdentity,mailbox);return true end
    ns.Mailbox=nil
    local state,reason=ns.Persistence.Bridge();if not state then return nil,reason end
    local build;build,reason=ns.Platform.ObserveBuild();if not build then return nil,reason end
    local actor;actor,reason=ns.Platform.ObserveActor();if not actor then return nil,reason end
    local epoch=state.memoryEpoch or 0
    if (issecretvalue and issecretvalue(epoch)) or type(epoch)~="number" or epoch%1~=0 or epoch<0 or epoch>=4294967295 then return nil,"native_epoch_invalid" end
    local clock=ns.Compat.MonotonicSeconds();if not clock or clock>=4294967295 then return nil,"native_clock_unavailable" end
    -- Full inventory once per new runtime. Every load rechecks the exact slot;
    -- host still verifies the complete immutable managed file receipt.
    for slot=1,500 do local name,failure=metadata(slot);if not name then return nil,failure end end
    state.memoryEpoch=epoch+1
    local token=string.format("%08x%08x%04x%04x%04x%04x",epoch+1,math.floor(clock),math.random(0,65535),math.random(0,65535),math.random(0,65535),math.random(0,65535))
    local challenge=0
    ns.AutomationHistory.Begin(token,{guid=actor.guid,character=actor.character,realm=actor.realm,build=build.build,product=build.product})
    runtime=factory({runtime=token,build=build.build,product=build.product,release=ns.Release,inventory=500,clock=ns.Compat.MonotonicSeconds,
        actor=function()local current=ns.Platform.ObserveActor();if current and current.guid==actor.guid then return current end end,
        challenge=function()challenge=challenge+1;return token..":"..challenge end,
        encode=ns.CaptureWriter.Encode,compile=function(code)return loadstring(code,"=LycheeNativeProbe")end,
        execute=function(fn,budget,done)
            ns.ProbeExecution.Run(fn,budget,done)
            -- ProbeExecution reports through done, including synchronous code.
            return nil
        end,
        cleanup=function(ticket)if ns.ActivityView then ns.ActivityView.Finish(ticket)end return true end,
        observe=ns.AutomationHistory.Observe,
        activity=function(state,ticket)
            if not ns.ActivityView then return end
            if state=="idle"then ns.ActivityView.Finish(ticket)
            elseif state=="collecting"then ns.ActivityView.Collecting(ticket)
            else ns.ActivityView.Begin(ticket)end
        end,
        releaseInput=release,
        load=function(name)local slot=tonumber(name:match("(%d+)$"));return load(slot,false)end})
    mailbox={schema="lycheedev.mailbox.v2",release=ns.Release,runtime=token}
    runtime.Protocol.AttachMailbox(mailbox);runtime.Protocol.Identity();ns.Mailbox=mailbox
    ns.InputState.Start(runtime.Protocol.InputIdentity,mailbox)
    return true
end
local function dispatch(action)
    if not ns.Persistence.BridgeEnabled() then
        if action~="wake" then return nil,"native_disabled" end
        if ns.Platform.ObserveInputState()~=true then return nil,"native_input_not_ready" end
        local state,reason=ns.Persistence.Current();if not state then return nil,reason end
        state.options.bridgeEnabled=true -- Explicit Enable, never a toggle.
        local ok;ok,reason=start();if not ok then return nil,reason end
        if ns.StartupBeacon then ns.StartupBeacon.Stop() end
        return true,"native_activated" -- No bootstrap module/business load.
    end
    local ok,reason=start();if not ok then return nil,reason end
    if ns.Platform.ObserveInputState()~=true then return nil,"native_input_not_ready" end
    local p=runtime.Protocol
    if action=="wake" and not p.InputIdentity().owner:match("%S") then
        -- Loaded LoD modules never execute again in this runtime. One bounded
        -- bind search is permitted; reconnect requires a qualified reload.
        if openedOnce then return nil,"native_reload_required" end
        openedOnce=true
    elseif action=="wake" and p.Fused() then
        if recoveryAttempted then return nil,"native_recovery_exhausted" end
        recoveryAttempted=true
    elseif action=="wake" and not p.CanExecuteWake() then return nil,"native_execute_not_ready"
    elseif action=="settle" and not p.CanSettle() then return nil,"native_result_not_ready"
    elseif action=="close" and not p.CanClose() then return nil,"native_request_pending" end
    lease,reason=ns.InputProtection.Acquire(2,release,true);if not lease then return nil,reason end
    if ns.StartupBeacon then ns.StartupBeacon.Stop() end
    local succeeded,result,failure=pcall(function()
        if action=="wake" and p.InputIdentity().owner=="" then
            for slot=1,15,2 do
                local loaded,err=load(slot,true);if not loaded then return nil,err end
                if p.InputIdentity().owner~="" then return true end
            end
            return nil,"native_bootstrap_exhausted"
        elseif action=="wake" and p.Fused() then
            local lane=p.Route();return load(lane*2+2,true)
        elseif action=="wake" then return runtime.Wake()
        elseif action=="settle" then return runtime.Settle()
        else return runtime.Close() end
    end)
    expected,bootstrap=nil,nil;release();ns.InputState.Refresh()
    if not succeeded then return nil,"native_dispatch_exception" end
    return result,failure
end
local api={Create=factory,Start=start,Wake=function()return dispatch("wake")end,Settle=function()return dispatch("settle")end,
    Snapshot=function()return runtime and runtime.Protocol.InputIdentity()end,
    -- Local bridge-off releases only addon resources; it does not invent a
    -- host CLOSE receipt or consume an unrequested slot.
    Close=function()release();ns.InputProtection.Cancel();if not ns.Persistence.BridgeEnabled() then bootstrapPublication=nil;return passive() end end,
    IsActive=function()return lease~=nil end,
    Register=function()return ns.ReceiverBindings.Register({wake=function()return dispatch("wake")end,submit=function()return dispatch("settle")end,close=function()return dispatch("close")end})end,
    Receive=function(slot,e)
        if not runtime or expected~=slot then return nil,"native_unsolicited_loader" end
        received=true
        if bootstrap then
            if (issecretvalue and issecretvalue(e)) or type(e)~="table" or getmetatable(e) or (issecretvalue and issecretvalue(e.runtime))
                or e.runtime~=mailbox.runtime then return nil,"native_foreign_bootstrap" end
            return runtime.Receive(slot,e,true)
        end
        return runtime.Receive(slot,e,false)
    end}
ns.NativeRuntime=api
ns.SlotRuntime=api -- Shared workbench controls choose the native transport.
