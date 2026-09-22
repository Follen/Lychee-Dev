-- Window shell: zero cost before the first /dev, the eight-tab page registry
-- navigation, tab sizing, lifecycle callbacks, the history rail layout,
-- combat refusal/shutdown and the shared export ticket popup.
local Env, client, root = ...

local ns = Env.LoadWorkbench()
Env.LoadAddon("Core/Controls.lua", ns)
assert(ns.Persistence.Load(), "persistence did not load")
ns.Startup = { ready = true, identity = { product = client, build = "test" } }
assert(ns.Controls.Register(), "slash command was not registered")
assert(SLASH_LYCHEETOOLKIT1 == "/dev", "slash command was not named /dev")

local L = ns.L
local W = ns.Workbench

-- Nothing is built before the first /dev.
assert(Env.framesCreated == 0, "UI created frames before /dev was used")

-- Page API: a probe page fills the "objects" slot and records its lifecycle.
local probe = { build = 0, activate = 0, suspend = 0, shutdown = 0 }
local probeRail
assert(W.RegisterPage({
    key = "objects",
    titleKey = "TAB_OBJECTS",
    rail = true,
    build = function(parent)
        probe.build = probe.build + 1
        probeRail = W.GetHistoryRail()
        local page = CreateFrame("Frame", nil, parent)
        page:SetAllPoints(parent)
        return page
    end,
    activate = function()
        probe.activate = probe.activate + 1
    end,
    suspend = function()
        probe.suspend = probe.suspend + 1
    end,
    shutdown = function()
        probe.shutdown = probe.shutdown + 1
    end,
}), "probe page did not register")

assert(W.RegisterExportHook({
    key = "object_snapshot",
    page = "objects",
    GetPayload = function()
        return "object_snapshot", "Probe snapshot", "snapshot body", { path = "Probe.Path" }
    end,
}), "export hook did not register")

-- Unknown verbs keep their legacy usage error (bridge parsing untouched).
local usageOk, usageError = ns.Controls.Handle("frobnicate")
assert(usageOk == nil and usageError == "usage: /dev status | connect | disconnect",
    "unknown /dev verbs changed their usage contract")

-- First /dev builds the window lazily.
SlashCmdList.LYCHEETOOLKIT("")
assert(Env.framesCreated > 0, "UI did not create frames on first /dev")
local window = assert(LycheeToolkitWindow, "window frame was not created")
assert(window:IsShown(), "window did not open")
assert(UISpecialFrames[1] == "LycheeToolkitWindow", "window was not registered for escape close")
assert(window.width == 1040 and window.height == 720, "window did not use the workbench size")
assert(window:GetFrameStrata() == "DIALOG", "window strata changed")
assert(window.clampedToScreen and window.movable, "window lost its drag/clamp behavior")

-- Eight tabs from the page registry.
local expectedOrder = {
    "runner", "objects", "events", "trace", "diagnostics", "exports", "automation", "about",
}
local tabCount = 0
for _ in pairs(window.pageTabs) do
    tabCount = tabCount + 1
end
assert(tabCount == 8, "window did not create all eight workbench tabs")
assert(window.pageTabs.runner.point[1] == "BOTTOMLEFT", "first tab anchor changed")
assert(window.pageTabs.objects.point[4] == 8, "main navigation did not use a consistent visual gap")

-- Label-fit sizing: width = rendered label + 22 px, minimum 48.
for index = 1, #expectedOrder do
    local tab = window.pageTabs[expectedOrder[index]]
    assert(tab, "missing tab: " .. expectedOrder[index])
    assert(tab:GetHeight() == 32, "tab height changed")
    assert(tab:GetWidth() >= math.max(48, tab.label:GetStringWidth() + 22),
        "main navigation did not preserve text padding")
end
assert(window.pageTabs.exports:GetWidth() > window.pageTabs.runner:GetWidth(),
    "main navigation did not size tabs from their rendered labels")

