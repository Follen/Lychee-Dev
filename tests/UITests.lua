SlashCmdList = {}
UISpecialFrames = {}
local testClient = os.getenv("LYCHEE_TEST_CLIENT") or "retail"
local testBuilds = {
    retail = { "12.1.0", "70000", "Aug 19 2026", 120100 },
    classic = { "5.5.4", "64000", "Aug 04 2026", 50504 },
    titan = { "3.80.2", "63000", "Aug 05 2026", 38002 },
}

local frameCount = 0
local textures = {}

local function NewRegion(name)
    local region = {
        name = name,
        shown = true,
        text = "",
        width = 0,
        height = 0,
        verticalScroll = 0,
        minimumValue = 0,
        maximumValue = 0,
        value = 0,
    }

    function region:GetName()
        return self.name
    end

    function region:SetSize(width, height)
        self.width = width
        self.height = height
    end

    function region:SetWidth(width)
        self.width = width
    end

    function region:SetHeight(height)
        self.height = height
    end

    function region:GetWidth()
        return self.width
    end

    function region:GetHeight()
        return self.height
    end

    function region:SetPoint(...)
        self.point = { ... }
    end

    function region:ClearAllPoints()
        self.point = nil
    end

    function region:SetText(text)
        self.text = text or ""
    end

    function region:GetText()
        return self.text
    end

    function region:SetFocus()
        self.focused = true
    end

    function region:ClearFocus()
        self.focused = false
    end

    function region:HighlightText()
        self.highlighted = true
    end

    function region:SelectAll()
        self.focused = true
        self.highlighted = true
    end

    function region:GetStringHeight()
        return 14
    end

    function region:GetStringWidth()
        return #self.text * 7
    end

    function region:SetTexture(path)
        self.texture = path
        textures[#textures + 1] = path
    end

    function region:SetScript(scriptName, handler)
        local scripts = rawget(self, "scripts")
        if not scripts then
            scripts = {}
            rawset(self, "scripts", scripts)
        end
        scripts[scriptName] = handler
    end

    function region:Hide()
        local wasShown = self.shown
        self.shown = false
        local scripts = rawget(self, "scripts")
        if wasShown and scripts and scripts.OnHide then
            scripts.OnHide(self)
        end
    end

    function region:Show()
        local wasShown = self.shown
        self.shown = true
        local scripts = rawget(self, "scripts")
        if not wasShown and scripts and scripts.OnShow then
            scripts.OnShow(self)
        end
    end

    function region:SetShown(shown)
        if shown then self:Show() else self:Hide() end
    end

    function region:IsShown()
        return self.shown
    end

    function region:GetVerticalScroll()
        return self.verticalScroll
    end

    function region:SetVerticalScroll(offset)
        self.verticalScroll = offset
        local scripts = rawget(self, "scripts")
        if scripts and scripts.OnVerticalScroll then
            scripts.OnVerticalScroll(self, offset)
        end
    end

    function region:SetScrollChild(child)
        self.scrollChild = child
    end

    function region:GetScrollChild()
        return self.scrollChild
    end

    function region:GetVerticalScrollRange()
        local verticalScrollRange = rawget(self, "verticalScrollRange")
        if verticalScrollRange ~= nil then
            return verticalScrollRange
        end
        local childHeight = self.scrollChild and self.scrollChild:GetHeight() or 0
        return math.max(0, childHeight - (self:GetHeight() or 0))
    end

    function region:SetMinMaxValues(minimum, maximum)
        self.minimumValue = minimum
        self.maximumValue = maximum
    end

    function region:GetMinMaxValues()
        return self.minimumValue, self.maximumValue
    end

    function region:SetValue(value)
        self.value = value
        local scripts = rawget(self, "scripts")
        if scripts and scripts.OnValueChanged then
            scripts.OnValueChanged(self, value)
        end
    end

    function region:GetValue()
        return self.value
    end

    function region:SetEnabled(enabled)
        self.enabled = enabled and true or false
    end

    function region:IsEnabled()
        return self.enabled ~= false
    end

    function region:Click()
        if not self:IsEnabled() then
            return
        end
        local scripts = rawget(self, "scripts")
        if scripts and scripts.OnClick then
            scripts.OnClick(self, "LeftButton")
        end
    end

    function region:GetFrameLevel()
        return 1
    end

    function region:CreateTexture()
        return NewRegion()
    end

    function region:CreateFontString()
        return NewRegion()
    end

    setmetatable(region, {
        __index = function(target, key)
            if key == "GetTextHeight" then
                return nil
            end
            local noOp = function()
            end
            rawset(target, key, noOp)
            return noOp
        end,
    })
    return region
end

function CreateFrame(frameType, name, _, template)
    frameCount = frameCount + 1
    local frame = NewRegion(name)
    frame.frameType = frameType
    frame.hasBackdrop = template == "BackdropTemplate"
    if not frame.hasBackdrop then
        frame.SetBackdrop = false
        frame.SetBackdropColor = false
        frame.SetBackdropBorderColor = false
    end
    if name then
        _G[name] = frame
    end
    return frame
end

function wipe(target)
    for key in pairs(target) do
        target[key] = nil
    end
end

function time()
    return 1234567890
end

function date(_, timestamp)
    return tostring(timestamp)
end

function issecretvalue()
    return false
end

tinsert = table.insert
UIParent = NewRegion("UIParent")
ChatFontNormal = {}
GameFontNormal = {}
GameFontNormalLarge = {}
GameFontHighlightSmall = {}
GameFontDisableSmall = {}
local now = 10
local activeTicker
C_Timer = {
    After = function(_, callback) callback() end,
    NewTicker = function(interval, callback)
        activeTicker = { interval = interval, callback = callback, cancelled = false }
        function activeTicker:Cancel() self.cancelled = true end
        return activeTicker
    end,
}
local inCombat = false
local reloadCalled = false
function InCombatLockdown() return inCombat end
function ReloadUI() reloadCalled = true end
function GetTime() return now end
function GetCVarBool() return false end
local functionCpuUsage = 0
function GetFunctionCPUUsage()
    functionCpuUsage = functionCpuUsage + 1
    return functionCpuUsage, functionCpuUsage
end
function UpdateAddOnMemoryUsage() end
function GetAddOnMemoryUsage(name) return name == "OtherAddOn" and 2048 or 512 end
function IsCpuBound() return true end
function GetBuildInfo()
    local build = assert(testBuilds[testClient])
    return build[1], build[2], build[3], build[4]
end
function GetLocale() return "zhCN" end
Enum = { AddOnProfilerMetric = {} }
local metricNames = {
    "SessionAverageTime", "RecentAverageTime", "EncounterAverageTime", "LastTime",
    "PeakTime", "CountTimeOver1Ms", "CountTimeOver5Ms", "CountTimeOver10Ms",
    "CountTimeOver50Ms", "CountTimeOver100Ms", "CountTimeOver500Ms", "CountTimeOver1000Ms",
}
for index = 1, #metricNames do Enum.AddOnProfilerMetric[metricNames[index]] = index end
C_AddOnProfiler = {
    IsEnabled = function() return true end,
    GetAddOnMetric = function(name, metric) return (name == "OtherAddOn" and 1 or 0.1) * metric end,
    GetOverallMetric = function(metric) return metric * 10 end,
    GetApplicationMetric = function(metric) return metric * 20 end,
    MeasureCall = function(func, ...)
        func(...)
        return {
            elapsedMilliseconds = 1,
            elapsedTicks = 100,
            allocatedBytes = 1024,
            deallocatedBytes = 256,
            events = {},
        }
    end,
}
C_AddOns = {
    GetNumAddOns = function() return 2 end,
    GetAddOnInfo = function(index)
        if index == 1 then return "Lychee Dev", "[Lychee] Dev Tools" end
        return "OtherAddOn", "Other AddOn"
    end,
    IsAddOnLoaded = function() return true, true end,
    GetAddOnMetadata = function(name, key)
        if name == "OtherAddOn" and key == "SavedVariables" then return "OtherDB" end
        return ""
    end,
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
local catalogFiles = {
    retail = "Modules/Events/CatalogData_Mainline.lua",
    classic = "Modules/Events/CatalogData_Mists.lua",
    titan = "Modules/Events/CatalogData_Titan.lua",
}
LoadAddonFile(assert(clientFiles[testClient], "unknown test client: " .. testClient), ns)
assert(ns.Client.id == testClient and select(4, GetBuildInfo()) == ns.Client.interface,
    "UI test client profile mismatch")
LoadAddonFile("Core/Compatibility.lua", ns)
LoadAddonFile("Core/Locale.lua", ns)
LoadAddonFile("Core/Locale_enUS.lua", ns)
LoadAddonFile("Core/Database.lua", ns)
LoadAddonFile("Core/Serializer.lua", ns)
LoadAddonFile("Core/Inspector.lua", ns)
LoadAddonFile("Core/Safety.lua", ns)
LoadAddonFile("Core/Bootstrap.lua", ns)
LoadAddonFile("Modules/ObjectInspector.lua", ns)
LoadAddonFile("Modules/Diagnostics.lua", ns)
LoadAddonFile("Modules/FunctionTrace.lua", ns)
LoadAddonFile("Modules/Performance.lua", ns)
LoadAddonFile(assert(catalogFiles[testClient]), ns)
LoadAddonFile("Modules/Events/Catalog.lua", ns)
LoadAddonFile("Modules/Events/Monitor.lua", ns)
LoadAddonFile("UI/Export.lua", ns)
LoadAddonFile("UI/Pages/ExportRecords.lua", ns)
LoadAddonFile("UI/Features.lua", ns)
LoadAddonFile("UI/Pages/Object.lua", ns)
LoadAddonFile("UI/Pages/Trace.lua", ns)
LoadAddonFile("UI/Pages/Performance.lua", ns)
LoadAddonFile("UI/Pages/Diagnostics.lua", ns)
LoadAddonFile("UI/Pages/About.lua", ns)
LoadAddonFile("UI/MainWindow.lua", ns)

LycheeDevDB = {
    schemaVersion = 4,
    history = {
        {
            code = "return { persisted = true }",
            result = "{ persisted = true }",
            succeeded = true,
            timestamp = 1234567889,
            tree = {
                roots = {
                    {
                        label = "[1]",
                        kind = "table",
                        value = "表（1 项）",
                        expanded = true,
                        loaded = true,
                        hasMore = false,
                        children = {
                            { label = "persisted", kind = "boolean", value = "true" },
                        },
                    },
                },
            },
        },
    },
}

assert(frameCount == 0, "UI created frames before /dev was used")
SlashCmdList.LYCHEEDEV()
assert(frameCount > 0, "UI did not create frames on first /dev")
assert(LycheeDevWindow and LycheeDevWindow:IsShown(), "window did not open")
assert(UISpecialFrames[1] == "LycheeDevWindow", "window was not registered for escape close")
assert(LycheeDevWindow.width == 1040 and LycheeDevWindow.height == 720, "window did not use the expanded workbench size")

local pageCount = 0
for _ in pairs(LycheeDevWindow.pages) do pageCount = pageCount + 1 end
assert(pageCount == 8, "window did not create all eight workbench pages")
assert(LycheeDevWindow.pages.runner:IsShown(), "runner page was not active by default")
assert(LycheeDevWindow.pageTabs.runner:GetWidth()
        >= math.max(48, LycheeDevWindow.pageTabs.runner.label:GetStringWidth() + 22),
    "main navigation did not preserve text padding")
assert(LycheeDevWindow.pageTabs.exports:GetWidth() > LycheeDevWindow.pageTabs.runner:GetWidth(),
    "main navigation did not size tabs from their rendered labels")
assert(LycheeDevWindow.pageTabs.objects.point[4] == 8,
    "main navigation did not use a consistent visual gap")
assert(LycheeDevWindow.historyButtons[1].background
        and LycheeDevWindow.historyButtons[1].divider
        and LycheeDevWindow.historyButtons[1]:GetWidth() == 206,
    "history list did not use the aligned flat-row component")
assert(LycheeDevWindow.resultTextTab and LycheeDevWindow.resultTreeTab, "result mode buttons were not created")
local resultScrollbar = LycheeDevWindow.resultPanel.scroll.scrollbar
LycheeDevWindow.resultPanel.scroll:SetHeight(100)
LycheeDevWindow.resultPanel.editBox:SetHeight(300)
LycheeDevWindow.resultPanel.scroll:UpdateScrollChildRect()
LycheeDevWindow.resultPanel.scroll:SetVerticalScroll(50)
local minimumScroll, maximumScroll = resultScrollbar:GetMinMaxValues()
assert(LycheeDevWindow.resultPanel.scroll.frameType == "ScrollFrame",
    "text areas did not use a native scroll frame")
assert(LycheeDevWindow.resultPanel.scroll:GetScrollChild() == LycheeDevWindow.resultPanel.editBox,
    "edit box was not the native scroll child")
assert(minimumScroll == 0 and maximumScroll == 200, "custom scroll range did not follow content height")
local resultScrollScripts = rawget(LycheeDevWindow.resultPanel.scroll, "scripts")
LycheeDevWindow.resultPanel.scroll.verticalScrollRange = 240
resultScrollScripts.OnScrollRangeChanged(LycheeDevWindow.resultPanel.scroll, 0, 240)
minimumScroll, maximumScroll = resultScrollbar:GetMinMaxValues()
assert(minimumScroll == 0 and maximumScroll == 240,
    "custom scrollbar did not follow the native scroll range")
LycheeDevWindow.resultPanel.scroll.verticalScrollRange = 200
resultScrollScripts.OnScrollRangeChanged(LycheeDevWindow.resultPanel.scroll, 0, 200)
local resultEditScripts = rawget(LycheeDevWindow.resultPanel.editBox, "scripts")
assert(resultEditScripts and resultEditScripts.OnMouseWheel, "result edit box did not capture mouse wheel input")
resultEditScripts.OnMouseWheel(LycheeDevWindow.resultPanel.editBox, -1)
assert(resultScrollbar:GetValue() == 86, "mouse wheel input did not move the custom scrollbar")
assert(LycheeDevWindow.resultPanel.scroll:GetVerticalScroll() == 86,
    "mouse wheel input did not move the text viewport")
assert(LycheeDevWindow.resultPanel.editBox.point[5] == 0,
    "native scrolling changed the edit box anchor")
assert(not rawget(LycheeDevWindow.resultPanel, "wheelCatcher"),
    "read-only text area still covered the edit box with a wheel catcher")
resultScrollbar:SetValue(120)
assert(LycheeDevWindow.resultPanel.scroll:GetVerticalScroll() == 120,
    "custom scrollbar drag did not move the native text viewport")
LycheeDevWindow.inputPanel.scroll:SetHeight(100)
LycheeDevWindow.inputPanel.editBox:SetHeight(300)
LycheeDevWindow.inputPanel.scroll:UpdateScrollChildRect()
local inputEditScripts = rawget(LycheeDevWindow.inputPanel.editBox, "scripts")
inputEditScripts.OnMouseWheel(LycheeDevWindow.inputPanel.editBox, -1)
assert(LycheeDevWindow.inputPanel.scroll:GetVerticalScroll() == 36,
    "editable input mouse wheel did not move the native text viewport")
LycheeDevWindow.inputPanel.editBox:SetText("return { nested = { value = 7 } }")
LycheeDevWindow.runButton:Click()
assert(LycheeDevWindow.treeView:HasTree(), "table result did not create a tree")
assert(LycheeDevWindow.exportResultButton:IsEnabled(),
    "run result export did not enable after execution")
LycheeDevWindow.inputPanel.editBox:SetText("print('no return value')")
LycheeDevWindow.runButton:Click()
assert(not LycheeDevWindow.treeView:HasTree(), "run without return values unexpectedly created a tree")
LycheeDevWindow.historyButtons[2]:Click()
assert(LycheeDevWindow.treeView:HasTree(), "current-session history did not restore its tree")
assert(LycheeDevWindow.resultTreeTab:IsEnabled(), "restored history tree mode was not enabled")
LycheeDevWindow.historyButtons[3]:Click()
assert(LycheeDevWindow.treeView:HasTree(), "persisted history did not restore its stored tree")
LycheeDevWindow.resultTextTab:Click()
assert(LycheeDevWindow.resultPanel:IsShown() and not LycheeDevWindow.treeView.panel:IsShown(),
    "result text mode did not activate")
LycheeDevWindow.resultTreeTab:Click()
assert(LycheeDevWindow.treeView.panel:IsShown() and not LycheeDevWindow.resultPanel:IsShown(),
    "result tree mode did not activate")
local inputBeforeClearHistory = LycheeDevWindow.inputPanel.editBox:GetText()
LycheeDevWindow.clearHistoryButton:Click()
assert(#ns.GetHistory() == 0 and LycheeDevWindow.historyEmpty:IsShown(),
    "clear history did not empty the history list")
assert(LycheeDevWindow.currentResultText == ""
        and LycheeDevWindow.resultPanel.editBox:GetText() == ""
        and not LycheeDevWindow.treeView:HasTree(),
    "clear history did not clear the current result")
assert(LycheeDevWindow.resultPanel:IsShown() and not LycheeDevWindow.treeView.panel:IsShown(),
    "clear history did not restore the empty text result view")
assert(not LycheeDevWindow.exportResultButton:IsEnabled()
        and LycheeDevWindow.status:GetText() == ns.L.READY,
    "clear history did not reset result actions and status")
assert(LycheeDevWindow.inputPanel.editBox:GetText() == inputBeforeClearHistory,
    "clear history unexpectedly cleared the current Lua input")
LycheeDevWindow.pageTabs.objects:Click()
assert(LycheeDevWindow.pages.objects:IsShown(), "object page did not activate")
assert(not LycheeDevWindow.pages.runner:IsShown(), "runner page stayed visible after navigation")
local objectPage = LycheeDevWindow.pages.objects
assert(objectPage.inspectButton.variant == "primary", "object inspect action was not primary")
assert(not objectPage.selectSnapshot:IsEnabled(), "empty object snapshot could be selected")
assert(not objectPage.exportSnapshot:IsEnabled(), "empty object snapshot could be exported")
local originalInspectPath = ns.ObjectInspector.InspectPath
local inspectedObject = { nested = { value = 7 } }
for index = 1, 2500 do
    inspectedObject["streamField" .. index] = string.rep("v", 16)
end
ns.ObjectInspector.InspectPath = function(path)
    local stream = ns.CreateSerializationStream(inspectedObject)
    local serialized = stream:ReadChunk()
    return true, {
        value = inspectedObject,
        label = path,
        valueType = "table",
        text = string.format(ns.L.OBJECT_TEXT_HEADER, path, "table") .. "\n" .. serialized,
        textStream = stream,
        tree = ns.CreateValueTree({ n = 1, inspectedObject }),
    }
end
objectPage.inspectButton:Click()
ns.ObjectInspector.InspectPath = originalInspectPath
assert(objectPage.selectSnapshot:IsEnabled(), "object snapshot action did not enable after inspection")
local initialObjectTextLength = #objectPage.textView.editBox:GetText()
objectPage.textView.scroll.verticalRange = 100
local objectTextScripts = rawget(objectPage.textView.editBox, "scripts")
objectTextScripts.OnMouseWheel(objectPage.textView.editBox, -1)
assert(#objectPage.textView.editBox:GetText() > initialObjectTextLength,
    "object text wheel scrolling did not append another chunk near the bottom")
local loadedObjectTextLength = #objectPage.textView.editBox:GetText()
objectPage.exportSnapshot:Click()
local exportedObjects = ns.GetExports()
local objectExport = ns.GetExport(exportedObjects.order[1])
assert(objectExport and objectExport.source.kind == "object_snapshot"
        and #objectExport.payload.content > loadedObjectTextLength,
    "object export did not serialize the full source independently of the edit box")
assert(LycheeDevWindow.exportController.popup.overlay:IsShown(),
    "export ticket popup did not open")
assert(LycheeDevWindow.exportController.popup.copyButton == nil
        and LycheeDevWindow.exportController.popup.ticketBox.focused
        and LycheeDevWindow.exportController.popup.ticketBox.highlighted
        and LycheeDevWindow.exportController.popup.hint:GetText() == ns.L.EXPORT_TICKET_HELP,
    "export popup did not select the Ticket by default")
LycheeDevWindow.exportController.popup.reloadButton:Click()
assert(reloadCalled, "export popup reload action did not call ReloadUI")
LycheeDevWindow.exportController.popup.laterButton:Click()
local objectRootRow = objectPage.treeView.rows[1]
rawget(objectRootRow, "scripts").OnClick(objectRootRow, "RightButton")
assert(objectPage.treeView:GetSelectedNode() == objectRootRow.node,
    "object tree context action did not select its node")
assert(objectPage.nodePopup and objectPage.nodePopup.overlay:IsShown(),
    "object tree context action did not open the node text popup")
assert(objectPage.nodePopup.textPanel.editBox:GetText():find("nested", 1, true),
    "node text popup did not contain the selected object")
objectPage.nodePopup.exportButton:Click()
local nodeExport = ns.GetExport(ns.GetExports().order[1])
assert(nodeExport and nodeExport.source.kind == "object_node"
        and #nodeExport.payload.content > #objectPage.nodePopup.textPanel.editBox:GetText(),
    "node export did not serialize the full subtree")
LycheeDevWindow.exportController.popup.laterButton:Click()
objectPage.nodePopup.closeButton:Click()
assert(not objectPage.nodePopup.overlay:IsShown()
        and rawget(objectPage.nodePopup.textPanel, "serializationStream") == nil,
    "closing the node text popup did not release its serialization stream")

LycheeDevWindow.pageTabs.events:Click()
local eventsPage = LycheeDevWindow.pages.events
assert(eventsPage.monitorButton.label:GetText() == ns.L.START_MONITORING,
    "event monitor did not show its start action")
assert(not eventsPage.monitorButton:IsEnabled(), "event monitor started without a selected event")
assert(not eventsPage.clearButton:IsEnabled(), "empty event log could be cleared")
assert(not eventsPage.exportDetail:IsEnabled(), "empty event detail could be exported")
local eventInputScripts = rawget(eventsPage.inputPanel.editBox, "scripts")
eventsPage.inputPanel.editBox:SetText("PLAYER_TARGET_CHANGED")
eventInputScripts.OnTextChanged(eventsPage.inputPanel.editBox)
eventInputScripts.OnEnterPressed(eventsPage.inputPanel.editBox)
assert(eventsPage.monitorButton:IsEnabled(), "event monitor did not enable after selecting an event")
assert(eventsPage.selectedPanel:GetHeight() == 42, "single selected event left excess empty space")
local monitorRunning = false
local originalMonitorStart = ns.EventMonitor.Start
local originalMonitorStop = ns.EventMonitor.Stop
local originalMonitorIsRunning = ns.EventMonitor.IsRunning
ns.EventMonitor.Start = function() monitorRunning = true return true end
ns.EventMonitor.Stop = function() monitorRunning = false end
ns.EventMonitor.IsRunning = function() return monitorRunning end
eventsPage.monitorButton:Click()
assert(eventsPage.monitorButton.label:GetText() == ns.L.STOP_MONITORING,
    "event monitor did not switch to its stop action")
assert(eventsPage.monitorButton.variant == "danger", "active event monitor did not show its stop state")
assert(not eventsPage.inputPanel.editBox:IsEnabled(), "event search stayed editable while monitoring")
eventsPage.monitorButton:Click()
assert(eventsPage.monitorButton.label:GetText() == ns.L.START_MONITORING,
    "event monitor did not return to its start action")
assert(eventsPage.inputPanel.editBox:IsEnabled(), "event search did not unlock after monitoring stopped")
ns.EventMonitor.Start = originalMonitorStart
ns.EventMonitor.Stop = originalMonitorStop
ns.EventMonitor.IsRunning = originalMonitorIsRunning

LycheeDevWindow.pageTabs.trace:Click()
local tracePage = LycheeDevWindow.pages.trace
assert(tracePage.traceButton.label:GetText() == ns.L.START_TRACE,
    "function trace did not show its start action")
assert(not tracePage.clearButton:IsEnabled(), "empty trace log could be cleared")
assert(not tracePage.exportDetail:IsEnabled(), "empty trace detail could be exported")
local traceRunning = false
local originalTraceStart = ns.FunctionTrace.Start
local originalTraceStop = ns.FunctionTrace.Stop
local originalTraceIsRunning = ns.FunctionTrace.IsRunning
local originalTraceGetActivePath = ns.FunctionTrace.GetActivePath
ns.FunctionTrace.Start = function() traceRunning = true return true end
ns.FunctionTrace.Stop = function() traceRunning = false end
ns.FunctionTrace.IsRunning = function() return traceRunning end
ns.FunctionTrace.GetActivePath = function() return "Test.Trace" end
tracePage.traceButton:Click()
assert(tracePage.traceButton.label:GetText() == ns.L.STOP_TRACE,
    "function trace did not switch to its stop action")
tracePage.traceButton:Click()
assert(tracePage.traceButton.label:GetText() == ns.L.START_TRACE,
    "function trace did not return to its start action")
ns.FunctionTrace.Start = originalTraceStart
ns.FunctionTrace.Stop = originalTraceStop
ns.FunctionTrace.IsRunning = originalTraceIsRunning
ns.FunctionTrace.GetActivePath = originalTraceGetActivePath

LycheeDevWindow.pageTabs.performance:Click()
local performancePage = LycheeDevWindow.pages.performance
for _, tab in pairs(performancePage.modeTabs) do
    assert(tab:GetWidth() >= tab.label:GetStringWidth() + 22,
        "performance mode tab did not preserve text padding")
end
for _, tab in pairs(performancePage.resultTabs) do
    assert(tab:GetWidth() >= tab.label:GetStringWidth() + 22,
        "performance result tab did not preserve text padding")
end
assert(performancePage.modeTabs.capture.point[4] == 8
        and performancePage.resultTabs.cpu.point[4] == 8,
    "performance tabs did not use consistent visual gaps")
assert(performancePage.captureButton:GetWidth() >= 140,
    "capture button did not reserve its longest state label")
assert(performancePage:IsShown() and performancePage.modeTabs.health.active,
    "performance lab was not split into its own main page")
assert(performancePage.healthRows[1] and performancePage.healthRows[1]:IsShown()
        and performancePage.healthReport.editBox:GetText() ~= "",
    "performance health view did not render native profiler evidence")
assert(not performancePage.modeTabs.capture:IsEnabled()
        and not performancePage.changeAddon:IsShown(),
    "performance detail views were available before selecting an addon")
local otherAddonRow
for index = 1, #performancePage.healthRows do
    if performancePage.healthRows[index].entry.name == "OtherAddOn" then
        otherAddonRow = performancePage.healthRows[index]
        break
    end
end
assert(otherAddonRow, "performance list did not expose the target addon")
otherAddonRow:Click()
assert(performancePage.modeTabs.capture:IsEnabled()
        and performancePage.changeAddon:IsShown()
        and performancePage.views.capture:IsShown(),
    "selecting an addon did not establish the shared performance context")
OtherAddOn = { Refresh = function() return true end }
OtherDB = { records = {} }
performancePage.captureButton:Click()
OtherDB.records[1] = { value = "test" }
assert(ns.Performance.IsCapturing() and activeTicker and not activeTicker.cancelled
        and LycheeDevCaptureDock:IsShown() and not LycheeDevWindow:IsShown(),
    "performance capture did not minimize the window into the Lychee logo")
LycheeDevCaptureDock:Click()
assert(not ns.Performance.IsCapturing() and activeTicker.cancelled
        and LycheeDevWindow:IsShown() and performancePage.sessionRows[1]:IsShown(),
    "performance capture did not stop, restore, and retain its session")
performancePage.resultTabs.cpu:Click()
assert(performancePage.captureReport.editBox:GetText():find(ns.L.PERFORMANCE_FUNCTION_HOTSPOTS, 1, true),
    "performance CPU view did not expose function hotspots")
performancePage.resultTabs.objects:Click()
assert(performancePage.captureReport.editBox:GetText():find(ns.L.PERFORMANCE_CLOSURE_ANALYSIS, 1, true),
    "performance object view did not expose object and closure analysis")
performancePage.resultTabs.storage:Click()
assert(performancePage.captureReport.editBox:GetText():find(ns.L.PERFORMANCE_STORAGE_DELTA, 1, true)
        and performancePage.captureReport.editBox:GetText():find(ns.L.PERFORMANCE_STORAGE_ESTIMATE_NOTE, 1, true),
    "performance storage view did not expose capture-correlated data")
performancePage.resultTabs.full:Click()
assert(performancePage.captureReport.editBox:GetText():find(ns.L.PERFORMANCE_RESULT_CPU, 1, true)
        and performancePage.captureReport.editBox:GetText():find(ns.L.PERFORMANCE_RESULT_OBJECTS, 1, true),
    "performance full report omitted deep evidence sections")
TestPerformanceFunction = function() return true end
performancePage.modeTabs.functionLab:Click()
performancePage.functionPath.editBox:SetText("TestPerformanceFunction")
performancePage.runBenchmark:Click()
assert(performancePage.benchmarkReport.editBox:GetText():find("1.0 KB", 1, true),
    "function experiment did not report exact allocation data")
assert(performancePage.benchmarkReport.editBox:GetText():find("OtherAddOn", 1, true),
    "advanced experiment did not retain the selected addon context")
assert(rawget(performancePage, "captureAddon") == nil
        and rawget(performancePage, "storageAddon") == nil
        and rawget(performancePage.modeTabs, "objects") == nil
        and rawget(performancePage.modeTabs, "storage") == nil,
    "performance workflow retained conflicting or cross-page tools")
performancePage.changeAddon:Click()
assert(performancePage.views.health:IsShown()
        and not performancePage.changeAddon:IsShown(),
    "change addon action did not return to addon selection")
local lycheeAddonRow
for index = 1, #performancePage.healthRows do
    if performancePage.healthRows[index].entry.name == "Lychee Dev" then
        lycheeAddonRow = performancePage.healthRows[index]
        break
    end
end
assert(lycheeAddonRow, "performance list did not expose Lychee Dev")
lycheeAddonRow:Click()
performancePage.modeTabs.capture:Click()
assert(not performancePage.sessionRows[1]:IsShown(),
    "capture history leaked sessions from another addon context")
performancePage.captureButton:Click()
LycheeDevCaptureDock:Click()
performancePage.resultTabs.storage:Click()
local noStorageReport = performancePage.captureReport.editBox:GetText()
assert(noStorageReport:find(ns.L.PERFORMANCE_STORAGE_NOT_DECLARED, 1, true)
        and not noStorageReport:find(ns.L.PERFORMANCE_STORAGE_DELTA_TOTAL, 1, true),
    "undeclared SavedVariables were rendered as a misleading zero-byte disk change")

LycheeDevWindow.pageTabs.diagnostics:Click()
local diagnosticsPage = LycheeDevWindow.pages.diagnostics
assert(rawget(diagnosticsPage, "performanceTab") == nil and rawget(diagnosticsPage, "errorsTab") == nil,
    "diagnostics still contained the old nested performance tabs")
assert(diagnosticsPage.currentScope.variant == "selected" and diagnosticsPage.allScope.variant == "secondary",
    "diagnostic scope did not expose its selected state")
assert(not diagnosticsPage.selectReport:IsEnabled(), "empty diagnostic report could be selected")
assert(not diagnosticsPage.exportReport:IsEnabled(), "empty diagnostic report could be exported")
assert(not diagnosticsPage.clearButton:IsEnabled(), "empty diagnostic error list could be cleared")

for index = 1, 12 do
    ns.AddExport("test", "测试记录 " .. index, "record " .. index)
end
LycheeDevWindow.pageTabs.exports:Click()
local exportPage = LycheeDevWindow.pages.exports
local exportCountBeforeDelete = ns.GetExportStats()
assert(exportPage.ticketBox:GetText() == ns.GetExports().order[1]
        and exportPage.rows[1]:IsShown()
        and exportPage.rows[1].background and exportPage.rows[1].divider
        and exportPage.rows[1].status:GetText() == ns.L.EXPORT_STATUS_PENDING
        and exportPage.detailStatus:GetText() == ns.L.EXPORT_STATUS_PENDING
        and exportPage.pendingText:GetText() ~= "",
    "export records page did not select the newest record")
local exportWheel = rawget(exportPage.listScroll, "scripts").OnMouseWheel
exportWheel(exportPage.listScroll, -1)
assert(exportPage.listScroll:GetVerticalScroll() > 0,
    "export record list did not respond to the mouse wheel")
exportPage.rows[2]:Click()
assert(exportPage.ticketBox:GetText() == exportPage.rows[2].ticket,
    "export record rows could not be selected")
assert(rawget(exportPage, "selectTicket") == nil and exportPage.ticketBox.focused
        and exportPage.ticketBox.highlighted
        and exportPage.ticketHint:GetText() == ns.L.EXPORT_TICKET_HELP,
    "export record did not select its Ticket by default")
exportPage.deleteButton:Click()
assert(exportPage.deleteButton.label:GetText() == ns.L.EXPORT_RECORD_DELETE_CONFIRM,
    "export record deletion did not require confirmation")
exportPage.deleteButton:Click()
assert(ns.GetExportStats() == exportCountBeforeDelete - 1,
    "selected export record was not deleted")
reloadCalled = false
exportPage.reloadButton:Click()
assert(reloadCalled, "export records page reload action did not call ReloadUI")
local exportNextId = ns.GetExports().nextId
assert(exportPage.clearButton:IsEnabled(), "nonempty export cache could not be cleared")
exportPage.clearButton:Click()
assert(exportPage.clearButton.label:GetText() == ns.L.CONFIRM_CLEAR_CACHE,
    "export cache clear did not require confirmation")
exportPage.clearButton:Click()
local remainingExports = ns.GetExportStats()
assert(remainingExports == 0 and ns.GetExports().nextId == exportNextId,
    "export cache clear did not remove records or changed the ticket sequence")
assert(ns.GetPendingExportCount() == 0 and not exportPage.reloadButton:IsEnabled()
        and exportPage.pendingText:GetText() == "",
    "clearing exports did not clear the pending disk-write state")

LycheeDevWindow.pageTabs.about:Click()
local aboutPage = LycheeDevWindow.pages.about
assert(aboutPage.metaItems.clients.value.point[3]
        ~= aboutPage.metaItems.command.value.point[3],
    "supported clients did not receive a dedicated metadata row")
assert(aboutPage.metaItems.clients.value:GetWidth() == 920,
    "supported clients did not receive the full metadata width")
assert(rawget(aboutPage, "linkPopup") == nil,
    "about page created the repository popup before it was needed")
aboutPage.githubButton:Click()
assert(aboutPage.linkPopup and aboutPage.linkPopup:IsShown()
        and aboutPage.linkBackdrop:IsShown(),
    "GitHub button did not open the repository popup")
assert(aboutPage.urlBox:GetText() == "https://github.com/Follen/Lychee-Dev",
    "repository popup did not show the correct address")
rawget(aboutPage.urlBox, "scripts").OnEscapePressed(aboutPage.urlBox)
assert(not aboutPage.linkPopup:IsShown() and not aboutPage.linkBackdrop:IsShown(),
    "Escape did not close the repository popup")
aboutPage.githubButton:Click()
aboutPage.linkBackdrop:Click()
assert(not aboutPage.linkPopup:IsShown() and not aboutPage.linkBackdrop:IsShown(),
    "clicking outside did not close the repository popup")
aboutPage.githubButton:Click()
LycheeDevWindow.pageTabs.runner:Click()
assert(not aboutPage.linkPopup:IsShown() and not aboutPage.linkBackdrop:IsShown(),
    "leaving the About page did not close the repository popup")
LycheeDevWindow.pageTabs.about:Click()

local foundLogo, foundGitHub
for index = 1, #textures do
    if textures[index] == "Interface\\AddOns\\Lychee Dev\\Media\\Logo.png" then
        foundLogo = true
    elseif textures[index] == "Interface\\AddOns\\Lychee Dev\\Media\\GitHub.png" then
        foundGitHub = true
    end
end
assert(foundLogo, "logo texture was not loaded from the addon")
assert(foundGitHub, "about page did not load the GitHub icon")

SlashCmdList.LYCHEEDEV()
assert(not LycheeDevWindow:IsShown(), "second /dev did not close the window")

SlashCmdList.LYCHEEDEV()
assert(LycheeDevWindow:IsShown(), "window did not reopen")
inCombat = true
ns.ShutdownForCombat()
assert(not LycheeDevWindow:IsShown(), "combat shutdown did not close the window")
SlashCmdList.LYCHEEDEV()
assert(not LycheeDevWindow:IsShown(), "/dev opened the window during combat")

print("Lychee Dev UI tests passed")
