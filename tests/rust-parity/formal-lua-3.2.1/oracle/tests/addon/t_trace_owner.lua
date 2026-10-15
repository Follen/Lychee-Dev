-- R3-01: typed non-table owners fail through the real Trace button, not Lua.
local Env = ...
local ns = Env.LoadWorkbench()
Env.LoadAddon("Modules/ObjectInspector.lua", ns)
Env.LoadAddon("Modules/FunctionTrace.lua", ns)
Env.LoadAddon("UI/Pages/Trace.lua", ns)
assert(ns.Persistence.Load())
assert(Env.framesCreated == 0 and #Env.pendingTimers == 0,
    "unused Trace created runtime work")

local hookCount = 0
function hooksecurefunc(owner, key, callback)
    assert(type(owner[key]) == "function")
    hookCount = hookCount + 1
end

assert(ns.Workbench.ShowPage("trace"))
local page = assert(ns.Workbench.GetPage("trace"))
local input
for _, child in ipairs(page.children) do
    if child.editBox and child.editBox:GetText() == "C_AddOns.GetAddOnInfo" then
        input = child.editBox
    end
end
assert(input, "real Trace input was not found")
local function HasStatus(message)
    for _, child in ipairs(page.children) do
        if child:GetObjectType() == "FontString" and child:GetText() == message then return true end
    end
    return false
end

TraceOwnerNumber, TraceOwnerBoolean, TraceOwnerString = 7, false, "text"
TraceOwnerSecret = Env.MakeSecret()
TraceOwnerSecretTarget = { foo = Env.MakeSecret() }
TraceOwnerUnreadable = setmetatable({}, { __index = function() error("unreadable member") end })
TraceOwnerUserdata = newproxy(true)
getmetatable(TraceOwnerUserdata).__index = { foo = function() end }
for _, path in ipairs({ "GetTime.foo", "TraceOwnerNumber.foo", "TraceOwnerBoolean.foo", "TraceOwnerString.foo", "TraceOwnerUnreadable.foo", "TraceOwnerSecretTarget.foo" }) do
    input:SetText(path)
    local frames, timers = Env.framesCreated, #Env.pendingTimers
    local clicked, failure = pcall(function() page.traceButton:Click() end)
    assert(clicked, "Trace click leaked an invalid owner error: " .. tostring(failure))
    assert(HasStatus(ns.L.FUNCTION_NOT_FOUND), "invalid owner did not reach the page failure status")
    assert(not ns.FunctionTrace.IsRunning() and ns.FunctionTrace.GetActivePath() == nil,
        "invalid owner started Trace")
    assert(page.traceButton.label:GetText() == ns.L.START_TRACE and page.traceButton.variant == "primary",
        "invalid owner changed the Trace button state")
    assert(hookCount == 0 and Env.framesCreated == frames and #Env.pendingTimers == timers,
        "invalid owner installed a hook or runtime work")
end
input:SetText("TraceOwnerSecret.foo")
page.traceButton:Click()
assert(HasStatus(ns.L.SECRET_VALUE_BLOCKED) and hookCount == 0,
    "secret owner was read or hooked")

TraceOwnerTable = { Method = function() return true end }
input:SetText("TraceOwnerTable.Method")
page.traceButton:Click()
assert(ns.FunctionTrace.IsRunning() and ns.FunctionTrace.GetActivePath() == "TraceOwnerTable.Method"
    and hookCount == 1 and page.traceButton.label:GetText() == ns.L.STOP_TRACE,
    "normal table method no longer starts through the Trace page")
page.traceButton:Click()
assert(not ns.FunctionTrace.IsRunning() and page.traceButton.label:GetText() == ns.L.START_TRACE)
input:SetText("UIParent.Show")
page.traceButton:Click()
assert(ns.FunctionTrace.IsRunning() and ns.FunctionTrace.GetActivePath() == "UIParent.Show" and hookCount == 2,
    "table-backed frame methods no longer start through the Trace page")
page.traceButton:Click()
-- Keep indexable owners on the existing protected hook-install path. Client
-- hook support is still decided by hooksecurefunc, not a new Lua type filter.
input:SetText("TraceOwnerUserdata.foo")
page.traceButton:Click()
assert(ns.FunctionTrace.IsRunning() and hookCount == 3,
    "indexable userdata no longer reaches the runtime hook installation boundary")
page.traceButton:Click()
TraceOwnerInherited = setmetatable({}, { __index = { Method = function() end } })
input:SetText("TraceOwnerInherited.Method")
page.traceButton:Click()
assert(ns.FunctionTrace.IsRunning() and hookCount == 4, "inherited methods no longer resolve")
page.traceButton:Click()
print("Trace owner validation through real page clicks ok")
