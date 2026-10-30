local root = assert(arg[1])
local frames, player, saved, failKey, combat, loggedIn
local bindingSet, savedSet, writes, changeSetOnWrite = 2, nil, 0, false
local secretSet = {}
issecretvalue = function(value) return value == secretSet end
local function emit(event)
    for _, frame in ipairs(frames) do
        if frame.events[event] and frame.OnEvent then frame.OnEvent(frame, event) end
    end
end
UIParent = {}
InCombatLockdown = function() return combat end
IsLoggedIn = function() return loggedIn end
GetBindingAction = function(key) return player[key] or "" end
GetBindingKey = function(action)
    for key, bound in pairs(player) do if bound == action then return key end end
end
SetBinding = function(key, action)
    if key == failKey then return false end
    writes = writes + 1
    player[key] = action
    if changeSetOnWrite then bindingSet = 0 end
    emit("UPDATE_BINDINGS")
    return true
end
GetCurrentBindingSet = function() return bindingSet end
SaveBindings = function(set)
    assert(set == 1 or set == 2, "Usage: SaveBindings(1|2)")
    saved, savedSet = saved + 1, set
    emit("UPDATE_BINDINGS")
end
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

-- A not-yet-loaded binding set must not leave unsaved defaults in memory.
-- Both event paths retry the current profile, preserve player edits and stop
-- listening before SetBinding/SaveBindings synchronously emit UPDATE_BINDINGS.
for _, invalid in ipairs({ {value=0}, {}, {value=3}, {value="2"}, {value=secretSet} }) do
    for _, event in ipairs({ "BINDINGS_LOADED", "UPDATE_BINDINGS" }) do
        frames, player, saved, writes, failKey = {}, {F1="PLAYER_ACTION"}, 0, 0, nil
        combat, loggedIn, bindingSet = false, false, invalid.value
        local pending = load()
        local published = 0
        pending.Bootstrap = {Publish=function() published = published + 1 end}
        assert(pending.ReceiverBindings.Register(callbacks))
        loggedIn = true
        emit("PLAYER_LOGIN")
        assert(writes == 0 and saved == 0 and player.F1 == "PLAYER_ACTION")
        assert(frames[1].events.BINDINGS_LOADED and frames[1].events.UPDATE_BINDINGS)
        emit(event) -- Still invalid: no writes and one waiting frame.
        assert(writes == 0 and saved == 0 and #frames == 1)
        assert(pending.Startup.receiverFailure == "receiver_binding_set_unavailable")
        -- A binding supplied by the player while waiting remains authoritative.
        player["ALT-CTRL-F5"] = "LYCHEEDEV_WAKE"
        bindingSet = event == "BINDINGS_LOADED" and 1 or 2
        local selected = bindingSet
        changeSetOnWrite = true
        emit(event)
        changeSetOnWrite = false
        assert(writes == 1 and saved == 1 and savedSet == selected)
        assert(player["ALT-CTRL-F5"] == "LYCHEEDEV_WAKE" and not player["ALT-CTRL-F12"])
        assert(player["ALT-CTRL-F11"] == "LYCHEEDEV_RELOAD" and player.F1 == "PLAYER_ACTION")
        assert(not pending.Startup.receiverFailure and published == 3)
        assert(next(frames[1].events) == nil and not frames[1].OnEvent and not frames[1].OnUpdate)
        emit(event)
        assert(saved == 1 and writes == 1)
    end
end

-- Startup after login can wait for the binding set, then combat, then the set
-- again. No mutation is allowed until both conditions are satisfied.
frames, player, saved, writes, failKey = {}, {}, 0, 0, nil
combat, loggedIn, bindingSet = false, true, 0
ns = load()
assert(ns.ReceiverBindings.Register(callbacks))
combat, bindingSet = true, 2
emit("BINDINGS_LOADED")
assert(frames[1].events.PLAYER_REGEN_ENABLED and not frames[1].events.UPDATE_BINDINGS)
combat, bindingSet = false, 0
emit("PLAYER_REGEN_ENABLED")
assert(frames[1].events.BINDINGS_LOADED and writes == 0 and saved == 0)
bindingSet = 2
emit("UPDATE_BINDINGS")
assert(writes == 2 and saved == 1 and savedSet == 2)
assert(next(frames[1].events) == nil and not frames[1].OnEvent)

-- Manual reset reports an invalid set without writing. Once it is valid,
-- reset also retires any pending startup listener before synchronous events.
frames, player, saved, writes = {}, {}, 0, 0
bindingSet = 0
ns = load()
local reset, reason = ns.ReceiverBindings.Reset()
assert(not reset and reason == "receiver_binding_set_unavailable" and writes == 0 and saved == 0)
assert(ns.ReceiverBindings.Register(callbacks))
emit("UPDATE_BINDINGS")
assert(ns.Startup.receiverFailure == "receiver_binding_set_unavailable")
bindingSet = 1
assert(ns.ReceiverBindings.Reset())
assert(writes == 2 and saved == 1 and savedSet == 1)
assert(next(frames[1].events) == nil and not frames[1].OnEvent)
assert(not ns.Startup.receiverFailure)

-- Existing assignments do not require a writable binding set or a new save.
bindingSet = 0
assert(ns.ReceiverBindings.Reset())
assert(writes == 2 and saved == 1)
print("receiver bindings: saved actions, deferred lifecycle and assignment rollback")
