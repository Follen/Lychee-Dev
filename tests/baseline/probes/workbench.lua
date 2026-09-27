-- Functional page activation only. Visual quality requires separate WGC review.
local probe = ...
assert(probe:Async(60))
local ns = LycheeDevInternal
assert(ns and ns.Startup.ready, "addon_not_ready")
assert(ns.Persistence.Bridge() == LycheeToolkitBridgeDB, "wrong_report_scope")
local timers, results = {}, {}
local pages = { "runner", "objects", "events", "trace", "diagnostics", "exports", "automation", "about" }
local function clear()
    assert(not ns.Receiver.IsActive(), "receiver_active_during_execution")
    assert(ns.ReceiptView.Current() == nil, "optical_overlay_during_execution")
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
    assert(ns.Workbench.ShowSettings())
    local page = assert(ns.Workbench.GetSettingsPage())
    for _, action in ipairs({ "wake", "submit", "close" }) do
        assert(page.bindings[action].button.label:GetText() == ns.ReceiverBindings.Display(action), "binding_display_mismatch")
    end
    results[#results + 1] = "settings"
end)))
timers[#timers + 1] = C_Timer.NewTimer(55, assert(probe:Callback(function()
    clear()
    probe:Finish({ passed = true, baseline = "LIVE-04", pages = results, scope = "character-v1", unobstructed = true })
end)))
