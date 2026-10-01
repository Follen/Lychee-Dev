local _, ns = ...

-- These are runtime override bindings. Only our SavedVariables option is
-- written; the player's account key bindings are never saved or replaced.
local defaults = { wake = "ALT-CTRL-]", submit = "ALT-CTRL-SHIFT-]", close = "ALT-CTRL-[" }
local nativePrimary = { wake = "ALT-CTRL-F12", submit = "ALT-CTRL-SHIFT-F12", close = "ALT-CTRL-[" }
local nativeFallback = { wake = "ALT-CTRL-F11", submit = "ALT-CTRL-SHIFT-F11", close = "ALT-CTRL-[" }
local actions = { "wake", "submit", "close" }
local owner, buttons, handlers, effective
local nativeWarningPrinted = false
local function restricted(value) return issecretvalue and issecretvalue(value) end
local function copy(profile)
    return { wake = profile.wake, submit = profile.submit, close = profile.close }
end
local function terminal(chord)
    if restricted(chord) or type(chord) ~= "string" or #chord > 24 then return nil end
    local key = string.match(chord, "^ALT%-CTRL%-SHIFT%-(.+)$")
        or string.match(chord, "^ALT%-CTRL%-(.+)$")
    if key == "[" or key == "]" then return key end
    if type(key) == "string" then
        local n = string.match(key, "^F([1-9]%d?)$")
        if n and tonumber(n) <= 12 and key == "F" .. tostring(tonumber(n)) then return key end
    end
    return nil
end
local function validate(profile)
    if restricted(profile) or type(profile) ~= "table" then return nil, "receiver_binding_invalid" end
    local keys, seen = {}, {}
    for _, action in ipairs(actions) do
        local chord = profile[action]
        local key = terminal(chord)
        if not key then return nil, "receiver_binding_invalid" end
        if seen[chord] then return nil, "receiver_binding_duplicate" end
        seen[chord], keys[action] = true, key
    end
    -- Focused EditBox key events do not reliably carry physical modifier state
    -- on the tested PostMessage path. Closing must have a unique terminal key.
    if keys.close == keys.wake or keys.close == keys.submit then
        return nil, "receiver_close_key_conflict"
    end
    return copy(profile)
end
local function noncombat()
    if type(InCombatLockdown) ~= "function" then return nil, "receiver_combat_unavailable" end
    local ok, combat = pcall(InCombatLockdown)
    if not ok or restricted(combat) or type(combat) ~= "boolean" then
        return nil, "receiver_combat_unavailable"
    end
    if combat then return nil, "receiver_combat" end
    return true
end
local function bindingAction(action)
    return "CLICK LycheeDevReceiver" .. string.upper(string.sub(action, 1, 1)) .. string.sub(action, 2) .. ":LeftButton"
end
local function bindingDetail(value)
    if restricted(value) or type(value) ~= "string" then return nil end
    return string.sub(value, 1, 128):gsub("[%z\1-\31\127]", "?")
end
local function conflict(profile)
    if type(GetBindingAction) ~= "function" then return nil, "receiver_bindings_unavailable" end
    for _, action in ipairs(actions) do
        local chord = profile[action]
        for _, override in ipairs({ false, true }) do
            local ok, bound = pcall(GetBindingAction, chord, override)
            if not ok or restricted(bound) or (bound ~= nil and type(bound) ~= "string") then
                return nil, "receiver_bindings_unavailable"
            end
            if bound and bound ~= "" and bound ~= "NONE" and bound ~= bindingAction(action) then
                return nil, "receiver_binding_conflict:" .. action .. ":" .. chord, bindingDetail(bound)
            end
        end
    end
    return true
end
local function clear()
    if type(ClearOverrideBindings) ~= "function" then return nil, "receiver_bindings_unavailable" end
    local ok = pcall(ClearOverrideBindings, owner)
    if not ok then return nil, "receiver_bindings_unavailable" end
    return true
end
local function install(profile)
    if not buttons then
        buttons = {}
        for _, action in ipairs(actions) do
            local button = CreateFrame("Button", "LycheeDevReceiver" .. string.upper(string.sub(action, 1, 1)) .. string.sub(action, 2), UIParent)
            button:RegisterForClicks("AnyDown")
            button:SetScript("OnClick", handlers[action])
            buttons[action] = button
        end
    end
    local clean, failure = clear()
    if not clean then return nil, failure end
    for _, action in ipairs(actions) do
        local ok, result = pcall(SetOverrideBindingClick, owner, false, profile[action], buttons[action]:GetName())
        if not ok or result == false then
            clear()
            return nil, "receiver_bindings_unavailable"
        end
    end
    -- The API call can succeed while a higher-priority override still wins.
    -- Do not advertise a configured chord unless the actual lookup resolves
    -- to the button that owns that action.
    for _, action in ipairs(actions) do
        local ok, resolved = pcall(GetBindingAction, profile[action], true)
        if not ok or restricted(resolved) or type(resolved) ~= "string"
            or resolved ~= bindingAction(action) then
            clear()
            return nil, "receiver_binding_ineffective:" .. action .. ":" .. profile[action], bindingDetail(resolved)
        end
    end
    return true
