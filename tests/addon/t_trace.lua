-- Function trace feature tests: hook install, argument formatting, the record
-- ring, stop semantics, combat skips and the Trace page state. Self-contained
-- Lua 5.1 script: run `lua tests/addon/t_trace.lua` from the repository root
-- (or from addon/).
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

local hooks = {}
local hookInstalls = {}
function hooksecurefunc(owner, name, callback)
    hookInstalls[name] = (hookInstalls[name] or 0) + 1
    hooks[owner] = hooks[owner] or {}
    hooks[owner][name] = callback
end

C_AddOns = {
    GetAddOnInfo = function(index)
        if index == 1 then
            return "Lychee Dev", "Dev Tools"
        end
        return "OtherAddOn", "Other AddOn"
    end,
    GetAddOnMetadata = function() return "" end,
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
LoadAddonFile("Modules/FunctionTrace.lua", ns)

TestTrace = function() end
TestHolder = { Field = 1 }

-- Start resolves the path and installs exactly one hook per path.
local started = ns.FunctionTrace.Start("TestTrace")
assert(started and ns.FunctionTrace.IsRunning() and ns.FunctionTrace.GetActivePath() == "TestTrace",
    "function trace did not start")
assert(hookInstalls["TestTrace"] == 1, "function trace did not install its hook once")

-- Recorded call: bounded arguments and elapsed time.
now = 12.5
hooks[_G].TestTrace("alpha", 42)
assert(ns.FunctionTrace.GetCount() == 1, "function trace did not record a call")
local record = ns.FunctionTrace.GetRecord(1)
assert(record.arguments[1] == '"alpha"' and record.arguments[2] == "42",
    "function trace arguments were incorrect")
assert(record.summary == '"alpha", 42', "function trace summary was incorrect")
assert(record.elapsed == 2.5, "function trace elapsed time was incorrect")
assert(record.path == "TestTrace", "function trace record did not keep its path")

-- Argument caps: 16 arguments plus the overflow marker.
hooks[_G].TestTrace(1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17)
local overflowRecord = ns.FunctionTrace.GetRecord(1)
assert(#overflowRecord.arguments == 17 and overflowRecord.arguments[17] == "<1 more arguments>",
    "function trace did not mark overflow arguments")

-- Argument formatting: 180 byte strings, tables and secrets.
local secretArgument = {}
secretValues[secretArgument] = true
hooks[_G].TestTrace(string.rep("a", 300), { nested = true }, secretArgument)
local formatRecord = ns.FunctionTrace.GetRecord(1)
assert(#formatRecord.arguments[1] == 183 and formatRecord.arguments[1]:sub(-3) == "...",
    "function trace argument text was not bounded at 180 bytes")
assert(formatRecord.arguments[2] == "<table>", "function trace table argument was not collapsed")
assert(formatRecord.arguments[3] == "<secret>", "function trace secret argument was not masked")

-- Hooks stay installed but Stop disables recording.
ns.FunctionTrace.Stop()
assert(not ns.FunctionTrace.IsRunning() and ns.FunctionTrace.GetActivePath() == nil,
    "stopped function trace kept its active path")
hooks[_G].TestTrace("ignored")
assert(ns.FunctionTrace.GetCount() == 3, "stopped function trace continued recording")
local restarted = ns.FunctionTrace.Start("TestTrace")
assert(restarted and hookInstalls["TestTrace"] == 1,
    "function trace re-installed a hook that was already present")
hooks[_G].TestTrace("recorded")
assert(ns.FunctionTrace.GetCount() == 4, "restarted function trace did not record again")
ns.FunctionTrace.Stop()

-- Clear wipes the ring.
ns.FunctionTrace.Clear()
assert(ns.FunctionTrace.GetCount() == 0 and ns.FunctionTrace.GetRecord(1) == nil,
    "function trace clear did not wipe the records")

-- Ring bound: 300 records, newest first.
assert(ns.FunctionTrace.Start("TestTrace"), "function trace restart failed")
for index = 1, 305 do
    hooks[_G].TestTrace(index)
end
assert(ns.FunctionTrace.GetCount() == 300, "function trace ring did not cap at 300 records")
assert(ns.FunctionTrace.GetRecord(1).arguments[1] == "305", "function trace newest record was wrong")
assert(ns.FunctionTrace.GetRecord(300).arguments[1] == "6", "function trace ring did not drop the oldest record")
assert(ns.FunctionTrace.GetRecord(301) == nil, "function trace exposed an out-of-range record")
ns.FunctionTrace.Stop()

-- Invalid and unhookable targets.
local invalidOk, invalidMessage = ns.FunctionTrace.Start("TestTrace[1]")
assert(not invalidOk and invalidMessage == ns.L.FUNCTION_PATH_INVALID,
    "function trace accepted a non-string function key")
local secretPath = {}
secretValues[secretPath] = true
local secretOk, secretMessage = ns.FunctionTrace.Start(secretPath)
assert(not secretOk and secretMessage == ns.L.FUNCTION_PATH_INVALID,
    "function trace accepted a secret path")
local missingOk, missingMessage = ns.FunctionTrace.Start("TestHolder.Field")
assert(not missingOk and missingMessage == ns.L.FUNCTION_NOT_FOUND,
    "function trace accepted a non-function target")

TestTrace2 = function() end
local realHooksecurefunc = hooksecurefunc
hooksecurefunc = function() error("hook install failed") end
local hookFailOk, hookFailMessage = ns.FunctionTrace.Start("TestTrace2")
hooksecurefunc = realHooksecurefunc
assert(not hookFailOk and hookFailMessage == ns.L.FUNCTION_HOOK_FAILED,
    "function trace did not report a failed hook install")
assert(not ns.FunctionTrace.IsRunning() and ns.FunctionTrace.GetActivePath() == nil,
    "failed hook install left partial trace state")

-- Combat: start refused, recording skipped.
inCombat = true
local combatStart, combatStartMessage = ns.FunctionTrace.Start("TestTrace")
assert(not combatStart and combatStartMessage == ns.L.COMBAT_BLOCKED, "function trace started during combat")
inCombat = false
ns.FunctionTrace.Clear()
assert(ns.FunctionTrace.Start("TestTrace"), "function trace restart after combat failed")
local countBeforeCombatCall = ns.FunctionTrace.GetCount()
inCombat = true
hooks[_G].TestTrace("combat-ignored")
assert(ns.FunctionTrace.GetCount() == countBeforeCombatCall, "function trace recorded during combat")
inCombat = false
hooks[_G].TestTrace("after-combat")
assert(ns.FunctionTrace.GetCount() == countBeforeCombatCall + 1, "function trace skipped a call after combat")
ns.FunctionTrace.Stop()
ns.FunctionTrace.Clear()

-- Trace page state.
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
        return "LYCHEE-TEST-0002"
    end,
    Open = function() return true end,
    Close = function() end,
    IsShown = function() return true end,
}
LoadAddonFile("UI/Widgets.lua", ns)
LoadAddonFile("UI/Pages/Trace.lua", ns)

