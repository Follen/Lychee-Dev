-- Execute the documented manual probes against current addon modules. This is
-- an offline API contract, not a substitute for real-client UI acceptance.
local Env, client, addonRoot = ...
local probes = addonRoot .. "/../tests/baseline/probes/"
local timers, frames = {}, {}
Env.SetNow(100)
local originalCreate = CreateFrame
CreateFrame = function(...)
    local frame = originalCreate(...)
    frames[#frames + 1] = frame
    return frame
end
C_Timer.NewTimer = function(seconds, callback)
    local timer = { at = GetTime() + seconds, callback = callback }
    function timer:Cancel() self.cancelled = true end
    timers[#timers + 1] = timer
    return timer
end
C_Timer.After = function(seconds, callback) C_Timer.NewTimer(seconds, callback) end
local bindings = { ["ALT-CTRL-F8"] = "LYCHEEDEV_WAKE", ["ALT-CTRL-F7"] = "LYCHEEDEV_RELOAD" }
local overrides = {}
function GetBindingKey(action)
    for key, value in pairs(bindings) do if value == action then return key end end
end
function GetBindingAction(key, effective)
    return (effective and overrides[key]) or bindings[key] or ""
end
function SetBinding() error("existing player bindings must be retained") end
function SaveBindings() error("existing player bindings must not be rewritten") end
function GetCurrentBindingSet() return 1 end
function IsLoggedIn() return true end
function UnitFullName() return "Probe", "Fixture" end
function UnitGUID() return "Player-1" end
function GetCurrentKeyBoardFocus()
    for _, frame in ipairs(frames) do if frame.focused then return frame end end
end
local ns, loader = {}
for line in io.lines(addonRoot .. "/Lychee Dev.toc") do
    if line ~= "" and line:sub(1, 1) ~= "#" then
        local source = line:gsub("\\", "/")
        Env.LoadAddon(source, ns)
        if source == "Core/Runtime.lua" then loader = frames[#frames] end
    end
end
Env.FireScript(assert(loader), "OnEvent", "ADDON_LOADED", "Lychee Dev")
assert(ns.Startup.ready, ns.Startup.reason)
assert(not ns.Receiver and not ns.ReceiptView and not ns.Workbench.ShowSettings)
assert(ns.Persistence.Bridge() == LycheeToolkitBridgeDB)
local function run(name)
    local outcome
    local firstTimer = #timers + 1
    ns.ProbeExecution.Run(assert(loadfile(probes .. name .. ".lua")), 120, function(ok, value, metadata)
        assert(not outcome, "probe completed twice")
        outcome = { ok = ok, value = value, metadata = metadata }
    end)
    local deadline = GetTime() + 120
    while not outcome do
        local nextTimer
        for _, timer in ipairs(timers) do
            if not timer.cancelled and not timer.fired and (not nextTimer or timer.at < nextTimer.at) then
                nextTimer = timer
            end
        end
        assert(nextTimer and nextTimer.at <= deadline, name .. ": no bounded completion")
        Env.SetNow(nextTimer.at)
        nextTimer.fired = true
        nextTimer.callback()
    end
    assert(outcome.metadata.resourcesReleased, name .. ": cleanup failed")
    for index = firstTimer, #timers do
        assert(timers[index].cancelled or timers[index].fired, name .. ": retained a timer")
    end
    return outcome
end
local function succeeds(name)
    local outcome = run(name)
    assert(outcome.ok, name .. ": " .. tostring(outcome.value))
    assert(outcome.value.passed, name .. ": missing passed result")
    return outcome.value
end
local function fails(name, reason)
    local outcome = run(name)
    assert(not outcome.ok and type(outcome.value) == "string"
        and outcome.value:find(reason, 1, true), name .. ": " .. tostring(outcome.value))
end
local sync = succeeds("sync")
assert(sync.baseline == "LIVE-01" and sync.sum == 55)
fails("failure", "BASELINE_EXPECTED_FAILURE")
local workbench = succeeds("workbench")
assert(table.concat(workbench.pages, ",") == "runner,objects,events,trace,diagnostics,exports,automation,about,native_bindings")
for _, key in ipairs(workbench.pages) do
    if key ~= "native_bindings" then assert(ns.Workbench.GetPage(key), "missing real page: " .. key) end
end
assert(workbench.unobstructed and workbench.scope == "character-v1" and not LycheeToolkitWindow:IsShown())
local ticket = string.rep("a", 32)
assert(ns.AutomationHistory.Observe({ state = "running", envelope = {
    ticket = ticket, nonce = string.rep("b", 32), codeBytes = 9, codeChecksum = 42,
} }))
ns.ActivityView.Begin(ticket)
local activity = succeeds("ui_activity")
assert(table.concat(activity.pages, ",") == "events-empty,events-selected,diagnostics,automation-result,automation-details,native-bindings,probe-indicator-only")
assert(activity.probeActivity and activity.inputReleased and activity.nativeBindings)
assert(not LycheeToolkitWindow:IsShown() and ns.ActivityView.Current() == "probe")
ns.ActivityView.Stop()

-- Another owner's lease must be detected even though this probe owns none.
for _, name in ipairs({ "sync", "failure", "workbench", "ui_activity" }) do
    local lease = assert(ns.InputProtection.Acquire(2))
    fails(name, "input_protected_during_execution")
    assert(ns.InputProtection.IsActive(), "probe released another owner's lease")
    assert(ns.InputProtection.Release(lease))
end
fails("ui_activity", "probe_activity_lost")
for _, name in ipairs({ "workbench", "ui_activity" }) do
    ns.ActivityView.Begin(ticket)
    overrides["ALT-CTRL-F7"] = "OTHER_ACTION"
    fails(name, "receiver_binding_unavailable:reload")
    overrides["ALT-CTRL-F7"] = nil
    ns.ActivityView.Stop()
end
assert(not ns.InputProtection.IsActive() and not LycheeToolkitWindow:IsShown())
for _, timer in ipairs(timers) do assert(timer.cancelled or timer.fired, "probe retained a timer") end
CreateFrame = originalCreate
print("baseline probe contracts ok: real pages, expected failure, activity and effective bindings [" .. client .. "]")