end
local function apply(profile)
    local valid, failure = validate(profile)
    if not valid then return nil, failure end
    local ready, detail
    ready, failure = noncombat()
    if not ready then return nil, failure end
    ready, failure, detail = conflict(valid)
    if not ready then return nil, failure, detail end
    local prior = effective and copy(effective)
    ready, failure, detail = install(valid)
    if not ready then
        if prior then
            local restored = install(prior)
            effective = restored and prior or nil
        else
            effective = nil
        end
        return nil, failure, detail
    end
    effective = valid
    return copy(valid)
end
local function nativeConflict(reason)
    for _, action in ipairs(actions) do
        local suffix = action .. ":" .. nativePrimary[action]
        if reason == "receiver_binding_conflict:" .. suffix
            or reason == "receiver_binding_ineffective:" .. suffix then return true end
    end
    return false
end
local function displayChord(chord)
    return chord:gsub("ALT%-CTRL%-SHIFT%-", "Ctrl+Alt+Shift+"):gsub("ALT%-CTRL%-", "Ctrl+Alt+")
end
local function nativeFailureText(reason, profile)
    local locale = ns.L or {}
    for _, action in ipairs(actions) do
        local suffix = action .. ":" .. profile[action]
        local label = locale["NATIVE_RECEIVER_BINDING_" .. string.upper(action)] or action
        if reason == "receiver_binding_conflict:" .. suffix then
            return string.format(locale.NATIVE_RECEIVER_BINDING_CONFLICT or "%s shortcut %s is occupied", label, displayChord(profile[action]))
        elseif reason == "receiver_binding_ineffective:" .. suffix then
            return string.format(locale.NATIVE_RECEIVER_BINDING_INEFFECTIVE or "%s shortcut %s is ineffective", label, displayChord(profile[action]))
        end
    end
    return tostring(reason)
end
local function applyNative(warn)
    local profile, failure, detail = apply(nativePrimary)
    if profile or not nativeConflict(failure) then return profile, failure end
    local primaryFailure, primaryDetail = failure, detail
    profile, failure, detail = apply(nativeFallback)
    if warn and not nativeWarningPrinted then
        nativeWarningPrinted = true
        local locale = ns.L or {}
        local unknown = locale.NOT_AVAILABLE or "unavailable"
        local text
        if profile then
            text = string.format(locale.NATIVE_RECEIVER_BINDING_FALLBACK
                or "%s (binding action: %s). Selected fallback shortcuts: wake %s, submit %s, close %s.",
                nativeFailureText(primaryFailure, nativePrimary), primaryDetail or unknown,
                displayChord(profile.wake), displayChord(profile.submit), displayChord(profile.close))
        else
            text = string.format(locale.NATIVE_RECEIVER_BINDING_BLOCKED
                or "Primary and fallback shortcuts are unavailable; input is blocked. Primary: %s (binding action: %s); fallback: %s (binding action: %s). Existing bindings were preserved.",
                nativeFailureText(primaryFailure, nativePrimary), primaryDetail or unknown,
                nativeFailureText(failure, nativeFallback), detail or unknown)
        end
        print("|cffd83b4eLychee Dev:|r " .. text)
    end
    return profile, failure
end
local function stored()
    if ns.SlotRuntime then return copy(defaults) end
    local state, failure = ns.Persistence.Current()
    if not state then return nil, failure end
    local profile = state.options and state.options.receiverBindings
    if profile == nil then return copy(defaults) end
    return validate(profile)
end
local function current()
    if not effective then return nil, "receiver_bindings_unavailable" end
    if type(GetBindingAction) ~= "function" then return nil, "receiver_bindings_unavailable" end
    for _, action in ipairs(actions) do
        local ok, resolved = pcall(GetBindingAction, effective[action], true)
        if not ok or restricted(resolved) or type(resolved) ~= "string"
            or not buttons or not buttons[action]
            or resolved ~= bindingAction(action) then
            return nil, "receiver_binding_ineffective:" .. action .. ":" .. effective[action]
        end
    end
    return copy(effective)
