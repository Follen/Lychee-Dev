SlashCmdList = {}
UISpecialFrames = {}

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

    function region:GetStringHeight()
        return 14
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

function CreateFrame(frameType, name)
    frameCount = frameCount + 1
    local frame = NewRegion(name)
    frame.frameType = frameType
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
C_Timer = { After = function(_, callback) callback() end }
local inCombat = false
function InCombatLockdown() return inCombat end

local function LoadAddonFile(path, namespace)
    local chunk, loadError = loadfile(path)
    assert(chunk, loadError)
    return chunk("Lychee Dev", namespace)
end

local ns = {}
LoadAddonFile("Locale.lua", ns)
LoadAddonFile("Database.lua", ns)
LoadAddonFile("Serializer.lua", ns)
LoadAddonFile("Inspector.lua", ns)
LoadAddonFile("Safety.lua", ns)
LoadAddonFile("Core.lua", ns)
LoadAddonFile("ObjectInspector.lua", ns)
LoadAddonFile("Diagnostics.lua", ns)
LoadAddonFile("FunctionTrace.lua", ns)
LoadAddonFile("EventCatalogData.lua", ns)
LoadAddonFile("EventCatalog.lua", ns)
LoadAddonFile("EventMonitor.lua", ns)
LoadAddonFile("UIFeatures.lua", ns)
LoadAddonFile("UIObjectPage.lua", ns)
LoadAddonFile("UITracePage.lua", ns)
LoadAddonFile("UIDiagnosticsPage.lua", ns)
LoadAddonFile("UIAboutPage.lua", ns)
LoadAddonFile("UI.lua", ns)

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
assert(pageCount == 6, "window did not create all six workbench pages")
assert(LycheeDevWindow.pages.runner:IsShown(), "runner page was not active by default")
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
LycheeDevWindow.pageTabs.objects:Click()
assert(LycheeDevWindow.pages.objects:IsShown(), "object page did not activate")
assert(not LycheeDevWindow.pages.runner:IsShown(), "runner page stayed visible after navigation")
local objectPage = LycheeDevWindow.pages.objects
assert(objectPage.inspectButton.variant == "primary", "object inspect action was not primary")
assert(not objectPage.selectSnapshot:IsEnabled(), "empty object snapshot could be selected")
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
local objectRootRow = objectPage.treeView.rows[1]
rawget(objectRootRow, "scripts").OnClick(objectRootRow, "RightButton")
assert(objectPage.treeView:GetSelectedNode() == objectRootRow.node,
    "object tree context action did not select its node")
assert(objectPage.nodePopup and objectPage.nodePopup.overlay:IsShown(),
    "object tree context action did not open the node text popup")
assert(objectPage.nodePopup.textPanel.editBox:GetText():find("nested", 1, true),
    "node text popup did not contain the selected object")
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

LycheeDevWindow.pageTabs.diagnostics:Click()
local diagnosticsPage = LycheeDevWindow.pages.diagnostics
assert(diagnosticsPage.errorsTab.active and not diagnosticsPage.performanceTab.active,
    "diagnostics did not expose the active view as a tab")
assert(diagnosticsPage.currentScope.variant == "selected" and diagnosticsPage.allScope.variant == "secondary",
    "diagnostic scope did not expose its selected state")
assert(not diagnosticsPage.selectReport:IsEnabled(), "empty diagnostic report could be selected")
assert(not diagnosticsPage.clearButton:IsEnabled(), "empty diagnostic error list could be cleared")

LycheeDevWindow.pageTabs.about:Click()
local aboutPage = LycheeDevWindow.pages.about
aboutPage.selectAddressButton:Click()
assert(aboutPage.selectAddressButton.label:GetText() == ns.L.ADDRESS_SELECTED,
    "about page did not confirm repository selection")
assert(aboutPage.copyHint:GetText() == ns.L.ABOUT_SELECTED_HINT,
    "about page did not explain the copy action")
rawget(aboutPage.urlBox, "scripts").OnEditFocusLost(aboutPage.urlBox)
assert(aboutPage.selectAddressButton.label:GetText() == ns.L.SELECT_ADDRESS,
    "about page did not reset repository selection feedback")

local foundLogo, foundGitHub
for index = 1, #textures do
    if textures[index] == "Interface\\AddOns\\Lychee Dev\\Media\\Logo.png" then
        foundLogo = true
    elseif textures[index] == "Interface\\AddOns\\Lychee Dev\\Media\\GitHub.png" then
        foundGitHub = true
    end
end
assert(foundLogo, "logo texture was not loaded from the addon")
assert(foundGitHub, "GitHub texture was not loaded from the addon")

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
