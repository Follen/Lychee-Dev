local ADDON_NAME, ns = ...

ns.Release = "2.0.2"
ns.Startup = { ready = false, reason = "addon_not_loaded" }
-- Observability handle: mirrors the addon namespace for host-side /run
-- introspection (startup state, identity, commandFailure) on every client.
_G.LycheeDevInternal = ns

-- This one-shot loader is core infrastructure, not an enabled bridge feature.
-- No optional feature creates frames, hooks, timers, or event subscriptions here.
--
-- The name-matched ADDON_LOADED owns startup, success or not; PLAYER_LOGIN is
-- the fallback trigger when that event never matches (some engines signal
-- ADDON_LOADED under a modified internal name). Every distinct startup
-- failure is reported to chat exactly once, so a stalled runtime is never
-- invisible again.
local started = false
local printed = {}
local loader

local function report(trigger, failure)
    if printed[failure] then return end
    printed[failure] = true
    print("|cffd83b4eLychee Dev:|r startup via " .. trigger .. " failed: " .. tostring(failure))
end

local function start(trigger)
    if started then return true end
    local identity, reason = ns.Platform.ObserveBuild()
    if not identity then
        ns.Startup = { ready = false, reason = reason }
        report(trigger, reason)
        return false
    end
    local loaded, failure = ns.Persistence.Load()
    if not loaded then
        ns.Startup = { ready = false, reason = failure }
        report(trigger, failure)
        return false
    end
    ns.Startup = { ready = true, identity = identity }
    started = true
    local registered, registrationFailure = ns.Controls.Register()
    if not registered then ns.Startup.commandFailure = registrationFailure end
    if registered then
        local resumed, resumeFailure = ns.Reentry.Start(loader)
        if not resumed then ns.Startup.resumeFailure = resumeFailure end
    end
    return true
end

loader = CreateFrame("Frame")
loader:RegisterEvent("ADDON_LOADED")
loader:RegisterEvent("PLAYER_LOGIN")
loader:SetScript("OnEvent", function(self, event, name)
    if event == "ADDON_LOADED" and name ~= ADDON_NAME then return end
    -- One-shot: the name-matched ADDON_LOADED owns startup, success or not;
    -- PLAYER_LOGIN is the fallback trigger when that event never matched.
    self:UnregisterAllEvents()
    self:SetScript("OnEvent", nil)
    start(event)
end)
