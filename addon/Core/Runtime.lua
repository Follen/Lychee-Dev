local ADDON_NAME, ns = ...

ns.Release = "2.0.0-dev"
ns.Startup = { ready = false, reason = "addon_not_loaded" }

-- This one-shot loader is core infrastructure, not an enabled bridge feature.
-- No optional feature creates frames, hooks, timers, or event subscriptions here.
local loader = CreateFrame("Frame")
loader:RegisterEvent("ADDON_LOADED")
loader:SetScript("OnEvent", function(self, event, name)
    if name ~= ADDON_NAME then return end
    self:UnregisterAllEvents()
    self:SetScript("OnEvent", nil)
    local identity, reason = ns.Platform.ObserveBuild()
    if not identity then ns.Startup.reason = reason; return end
    local loaded, failure = ns.Persistence.Load()
    if not loaded then ns.Startup.reason = failure; return end
    ns.Startup = { ready = true, identity = identity }
    local registered, registrationFailure = ns.Controls.Register()
    if not registered then ns.Startup.commandFailure = registrationFailure end
    if registered then
        local resumed, resumeFailure = ns.Reentry.Start(self)
        if not resumed then ns.Startup.resumeFailure = resumeFailure end
    end
end)
