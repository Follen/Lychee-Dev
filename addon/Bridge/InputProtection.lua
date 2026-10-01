local _, ns = ...

-- One short, runtime-local lease. No UI or event work until explicitly acquired.
-- This shields this game's UI; it is not a system-wide keyboard/mouse lock.
local frame, active, unavailable, activeBindings
local function release(token, reason)
    if not active or active ~= token then return false end
    local prior=active;active=nil;activeBindings=nil
    if prior.timer then pcall(prior.timer.Cancel,prior.timer) end
    frame:Hide();frame:UnregisterAllEvents();frame:SetScript("OnEvent",nil)
    if ns.ActivityView then pcall(ns.ActivityView.Receiving,false) end
    if reason and prior.interrupted then prior.interrupted(reason) end
    return true
end
ns.InputProtection={
    Acquire=function(seconds, interrupted, transport)
        if unavailable then return nil,"input_protection_unavailable" end
        if active then return nil,"input_protection_busy" end
        if (issecretvalue and issecretvalue(seconds)) or type(seconds)~="number"
            or seconds~=seconds or seconds<=0 or seconds==math.huge then return nil,"input_protection_invalid_budget" end
        if ns.Platform.ObserveInputState(transport==true)~=true then return nil,"input_protection_not_ready" end
        if not C_Timer or type(C_Timer.NewTimer)~="function" then return nil,"input_protection_timer_unavailable" end
        local bindings
        if transport==true then
            if not ns.ReceiverBindings or type(ns.ReceiverBindings.Current)~="function" then return nil,"input_protection_not_ready" end
            local ok,profile=pcall(ns.ReceiverBindings.Current)
            if not ok or (issecretvalue and issecretvalue(profile)) or type(profile)~="table" then return nil,"input_protection_not_ready" end
            for _,action in ipairs({"wake","submit","close"}) do
                if issecretvalue and issecretvalue(profile[action]) then return nil,"input_protection_not_ready" end
            end
            if profile.close~="ALT-CTRL-[" or not
                ((profile.wake=="ALT-CTRL-F12" and profile.submit=="ALT-CTRL-SHIFT-F12")
                or (profile.wake=="ALT-CTRL-F11" and profile.submit=="ALT-CTRL-SHIFT-F11")) then return nil,"input_protection_not_ready" end
            bindings={wake=profile.wake,submit=profile.submit,close=profile.close}
        end
        if not frame then
            local candidate
            local created=pcall(function()
            candidate=CreateFrame("Frame",nil,UIParent)
            candidate:Hide();candidate:SetAllPoints(UIParent);candidate:SetFrameStrata("TOOLTIP")
            candidate:EnableMouse(true);candidate:EnableMouseWheel(true);candidate:EnableKeyboard(true)
            candidate:SetScript("OnKeyDown",function(self,key)
                local chord=ns.Compat.ReceiverChord(key)
                local token=active
                self:SetPropagateKeyboardInput(false)
                if chord=="ALT-CTRL-[" then release(token,"input_protection_cancelled");return end
                if token and activeBindings then
                    self:SetPropagateKeyboardInput(chord==activeBindings.wake or chord==activeBindings.submit)
                end
            end)
            end)
            if not created then
                unavailable=true
                if candidate then pcall(candidate.Hide,candidate) end
                return nil,"input_protection_unavailable"
            end
            frame=candidate
        end
        local token={interrupted=interrupted,transport=transport==true}
        active=token;activeBindings=bindings
        local ok=pcall(function()
            token.timer=C_Timer.NewTimer(seconds,function()release(token,"input_protection_timeout")end)
            assert(token.timer,"input_protection_timer_unavailable")
            frame:RegisterEvent("PLAYER_REGEN_DISABLED");frame:RegisterEvent("PLAYER_LEAVING_WORLD")
            frame:SetScript("OnEvent",function(_,event)release(token,event=="PLAYER_REGEN_DISABLED"
                and "input_protection_combat" or "input_protection_world_changed")end)
            frame:Show()
            if ns.ActivityView then ns.ActivityView.Receiving(true) end
        end)
        if not ok then release(token);return nil,"input_protection_unavailable" end
        return token
    end,
    Release=function(token)return release(token)end,
    Cancel=function()return release(active,"input_protection_cancelled")end,
    IsActive=function()return active~=nil end,
}
