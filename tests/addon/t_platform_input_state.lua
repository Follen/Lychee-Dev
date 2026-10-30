local Env = ...
local originalLogin, originalCombat, originalFocus = IsLoggedIn, InCombatLockdown, GetCurrentKeyBoardFocus
local loggedIn, combat, focus = true, false, nil
local counts = { login = 0, combat = 0, focus = 0 }
IsLoggedIn = function() counts.login = counts.login + 1; return loggedIn end
InCombatLockdown = function() counts.combat = counts.combat + 1; return combat end
GetCurrentKeyBoardFocus = function() counts.focus = counts.focus + 1; return focus end
local ns, frames = Env.NewNamespace(), Env.framesCreated
Env.LoadAddon(Env.ClientProfilePath(), ns)
Env.LoadAddon("Core/Platform.lua", ns)
assert(counts.login == 0 and counts.combat == 0 and counts.focus == 0,
    "loading platform observed input before a request")
assert(ns.Platform.ObserveInputState() == true)
loggedIn = false
local ready, reason = ns.Platform.ObserveInputState()
assert(ready == false and reason == "input_not_logged_in" and counts.combat == 1 and counts.focus == 1)
loggedIn, combat = true, true
ready, reason = ns.Platform.ObserveInputState()
assert(ready == false and reason == "input_combat_lockdown" and counts.focus == 1)
combat, focus = false, {}
ready, reason = ns.Platform.ObserveInputState()
assert(ready == false and reason == "input_keyboard_focus")
assert(ns.Platform.ObserveInputState(true) == true, "non-text slot input rejected an ordinary editor")
combat = true
assert(ns.Platform.ObserveInputState(true) == false, "focused slot mode bypassed combat")
combat, focus = false, nil
assert(ns.Platform.ObserveInputState() == true)

local secret = Env.MakeSecret()
for _, value in ipairs({ secret, "unexpected", 42, {} }) do
    loggedIn = value
    ready, reason = ns.Platform.ObserveInputState()
    assert(ready == nil and reason == "input_login_unavailable")
    loggedIn, combat = true, value
    ready, reason = ns.Platform.ObserveInputState()
    assert(ready == nil and reason == "input_combat_unavailable")
    combat = false
end
focus = secret
ready, reason = ns.Platform.ObserveInputState()
assert(ready == nil and reason == "input_focus_unavailable")
assert(ns.Platform.ObserveInputState(true) == nil, "focused slot mode bypassed secret focus")
focus = nil
for _, api in ipairs({
    { "IsLoggedIn", "input_login_unavailable" },
    { "InCombatLockdown", "input_combat_unavailable" },
    { "GetCurrentKeyBoardFocus", "input_focus_unavailable" },
}) do
    local original = _G[api[1]]
    _G[api[1]] = function() error("restricted API fixture") end
    ready, reason = ns.Platform.ObserveInputState()
    assert(ready == nil and reason == api[2])
    _G[api[1]] = nil
    ready, reason = ns.Platform.ObserveInputState()
    assert(ready == nil and reason == "input_observation_unavailable")
    _G[api[1]] = original
end
assert(Env.framesCreated == frames, "input observation allocated UI")
IsLoggedIn, InCombatLockdown, GetCurrentKeyBoardFocus = originalLogin, originalCombat, originalFocus
print("platform input: current client, focus, restricted APIs and fail-closed observations")
