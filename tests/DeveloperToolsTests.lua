local inCombat = false
local now = 10
local loadedQueryArguments = {}
local memoryUpdated = false
local cpuUpdated = false

function InCombatLockdown() return inCombat end
function issecretvalue() return false end
function GetTime() return now end
function time() return 1700000000 end
function date(_, value) return "date:" .. tostring(value) end
function GetBuildInfo() return "12.1.0", "70000", "Aug 19 2026", 120100 end
function GetLocale() return "zhCN" end
function GetCVarBool(name) return name == "scriptProfile" end
function UpdateAddOnMemoryUsage() memoryUpdated = true end
function UpdateAddOnCPUUsage() cpuUpdated = true end
function GetAddOnMemoryUsage(name) return name == "Lychee Dev" and 2048 or 512 end
function GetAddOnCPUUsage(name) return name == "Lychee Dev" and 12.5 or 2.5 end
function GetCursorPosition() return 960, 540 end
function wipe(target) for key in pairs(target) do target[key] = nil end end

C_AddOns = {
    GetNumAddOns = function() return 2 end,
    GetAddOnInfo = function(index)
        if index == 1 then return "Lychee Dev", "|cffd83b4e[Lychee]|r Dev Tools" end
        return "OtherAddOn", "Other AddOn"
    end,
    IsAddOnLoaded = function(name)
        loadedQueryArguments[#loadedQueryArguments + 1] = name
        return true, true
    end,
}

local function NewRegion()
    local region = { shown = true, events = {} }
    function region:CreateTexture() return NewRegion() end
    function region:RegisterEvent(event) self.events[event] = true end
    function region:UnregisterAllEvents() wipe(self.events) end
    function region:SetScript(name, callback) self[name] = callback end
    function region:Show() self.shown = true end
    function region:Hide() self.shown = false end
    function region:IsShown() return self.shown end
    setmetatable(region, { __index = function() return function() end end })
    return region
end

function CreateFrame() return NewRegion() end
UIParent = NewRegion()
function UIParent:GetWidth() return 1920 end
function UIParent:GetHeight() return 1080 end
function UIParent:GetEffectiveScale() return 1 end

local mouseFocus = {
    GetObjectType = function() return "Frame" end,
    GetName = function() return "TargetFrame" end,
    IsForbidden = function() return false end,
    IsShown = function() return true end,
    GetWidth = function() return 240 end,
    GetHeight = function() return 120 end,
    GetEffectiveScale = function() return 1 end,
    GetFrameLevel = function() return 4 end,
    GetFrameStrata = function() return "MEDIUM" end,
    GetParent = function() return UIParent end,
}
local childFocus = {
    GetObjectType = function() return "Button" end,
    GetName = function() return "TargetChild" end,
}
mouseFocus.GetChildren = function() return childFocus end
mouseFocus.GetRegions = function() return end
function GetMouseFoci() return { mouseFocus } end

local hooks = {}
function hooksecurefunc(owner, name, callback)
    hooks[owner] = hooks[owner] or {}
    hooks[owner][name] = callback
end

local errors = {
    { message = "OtherAddOn failed", stack = "AddOns/OtherAddOn/Core.lua:20", counter = 1, time = 1699999998, session = 1 },
    { message = "Lychee exploded", stack = "AddOns/Lychee Dev/Core.lua:42", locals = "value = nil", counter = 3, time = 1699999999, session = 2 },
    { message = "Network warning", stack = "AddOns/Network/UI.lua:8", counter = 2, time = 1700000000, session = 2 },
}
local resetCalled = false
BugGrabber = {
    GetDB = function() return errors end,
    GetSessionId = function() return 2 end,
    IsPaused = function() return false end,
    Reset = function() resetCalled = true wipe(errors) end,
}

local function LoadAddonFile(path, namespace)
    local chunk, loadError = loadfile(path)
    assert(chunk, loadError)
    return chunk("Lychee Dev", namespace)
end

local ns = {}
LoadAddonFile("Locale.lua", ns)
LoadAddonFile("Serializer.lua", ns)
LoadAddonFile("Inspector.lua", ns)
LoadAddonFile("Safety.lua", ns)
LoadAddonFile("ObjectInspector.lua", ns)
LoadAddonFile("Diagnostics.lua", ns)
LoadAddonFile("FunctionTrace.lua", ns)

TestRoot = {
    Alpha = 1,
    Alphabet = 2,
    BetaAlpha = 3,
    Nested = { Value = "ok", Controls = { UpdateAddButton = function() end } },
}
local succeeded, value = ns.ObjectInspector.ResolvePath("_G.TestRoot.Nested.Value")
assert(succeeded and value == "ok", "object path did not resolve")
local searched, searchResult = ns.ObjectInspector.SearchPath("TestRoot", "alpha")
assert(searched and searchResult.totalMatches == 3, "object keyword search did not rank all matches")
assert(searchResult.results[1].key == "Alpha", "exact object search match was not ranked first")
assert(searchResult.results[1].value == 1, "object search result did not retain its inspectable value")
local directSearched, directResult = ns.ObjectInspector.SearchValue(TestRoot, "nested")
assert(directSearched and directResult.results[1].value == TestRoot.Nested,
    "direct object search did not support captured targets")
local nestedSearched, nestedResult = ns.ObjectInspector.SearchValue(TestRoot, "add")
assert(nestedSearched and nestedResult.totalMatches == 1
    and nestedResult.results[1].path == "Nested.Controls.UpdateAddButton",
    "nested object search did not return the matched field path")
local captured, inspection = ns.ObjectInspector.CaptureMouseFocus()
assert(captured and inspection.isFrame and inspection.label == "TargetFrame", "mouse frame snapshot failed")
assert(inspection.value == mouseFocus, "mouse capture did not retain the selected object")
local capturedRoot = inspection.tree.roots[1]
local foundChildren
for index = 1, #capturedRoot.children do
    if capturedRoot.children[index].label == ns.L.FRAME_CHILDREN then
        foundChildren = capturedRoot.children[index]
        break
    end
end
assert(foundChildren and not foundChildren.loaded, "captured frame children were not available lazily")
ns.LoadMoreValueTreeNode(foundChildren)
assert(foundChildren.children[1].source == childFocus,
    "captured frame child did not remain available for nested inspection")

local gotErrors, current = ns.Diagnostics.GetErrors("current", "")
assert(gotErrors and #current.errors == 2, "current BugGrabber session filter failed")
local gotAll, all = ns.Diagnostics.GetErrors("all", "")
assert(gotAll and #all.errors == 3, "all BugGrabber sessions were not returned")
local gotFiltered, filtered = ns.Diagnostics.GetErrors("all", "lychee")
assert(gotFiltered and #filtered.errors == 1, "error keyword filter failed")
local report = ns.Diagnostics.FormatAgentReport(filtered.errors[1])
assert(report:find("Lychee exploded", 1, true), "agent report omitted the error message")
assert(report:find("AddOns/Lychee Dev/Core.lua:42", 1, true), "agent report omitted the stack")
assert(report:find("value = nil", 1, true), "agent report omitted locals")
local systemOk, system = ns.Diagnostics.CollectSystem()
assert(systemOk and #system.addons == 2, "performance snapshot failed")
assert(memoryUpdated and cpuUpdated, "performance counters were not updated")
assert(loadedQueryArguments[1] == "Lychee Dev" and loadedQueryArguments[2] == "OtherAddOn", "IsAddOnLoaded was not called with addon names")
local resetOk = ns.Diagnostics.ResetErrors()
assert(resetOk and resetCalled and #errors == 0, "BugGrabber reset delegation failed")

TestTrace = function() end
local traceOk = ns.FunctionTrace.Start("TestTrace")
assert(traceOk, "function trace did not start")
hooks[_G].TestTrace("alpha", 42)
assert(ns.FunctionTrace.GetCount() == 1, "function trace did not record a call")
local record = ns.FunctionTrace.GetRecord(1)
assert(record.arguments[1] == '"alpha"' and record.arguments[2] == "42", "function trace arguments were incorrect")
ns.FunctionTrace.Stop()
hooks[_G].TestTrace("ignored")
assert(ns.FunctionTrace.GetCount() == 1, "stopped function trace continued recording")

inCombat = true
local combatInspect, _, combatMessage = ns.ObjectInspector.InspectPath("TestRoot")
assert(not combatInspect and combatMessage == ns.L.COMBAT_BLOCKED, "object inspector did not block combat")
print("Lychee Dev developer tools tests passed")
