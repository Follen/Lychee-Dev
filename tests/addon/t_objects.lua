-- Objects feature tests: path grammar, inspection streaming, ranked search and
-- the Object page state (picker capture, lazy children, node popup stream
-- release). Self-contained Lua 5.1 script: run `lua tests/addon/t_objects.lua`
-- from the repository root (or from addon/).
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
    function region:RegisterEvent(event) self.events = self.events or {} self.events[event] = true end
    function region:UnregisterAllEvents() self.events = {} end
    function region:Hide()
        local wasShown = self.shown
        self.shown = false
        local scripts = rawget(self, "scripts")
        if wasShown and scripts and scripts.OnHide then scripts.OnHide(self) end
    end
    function region:Show()
        local wasShown = self.shown
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
    function region:GetMinMaxValues() return self.minimumValue, self.maximumValue end
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

local mouseFocus = {
    GetObjectType = function() return "Frame" end,
    GetName = function() return "TargetFrame" end,
}
local childFocus = {
    GetObjectType = function() return "Button" end,
    GetName = function() return "TargetChild" end,
}
mouseFocus.GetChildren = function() return childFocus end
mouseFocus.GetRegions = function() return end
function GetMouseFoci() return { mouseFocus } end

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

-- Path grammar and error codes.
TestRoot = {
    Alpha = 1,
    Alphabet = 2,
    BetaAlpha = 3,
    Nested = { Value = "ok", Controls = { UpdateAddButton = function() end } },
}
local resolved, value = ns.ObjectInspector.ResolvePath("_G.TestRoot.Nested.Value")
assert(resolved and value == "ok", "object path did not resolve")
local bracketResolved, bracketValue = ns.ObjectInspector.ResolvePath('TestRoot.Nested["Value"]')
assert(bracketResolved and bracketValue == "ok", 'quoted ["key"] path did not resolve')
local singleResolved, singleValue = ns.ObjectInspector.ResolvePath("TestRoot.Nested['Value']")
assert(singleResolved and singleValue == "ok", "quoted ['key'] path did not resolve")
local indexedResolved, _, indexedError = ns.ObjectInspector.ResolvePath("TestRoot.Nested.List[1]")
assert(not indexedResolved and indexedError == ns.L.OBJECT_NOT_FOUND, "missing index reported the wrong error")
TestRoot.Nested.List = { "first" }
local indexOk, indexedValue = ns.ObjectInspector.ResolvePath("TestRoot.Nested.List[1]")
assert(indexOk and indexedValue == "first", "numeric index path did not resolve")
local missingOk, _, missingError = ns.ObjectInspector.ResolvePath("TestRoot.Missing")
assert(not missingOk and missingError == ns.L.OBJECT_NOT_FOUND, "missing member did not report OBJECT_NOT_FOUND")
local invalidOk, _, invalidError = ns.ObjectInspector.ResolvePath("TestRoot..bad")
assert(not invalidOk and invalidError == ns.L.OBJECT_PATH_INVALID, "invalid path did not report OBJECT_PATH_INVALID")
local emptyOk, _, emptyError = ns.ObjectInspector.ResolvePath("   ")
assert(not emptyOk and emptyError == ns.L.OBJECT_PATH_REQUIRED, "empty path did not report OBJECT_PATH_REQUIRED")
TestRoot.Fn = function() end
local unreadableOk, _, unreadableError = ns.ObjectInspector.ResolvePath("TestRoot.Fn.Field")
assert(not unreadableOk and unreadableError == ns.L.OBJECT_PATH_UNREADABLE,
    "unreadable member did not report OBJECT_PATH_UNREADABLE")

-- Inspection keeps its incremental text stream and a live value tree.
local inspected, inspection = ns.ObjectInspector.InspectPath("TestRoot")
assert(inspected and inspection.textStream, "object inspection did not retain an incremental text stream")
assert(inspection.label == "TestRoot" and inspection.valueType == "table" and inspection.value == TestRoot,
    "object inspection did not keep label, type and value")
assert(inspection.text:find("Object: TestRoot", 1, true), "object text header was missing")
assert(inspection.tree and inspection.tree.roots and inspection.tree.roots[1].source == TestRoot,
    "object inspection did not build a live value tree")
local bigObject = { nested = { value = 7 } }
for index = 1, 2500 do
    bigObject["streamField" .. index] = string.rep("v", 16)
end
local bigInspected, bigInspection = ns.ObjectInspector.InspectValue(bigObject, "big")
assert(bigInspected and bigInspection.textStream and not bigInspection.textStream:IsFinished(),
    "large object did not keep a pending incremental stream")
