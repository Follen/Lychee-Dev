local ADDON_NAME, ns = ...

-- Workbench window shell: a lazy 1040x720 DIALOG-strata window with the
-- header, the eight-tab page registry navigation and the shell-owned history
-- rail layout. No frame, event, hook or timer exists before the first /dev.
local W = ns.Widgets
local colors = W.Colors

local WINDOW_WIDTH = 1040
local WINDOW_HEIGHT = 720
local HISTORY_WIDTH = 232
local WINDOW_NAME = "LycheeToolkitWindow"

local Layout = {
    WINDOW_WIDTH = WINDOW_WIDTH,
    WINDOW_HEIGHT = WINDOW_HEIGHT,
    RAIL_LEFT = 14,
    RAIL_WIDTH = HISTORY_WIDTH,
    -- Main content left edge beside the history rail.
    CONTENT_LEFT = HISTORY_WIDTH + 36,
    -- Full-width content left edge for pages without the rail.
    PAGE_LEFT = 17,
    HEADING_TOP = -84,
    CONTENT_TOP = -104,
    CONTENT_RIGHT = -14,
    CONTENT_BOTTOM = 54,
}

-- The eight workbench pages in navigation order. Their builders arrive
-- through RegisterPage as page modules load.
local PRESET_PAGES = {
    { key = "runner", titleKey = "TAB_RUNNER" },
    { key = "objects", titleKey = "TAB_OBJECTS" },
    { key = "events", titleKey = "TAB_EVENTS" },
    { key = "trace", titleKey = "TAB_TRACE" },
    { key = "diagnostics", titleKey = "TAB_DIAGNOSTICS" },
    { key = "exports", titleKey = "TAB_EXPORTS" },
    { key = "automation", titleKey = "TAB_AUTOMATION" },
    { key = "about", titleKey = "TAB_ABOUT" },
}

