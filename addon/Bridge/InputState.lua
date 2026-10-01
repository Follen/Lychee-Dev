local _, ns = ...

-- Owner-approved native transport telemetry. One sampler/display, no input
-- hooks or focus mutation. Explicit bridge-off removes all recurring work.
local frame, provider, record, last, sequence, lastTick,mailbox
local loading,leaving,refreshing,heartbeat
local zero=string.rep("0",32)
local function nativeBindingsReady()
    if not ns.SlotRuntime then return true end
    if not ns.ReceiverBindings or type(ns.ReceiverBindings.Current)~="function" then return false end
    local ok,profile=pcall(ns.ReceiverBindings.Current)
    if not ok or (issecretvalue and issecretvalue(profile)) or type(profile)~="table" then return false end
    for _,key in ipairs({"wake","submit","close"}) do
        if issecretvalue and issecretvalue(profile[key]) then return false end
    end
    return profile.wake=="ALT-CTRL-F12" and profile.submit=="ALT-CTRL-SHIFT-F12" and profile.close=="ALT-CTRL-["
end
local function hide()record=nil;if mailbox then rawset(mailbox,"input",nil) end;ns.InputSignal.Hide() end
local function sample()
    if loading or leaving or ns.Compat.InWorld()~=true then hide();return end
    local identity=provider and provider()
    if not identity then hide();return end
    local tick=ns.Compat.MonotonicSeconds()
    if not tick or (lastTick and tick<lastTick) then hide();return end
    lastTick=tick
    local ready,reason=ns.Platform.ObserveInputState()
    if issecretvalue and (issecretvalue(ready) or issecretvalue(reason)) then ready,reason=nil,"input_observation_unavailable" end
    if not nativeBindingsReady() then ready,reason=false,"input_binding_unavailable" end
    if ready~=true and (type(reason)~="string" or reason=="") then reason="input_observation_unavailable" end
    sequence=sequence+1
    if sequence>=4294967295 then ns.InputState.Stop();return end
    identity.schema="lycheedev.input.v2"
    identity.sampleMillis=math.floor(tick*1000)
    identity.inputBlocked=ready~=true
    identity.reason=reason or ""
    -- Always obtain a detached private snapshot; public mailbox values never
    -- drive the attempt sequence or any input/receipt authorization.
    if ns.SlotRuntime and ns.SlotRuntime.InputDiagnostic then identity.inputAttempt=ns.SlotRuntime.InputDiagnostic() end
    local text=ns.CaptureWriter.Encode(identity,2048)
    if not text then hide();return end
    record=ns.MemoryProtocol.Encode(identity.runtime,identity.runtime,zero,5,1,sequence,text)
    if not record then hide();return end
    local state=ready==true and "ready" or (reason=="input_keyboard_focus" or reason=="input_combat_lockdown") and reason or "unknown"
    local nextHeartbeat=not heartbeat
    if not ns.InputSignal.Render(state,nextHeartbeat) then hide();return end
    heartbeat=nextHeartbeat
    if mailbox then rawset(mailbox,"input",record) end
end
local function refresh()
    if refreshing or not provider or not frame or not frame:GetScript("OnUpdate") then return end
    refreshing=true
    local ok=pcall(sample)
    refreshing=false
    if not ok then hide() end
end
ns.InputState={
    Start=function(observe,publication)
        if mailbox~=publication then
            if mailbox then rawset(mailbox,"input",nil) end
            mailbox=publication
            if mailbox then rawset(mailbox,"input",nil) end
        end
        provider=observe
        if frame and frame:GetScript("OnUpdate") then return true end
        sequence,last=sequence or 0,0
        if not frame then frame=ns.InputSignal.Frame() end
        loading,leaving=false,false
        for _,event in ipairs({"PLAYER_ENTERING_WORLD","PLAYER_LEAVING_WORLD","LOADING_SCREEN_ENABLED","LOADING_SCREEN_DISABLED","PLAYER_REGEN_ENABLED","PLAYER_REGEN_DISABLED"}) do frame:RegisterEvent(event) end
        frame:SetScript("OnEvent",function(_,event)
            if not provider then return end
            if event=="PLAYER_LEAVING_WORLD" then leaving=true;hide();return end
            if event=="LOADING_SCREEN_ENABLED" then loading=true;hide();return end
            if event=="PLAYER_ENTERING_WORLD" then leaving=false end
            if event=="LOADING_SCREEN_DISABLED" then loading=false end
            refresh()
        end)
        frame:SetScript("OnHide",hide)
        frame:SetScript("OnShow",refresh)
        frame:SetScript("OnUpdate",function(_,elapsed)
            last=last+elapsed
            if last<1 then return end
            last=0
            refresh()
        end)
        refreshing=true;frame:Show();refreshing=false
        refresh()
        return true
    end,
    -- Transaction completion refreshes facts without moving the periodic tick.
    Refresh=refresh,
    Stop=function()
        if frame then
            frame:SetScript("OnUpdate",nil);frame:SetScript("OnEvent",nil)
            frame:SetScript("OnShow",nil);frame:SetScript("OnHide",nil);frame:UnregisterAllEvents()
        end
        provider,record,lastTick=nil,nil,nil
        hide()
        mailbox=nil
        if frame then frame:Hide() end
        if ns.StartupBeacon then ns.StartupBeacon.Stop() end
    end,
    Snapshot=function()return record end,
}