local traceDef = assert(pageDefs["trace"], "trace page did not register")
assert(traceDef.titleKey == "TAB_TRACE", "trace page registered with the wrong title key")
assert(exportHooks["trace"], "trace page did not register its export hook")
assert(exportHooks["trace"].GetPayload() == nil, "empty trace log exposed an export payload")
local container = NewRegion("container")
local tracePage = traceDef.build(container)
assert(tracePage.traceButton.label:GetText() == ns.L.START_TRACE,
    "function trace did not show its start action")
assert(not tracePage.clearButton:IsEnabled(), "empty trace log could be cleared")
assert(not tracePage.exportDetail:IsEnabled(), "empty trace detail could be exported")

tracePage.traceButton:Click()
assert(ns.FunctionTrace.IsRunning() and ns.FunctionTrace.GetActivePath() == "C_AddOns.GetAddOnInfo",
    "trace page did not start the typed function path")
assert(tracePage.traceButton.label:GetText() == ns.L.STOP_TRACE,
    "function trace did not switch to its stop action")
assert(tracePage.traceButton.variant == "danger", "active function trace did not show its stop state")

for index = 1, 20 do
    hooks[C_AddOns].GetAddOnInfo("call-" .. index)
end
FlushTimers()
assert(#tracePage.callRows == 18, "trace call list did not virtualize 18 rows")
assert(tracePage.callRows[1]:GetHeight() == 38, "trace call rows were not 38 px")
assert(tracePage.listPanel:GetWidth() == 520, "trace call list width changed")
assert(tracePage.callRows[1]:IsShown() and tracePage.clearButton:IsEnabled() and tracePage.exportDetail:IsEnabled(),
    "trace page actions did not enable after a recorded call")

tracePage.callRows[1]:Click()
local detailText = tracePage.detailPanel.editBox:GetText()
assert(detailText:find(string.format(ns.L.TRACE_PATH, "C_AddOns.GetAddOnInfo"), 1, true),
    "trace detail pane omitted the traced path")
assert(detailText:find('[1] = "call-20"', 1, true), "trace detail pane omitted the arguments")

-- Save exports the bounded newest-first record set through the export UI.
tracePage.exportDetail:Click()
local traceExport = savedExports[#savedExports]
assert(traceExport and traceExport.kind == "function_trace" and traceExport.metadata.recordCount == 20
    and traceExport.content:find("GetAddOnInfo", 1, true),
    "trace export did not serialize the recorded calls")
local hookKind, _, hookContent, hookMetadata = exportHooks["trace"].GetPayload()
assert(hookKind == "function_trace" and hookMetadata.recordCount == 20
    and type(hookContent) == "function",
    "trace export hook did not expose the recorded calls")

-- Stop semantics from the page, then Clear.
tracePage.traceButton:Click()
assert(not ns.FunctionTrace.IsRunning() and tracePage.traceButton.label:GetText() == ns.L.START_TRACE
    and tracePage.traceButton.variant == "primary",
    "function trace did not return to its start action")
hooks[C_AddOns].GetAddOnInfo("after-stop")
assert(ns.FunctionTrace.GetCount() == 20, "stopped function trace kept recording")

tracePage.clearButton:Click()
assert(ns.FunctionTrace.GetCount() == 0 and not tracePage.clearButton:IsEnabled()
    and not tracePage.exportDetail:IsEnabled(),
    "trace clear did not empty the log and disable its actions")
assert(tracePage.detailPanel.editBox:GetText() == ns.L.SELECT_CALL_DETAIL,
    "trace clear did not reset the detail pane")
assert(exportHooks["trace"].GetPayload() == nil, "cleared trace log exposed an export payload")

-- Page stop on window hide: shutdown disables recording.
tracePage.traceButton:Click()
assert(ns.FunctionTrace.IsRunning(), "trace page did not restart the trace")
traceDef.shutdown(tracePage)
assert(not ns.FunctionTrace.IsRunning(), "window teardown did not stop the function trace")

print("Lychee Toolkit trace tests passed")
return true

