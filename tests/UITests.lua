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

function CreateFrame(_, name)
    frameCount = frameCount + 1
    local frame = NewRegion(name)
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
assert(minimumScroll == 0 and maximumScroll == 200, "custom scroll range did not follow content height")
local resultEditScripts = rawget(LycheeDevWindow.resultPanel.editBox, "scripts")
assert(resultEditScripts and resultEditScripts.OnMouseWheel, "result edit box did not capture mouse wheel input")
resultEditScripts.OnMouseWheel(LycheeDevWindow.resultPanel.editBox, -1)
assert(resultScrollbar:GetValue() == 86, "mouse wheel input did not move the custom scrollbar")
assert(LycheeDevWindow.resultPanel.scroll:GetVerticalScroll() == 86,
    "mouse wheel input did not move the text viewport")
assert(LycheeDevWindow.resultPanel.editBox.point[5] == 86,
    "mouse wheel input did not offset the clipped content")
LycheeDevWindow.resultPanel.scroll:SetVerticalScroll(50)
local wheelCatcherScripts = rawget(LycheeDevWindow.resultPanel.wheelCatcher, "scripts")
assert(wheelCatcherScripts and wheelCatcherScripts.OnMouseWheel,
    "text area did not create a dedicated wheel catcher")
wheelCatcherScripts.OnMouseWheel(LycheeDevWindow.resultPanel.wheelCatcher, -1)
assert(LycheeDevWindow.resultPanel.scroll:GetVerticalScroll() == 86,
    "dedicated wheel catcher did not move the text viewport")
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

local foundLogo
for index = 1, #textures do
    if textures[index] == "Interface\\AddOns\\Lychee Dev\\Media\\Logo.png" then
        foundLogo = true
        break
    end
end
assert(foundLogo, "logo texture was not loaded from the addon")

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
