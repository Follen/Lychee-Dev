local root = assert(arg[1])
local frames, player, saved, failKey, combat, loggedIn
UIParent = {}
InCombatLockdown = function() return combat end
IsLoggedIn = function() return loggedIn end
GetBindingAction = function(key) return player[key] or "" end
GetBindingKey = function(action)
    for key, bound in pairs(player) do if bound == action then return key end end
end
SetBinding = function(key, action)
    if key == failKey then return false end
    player[key] = action
    return true
end
GetCurrentBindingSet = function() return 2 end
SaveBindings = function(set) assert(set == 2); saved = saved + 1 end
SetOverrideBindingClick = function() error("native actions must use real bindings") end
CreateFrame = function(kind)
    assert(kind == "Frame")
    local frame = { events = {} }
    function frame:RegisterEvent(event) self.events[event] = true end
    function frame:UnregisterAllEvents() self.events = {} end
    function frame:SetScript(event, callback) self[event] = callback end
    frames[#frames + 1] = frame
    return frame
end
local function load()
    local ns = { Startup = {}, L = {
        NATIVE_RECEIVER_BINDING_WAKE = "Execute CLI command",
        NATIVE_RECEIVER_BINDING_RELOAD = "Reload UI",
    } }
    assert(loadfile(root .. "/Bridge/ReceiverBindings.lua"))("Lychee Dev", ns)
    return ns
end
local callbacks = { wake = function() end, reload = function() end }
-- Login/combat deferral is one-shot. Once registered, there are no idle events,
-- timers, key capture frames or hidden override buttons.
for _, scenario in ipairs({ "ready", "login", "combat", "combat-login" }) do
    frames, player, saved, failKey = {}, {}, 0, nil
    combat = scenario == "combat" or scenario == "combat-login"
    loggedIn = scenario == "ready" or scenario == "combat"
    local ns = load()
    assert(#frames == 0)
    assert(ns.ReceiverBindings.Register(callbacks))
    if not loggedIn then
        assert(#frames == 1 and frames[1].events.PLAYER_LOGIN and saved == 0)
        loggedIn = true
        frames[1].OnEvent(frames[1], "PLAYER_LOGIN")
    end
    if combat then
        assert(#frames == 1 and frames[1].events.PLAYER_REGEN_ENABLED and saved == 0)
        combat = false
        frames[1].OnEvent(frames[1], "PLAYER_REGEN_ENABLED")
    end
    local profile = assert(ns.ReceiverBindings.Current())
    assert(profile.wake == "ALT-CTRL-F12" and profile.reload == "ALT-CTRL-F11")
    assert(saved == 1 and #frames == (scenario == "ready" and 0 or 1))
    for _, frame in ipairs(frames) do
        assert(next(frame.events) == nil and not frame.OnEvent and not frame.OnUpdate)
    end
    assert(ns.ReceiverBindings.Register(callbacks) and saved == 1)
end
-- A rejected second assignment rolls back only the new first assignment and
-- never persists a partial profile or touches an unrelated player binding.
frames, player, saved, combat, loggedIn = {}, { F1 = "PLAYER_ACTION" }, 0, false, true
failKey = "ALT-CTRL-F11"
local ns = load()
assert(not ns.ReceiverBindings.Register(callbacks))
assert(player.F1 == "PLAYER_ACTION" and not player["ALT-CTRL-F12"] and saved == 0)
failKey = nil
assert(ns.ReceiverBindings.Reset() and saved == 1)
print("receiver bindings: saved actions, deferred lifecycle and assignment rollback")
