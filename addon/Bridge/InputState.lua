local _, ns = ...

-- Owner-approved native transport telemetry. One sampler, no input
-- hooks or focus mutation. Explicit bridge-off removes all recurring work.
local frame, provider, record, last, sequence, lastTick,mailbox
local loading,leaving,refreshing
local zero=string.rep("0",32)
local function nativeBindings()
    if not ns.ReceiverBindings or type(ns.ReceiverBindings.Current)~="function" then return nil end
    local ok,profile=pcall(ns.ReceiverBindings.Current)
    if not ok or (issecretvalue and issecretvalue(profile)) or type(profile)~="table" then return nil end
    return {wake=profile.wake,reload=profile.reload}
end
local function hide()record=nil;if mailbox then rawset(mailbox,"input",nil) end end
local function sample()
    if loading or leaving or ns.Compat.InWorld()~=true then hide();return end
    local identity=provider and provider()
    if not identity then hide();return end
    local tick=ns.Compat.MonotonicSeconds()
    if not tick or (lastTick and tick<lastTick) then hide();return end
    lastTick=tick
    local forwarded=ns.FocusBindings and ns.FocusBindings.Refresh()
    local ready,reason=ns.Platform.ObserveInputState(forwarded==true)
    if ns.FocusBindings and reason=="input_keyboard_focus" then reason="input_focus_unavailable" end
    if issecretvalue and (issecretvalue(ready) or issecretvalue(reason)) then ready,reason=nil,"input_observation_unavailable" end
    local bindings=nativeBindings()
    if not bindings then ready,reason=false,"input_binding_unavailable" end
    if ready~=true and (type(reason)~="string" or reason=="") then reason="input_observation_unavailable" end
    sequence=sequence+1
    if sequence>=4294967295 then ns.InputState.Stop();return end
    identity.schema="Lycheedev mailbox protocol v1"
    identity.bindings=bindings or {wake="",reload=""}
    identity.sampleMillis=math.floor(tick*1000)
    identity.inputBlocked=ready~=true
    identity.reason=reason or ""
    identity.escapeLimit=0
    if ns.SlotRuntime and ns.SlotRuntime.SlotReloadRequired then
        identity.slotReloadRequired=ns.SlotRuntime.SlotReloadRequired() or nil
    end
    -- Always obtain a detached private snapshot; public mailbox values never
    -- drive the attempt sequence or any input/receipt authorization.
    if ns.SlotRuntime and ns.SlotRuntime.InputDiagnostic then identity.inputAttempt=ns.SlotRuntime.InputDiagnostic() end
    local text=ns.CaptureWriter.Encode(identity,2048)
    if not text then hide();return end
    record=ns.MemoryProtocol.Encode(identity.runtime,identity.runtime,zero,5,1,sequence,text)
    if not record then hide();return end
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
        if ns.FocusBindings then ns.FocusBindings.Start() end
        sequence,last=sequence or 0,0
        -- AFK/cinematic UIs hide UIParent. Transport telemetry has no visual
        -- content and must remain independent while the player is in-world.
        if not frame then frame=CreateFrame("Frame");frame:Hide();frame:EnableMouse(false) end
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
        if ns.FocusBindings then ns.FocusBindings.Stop() end
        if frame then
            frame:SetScript("OnUpdate",nil);frame:SetScript("OnEvent",nil)
            frame:SetScript("OnShow",nil);frame:SetScript("OnHide",nil);frame:UnregisterAllEvents()
        end
        provider,record,lastTick=nil,nil,nil
        hide()
        mailbox=nil
        if frame then frame:Hide() end
    end,
    Snapshot=function()return record end,
}
