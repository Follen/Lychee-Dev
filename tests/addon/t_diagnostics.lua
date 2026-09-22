-- Diagnostics feature tests: !BugGrabber scope filters, agent reports, the
-- machine snapshot contract and the Diagnostics page state. Self-contained
-- Lua 5.1 script: run `lua tests/addon/t_diagnostics.lua` from the repository
-- root (or from addon/).
local inCombat = false
local now = 10
local secretValues = {}

function InCombatLockdown() return inCombat end
function issecretvalue(value) return secretValues[value] == true end
function GetTime() return now end
function time() return 1700000000 end
function date(_, value) return "date:" .. tostring(value) end
function GetBuildInfo() return "12.1.0", "70000", "Aug 19 2026", 120100 end
function GetLocale() return "enUS" end
function wipe(target) for key in pairs(target) do target[key] = nil end end

local pendingTimers = {}
C_Timer = {
    After = function(_, callback) pendingTimers[#pendingTimers + 1] = callback end,
}
local function FlushTimers()
    while #pendingTimers > 0 do
        local batch = pendingTimers
        pendingTimers = {}
        for index = 1, #batch do
            batch[index]()
        end
    end
end

ChatFontNormal = {}
GameFontNormal = {}
GameFontNormalLarge = {}
GameFontHighlightSmall = {}
GameFontDisableSmall = {}

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

    function region:GetName() return rawget(self, "name") end
    function region:SetSize(width, height) self.width = width self.height = height end
    function region:SetWidth(width) self.width = width end
    function region:SetHeight(height) self.height = height end
    function region:GetWidth() return rawget(self, "width") or 0 end
    function region:GetHeight() return rawget(self, "height") or 0 end
    function region:SetPoint(...) self.point = { ... } end
    function region:ClearAllPoints() self.point = nil end
    function region:SetAllPoints(target) self.allPoints = target or true end
    function region:SetText(text) self.text = text or "" end
    function region:GetText() return rawget(self, "text") or "" end
    function region:SetJustifyH() end
    function region:SetWordWrap() end
    function region:SetTextColor() end
    function region:SetFontObject() end
    function region:SetTextInsets() end
    function region:SetMultiLine() end
    function region:SetAutoFocus() end
    function region:SetCountInvisibleLetters() end
    function region:SetCursorPosition(position) self.cursorPosition = position end
    function region:SetFocus() self.focused = true end
    function region:ClearFocus() self.focused = false end
    function region:HasFocus() return rawget(self, "focused") == true end
    function region:HighlightText() self.highlighted = true end
    function region:SelectAll() self.focused = true self.highlighted = true end
    function region:GetStringHeight() return 14 end
    function region:GetStringWidth() return #(rawget(self, "text") or "") * 7 end
    function region:CreateTexture() return NewRegion() end
    function region:CreateFontString() return NewRegion() end
    function region:SetTexture(path) self.texture = path end
    function region:SetTexCoord() end
    function region:SetAlpha(alpha) self.alpha = alpha end
    function region:SetRotation() end
    function region:SetColorTexture() end
    function region:SetBackdrop(backdrop) self.backdrop = backdrop end
    function region:SetBackdropColor() end
    function region:SetBackdropBorderColor() end
    function region:SetFrameStrata(strata) self.strata = strata end
    function region:GetFrameStrata() return rawget(self, "strata") or "MEDIUM" end
    function region:SetFrameLevel(level) self.frameLevel = level end
    function region:GetFrameLevel() return rawget(self, "frameLevel") or 1 end
    function region:EnableMouse() end
    function region:EnableMouseWheel() end
    function region:SetClipsChildren() end
    function region:SetClampedToScreen() end
    function region:SetMovable() end
    function region:RegisterForDrag() end
    function region:RegisterForClicks() end
    function region:StartMoving() end
    function region:StopMovingOrSizing() end
    function region:EnableKeyboard(enabled) self.keyboardEnabled = enabled end
    function region:SetPropagateKeyboardInput(propagate) self.propagateKeyboardInput = propagate end
    function region:SetEnabled(enabled) self.enabled = enabled and true or false end
    function region:IsEnabled() return rawget(self, "enabled") ~= false end
    function region:SetScript(scriptName, handler)
        local scripts = rawget(self, "scripts")
        if not scripts then
            scripts = {}
            rawset(self, "scripts", scripts)
        end
        scripts[scriptName] = handler
    end
    function region:RegisterEvent(event)
        local events = rawget(self, "events")
        if not events then
            events = {}
            rawset(self, "events", events)
        end
        events[event] = true
    end
    function region:UnregisterAllEvents()
        rawset(self, "events", {})
    end
    function region:Hide()
        local wasShown = rawget(self, "shown")
        self.shown = false
        local scripts = rawget(self, "scripts")
        if wasShown and scripts and scripts.OnHide then scripts.OnHide(self) end
    end
    function region:Show()
        local wasShown = rawget(self, "shown")
        self.shown = true
        local scripts = rawget(self, "scripts")
        if not wasShown and scripts and scripts.OnShow then scripts.OnShow(self) end
    end
    function region:SetShown(shown) if shown then self:Show() else self:Hide() end end
    function region:IsShown() return rawget(self, "shown") == true end
    function region:GetVerticalScroll() return rawget(self, "verticalScroll") or 0 end
    function region:SetVerticalScroll(offset)
        self.verticalScroll = offset
        local scripts = rawget(self, "scripts")
        if scripts and scripts.OnVerticalScroll then scripts.OnVerticalScroll(self, offset) end
    end
    function region:SetScrollChild(child) self.scrollChild = child end
    function region:GetScrollChild() return rawget(self, "scrollChild") end
    function region:GetVerticalScrollRange()
        local child = rawget(self, "scrollChild")
        local childHeight = child and child:GetHeight() or 0
        return math.max(0, childHeight - (self:GetHeight() or 0))
    end
    function region:SetMinMaxValues(minimum, maximum) self.minimumValue = minimum self.maximumValue = maximum end
    function region:GetMinMaxValues() return rawget(self, "minimumValue") or 0, rawget(self, "maximumValue") or 0 end
    function region:SetOrientation() end
    function region:SetThumbTexture(texture) self.thumb = texture end
    function region:SetHitRectInsets() end
    function region:SetObeyStepOnDrag() end
    function region:SetValue(value)
        self.value = value
        local scripts = rawget(self, "scripts")
        if scripts and scripts.OnValueChanged then scripts.OnValueChanged(self, value) end
    end
    function region:GetValue() return rawget(self, "value") or 0 end
    function region:Click()
        if not self:IsEnabled() then return end
        local scripts = rawget(self, "scripts")
        if scripts and scripts.OnClick then scripts.OnClick(self, "LeftButton") end
    end

    setmetatable(region, {
        __index = function(target, key)
            local noOp = function() end
            rawset(target, key, noOp)
            return noOp
        end,
    })
    return region
end

function CreateFrame(frameType, name, _, template)
    local frame = NewRegion(name)
    frame.frameType = frameType
    frame.hasBackdrop = template == "BackdropTemplate"
    if name then
        _G[name] = frame
    end
    return frame
end

UIParent = NewRegion("UIParent")

-- !BugGrabber provider fixture: SOFT integration, read under pcall.
local grabberDB = {
    { message = "OtherAddOn failed", stack = "AddOns/OtherAddOn/Core.lua:20", counter = 1, time = 1699999998, session = 1 },
    { message = "Lychee exploded", stack = "AddOns/Lychee Dev/Core.lua:42", locals = "value = nil", counter = 3, time = 1699999999, session = 2 },
    { message = "Network warning", stack = "AddOns/Network/UI.lua:8", counter = 2, time = 1700000000, session = 2 },
}
local resetCalled = false
BugGrabber = {
    version = "1.2.3",
    GetDB = function() return grabberDB end,
    GetSessionId = function() return 2 end,
    IsPaused = function() return false end,
    Reset = function()
        resetCalled = true
        wipe(grabberDB)
    end,
}

local registeredCallbacks = {}
EventRegistry = {
    RegisterCallback = function(_, event, callback, owner)
        registeredCallbacks[event] = { callback = callback, owner = owner }
    end,
    UnregisterCallback = function(_, event, owner)
        local current = registeredCallbacks[event]
        if current and current.owner == owner then
            registeredCallbacks[event] = nil
        end
    end,
}

local function FindAddonRoot()
    local candidates = { "addon", "../addon", "../../addon", "." }
    for index = 1, #candidates do
        local handle = io.open(candidates[index] .. "/Core/Locale.lua", "r")
        if handle then
            handle:close()
            return candidates[index]
        end
    end
    error("addon root not found; run from the repository root or addon/")
end

local ROOT = FindAddonRoot()
local function LoadAddonFile(path, namespace)
    local chunk, loadError = loadfile(ROOT .. "/" .. path)
    assert(chunk, loadError)
    return chunk("Lychee Dev", namespace)
end

local ns = {}
LoadAddonFile("Core/Locale.lua", ns)
LoadAddonFile("Core/Locale_enUS.lua", ns)
LoadAddonFile("Core/Serializer.lua", ns)
LoadAddonFile("Core/Inspector.lua", ns)
LoadAddonFile("Core/Safety.lua", ns)
LoadAddonFile("Core/Compat.lua", ns)
LoadAddonFile("Modules/ObjectInspector.lua", ns)
LoadAddonFile("Modules/Diagnostics.lua", ns)

-- Scope filters: current session 2 / all records 3 / keyword 1, newest first.
local gotErrors, current = ns.Diagnostics.GetErrors("current", "")
assert(gotErrors and #current.errors == 2, "current BugGrabber session filter failed")
assert(current.errors[1].message == "Network warning" and current.session == 2,
    "current BugGrabber errors were not newest first")
local gotAll, all = ns.Diagnostics.GetErrors("all", "")
assert(gotAll and #all.errors == 3, "all BugGrabber sessions were not returned")
assert(all.errors[1].message == "Network warning" and all.errors[3].message == "OtherAddOn failed",
    "all BugGrabber errors were not newest first")
local gotFiltered, filtered = ns.Diagnostics.GetErrors("all", "  lychee  ")
assert(gotFiltered and #filtered.errors == 1, "error keyword filter failed")

-- Agent report contents: message, stack, locals and the context blocks.
local report = ns.Diagnostics.FormatAgentReport(filtered.errors[1])
assert(report:find(ns.L.AGENT_REPORT_TITLE, 1, true), "agent report omitted its title")
assert(report:find(ns.L.REPORT_CONTEXT, 1, true), "agent report omitted its context block")
assert(report:find(ns.L.REPORT_ENVIRONMENT, 1, true), "agent report omitted its environment block")
assert(report:find("Lychee exploded", 1, true), "agent report omitted the error message")
assert(report:find("AddOns/Lychee Dev/Core.lua:42", 1, true), "agent report omitted the stack")
assert(report:find("value = nil", 1, true), "agent report omitted locals")
assert(report:find("date:1699999999", 1, true), "agent report omitted the error time")
assert(report:find("Lychee Dev", 1, true), "agent report omitted the detected source")
local noStackReport = ns.Diagnostics.FormatAgentReport({ message = "boom", counter = 1, time = 1, session = 2 })
assert(noStackReport:find(ns.L.REPORT_NOT_AVAILABLE, 1, true),
    "agent report did not mark a missing stack")
assert(ns.Diagnostics.FormatAgentReport("not a table") == ns.L.SELECT_ERROR_DETAIL,
    "agent report did not guard a non-table entry")

-- Report field caps: 48 KB with a visible marker, secrets masked.
local hugeReport = ns.Diagnostics.FormatAgentReport({
    message = string.rep("m", 50000), stack = "s", locals = "l", counter = 1, time = 1, session = 2,
})
assert(hugeReport:find("\n... <truncated>", 1, true), "agent report did not mark a truncated field")
local secretEntry = {}
secretValues[secretEntry] = true
local secretReport = ns.Diagnostics.FormatAgentReport({
    message = secretEntry, stack = "s", locals = "l", counter = 1, time = 1, session = 2,
})
assert(secretReport:find("<secret>", 1, true), "agent report did not mask a secret field")

-- Source detection: explicit source, AddOns/<name>/ fallback, unknown.
assert(ns.Diagnostics.DetectSource({ source = "Interface\\AddOns\\X\\y.lua:1" })
    == "Interface\\AddOns\\X\\y.lua:1", "explicit error source was replaced")
assert(ns.Diagnostics.DetectSource({ message = "x", stack = "AddOns\\Foo\\Bar.lua:9" }) == "Foo",
    "error source fallback did not parse the addon folder")
assert(ns.Diagnostics.DetectSource({ message = "x", stack = "C stack" }) == ns.L.UNKNOWN_SOURCE,
    "unknown error source did not fall back")

-- Reset delegates to BugGrabber:Reset.
local resetOk, resetMessage = ns.Diagnostics.ResetErrors()
assert(resetOk and resetCalled and #grabberDB == 0 and resetMessage == ns.L.ERRORS_CLEARED,
    "BugGrabber reset delegation failed")

-- Missing provider is explicit, never fabricated.
local realGrabber = BugGrabber
BugGrabber = nil
local unavailableOk, _, unavailableMessage = ns.Diagnostics.GetErrors("all", "")
assert(not unavailableOk and unavailableMessage == ns.L.BUGGRABBER_UNAVAILABLE,
    "missing BugGrabber did not report BUGGRABBER_UNAVAILABLE")
local unavailableReset = ns.Diagnostics.ResetErrors()
assert(not unavailableReset, "missing BugGrabber reset did not fail")
BugGrabber = realGrabber

-- Machine snapshot contract for the future host "live bugs" entry.
grabberDB = {
    { message = "OtherAddOn failed", stack = "AddOns/OtherAddOn/Core.lua:20", counter = 1, time = 1699999998, session = 1 },
    { message = "Lychee exploded", stack = "AddOns/Lychee Dev/Core.lua:42", locals = "value = nil", counter = 3, time = 1699999999, session = 2 },
    { message = "Network warning", stack = "AddOns/Network/UI.lua:8", counter = 2, time = 1700000000, session = 2 },
}
local _, invalidCount = ns.Diagnostics.SnapshotRecentErrors(0)
assert(invalidCount == "invalid_count", "snapshot accepted a zero count")
local _, invalidHigh = ns.Diagnostics.SnapshotRecentErrors(101)
assert(invalidHigh == "invalid_count", "snapshot accepted a count above 100")
local _, invalidFraction = ns.Diagnostics.SnapshotRecentErrors(2.5)
assert(invalidFraction == "invalid_count", "snapshot accepted a fractional count")
local _, invalidType = ns.Diagnostics.SnapshotRecentErrors("3")
assert(invalidType == "invalid_count", "snapshot accepted a non-number count")

local sessionSnapshot = ns.Diagnostics.SnapshotRecentErrors(2, "current_session")
assert(sessionSnapshot.scope == "current_session"
    and sessionSnapshot.requestedCount == 2
    and sessionSnapshot.returnedCount == 2
    and sessionSnapshot.availableCount == 2
    and sessionSnapshot.ordering == "provider_storage_reverse"
    and sessionSnapshot.session == 2
    and sessionSnapshot.providerVersion == "1.2.3"
    and sessionSnapshot.capturedAt == 1700000000,
    "session snapshot metadata was incorrect")
assert(sessionSnapshot.errors[1].message == "Network warning"
    and sessionSnapshot.errors[2].message == "Lychee exploded",
    "session snapshot was not newest first")
assert(sessionSnapshot.errors[2].stack == "AddOns/Lychee Dev/Core.lua:42"
    and sessionSnapshot.errors[2].locals == "value = nil",
    "session snapshot did not copy its text fields")

local storageSnapshot = ns.Diagnostics.SnapshotRecentErrors(5, "provider_storage")
assert(storageSnapshot.scope == "provider_storage" and storageSnapshot.requestedCount == 5
    and storageSnapshot.returnedCount == 3 and storageSnapshot.availableCount == 3,
    "provider storage snapshot counts were incorrect")

-- Absent locals mark the copy incomplete; source stays optional.
local incompleteEntry = ns.Diagnostics.SnapshotRecentErrors(1, "provider_storage")
assert(incompleteEntry.complete == false and incompleteEntry.errors[1].message == "Network warning",
    "snapshot completeness was incorrect")
local foundLocalsReason = false
for index = 1, #incompleteEntry.incompleteReasons do
    if incompleteEntry.incompleteReasons[index] == "error[1].locals" then
        foundLocalsReason = true
    end
end
assert(foundLocalsReason, "snapshot did not report the missing locals field")
local completeSnapshot = ns.Diagnostics.SnapshotRecentErrors(1, "current_session")
assert(completeSnapshot.complete == false, "snapshot ignored a missing locals field")

-- Secret and non-string fields land in missingFields without substitution.
grabberDB = {
    {
        message = secretEntry, stack = { "not a string" }, locals = "l", source = "s",
        time = 1, session = 2, counter = 4,
    },
}
local secretSnapshot = ns.Diagnostics.SnapshotRecentErrors(1, "current_session")
assert(secretSnapshot.complete == false and secretSnapshot.errors[1].message == nil
    and secretSnapshot.errors[1].stack == nil,
    "snapshot substituted a secret or non-string field")
local missingByName = {}
for index = 1, #secretSnapshot.errors[1].missingFields do
    missingByName[secretSnapshot.errors[1].missingFields[index]] = true
end
assert(missingByName.message and missingByName.stack and not missingByName.source,
    "snapshot missingFields were incorrect")

-- Provider failure codes and combat refusal.
BugGrabber = { GetDB = function() error("provider failure") end, GetSessionId = function() return 2 end }
local _, providerCallCode = ns.Diagnostics.SnapshotRecentErrors(1, "current_session")
assert(providerCallCode == "provider_error", "snapshot did not report provider_error")
BugGrabber = nil
local _, providerCode = ns.Diagnostics.SnapshotRecentErrors(1, "current_session")
assert(providerCode == "provider_unavailable", "snapshot did not report provider_unavailable")
BugGrabber = realGrabber
inCombat = true
local _, combatCode = ns.Diagnostics.SnapshotRecentErrors(1, "current_session")
assert(combatCode == "combat_blocked", "snapshot did not report combat_blocked")
local combatErrors, _, combatErrorsMessage = ns.Diagnostics.GetErrors("all", "")
assert(not combatErrors and combatErrorsMessage == ns.L.COMBAT_BLOCKED, "error list did not block combat")
assert(not ns.Diagnostics.ResetErrors(), "error reset ran during combat")
inCombat = false

-- Diagnostics page state.
grabberDB = {
    { message = "OtherAddOn failed\nsecond line", stack = "AddOns/OtherAddOn/Core.lua:20", counter = 1, time = 1699999998, session = 1 },
    { message = "Lychee exploded", stack = "AddOns/Lychee Dev/Core.lua:42", locals = "value = nil", counter = 3, time = 1699999999, session = 2 },
    { message = "Network warning", stack = "AddOns/Network/UI.lua:8", counter = 2, time = 1700000000, session = 2 },
}
local savedExports = {}
local pageDefs = {}
local exportHooks = {}
ns.Workbench = {
    Layout = {
        WINDOW_WIDTH = 1040,
        WINDOW_HEIGHT = 720,
        CONTENT_LEFT = 268,
        PAGE_LEFT = 17,
        HEADING_TOP = -84,
        CONTENT_TOP = -104,
        CONTENT_RIGHT = -14,
        CONTENT_BOTTOM = 54,
    },
    RegisterPage = function(def) pageDefs[def.key] = def return def end,
    RegisterExportHook = function(hook) exportHooks[hook.key] = hook return hook end,
    RegisterShutdown = function() end,
    SaveToDisk = function(kind, title, content, metadata)
        if type(content) == "function" then
            content = content()
        end
        savedExports[#savedExports + 1] = { kind = kind, title = title, content = content, metadata = metadata }
        return "LYCHEE-TEST-0003"
    end,
    Open = function() return true end,
    Close = function() end,
    IsShown = function() return true end,
}
LoadAddonFile("UI/Widgets.lua", ns)
LoadAddonFile("UI/Pages/Diagnostics.lua", ns)

local diagnosticsDef = assert(pageDefs["diagnostics"], "diagnostics page did not register")
assert(diagnosticsDef.titleKey == "TAB_DIAGNOSTICS", "diagnostics page registered with the wrong title key")
assert(exportHooks["diagnostics"], "diagnostics page did not register its export hook")
local container = NewRegion("container")
local diagnosticsPage = diagnosticsDef.build(container)
assert(diagnosticsPage.currentScope.variant == "selected" and diagnosticsPage.allScope.variant == "secondary",
    "diagnostic scope did not expose its selected state")
assert(not diagnosticsPage.selectReport:IsEnabled(), "empty diagnostic report could be selected")
assert(not diagnosticsPage.exportReport:IsEnabled(), "empty diagnostic report could be exported")
assert(not diagnosticsPage.clearButton:IsEnabled(), "empty diagnostic error list could be cleared")
assert(registeredCallbacks["BugGrabber.BugGrabbed"] == nil,
    "diagnostics registered its live callback before activation")

diagnosticsDef.activate(diagnosticsPage)
assert(registeredCallbacks["BugGrabber.BugGrabbed"] ~= nil,
    "diagnostics did not register its live callback on activation")
assert(#diagnosticsPage.errorRows == 2, "diagnostics page did not list the current session errors")
assert(diagnosticsPage.clearButton:IsEnabled() and diagnosticsPage.exportReport:IsEnabled(),
    "diagnostics page actions stayed disabled after a refresh")
assert(not diagnosticsPage.selectReport:IsEnabled(), "diagnostics selected an error without a click")

-- Error rows: 44 px, 16-row pool, 430 px list with the legacy row content.
local wideDB = {}
for index = 1, 25 do
    wideDB[index] = {
        message = "error " .. index .. "\nsecond line", stack = "AddOns/Foo/Bar.lua:" .. index,
        counter = index, time = 1699999998 + index, session = 2,
    }
end
grabberDB = wideDB
diagnosticsPage.allScope:Click()
assert(diagnosticsPage.allScope.variant == "selected" and diagnosticsPage.currentScope.variant == "secondary",
    "diagnostic scope toggle did not switch its selected state")
assert(#diagnosticsPage.errorRows == 16, "diagnostic error list did not virtualize 16 rows")
assert(diagnosticsPage.errorRows[1]:GetHeight() == 44, "diagnostic error rows were not 44 px")
assert(diagnosticsPage.listPanel:GetWidth() == 430, "diagnostic error list width changed")
local firstRow = diagnosticsPage.errorRows[1]
assert(firstRow.message:GetText() == "error 25", "diagnostic rows did not show the first message line")
assert(firstRow.count:GetText() == "x25", "diagnostic rows did not show their counters")
assert(firstRow.metadata:GetText():find("Session 2", 1, true), "diagnostic rows did not show their session metadata")

-- Row click fills the report; Select Report focuses and highlights it.
firstRow:Click()
assert(diagnosticsPage.selectReport:IsEnabled(), "diagnostic report action stayed disabled after selection")
assert(diagnosticsPage.reportPanel.editBox:GetText():find("error 25", 1, true),
    "diagnostic row click did not fill the agent report")
diagnosticsPage.selectReport:Click()
assert(diagnosticsPage.reportPanel.editBox.focused and diagnosticsPage.reportPanel.editBox.highlighted,
    "Select Report did not focus and highlight the agent report")

-- Save exports the visible error log through the single export UI.
diagnosticsPage.exportReport:Click()
local errorExport = savedExports[#savedExports]
assert(errorExport and errorExport.kind == "error_log" and errorExport.metadata.scope == "all"
    and errorExport.metadata.query == "" and errorExport.metadata.recordCount == 25
    and errorExport.content:find("error 25", 1, true),
    "error log export did not serialize the visible errors")
local hookKind, _, hookContent, hookMetadata = exportHooks["diagnostics"].GetPayload()
assert(hookKind == "error_log" and hookMetadata.scope == "all" and hookMetadata.recordCount == 25
    and type(hookContent) == "function",
    "diagnostics export hook did not expose the visible errors")

-- Search filters the list and rides along in the export metadata.
diagnosticsPage.searchPanel.editBox:SetText("error 13")
diagnosticsPage.searchButton:Click()
assert(diagnosticsPage.errorRows[1].message:GetText() == "error 13"
    and not diagnosticsPage.errorRows[2]:IsShown(),
    "diagnostic search did not filter the error list")
diagnosticsPage.exportReport:Click()
assert(savedExports[#savedExports].metadata.query == "error 13",
    "error log export did not record the search query")
diagnosticsPage.searchPanel.editBox:SetText("")

-- Live refresh through BugGrabber.BugGrabbed is debounced onto one timer.
wideDB[#wideDB + 1] = {
    message = "error 26", stack = "AddOns/Foo/Bar.lua:26", counter = 1, time = 1700000024, session = 2,
}
local grabbed = registeredCallbacks["BugGrabber.BugGrabbed"]
grabbed.callback()
grabbed.callback()
grabbed.callback()
assert(#pendingTimers == 1, "diagnostics live refresh did not debounce its work")
FlushTimers()
assert(diagnosticsPage.errorRows[1].message:GetText() == "error 26",
    "diagnostics live refresh did not update the error list")

-- Callback lifecycle: registered on show, unregistered on hide and shutdown.
diagnosticsDef.suspend(diagnosticsPage)
assert(registeredCallbacks["BugGrabber.BugGrabbed"] == nil,
    "diagnostics kept its live callback after leaving the page")
diagnosticsDef.activate(diagnosticsPage)
assert(registeredCallbacks["BugGrabber.BugGrabbed"] ~= nil,
    "diagnostics did not re-register its live callback on activation")

-- Two-click destructive confirm, disarmed and reset on success.
resetCalled = false
diagnosticsPage.clearButton:Click()
assert(diagnosticsPage.clearButton.label:GetText() == ns.L.CONFIRM_CLEAR_ERRORS
    and diagnosticsPage.clearButton.variant == "danger" and not resetCalled,
    "diagnostic clear did not arm its confirmation")
diagnosticsPage.clearButton:Click()
assert(resetCalled and diagnosticsPage.clearButton.label:GetText() == ns.L.CLEAR_ERRORS
    and diagnosticsPage.clearButton.variant == "secondary",
    "confirmed diagnostic clear did not reset the provider")
assert(not diagnosticsPage.clearButton:IsEnabled(), "cleared diagnostic list kept its clear action enabled")

-- Combat refusal and shutdown teardown.
grabberDB = wideDB
diagnosticsDef.activate(diagnosticsPage)
local exportsBeforeCombat = #savedExports
inCombat = true
diagnosticsPage.exportReport:Click()
assert(#savedExports == exportsBeforeCombat, "error log export ran during combat")
inCombat = false
diagnosticsDef.shutdown(diagnosticsPage)
assert(registeredCallbacks["BugGrabber.BugGrabbed"] == nil,
    "window teardown did not unregister the diagnostics live callback")

print("Lychee Toolkit diagnostics tests passed")
return true

