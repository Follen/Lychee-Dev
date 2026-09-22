-- WoW API stubs for the standalone Lua 5.1 workbench tests. Every stub is
-- inert until the code under test touches it; CreateFrame calls are counted so
-- tests can prove nothing is built before the first /dev.

local Env = {}

Env.framesCreated = 0
Env.inCombat = false
Env.now = 1234567890
Env.locale = "zhCN"
Env.reloadCalled = false
Env.textures = {}
Env.prints = {}
Env.pendingTimers = {}
Env.mouseFoci = {}
Env.secrets = setmetatable({}, { __mode = "k" })

local realPrint = print

local TEST_BUILDS = {
    retail = { "12.1.0", "70000", "Aug 19 2026", 120100 },
    classic = { "5.5.4", "64000", "Aug 04 2026", 50504 },
    titan = { "3.80.2", "63000", "Aug 05 2026", 38002 },
    forever = { "1.60.1", "69893", "Sep 17 2026", 16001 },
}

local CLIENT_PROFILES = {
    retail = "Clients/Live.lua",
    classic = "Clients/Pandaria.lua",
    titan = "Clients/Titan.lua",
    forever = "Clients/Evergreen.lua",
}

Env.testBuilds = TEST_BUILDS
Env.clientProfiles = CLIENT_PROFILES

local client = "retail"
local addonRoot = "../../addon"

function Env.MakeSecret()
    local secret = {}
    Env.secrets[secret] = true
    return secret
end

function Env.SetInCombat(value)
    Env.inCombat = value and true or false
end

function Env.SetLocale(value)
    Env.locale = value
end

function Env.SetNow(value)
    Env.now = value
end

function Env.FireScript(target, name, ...)
    local scripts = rawget(target, "scripts")
    local handler = scripts and scripts[name]
    if handler then
        return handler(target, ...)
    end
    return nil
end

function Env.PumpTimers()
    while #Env.pendingTimers > 0 do
        local batch = Env.pendingTimers
        Env.pendingTimers = {}
        for index = 1, #batch do
            batch[index]()
        end
    end
end

function Env.LoadAddon(relativePath, namespace)
    local chunk, loadError = loadfile(addonRoot .. "/" .. relativePath)
    assert(chunk, tostring(loadError) .. " (" .. relativePath .. ")")
    return chunk("Lychee Dev", namespace)
end

function Env.ClientProfilePath()
    return CLIENT_PROFILES[client]
end

function Env.Client()
    return client
end

function Env.NewNamespace()
    return {}
end

-- Loads the full workbench foundation (everything except Controls/Bridge) into
-- a fresh namespace and initializes persistence.
function Env.LoadWorkbench()
    local ns = Env.NewNamespace()
    Env.LoadAddon(Env.ClientProfilePath(), ns)
    local sources = {
        "Core/Locale.lua",
        "Core/Locale_enUS.lua",
        "Core/Compat.lua",
        "Core/Serializer.lua",
        "Core/Inspector.lua",
        "Core/Safety.lua",
        "Core/Execute.lua",
        "Core/Stores.lua",
        "Core/Persistence.lua",
        "UI/Widgets.lua",
        "UI/TreeView.lua",
        "UI/Export.lua",
        "UI/Window.lua",
        "UI/Pages/About.lua",
    }
    for index = 1, #sources do
        Env.LoadAddon(sources[index], ns)
    end
    return ns
end

-- Frame/region stub -------------------------------------------------------