assert(#bigInspection.text < 45000, "large object text was not bounded at the first chunk")
local secondChunk = bigInspection.textStream:ReadChunk(44000)
assert(#secondChunk > 0 and not bigInspection.textStream:IsFinished(),
    "large object stream did not produce another bounded chunk")

-- Previews are bounded at 120 bytes with a visible suffix.
local longString = string.rep("a", 400)
local previewInspected, previewInspection = ns.ObjectInspector.InspectValue(longString, "long")
assert(previewInspected, "long string could not be inspected")
assert(#previewInspection.preview <= 123 and previewInspection.preview:sub(-3) == "...",
    "object preview was not bounded at 120 bytes")

-- Secrets are refused instead of read.
local secretTarget = {}
secretValues[secretTarget] = true
local secretInspected, _, secretError = ns.ObjectInspector.InspectValue(secretTarget, "secret")
assert(not secretInspected and secretError == ns.L.SECRET_VALUE_BLOCKED,
    "secret inspection target was not blocked")

-- Search ranking, value retention, nested paths and the global top level.
local searched, searchResult = ns.ObjectInspector.SearchPath("TestRoot", "alpha")
assert(searched and searchResult.totalMatches == 3, "object keyword search did not rank all matches")
assert(searchResult.results[1].key == "Alpha", "exact object search match was not ranked first")
assert(searchResult.results[2].key == "Alphabet", "prefix object search match was not ranked second")
assert(searchResult.results[3].key == "BetaAlpha", "contains object search match was not ranked third")
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

-- Search bounds: 200 results with a truncation flag.
local wideTable = {}
for index = 1, 250 do
    wideTable["match" .. index] = index
end
local wideSearched, wideResult = ns.ObjectInspector.SearchValue(wideTable, "match")
assert(wideSearched and wideResult.totalMatches == 250 and #wideResult.results == 200
    and wideResult.truncated, "object search result limit was not enforced")

-- Combat refusal at every action entry.
inCombat = true
local combatInspect, _, combatMessage = ns.ObjectInspector.InspectPath("TestRoot")
assert(not combatInspect and combatMessage == ns.L.COMBAT_BLOCKED, "object inspector did not block combat")
local combatSearch, _, combatSearchMessage = ns.ObjectInspector.SearchGlobal("alpha")
assert(not combatSearch and combatSearchMessage == ns.L.COMBAT_BLOCKED, "object search did not block combat")
local combatCapture, _, combatCaptureMessage = ns.ObjectInspector.CaptureMouseFocus()
assert(not combatCapture and combatCaptureMessage == ns.L.COMBAT_BLOCKED, "mouse capture did not block combat")
inCombat = false

-- Picker capture keeps the snapshot, its label and lazy children.
local captured, captureInspection = ns.ObjectInspector.CaptureMouseFocus()
assert(captured and captureInspection.isFrame and captureInspection.label == "TargetFrame",
    "mouse frame snapshot failed")
assert(captureInspection.value == mouseFocus, "mouse capture did not retain the selected object")
local capturedRoot = captureInspection.tree.roots[1]
local foundChildren
for index = 1, #capturedRoot.children do
    if capturedRoot.children[index].label == ns.L.FRAME_CHILDREN then
        foundChildren = capturedRoot.children[index]
        break
    end
end
assert(foundChildren and not foundChildren.loaded, "captured frame children were not available lazily")
ns.Inspector.LoadMore(foundChildren)
assert(foundChildren.children[1].source == childFocus,
    "captured frame child did not remain available for nested inspection")

-- Object page state: buttons, incremental text, node popup and picker.
local savedExports = {}
local pageDefs = {}
local exportHooks = {}
local workbenchCalls = { open = 0, close = 0 }
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
        return "LYCHEE-TEST-0001"
    end,
    Open = function() workbenchCalls.open = workbenchCalls.open + 1 return true end,
    Close = function() workbenchCalls.close = workbenchCalls.close + 1 end,
    IsShown = function() return true end,
}
LoadAddonFile("UI/Widgets.lua", ns)
LoadAddonFile("UI/TreeView.lua", ns)
LoadAddonFile("UI/Pages/Object.lua", ns)

local objectDef = assert(pageDefs["objects"], "object page did not register")
assert(objectDef.titleKey == "TAB_OBJECTS", "object page registered with the wrong title key")
assert(exportHooks["objects"], "object page did not register its export hook")
local container = NewRegion("container")
local objectPage = objectDef.build(container)
assert(objectPage.selectSnapshot:IsEnabled() == false and objectPage.exportSnapshot:IsEnabled() == false,
    "empty object snapshot actions started enabled")

objectPage.pathPanel.editBox:SetText("TestRoot")
objectPage.inspectButton:Click()
assert(objectPage.selectSnapshot:IsEnabled() and objectPage.exportSnapshot:IsEnabled(),
    "object snapshot actions did not enable after inspection")
assert(objectPage.treeView:HasTree(), "object inspection did not fill the value tree")

-- Wheeling near the bottom appends the next 44 KB chunk.
objectPage.pathPanel.editBox:SetText("")
local bigPageInspected = ns.ObjectInspector.InspectValue(bigObject, "big")
assert(bigPageInspected, "big object inspection failed")
local originalInspect = ns.ObjectInspector.InspectPath
ns.ObjectInspector.InspectPath = function()
    local ok, bigPageInspection = ns.ObjectInspector.InspectValue(bigObject, "big")
    return ok, bigPageInspection