local pageOrder = {}
local pageDefs = {}
for index = 1, #PRESET_PAGES do
    local preset = PRESET_PAGES[index]
    pageOrder[#pageOrder + 1] = preset.key
    pageDefs[preset.key] = { key = preset.key, titleKey = preset.titleKey }
end

local windowFrame
local railRoot
local exportController
local activeKey
local builtPages = {}
local shutdownCallbacks = {}
local exportHooks = {}

local function RunShutdowns()
    for index = 1, #shutdownCallbacks do
        pcall(shutdownCallbacks[index])
    end
    for _, built in pairs(builtPages) do
        if built.def.shutdown then
            pcall(built.def.shutdown, built.page)
        end
    end
    if exportController then
        exportController:Hide()
    end
end

local function UpdateRailVisibility()
    if not railRoot then
        return
    end
    local def = activeKey and pageDefs[activeKey] or nil
    railRoot:SetShown(def ~= nil and def.rail == true)
end

local function BuildPage(key)
    local def = pageDefs[key]
    if not def or not def.build or builtPages[key] then
        return builtPages[key]
    end
    local container = CreateFrame("Frame", nil, windowFrame)
    container:SetAllPoints(windowFrame)
    container:Hide()
    local page = def.build(container)
    builtPages[key] = { def = def, container = container, page = page or container }
    return builtPages[key]
end

local function ActivatePage(key)
    if not windowFrame or not pageDefs[key] then
        return false
    end

    local previous = activeKey and builtPages[activeKey] or nil
    if previous and previous.def.suspend then
        previous.def.suspend(previous.page)
    end

    activeKey = key
    local built = BuildPage(key)

    for pageKey, entry in pairs(builtPages) do
        entry.container:SetShown(pageKey == key)
    end
    for pageKey, tab in pairs(windowFrame.pageTabs) do
        tab:SetActive(pageKey == key)
    end
    UpdateRailVisibility()

    if built and built.def.activate then
        built.def.activate(built.page)
    end
    return true
end

local function CreateHistoryRail()
    if railRoot then
        return railRoot
    end

    railRoot = CreateFrame("Frame", nil, windowFrame)
    railRoot:SetPoint("TOPLEFT", Layout.RAIL_LEFT, Layout.HEADING_TOP)
    railRoot:SetPoint("BOTTOMLEFT", Layout.RAIL_LEFT, Layout.CONTENT_BOTTOM)
    railRoot:SetWidth(HISTORY_WIDTH)

    local label = W.CreateSectionLabel(railRoot, ns.L.HISTORY)
    label:SetPoint("TOPLEFT", 3, 0)

    local panel = W.CreatePanel(railRoot, colors.editor[1], colors.editor[2], colors.editor[3], 0.78)
    panel:SetPoint("TOPLEFT", 0, -20)
    panel:SetPoint("BOTTOMLEFT", 0, 0)
    panel:SetWidth(HISTORY_WIDTH)

    local scroll = W.CreateScrollArea(panel, 8, 8, 7, 8)
    local content = CreateFrame("Frame", nil, scroll)
    content:SetWidth(HISTORY_WIDTH - 26)
    content:SetHeight(1)
    scroll:SetScrollChild(content)

    local empty = panel:CreateFontString(nil, "OVERLAY", "GameFontDisableSmall")
    empty:SetPoint("TOP", 0, -24)
    empty:SetText(ns.L.NO_HISTORY)
    empty:SetTextColor(1, 1, 1, 0.32)

    local rail = {
        root = railRoot,
        label = label,
        panel = panel,
        scroll = scroll,
        content = content,
        empty = empty,
        rowHeight = 52,
        rowWidth = HISTORY_WIDTH - 26,
    }
    railRoot.rail = rail
    railRoot:Hide()
    return railRoot
end

local function EnsureWindow()
    if windowFrame then
        return windowFrame
    end

    local ready, reason = ns.Stores.Initialize()
    if not ready then
        return nil, reason
    end

    local frame = CreateFrame("Frame", WINDOW_NAME, UIParent, "BackdropTemplate")
    frame:SetSize(WINDOW_WIDTH, WINDOW_HEIGHT)
    frame:SetPoint("CENTER")
    frame:SetFrameStrata("DIALOG")
    frame:SetClampedToScreen(true)
    frame:SetMovable(true)
    frame:EnableMouse(true)
    frame:RegisterForDrag("LeftButton")
    frame:SetScript("OnDragStart", function(self)
        self:StartMoving()
    end)
    frame:SetScript("OnDragStop", function(self)
        self:StopMovingOrSizing()
    end)
    frame:SetBackdrop(W.Backdrop)
    frame:SetBackdropColor(colors.panel[1], colors.panel[2], colors.panel[3], 0.985)
    frame:SetBackdropBorderColor(0.46, 0.51, 0.56, 0.48)
    frame:Hide()
    tinsert(UISpecialFrames, frame:GetName())

    local header = frame:CreateTexture(nil, "BACKGROUND")
    header:SetColorTexture(colors.surface[1], colors.surface[2], colors.surface[3], 0.52)
    header:SetPoint("TOPLEFT", 1, -1)
    header:SetPoint("TOPRIGHT", -1, -1)
    header:SetHeight(66)

    local logo = frame:CreateTexture(nil, "ARTWORK")
    logo:SetTexture(W.LOGO_TEXTURE)
    logo:SetTexCoord(0.18, 0.79, 0.17, 0.80)
    logo:SetSize(46, 46)
    logo:SetPoint("TOPLEFT", 14, -10)

    local title = frame:CreateFontString(nil, "OVERLAY", "GameFontNormalLarge")
    title:SetPoint("LEFT", logo, "RIGHT", 10, 0)
    title:SetText(ns.L.ADDON_TITLE)
    title:SetTextColor(1, 1, 1, 1)

    local closeButton = W.CreateCloseButton(frame)
    closeButton:SetPoint("TOPRIGHT", -14, -14)
    closeButton:SetScript("OnClick", function()
        frame:Hide()
    end)

    frame.pageTabs = {}
    local previousTab
    for index = 1, #pageOrder do
        local key = pageOrder[index]
        local def = pageDefs[key]
        local tab = W.CreateNavTab(frame, ns.L[def.titleKey] or key)
        W.FitNavTab(tab, 48, 32)
        if previousTab then
            tab:SetPoint("LEFT", previousTab, "RIGHT", 8, 0)
        else
            tab:SetPoint("BOTTOMLEFT", header, "BOTTOMLEFT", 300, 8)
        end
        tab:SetScript("OnClick", function()
            ActivatePage(key)
        end)
        frame.pageTabs[key] = tab
        previousTab = tab
    end

    exportController = ns.ExportUI.Create(frame)

    frame:SetScript("OnHide", RunShutdowns)
    windowFrame = frame

    -- Hiding on combat is registered here (not at file scope) so a session
    -- that never opens the workbench stays frameless.
    ns.Safety.RegisterCombatShutdown(function()
        if frame:IsShown() then
            frame:Hide()
        end
    end)
    return frame
end

ns.Workbench = {
    Layout = Layout,

    Toggle = function()
        if ns.Safety.IsCombatBlocked() then
            return false, ns.L.COMBAT_BLOCKED
        end
        local frame, reason = EnsureWindow()
        if not frame then
            return false, reason
        end
        if frame:IsShown() then
            frame:Hide()
            return false
        end
        frame:Show()
        ActivatePage(activeKey or pageOrder[1])
        return true
    end,

    Open = function()
        if ns.Safety.IsCombatBlocked() then
            return false, ns.L.COMBAT_BLOCKED
        end
        local frame, reason = EnsureWindow()
        if not frame then
            return false, reason
        end
        frame:Show()
        ActivatePage(activeKey or pageOrder[1])
        return true
    end,

    Close = function()
        if windowFrame then
            windowFrame:Hide()
        end
    end,

    IsShown = function()
        return windowFrame ~= nil and windowFrame:IsShown() and true or false
    end,

    -- Shows a page tab (opening the window first when needed).
    ShowPage = function(key)
        local shown, reason = ns.Workbench.Open()
        if not shown then
            return false, reason
        end
        return ActivatePage(key)
    end,

    GetActivePage = function()
        return activeKey
    end,

    -- The built page object (build result) once its tab was activated.
    GetPage = function(key)
        local built = builtPages[key]
        return built and built.page or nil
    end,

    -- Pages register at module load time (before the first /dev). A page is
    -- constructed on its first activation and receives its container frame:
    --   RegisterPage{ key, titleKey, build = function(parent) ... end,
    --                 activate, suspend, shutdown, rail }
    -- build(parent) returns the page frame (or any table; the container is
    -- used when nothing is returned). activate/suspend/shutdown receive that
    -- page object. rail = true keeps the shell history rail visible.
    RegisterPage = function(definition)
        if type(definition) ~= "table" or type(definition.key) ~= "string"
            or definition.key == "" or type(definition.build) ~= "function" then
            return nil, "page_invalid_definition"
        end
        if windowFrame then
            return nil, "page_registered_late"
        end
        local key = definition.key
        local def = pageDefs[key] or {}
        if def.build then
            return nil, "page_already_registered"
        end
        def.key = key
        def.titleKey = type(definition.titleKey) == "string" and definition.titleKey or def.titleKey or key
        def.build = definition.build
        def.activate = type(definition.activate) == "function" and definition.activate or nil
        def.suspend = type(definition.suspend) == "function" and definition.suspend or nil
        def.shutdown = type(definition.shutdown) == "function" and definition.shutdown or nil
        def.rail = definition.rail and true or false
        pageDefs[key] = def
        if not def.ordered then
            def.ordered = true
            local known = false
            for index = 1, #pageOrder do
                if pageOrder[index] == key then
                    known = true
                    break
                end
            end
            if not known then
                pageOrder[#pageOrder + 1] = key
            end
        end
        return def
    end,

    -- Runtime tools (event monitors, tracers, pickers) register their stop
    -- routine here; every callback runs when the window hides or combat
    -- starts.
    RegisterShutdown = function(callback)
        if type(callback) == "function" then
            shutdownCallbacks[#shutdownCallbacks + 1] = callback
        end
    end,

    -- Pages contribute their "Save (落盘)" payload provider here:
    --   RegisterExportHook{ key, page, GetPayload }
    -- GetPayload() returns kind, title, content, metadata (content may be a
    -- function producing the text) or nothing when there is nothing to save.
    RegisterExportHook = function(hook)
        if type(hook) ~= "table" or type(hook.key) ~= "string"
            or type(hook.GetPayload) ~= "function" then
            return nil, "export_hook_invalid"
        end
        exportHooks[hook.page or hook.key] = hook
        return hook
    end,

    SaveFromHook = function()
        local hook = activeKey and exportHooks[activeKey] or nil
        if not hook then
            return nil, ns.L.EXPORT_EMPTY
        end
        local kind, title, content, metadata = hook.GetPayload()
        if not kind then
            return nil, ns.L.EXPORT_EMPTY
        end
        return exportController:Save(kind, title, content, metadata)
    end,

    SaveToDisk = function(kind, title, content, metadata)
        if not exportController then
            return nil, ns.L.EXPORT_DATABASE_UNAVAILABLE
        end
        return exportController:Save(kind, title, content, metadata)
    end,

    ShowTicketPopup = function(id)
        if not exportController then
            return false
        end
        return exportController:Show(id)
    end,

    HideTicketPopup = function()
        if exportController then
            exportController:Hide()
        end
    end,

    -- The shared export controller (Save/Show/Hide plus its lazy popup).
    GetExportController = function()
        return exportController
    end,

    -- Shell-owned history rail layout; the Run page renders rows into it.
    GetHistoryRail = function()
        if not windowFrame then
            return nil
        end
        local root = CreateHistoryRail()
        UpdateRailVisibility()
        return root.rail
    end,
}