local function NewRegion(name, parent, objectType)
    local region = {
        name = name,
        objectType = objectType or "Frame",
        shown = true,
        text = "",
        width = 0,
        height = 0,
        verticalScroll = 0,
        minimumValue = 0,
        maximumValue = 0,
        value = 0,
        enabled = true,
        frameLevel = 1,
        frameStrata = "MEDIUM",
        effectiveScale = 1,
        parent = parent,
        children = {},
    }
    if parent and parent.children then
        parent.children[#parent.children + 1] = region
    end

    function region:GetName()
        return self.name
    end

    function region:GetObjectType()
        return self.objectType
    end

    function region:GetParent()
        return rawget(self, "parent")
    end

    function region:SetParent(parentFrame)
        self.parent = parentFrame
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

    function region:SetAllPoints()
        self.allPoints = true
    end

    function region:SetText(text)
        self.text = text or ""
        local scripts = rawget(self, "scripts")
        if scripts and scripts.OnTextChanged then
            scripts.OnTextChanged(self)
        end
    end

    function region:GetText()
        return self.text
    end

    function region:SetFocus()
        self.focused = true
        local scripts = rawget(self, "scripts")
        if scripts and scripts.OnEditFocusGained then
            scripts.OnEditFocusGained(self)
        end
    end

    function region:ClearFocus()
        self.focused = false
        local scripts = rawget(self, "scripts")
        if scripts and scripts.OnEditFocusLost then
            scripts.OnEditFocusLost(self)
        end
    end

    function region:HighlightText()
        self.highlighted = true
    end

    function region:SelectAll()
        self.focused = true
        self.highlighted = true
    end

    function region:Insert(text)
        self.text = self.text .. (text or "")
    end

    function region:SetCursorPosition(position)
        self.cursorPosition = position
    end

    function region:GetStringHeight()
        return 14
    end

    function region:GetStringWidth()
        return #self.text * 7
    end

    function region:SetTexture(path)
        self.texture = path
        Env.textures[#Env.textures + 1] = path
    end

    function region:SetColorTexture(r, g, b, a)
        self.colorTexture = { r, g, b, a }
    end

    function region:SetTexCoord(...)
        self.texCoord = { ... }
    end

    function region:SetAlpha(alpha)
        self.alpha = alpha
    end

    function region:SetRotation(rotation)
        self.rotation = rotation
    end

    function region:SetFontObject(font)
        self.fontObject = font
    end

    function region:SetTextInsets(...)
        self.textInsets = { ... }
    end

    function region:SetJustifyH(value)
        self.justifyH = value
    end

    function region:SetJustifyV(value)
        self.justifyV = value
    end

    function region:SetWordWrap(value)
        self.wordWrap = value
    end

    function region:SetTextColor(r, g, b, a)
        self.textColor = { r, g, b, a }
    end

    function region:SetScript(scriptName, handler)
        local scripts = rawget(self, "scripts")
        if not scripts then
            scripts = {}
            rawset(self, "scripts", scripts)
        end
        scripts[scriptName] = handler
    end

    function region:HookScript(scriptName, handler)
        local scripts = rawget(self, "scripts")
        if not scripts then
            scripts = {}
            rawset(self, "scripts", scripts)
        end
        local previous = scripts[scriptName]
        if previous then
            scripts[scriptName] = function(frame, ...)
                previous(frame, ...)
                handler(frame, ...)
            end
        else
            scripts[scriptName] = handler
        end
    end

    local function EffectivelyVisible(self)
        local current = self
        while current do
            if current.shown == false then
                return false
            end
            current = rawget(current, "parent")
        end
        return true
    end

    local FireTree
    local function FireDescendants(self, scriptName)
        local children = rawget(self, "children") or {}
        for index = 1, #children do
            FireTree(children[index], scriptName)
        end
    end

    FireTree = function(self, scriptName)
        local scripts = rawget(self, "scripts")
        if self.shown and scripts and scripts[scriptName] then
            scripts[scriptName](self)
        end
        FireDescendants(self, scriptName)
    end

    function region:Hide()
        local wasVisible = EffectivelyVisible(self)
        self.shown = false
        if wasVisible then
            local scripts = rawget(self, "scripts")
            if scripts and scripts.OnHide then
                scripts.OnHide(self)
            end
            FireDescendants(self, "OnHide")
        end
    end

    function region:Show()
        local wasVisible = EffectivelyVisible(self)
        self.shown = true
        if not wasVisible and EffectivelyVisible(self) then
            local scripts = rawget(self, "scripts")
            if scripts and scripts.OnShow then
                scripts.OnShow(self)
            end
            FireDescendants(self, "OnShow")
        end
    end

    function region:SetShown(shown)
        if shown then self:Show() else self:Hide() end
    end

    function region:IsShown()
        return self.shown
    end

    function region:IsVisible()
        return EffectivelyVisible(self)
    end

    function region:EnableMouse(enabled)
        self.mouseEnabled = enabled
    end

    function region:EnableMouseWheel(enabled)
        self.mouseWheelEnabled = enabled
    end

    function region:SetClipsChildren(clips)
        self.clipsChildren = clips
    end

    function region:RegisterForDrag(...)
        self.registeredDrags = { ... }
    end

    function region:RegisterForClicks(...)
        self.registeredClicks = { ... }
    end

    function region:RegisterEvent(event)
        self.registeredEvent = event
    end

    function region:UnregisterAllEvents()
        self.registeredEvent = nil
    end

    function region:StartMoving()
        self.moving = true
    end

    function region:StopMovingOrSizing()
        self.moving = false
    end

    function region:SetMovable(movable)
        self.movable = movable
    end

    function region:SetClampedToScreen(clamped)
        self.clampedToScreen = clamped
    end

    function region:SetFrameStrata(strata)
        self.frameStrata = strata
    end

    function region:GetFrameStrata()
        return self.frameStrata
    end

    function region:SetFrameLevel(level)
        self.frameLevel = level
    end

    function region:GetFrameLevel()
        return self.frameLevel
    end

    function region:GetEffectiveScale()
        return self.effectiveScale
    end

    function region:SetBackdrop(backdrop)
        self.backdrop = backdrop
    end

    function region:SetBackdropColor(r, g, b, a)
        self.backdropColor = { r, g, b, a }
    end

    function region:SetBackdropBorderColor(r, g, b, a)
        self.backdropBorderColor = { r, g, b, a }
    end

    function region:SetMultiLine(multiLine)
        self.multiLine = multiLine
    end

    function region:SetAutoFocus(autoFocus)
        self.autoFocus = autoFocus
    end

    function region:SetCountInvisibleLetters(count)
        self.countInvisibleLetters = count
    end

    function region:SetEnabled(enabled)
        self.enabled = enabled and true or false
    end

    function region:IsEnabled()
        return self.enabled ~= false
    end

    function region:Click(mouseButton)
        if not self:IsEnabled() then
            return
        end
        local scripts = rawget(self, "scripts")
        if scripts and scripts.OnClick then
            scripts.OnClick(self, mouseButton or "LeftButton")
        end
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
        local explicit = rawget(self, "verticalScrollRange")
        if explicit ~= nil then
            return explicit
        end
        local child = rawget(self, "scrollChild")
        local childHeight = child and child:GetHeight() or 0
        return math.max(0, childHeight - (self:GetHeight() or 0))
    end

    function region:SetMinMaxValues(minimum, maximum)
        self.minimumValue = minimum
        self.maximumValue = maximum
        local scripts = rawget(self, "scripts")
        if scripts and scripts.OnScrollRangeChanged then
            scripts.OnScrollRangeChanged(self, minimum, maximum)
        end
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

    function region:SetValueStep(step)
        self.valueStep = step
    end

    function region:SetObeyStepOnDrag(obey)
        self.obeyStepOnDrag = obey
    end

    function region:SetHitRectInsets(...)
        self.hitRectInsets = { ... }
    end

    function region:SetThumbTexture(texture)
        self.thumb = texture
    end

    function region:SetOrientation(orientation)
        self.orientation = orientation
    end

    function region:CreateTexture(name, layer)
        return NewRegion(name, self, "Texture")
    end

    function region:CreateFontString(name, layer, template)
        return NewRegion(name, self, "FontString")
    end

    -- Unknown Blizzard-style methods (UpperCamelCase) become no-ops; plain
    -- fields stay nil so addon nil-checks behave like the real client.
    setmetatable(region, {
        __index = function(target, key)
            if key == "GetTextHeight" then
                return nil
            end
            if type(key) == "string" and string.match(key, "^%u") then
                local noOp = function()
                end
                rawset(target, key, noOp)
                return noOp
            end
            return nil
        end,
    })
    return region
end

function CreateFrame(frameType, name, parent, template)
    Env.framesCreated = Env.framesCreated + 1
    local frame = NewRegion(name, parent, frameType or "Frame")
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

-- WoW globals -------------------------------------------------------------

function wipe(target)
    for key in pairs(target) do
        target[key] = nil
    end
end

tinsert = table.insert

function time()
    return Env.now
end

function date(format, timestamp)
    return os.date(format, timestamp or Env.now)
end

function GetTime()
    return Env.now
end

function issecretvalue(value)
    return Env.secrets[value] == true
end

function InCombatLockdown()
    return Env.inCombat
end

function ReloadUI()
    Env.reloadCalled = true
end

function GetBuildInfo()
    local build = assert(TEST_BUILDS[client], "unknown test client: " .. client)
    return build[1], build[2], build[3], build[4]
end

function GetLocale()
    return Env.locale
end

function GetMouseFoci()
    return Env.mouseFoci
end

function GetPhysicalScreenSize()
    return 2560, 1440
end

function GetScreenWidth()
    return 1280
end

function GetScreenHeight()
    return 720
end

C_AddOns = {
    GetNumAddOns = function()
        return 1
    end,
    GetAddOnInfo = function()
        return "Lychee Dev", "Lychee Dev"
    end,
    IsAddOnLoaded = function()
        return true, true
    end,
    GetAddOnMetadata = function(addonName, key)
        if addonName == "Lychee Dev" and key == "Version" then
            return "2.0.0"
        end
        return ""
    end,
}

C_Timer = {
    After = function(_, callback)
        Env.pendingTimers[#Env.pendingTimers + 1] = callback
    end,
}

EventRegistry = {
    RegisterCallback = function() end,
    UnregisterCallback = function() end,
    TriggerEvent = function() end,
}

SlashCmdList = {}
UISpecialFrames = {}
UIParent = NewRegion("UIParent", nil, "Frame")
ChatFontNormal = {}
GameFontNormal = {}
GameFontNormalLarge = {}
GameFontHighlightSmall = {}
GameFontDisableSmall = {}

print = function(...)
    local values = { n = select("#", ...), ... }
    local parts = {}
    for index = 1, values.n do
        parts[index] = tostring(values[index])
    end
    local line = table.concat(parts, "\t")
    Env.prints[#Env.prints + 1] = line
    realPrint(line)
end

-- Entry point used by run.lua ---------------------------------------------

function Env.Init(selectedClient, selectedRoot)
    client = assert(TEST_BUILDS[selectedClient] and selectedClient,
        "unknown test client: " .. tostring(selectedClient))
    if selectedRoot and selectedRoot ~= "" then
        addonRoot = selectedRoot
    end
    return Env
end

return Env