end
local function defer(event)
    owner:RegisterEvent(event)
    owner:SetScript("OnEvent", function(self)
        self:UnregisterAllEvents(); self:SetScript("OnEvent", nil)
        local profile, failure = stored()
        local ready
        if profile then
            if ns.SlotRuntime then ready, failure = applyNative(true)
            else ready, failure = apply(profile) end
        end
        if not ready and failure == "receiver_combat" then defer("PLAYER_REGEN_ENABLED")
        elseif not ready then ns.Startup.receiverFailure = failure end
    end)
    return true
end

ns.ReceiverBindings = {
    Defaults = function() return copy(defaults) end,
    Current = current,
    Display = function(action)
        local profile = current()
        if not profile or not profile[action] then return nil end
        return string.gsub(profile[action], "ALT%-CTRL%-SHIFT%-", "Ctrl+Alt+Shift+")
            :gsub("ALT%-CTRL%-", "Ctrl+Alt+")
    end,
    MatchKey = function(key, action)
        if restricted(key) or type(key) ~= "string" or not effective or not effective[action] then return false end
        return key == terminal(effective[action])
    end,
    ConfigureProfile = function(profile)
        if ns.SlotRuntime then return nil,"receiver_bindings_fixed" end
        if ns.Receiver and ns.Receiver.IsActive and ns.Receiver.IsActive() then return nil,"receiver_active" end
        if not owner then return nil,"receiver_bindings_unavailable" end
        local state,failure=ns.Persistence.Current()
        if not state then return nil,failure end
        local applied
        applied,failure=apply(profile)
        if not applied then return nil,failure end
        state.options.receiverBindings=copy(applied)
        return copy(applied)
    end,
    Configure = function(action, chord)
        if ns.SlotRuntime then return nil,"receiver_bindings_fixed" end
        if action ~= "wake" and action ~= "submit" and action ~= "close" then return nil, "receiver_binding_action_invalid" end
        if ns.Receiver and type(ns.Receiver.IsActive) == "function" and ns.Receiver.IsActive() then
            return nil, "receiver_active"
        end
        if not owner then return nil, "receiver_bindings_unavailable" end
        local state, failure = ns.Persistence.Current()
        if not state then return nil, failure end
        local proposed = copy(effective or defaults)
        proposed[action] = chord
        local applied
        applied, failure = apply(proposed)
        if not applied then return nil, failure end
        state.options = state.options or {}
        state.options.receiverBindings = applied
        return copy(applied)
    end,
    Reset = function()
        if ns.SlotRuntime and ns.SlotRuntime.IsActive and ns.SlotRuntime.IsActive() then
            return nil, "receiver_active"
        end
        if ns.Receiver and type(ns.Receiver.IsActive) == "function" and ns.Receiver.IsActive() then
            return nil, "receiver_active"
        end
        if not owner then return nil, "receiver_bindings_unavailable" end
        -- Native keys are fixed; explicit recovery reapplies only our runtime
        -- overrides and preserves the saved legacy profile.
        if ns.SlotRuntime then return applyNative(true) end
        local state, failure = ns.Persistence.Current()
        if not state then return nil, failure end
        local applied
        applied, failure = apply(defaults)
        if not applied then return nil, failure end
        state.options = state.options or {}
        state.options.receiverBindings = nil
        return copy(applied)
    end,
    Register = function(callbacks)
        if owner then
            local profile, failure = current()
            return profile and true or nil, failure
        end
        if not ns.Startup.ready then return nil, "addon_not_ready" end
        if type(callbacks) ~= "table" or type(callbacks.wake) ~= "function"
            or type(callbacks.submit) ~= "function" or type(callbacks.close) ~= "function" then
            return nil, "receiver_binding_callbacks_invalid"
        end
        if type(SetOverrideBindingClick) ~= "function" or type(CreateFrame) ~= "function" then
            return nil, "receiver_bindings_unavailable"
        end
        if ns.SlotRuntime then
            defaults = copy(nativePrimary)
        end
        handlers = callbacks
        owner = CreateFrame("Frame", nil, UIParent)
        local loggedIn = true
        if type(IsLoggedIn) == "function" then
            local ok, result = pcall(IsLoggedIn)
            loggedIn = ok and not restricted(result) and result == true
        end
        if not loggedIn then return defer("PLAYER_LOGIN") end
        local profile, failure = stored()
        if not profile then return nil, failure end
        local ready
        if ns.SlotRuntime then ready, failure = applyNative(true)
        else ready, failure = apply(profile) end
        if not ready and failure == "receiver_combat" then return defer("PLAYER_REGEN_ENABLED") end
        return ready and true or nil, failure
    end,
}
