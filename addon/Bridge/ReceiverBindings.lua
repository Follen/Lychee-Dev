local _, ns = ...

-- Real Bindings.xml actions: defaults are assigned only to free keys. The
-- current binding set remains authoritative, including changes in Settings.
local actions = { "wake", "reload" }
local defaults = { wake = "ALT-CTRL-F12", reload = "ALT-CTRL-F11" }
local fallback = { wake = "ALT-CTRL-F10", reload = "ALT-CTRL-F9" }
local handlers, deferred
_G.BINDING_HEADER_LYCHEEDEV = "Lychee Dev"
_G.BINDING_NAME_LYCHEEDEV_WAKE = ns.L.NATIVE_RECEIVER_BINDING_WAKE
_G.BINDING_NAME_LYCHEEDEV_RELOAD = ns.L.NATIVE_RECEIVER_BINDING_RELOAD
local function restricted(value) return issecretvalue and issecretvalue(value) end
local function actionName(action) return "LYCHEEDEV_" .. string.upper(action) end
local function validChord(chord)
    if restricted(chord) or type(chord) ~= "string" then return false end
    local key = chord:match("^ALT%-CTRL%-F(%d+)$")
    return key ~= nil and tonumber(key) >= 1 and tonumber(key) <= 12 and tostring(tonumber(key)) == key
end
local function noncombat()
    if type(InCombatLockdown) ~= "function" then return nil, "receiver_combat_unavailable" end
    local combat = InCombatLockdown()
    if restricted(combat) or type(combat) ~= "boolean" then return nil, "receiver_combat_unavailable" end
    if combat then return nil, "receiver_combat" end
    return true
end
local function current()
    if type(GetBindingKey) ~= "function" or type(GetBindingAction) ~= "function" then
        return nil, "receiver_bindings_unavailable"
    end
    local profile = {}
    for _, action in ipairs(actions) do
        local name = actionName(action)
        for _, key in ipairs({ GetBindingKey(name) }) do
            if validChord(key) and GetBindingAction(key, true) == name then profile[action] = key; break end
        end
        if not profile[action] then return nil, "receiver_binding_unavailable:" .. action end
    end
    return profile
end
local function stopWaiting()
    if deferred then deferred:UnregisterAllEvents(); deferred:SetScript("OnEvent", nil) end
end
local function installDefaults()
    local ready, reason = noncombat()
    if not ready then return nil, reason end
    if type(SetBinding) ~= "function" or type(GetBindingKey) ~= "function"
        or type(GetBindingAction) ~= "function" or type(SaveBindings) ~= "function"
        or type(GetCurrentBindingSet) ~= "function" then return nil, "receiver_bindings_unavailable" end
    local pending = {}
    for _, action in ipairs(actions) do
        -- An existing assignment belongs to the user, even if unsupported or
        -- currently masked by another addon's override. Never replace it.
        if not GetBindingKey(actionName(action)) then
            for _, profile in ipairs({ defaults, fallback }) do
                local key = profile[action]
                local bound, effective = GetBindingAction(key, false), GetBindingAction(key, true)
                if (not bound or bound == "" or bound == "NONE")
                    and (not effective or effective == "" or effective == "NONE") then
                    pending[#pending + 1] = { key = key, action = actionName(action) }
                    break
                end
            end
        end
    end
    -- Bindings can still be loading at PLAYER_LOGIN. Resolve the player's
    -- writable set before any mutation; never save defaults to a guessed set.
    local bindingSet
    if #pending > 0 then
        bindingSet = GetCurrentBindingSet()
        if restricted(bindingSet) or (bindingSet ~= 1 and bindingSet ~= 2) then
            return nil, "receiver_binding_set_unavailable"
        end
    end
    -- SetBinding/SaveBindings can synchronously emit UPDATE_BINDINGS, including
    -- when a manual reset completes an outstanding startup wait.
    stopWaiting()
    local assigned = {}
    for _, entry in ipairs(pending) do
        if not SetBinding(entry.key, entry.action) then
            for _, key in ipairs(assigned) do SetBinding(key) end
            return nil, "receiver_bindings_unavailable"
        end
        assigned[#assigned + 1] = entry.key
    end
    if #assigned > 0 then SaveBindings(bindingSet) end
    return current()
end
local function defer(event)
    if not deferred then deferred = CreateFrame("Frame") end
    deferred:RegisterEvent(event)
    if event == "BINDINGS_LOADED" then deferred:RegisterEvent("UPDATE_BINDINGS") end
    deferred:SetScript("OnEvent", function()
        stopWaiting()
        local ok, reason = installDefaults()
        if not ok and reason == "receiver_combat" then defer("PLAYER_REGEN_ENABLED")
        elseif not ok and reason == "receiver_binding_set_unavailable" then defer("BINDINGS_LOADED") end
        if ns.Startup then ns.Startup.receiverFailure = reason end
        if ns.Bootstrap then ns.Bootstrap.Publish() end
    end)
    return true
end
ns.ReceiverBindings = {
    Dispatch = function(action)
        if not handlers or not handlers[action] then return nil, "receiver_binding_not_ready" end
        return handlers[action]()
    end,
    Current = current,
    Display = function(action)
        local profile = current()
        return profile and profile[action] and profile[action]:gsub("ALT%-CTRL%-", "Ctrl+Alt+")
    end,
    Reset = function()
        if ns.InputProtection and ns.InputProtection.IsActive() then return nil, "receiver_active" end
        local profile, reason = installDefaults()
        if profile and ns.Startup then ns.Startup.receiverFailure = nil end
        return profile, reason
    end,
    Register = function(callbacks)
        if handlers then local profile, reason = current(); return profile and true or nil, reason end
        if type(callbacks) ~= "table" or type(callbacks.wake) ~= "function" or type(callbacks.reload) ~= "function" then
            return nil, "receiver_binding_callbacks_invalid"
        end
        handlers = callbacks
        if type(IsLoggedIn) == "function" and IsLoggedIn() ~= true then return defer("PLAYER_LOGIN") end
        local profile, reason = installDefaults()
        if not profile and reason == "receiver_combat" then return defer("PLAYER_REGEN_ENABLED") end
        if not profile and reason == "receiver_binding_set_unavailable" then return defer("BINDINGS_LOADED") end
        return profile and true or nil, reason
    end,
}
