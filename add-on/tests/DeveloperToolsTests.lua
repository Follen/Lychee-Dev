local inCombat = false
local now = 10
local testClient = os.getenv("LYCHEE_TEST_CLIENT") or "retail"
local testBuilds = {
    retail = { "12.1.0", "70000", "Aug 19 2026", 120100 },
    classic = { "5.5.4", "64000", "Aug 04 2026", 50504 },
    titan = { "3.80.2", "63000", "Aug 05 2026", 38002 },
}

function InCombatLockdown() return inCombat end
function issecretvalue() return false end
function GetTime() return now end
function time() return 1700000000 end
function date(_, value) return "date:" .. tostring(value) end
function GetBuildInfo()
    local build = assert(testBuilds[testClient])
    return build[1], build[2], build[3], build[4]
end
function GetLocale() return "zhCN" end
function GetCursorPosition() return 960, 540 end
function wipe(target) for key in pairs(target) do target[key] = nil end end

C_AddOns = {
    GetNumAddOns = function() return 2 end,
    GetAddOnInfo = function(index)
        if index == 1 then return "Lychee Dev", "|cffd83b4e[Lychee]|r Dev Tools" end
        return "OtherAddOn", "Other AddOn"
    end,
    IsAddOnLoaded = function(name)
        return true, true
    end,
    GetAddOnMetadata = function(name, key)
        if name == "OtherAddOn" and key == "SavedVariables" then return "OtherDB" end
        return ""
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
    GetSourceLocation = function() return "Interface\\AddOns\\OtherAddOn\\UI.lua:10" end,
    GetScript = function() return nil end,
}
local childFocus = {
    GetObjectType = function() return "Button" end,
    GetName = function() return "TargetChild" end,
    shown = true,
    IsShown = function(self) return self.shown end,
    GetSourceLocation = function() return "Interface\\AddOns\\OtherAddOn\\Rows.lua:20" end,
    GetScript = function() return nil end,
    GetChildren = function() end,
    GetRegions = function() end,
}
mouseFocus.GetChildren = function() return childFocus end
mouseFocus.GetRegions = function() return end
function GetMouseFoci() return { mouseFocus } end
local enumeratedFrames = { mouseFocus }
function EnumerateFrames(previous)
    if previous == nil then return enumeratedFrames[1] end
    for index = 1, #enumeratedFrames do
        if enumeratedFrames[index] == previous then return enumeratedFrames[index + 1] end
    end
end

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
local clientFiles = {
    retail = "Core/Clients/Mainline.lua",
    classic = "Core/Clients/Mists.lua",
    titan = "Core/Clients/Titan.lua",
}
LoadAddonFile(assert(clientFiles[testClient], "unknown test client: " .. testClient), ns)
assert(ns.Client.id == testClient and select(4, GetBuildInfo()) == ns.Client.interface,
    "developer tools test client profile mismatch")
LoadAddonFile("Core/Compatibility.lua", ns)
LoadAddonFile("Core/Locale.lua", ns)
LoadAddonFile("Core/Locale_enUS.lua", ns)
LoadAddonFile("Core/Serializer.lua", ns)
LoadAddonFile("Core/Inspector.lua", ns)
LoadAddonFile("Core/Safety.lua", ns)
LoadAddonFile("Modules/ObjectInspector.lua", ns)
LoadAddonFile("Modules/Diagnostics.lua", ns)
LoadAddonFile("Modules/FunctionTrace.lua", ns)

TestRoot = {
    Alpha = 1,
    Alphabet = 2,
    BetaAlpha = 3,
    Nested = { Value = "ok", Controls = { UpdateAddButton = function() end } },
}
local succeeded, value = ns.ObjectInspector.ResolvePath("_G.TestRoot.Nested.Value")
assert(succeeded and value == "ok", "object path did not resolve")
local inspected, objectInspection = ns.ObjectInspector.InspectPath("TestRoot")
assert(inspected and objectInspection.textStream,
    "object inspection did not retain an incremental text stream")
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
local globalSearched, globalResult = ns.ObjectInspector.SearchGlobal("testroot")
assert(globalSearched and globalResult.totalMatches == 1
    and globalResult.results[1].path == "TestRoot",
    "global object search did not stay on the top level")
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
