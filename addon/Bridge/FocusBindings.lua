local _, ns = ...

-- Enabled-only forwarding for the current ordinary EditBox. Keep its own
-- handler and typing behavior; only our two registered actions propagate.
local enabled, attached, original, wrapper, propagation
local callbackOwner = {}
local function restricted(value) return issecretvalue and issecretvalue(value) end
local function focus()
    if type(GetCurrentKeyBoardFocus) ~= "function" then return nil end
    local ok, value = pcall(GetCurrentKeyBoardFocus)
    if not ok or restricted(value) then return nil end
    return value
end
local function detach()
    if attached and attached:GetScript("OnKeyDown") == wrapper then
        -- Keep ownership until restoration is legal; INPUT handles the
        -- combat-end cleanup if the bridge is stopped in the meantime.
        if InCombatLockdown() then return false end
        attached:SetScript("OnKeyDown", original)
        attached:SetPropagateKeyboardInput(propagation)
    end
    attached, original, wrapper, propagation = nil, nil, nil, nil
    return true
end
local function eligible(editBox)
    if not editBox or editBox:IsForbidden() then return false end
    for _, method in ipairs({ "IsProtected", "IsPassword", "IsSecureText" }) do
        if type(editBox[method]) ~= "function" then return false end
        local value = editBox[method](editBox)
        if restricted(value) or value ~= false then return false end
    end
    return true
end
local function refresh()
    if not enabled or InCombatLockdown() then return false end
    local editBox = focus()
    if editBox == attached and attached and attached:GetScript("OnKeyDown") == wrapper then return true end
    detach()
    if not eligible(editBox) then return false end
    local prior = editBox:GetScript("OnKeyDown")
    local propagate = editBox:GetPropagateKeyboardInput()
    if restricted(prior) or restricted(propagate) then return false end
    local function onKeyDown(self, key, ...)
        if enabled and not InCombatLockdown() then
            self:SetPropagateKeyboardInput(propagate)
        end
        if prior then prior(self, key, ...) end
        -- The original handler may change state, so check again before
        -- forwarding our shortcut.
        if not enabled or InCombatLockdown() or restricted(key) then return end
        local chord = ns.Compat.ReceiverChord(key)
        if not chord then return end
        local profile = ns.ReceiverBindings.Current()
        if profile and (chord == profile.wake or chord == profile.reload) then
            self:SetPropagateKeyboardInput(true)
        end
    end
    attached, original, wrapper, propagation = editBox, prior, onKeyDown, propagate
    editBox:SetScript("OnKeyDown", onKeyDown)
    return true
end
ns.FocusBindings = {
    Refresh = refresh,
    Ready = function() return enabled and not InCombatLockdown() and attached ~= nil and focus() == attached and attached:GetScript("OnKeyDown") == wrapper end,
    Start = function()
        if enabled then return end
        enabled = true
        ns.Compat.ObserveChatFocus(callbackOwner, function()
            refresh()
            if ns.InputState then ns.InputState.Refresh() end
        end)
        refresh()
    end,
    Stop = function()
        if enabled then
            enabled = false
            ns.Compat.StopObservingChatFocus(callbackOwner)
        end
        return detach()
    end,
}