-- Pages build lazily on first activation.
assert(probe.build == 0, "page was constructed before its first activation")
window.pageTabs.objects:Click()
assert(probe.build == 1 and probe.activate == 1, "page did not build and activate")
assert(W.GetActivePage() == "objects", "active page was not tracked")
assert(probeRail and probeRail.root:IsShown(), "history rail did not follow its page")
assert(probeRail.content:GetWidth() == 206 and probeRail.rowWidth == 206
        and probeRail.rowHeight == 52, "history rail layout changed")

window.pageTabs.about:Click()
assert(probe.suspend == 1, "page was not suspended on navigation")
assert(not probeRail.root:IsShown(), "history rail stayed visible without its page")

window.pageTabs.objects:Click()
assert(probe.build == 1 and probe.activate == 2, "page was reconstructed on reactivation")

-- Shared export hook and ticket popup.
local savedId = W.SaveFromHook()
assert(savedId, "export hook did not save")
local entry = ns.Stores.Exports.Get(savedId)
assert(entry and entry.source.kind == "object_snapshot"
        and entry.source.path == "Probe.Path" and entry.payload.content == "snapshot body",
    "export hook payload was not committed")

local controller = W.GetExportController()
local popup = controller.popup
assert(popup and popup.overlay:IsShown(), "export ticket popup did not open")
assert(popup.ticketBox:GetText() == savedId, "export popup did not show its ticket")
assert(popup.ticketBox.focused and popup.ticketBox.highlighted,
    "export popup did not select the ticket by default")
assert(popup.hint:GetText() == L.EXPORT_TICKET_HELP, "export popup hint changed")

popup.ticketBox:SetText("tampered")
assert(popup.ticketBox:GetText() == savedId, "ticket box accepted edits")

Env.SetInCombat(true)
popup.reloadButton:Click()
assert(not Env.reloadCalled, "reload button ignored combat lockdown")
Env.SetInCombat(false)
popup.reloadButton:Click()
assert(Env.reloadCalled, "export popup reload action did not call ReloadUI")

popup.laterButton:Click()
assert(not popup.overlay:IsShown(), "later button did not close the popup")
assert(W.ShowTicketPopup(savedId) and popup.overlay:IsShown(), "ticket popup could not be reopened")
Env.FireScript(popup.ticketBox, "OnEscapePressed")
assert(not popup.overlay:IsShown(), "Escape did not close the ticket popup")

-- Save errors are printed as data and commit nothing.
Env.prints = {}
assert(W.SaveToDisk("run_result", "empty", "") == nil, "empty export was saved")
assert(W.SaveToDisk("run_result", "broken", function()
    error("boom")
end) == nil, "failing export builder was saved")

-- Window hide runs registered shutdowns and page shutdowns.
local shutdownCalls = 0
W.RegisterShutdown(function()
    shutdownCalls = shutdownCalls + 1
end)
SlashCmdList.LYCHEETOOLKIT("")
assert(not window:IsShown(), "second /dev did not close the window")
assert(shutdownCalls == 1 and probe.shutdown == 1, "hide did not stop owned runtime tools")

SlashCmdList.LYCHEETOOLKIT("")
assert(window:IsShown(), "window did not reopen")

-- Combat shutdown hides the window and refuses new opens.
Env.SetInCombat(true)
ns.Safety.RunCombatShutdown()
assert(not window:IsShown(), "combat shutdown did not close the window")
Env.prints = {}
SlashCmdList.LYCHEETOOLKIT("")
assert(not window:IsShown(), "/dev opened the window during combat")
assert(#Env.prints == 1 and Env.prints[1]:find(L.COMBAT_BLOCKED, 1, true),
    "combat refusal was not reported")
local toggled, toggleReason = W.Toggle()
assert(toggled == false and toggleReason == L.COMBAT_BLOCKED,
    "workbench toggle ignored combat lockdown")
Env.SetInCombat(false)

-- The combat driver frame is created once, at first registration only.
local framesBefore = Env.framesCreated
ns.Safety.RegisterCombatShutdown(function() end)
assert(Env.framesCreated == framesBefore, "combat driver was recreated")

assert(W.Toggle(), "workbench did not reopen after combat")
assert(window:IsShown(), "window did not reopen after combat")

print("shell ok")