end
objectPage.inspectButton:Click()
ns.ObjectInspector.InspectPath = originalInspect
local initialTextLength = #objectPage.textView.editBox:GetText()
objectPage.textView.scroll.verticalRange = 100
local textScripts = rawget(objectPage.textView, "scripts")
textScripts.OnMouseWheel(objectPage.textView, -1)
assert(#objectPage.textView.editBox:GetText() > initialTextLength,
    "object text wheel scrolling did not append another chunk near the bottom")
local loadedTextLength = #objectPage.textView.editBox:GetText()

-- Page Save exports the full snapshot through the single export UI.
objectPage.exportSnapshot:Click()
local snapshotExport = savedExports[#savedExports]
assert(snapshotExport and snapshotExport.kind == "object_snapshot"
    and #snapshotExport.content > loadedTextLength
    and snapshotExport.metadata.path == "big" and snapshotExport.metadata.valueType == "table",
    "object export did not serialize the full source independently of the edit box")

-- Shell export hook returns the same payload shape.
local hookKind, hookTitle, hookContent, hookMetadata = exportHooks["objects"].GetPayload()
assert(hookKind == "object_snapshot" and hookTitle == "big"
    and type(hookContent) == "function" and hookMetadata.path == "big",
    "object export hook did not expose the captured snapshot")

-- Node text popup: right-click opens it with an incremental stream, Save
-- exports the full subtree, Close releases the stream.
local rootRow = objectPage.treeView.rows[1]
rawget(rootRow, "scripts").OnClick(rootRow, "RightButton")
assert(objectPage.treeView:GetSelectedNode() == rootRow.node, "tree context action did not select its node")
assert(objectPage.nodePopup and objectPage.nodePopup.overlay:IsShown(),
    "tree context action did not open the node text popup")
assert(objectPage.nodePopup.textPanel.editBox:GetText():find("nested", 1, true),
    "node text popup did not contain the selected object")
assert(rawget(objectPage.nodePopup.textPanel, "serializationStream") ~= nil,
    "node text popup did not bind its incremental stream")
objectPage.nodePopup.exportButton:Click()
local nodeExport = savedExports[#savedExports]
assert(nodeExport and nodeExport.kind == "object_node"
    and nodeExport.content:find("streamField2500", 1, true)
    and #nodeExport.content > #objectPage.nodePopup.textPanel.editBox:GetText(),
    "node export did not serialize the full subtree")
objectPage.nodePopup.closeButton:Click()
assert(not objectPage.nodePopup.overlay:IsShown()
    and rawget(objectPage.nodePopup.textPanel, "serializationStream") == nil,
    "closing the node text popup did not release its serialization stream")

-- Search result rows: 38 px rows from a 16-row pool over a bounded result set.
ns.ObjectInspector.InspectPath = function()
    local ok, wideInspection = ns.ObjectInspector.InspectValue(wideTable, "wide")
    return ok, wideInspection
end
objectPage.inspectButton:Click()
ns.ObjectInspector.InspectPath = originalInspect
objectPage.pathPanel.editBox:SetText("match")
objectPage.searchButton:Click()
assert(#objectPage.resultRows == 16, "object search result list did not virtualize 16 rows")
assert(objectPage.resultRows[1]:GetHeight() == 38, "object search result rows were not 38 px")
assert(objectPage.listPanel:GetWidth() == 420, "object search result list width changed")
assert(objectPage.resultRows[1].name:GetText():find("match", 1, true),
    "object search result rows did not show their paths")

-- Mouse picker: window hides, dock shows, F captures, Esc cancels.
objectPage.mouseButton:Click()
assert(workbenchCalls.close == 1, "mouse picker did not hide the window")
local pickerDock = _G["LycheeToolkitPickerDock"]
assert(pickerDock and pickerDock:IsShown() and pickerDock:GetFrameStrata() == "TOOLTIP",
    "mouse picker dock was not shown in the TOOLTIP strata")
local dockScripts = rawget(pickerDock, "scripts")
dockScripts.OnKeyDown(pickerDock, "F")
assert(workbenchCalls.open == 1, "mouse picker capture did not restore the window")
local pickedValue, pickedLabel = objectPage:GetSelection()
assert(pickedValue == mouseFocus and pickedLabel == "TargetFrame",
    "mouse picker capture did not keep the captured object")
assert(not pickerDock:IsShown(), "mouse picker dock stayed visible after capture")

objectPage.mouseButton:Click()
dockScripts.OnKeyDown(pickerDock, "ESCAPE")
assert(not pickerDock:IsShown() and workbenchCalls.open == 2, "Esc did not cancel the mouse picker")

inCombat = true
objectPage.mouseButton:Click()
assert(not pickerDock:IsShown() and workbenchCalls.close == 2, "mouse picker started during combat")
inCombat = false

-- Window hide / combat teardown releases the page streams and the picker.
objectPage.mouseButton:Click()
objectDef.shutdown(objectPage)
assert(not pickerDock:IsShown(), "window teardown did not hide the mouse picker")

print("Lychee Toolkit object tests passed")
return true
