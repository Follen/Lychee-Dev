local inCombat = false
local now = 10
local testClient = os.getenv("LYCHEE_TEST_CLIENT") or "retail"
local testBuilds = {
    retail = { "12.1.0", "70000", "Aug 19 2026", 120100 },
    classic = { "5.5.4", "64000", "Aug 04 2026", 50504 },
    titan = { "3.80.2", "63000", "Aug 05 2026", 38002 },
}
local loadedQueryArguments = {}
local memoryUpdated = false
local cpuUpdated = false
local activeTicker
local metricOffset = 0

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
function GetCVarBool(name) return name == "scriptProfile" end
function UpdateAddOnMemoryUsage() memoryUpdated = true end
function UpdateAddOnCPUUsage() cpuUpdated = true end
function GetAddOnMemoryUsage(name) return name == "Lychee Dev" and 2048 or 512 end
function GetAddOnCPUUsage(name) return name == "Lychee Dev" and 12.5 or 2.5 end
function GetFrameCPUUsage(_, includeChildren)
    return now * (includeChildren and 2 or 1), math.floor(now)
end
function GetFunctionCPUUsage(_, includeSubroutines)
    return now * (includeSubroutines and 2 or 1), math.floor(now)
end
function IsCpuBound() return true end
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
    GetAddOnMetadata = function(name, key)
        if name == "OtherAddOn" and key == "SavedVariables" then return "OtherDB" end
        return ""
    end,
}

Enum = { AddOnProfilerMetric = {} }
local metricNames = {
    "SessionAverageTime", "RecentAverageTime", "EncounterAverageTime", "LastTime",
    "PeakTime", "CountTimeOver1Ms", "CountTimeOver5Ms", "CountTimeOver10Ms",
    "CountTimeOver50Ms", "CountTimeOver100Ms", "CountTimeOver500Ms", "CountTimeOver1000Ms",
}
for index = 1, #metricNames do Enum.AddOnProfilerMetric[metricNames[index]] = index end
C_AddOnProfiler = {
    IsEnabled = function() return true end,
    GetAddOnMetric = function(name, metric)
        local base = name == "OtherAddOn" and 2 or 0.2
        if metric >= 6 then return metricOffset + metric - 5 end
        return base * metric + metricOffset
    end,
    GetOverallMetric = function(metric) return metric * 10 + metricOffset end,
    GetApplicationMetric = function(metric) return metric * 20 + metricOffset end,
    MeasureCall = function(func, ...)
        func(...)
        return {
            elapsedMilliseconds = 1.25,
            elapsedTicks = 125,
            allocatedBytes = 2048,
            deallocatedBytes = 512,
            events = {},
        }
    end,
}
C_Timer = {
    NewTicker = function(interval, callback)
        activeTicker = { interval = interval, callback = callback, cancelled = false }
        function activeTicker:Cancel() self.cancelled = true end
        return activeTicker
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
LoadAddonFile("Modules/Performance.lua", ns)

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
local healthOk, health = ns.Performance.CollectHealth()
assert(healthOk and health.profilerEnabled and #health.addons == 2,
    "native addon health snapshot failed")
assert(health.addons[1].metrics.over1000 ~= nil and health.addons[1].findings,
    "health snapshot omitted deep profiler metrics")
assert(memoryUpdated, "performance memory counters were not updated")
assert(loadedQueryArguments[1] == "Lychee Dev" and loadedQueryArguments[2] == "OtherAddOn",
    "performance scan did not query loaded addons by name")

OtherDB = { records = { { value = string.rep("x", 32) } }, enabled = true }
local storageOk, storage = ns.Performance.CollectSavedVariables("OtherAddOn")
assert(storageOk and storage.declaredCount == 1 and storage.loadedCount == 1 and storage.totalBytes > 0
        and storage.roots[1].name == "OtherDB",
    "SavedVariables structure scan failed")

local pool = {
    active = 1,
    inactiveObjects = { {}, {} },
    GetNumActive = function(self) return self.active end,
    EnumerateActive = function() return pairs({}) end,
}
OtherAddOn = { Work = function() return true end, rowPool = pool }
local captureOk = ns.Performance.StartCapture("OtherAddOn", 10)
assert(captureOk and activeTicker and not activeTicker.cancelled,
    "performance capture did not start a bounded ticker")
local newChild = {
    GetObjectType = function() return "Texture" end,
    IsShown = function() return true end,
    GetSourceLocation = function() return "Interface\\AddOns\\OtherAddOn\\Rows.lua:30" end,
    GetChildren = function() end,
    GetRegions = function() end,
    GetScript = function() return nil end,
}
childFocus.shown = false
mouseFocus.GetChildren = function() return childFocus, newChild end
pool.active = 2
pool.inactiveObjects = { {} }
OtherAddOn.Work = function() return false end
OtherDB.records[2] = { value = string.rep("y", 64) }
now = now + 1
metricOffset = metricOffset + 1
activeTicker.callback()
childFocus.shown = true
now = now + 1
metricOffset = metricOffset + 1
activeTicker.callback()
local capture = ns.Performance.StopCapture("manual")
assert(capture and activeTicker.cancelled and capture.summary.sampleCount >= 3,
    "performance capture did not stop cleanly")
assert(capture.objectSummary.newlyObserved >= 1
        and capture.objectSummary.reusedActivations >= 1
        and capture.objectSummary.pools[1].reuseAcquisitions >= 1,
    "object creation, activation reuse, or pool reuse was not analyzed")
assert(capture.objectSummary.rootsDiscovered >= 1
        and capture.objectSummary.attributedFrames >= 1,
    "addon namespace or owned frames were not discovered automatically")
assert(capture.objectSummary.newFunctionInstances >= 1
        and capture.objectSummary.functionReplacements >= 1
        and #capture.objectSummary.hotFunctions >= 1,
    "function hotspots or closure replacement churn were not analyzed")
assert(capture.storageSummary and capture.storageSummary.bytesDelta > 0
        and capture.storageSummary.finalDeclaredCount == 1
        and capture.storageSummary.finalLoadedCount == 1,
    "SavedVariables growth was not correlated with the capture")

TestBenchmark = function() return true end
local benchmarkOk, benchmark = ns.Performance.RunBenchmark("TestBenchmark", 5, false)
assert(benchmarkOk and benchmark.summary.p95 == 1.25
        and benchmark.summary.allocatedPerCall == 2048
        and benchmark.summary.netPerCall == 1536,
    "function time and allocation benchmark failed")
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
