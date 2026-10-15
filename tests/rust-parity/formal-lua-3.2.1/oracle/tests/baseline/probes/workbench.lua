-- Functional page activation only. Visual quality requires separate real-client review.
local probe = ...
assert(probe:Async(60))
local ns = LycheeDevInternal
assert(ns and ns.Startup.ready, "addon_not_ready")
assert(ns.Persistence.Bridge() == LycheeToolkitBridgeDB, "wrong_report_scope")
local timers, results = {}, {}
local pages = { "runner", "objects", "events", "trace", "diagnostics", "exports", "automation", "about" }
local function clear()
    assert(not ns.InputProtection.IsActive(), "input_protected_during_execution")
end
clear()
assert(probe:OnCleanup(function()
    for _, timer in ipairs(timers) do timer:Cancel() end
    ns.Workbench.Close()
end))
for index, key in ipairs(pages) do
    timers[#timers + 1] = C_Timer.NewTimer(index * 5, assert(probe:Callback(function()
        clear()
        assert(ns.Workbench.ShowPage(key))
        assert(ns.Workbench.GetActivePage() == key)
        results[#results + 1] = key
    end)))
end
timers[#timers + 1] = C_Timer.NewTimer(45, assert(probe:Callback(function()
    clear()
    local profile = assert(ns.ReceiverBindings.Current())
    for _, action in ipairs({ "wake", "reload" }) do
        assert(GetBindingAction(profile[action], true) == "LYCHEEDEV_" .. string.upper(action), "binding_action_mismatch")
    end
    results[#results + 1] = "native_bindings"
end)))
timers[#timers + 1] = C_Timer.NewTimer(55, assert(probe:Callback(function()
    clear()
    probe:Finish({ passed = true, baseline = "LIVE-04", pages = results, scope = "character-v1", unobstructed = true })
end)))
