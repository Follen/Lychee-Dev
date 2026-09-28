local ADDON_NAME, ns = ...

-- Lazy workbench shell. The navigation owns a narrow permanent column;
-- investigation pages own an 800-unit canvas beside it.
local W = ns.Widgets
local colors = W.Colors
local Theme = ns.Theme

local WINDOW_WIDTH = 960
local WINDOW_HEIGHT = 660
local SIDEBAR_WIDTH = 152
local PAGE_WIDTH = 800
local HISTORY_WIDTH = 176
local WINDOW_NAME = "LycheeToolkitWindow"

local Layout = {
    WINDOW_WIDTH = PAGE_WIDTH,
    WINDOW_HEIGHT = WINDOW_HEIGHT,
    RAIL_LEFT = 14,
    RAIL_WIDTH = HISTORY_WIDTH,
    -- Main content left edge beside the history rail.
    CONTENT_LEFT = HISTORY_WIDTH + 30,
    -- Full-width content left edge for pages without the rail.
    PAGE_LEFT = 17,
    HEADING_TOP = -78,
    CONTENT_TOP = -104,
    CONTENT_RIGHT = -14,
    CONTENT_BOTTOM = 64,
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
local secondaryRoot
local secondarySource
local backButton
local pageViewport, pageCanvas
local pageOffsets = {}
local UpdateRailVisibility
local function ViewportDimension(method, fallback)
    if not UIParent or type(UIParent[method]) ~= "function" then return fallback end
    local ok, value = pcall(UIParent[method], UIParent)
    if not ok or (issecretvalue and issecretvalue(value))
        or type(value) ~= "number" or value <= 0 then return fallback end
    return value
end

local function HideSecondary()
    if not secondaryRoot or not secondaryRoot:IsShown() then return false end
    secondaryRoot:Hide()
    if secondaryRoot.content then secondaryRoot.content:Hide() end
    secondarySource = nil
    backButton:Hide()
    for key, tab in pairs(windowFrame.pageTabs) do tab:SetActive(key == activeKey) end
    if pageViewport then pageViewport:Show() end
    if windowFrame.FitPageViewport then windowFrame.FitPageViewport() end
    if windowFrame.FitNavigation then windowFrame.FitNavigation() end
    local current = activeKey and builtPages[activeKey]
    if current then current.container:Show() end
    UpdateRailVisibility()
    return true
end

local function RunShutdowns()
    Theme.StopAnimation(windowFrame)
    for index = 1, #shutdownCallbacks do
        pcall(shutdownCallbacks[index])
    end
    for _, built in pairs(builtPages) do
        Theme.StopAnimation(built.container)
        if built.def.shutdown then
            pcall(built.def.shutdown, built.page)
        end
    end
    if exportController then
        exportController:Hide()
    end
    HideSecondary()
end

UpdateRailVisibility = function()
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
    local container = CreateFrame("Frame", nil, pageCanvas)
    container:SetAllPoints(pageCanvas)
    container:Hide()
    local page = def.build(container)
    builtPages[key] = { def = def, container = container, page = page or container }
    return builtPages[key]
end

local function ActivatePage(key)
    if not windowFrame or not pageDefs[key] then
        return false
    end

    HideSecondary()
    local previous = activeKey and builtPages[activeKey] or nil
    if activeKey and pageViewport then
        pageOffsets[activeKey] = {
            pageViewport:GetHorizontalScroll() or 0,
            pageViewport:GetVerticalScroll() or 0,
        }
    end
    if previous then Theme.StopAnimation(previous.container) end
    if previous and previous.def.suspend then
        previous.def.suspend(previous.page)
    end

    activeKey = key
    if pageViewport then
        local offset = pageOffsets[key] or { 0, 0 }
        pageViewport:SetHorizontalScroll(math.min(offset[1],
            math.max(0, PAGE_WIDTH - (windowFrame:GetWidth() - SIDEBAR_WIDTH - 8))))
        pageViewport:SetVerticalScroll(math.min(offset[2],
            math.max(0, WINDOW_HEIGHT - windowFrame:GetHeight())))
    end
    local built = BuildPage(key)

    for pageKey, entry in pairs(builtPages) do
        entry.container:SetShown(pageKey == key)
    end
    if built then Theme.Reveal(built.container) end
    for pageKey, tab in pairs(windowFrame.pageTabs) do tab:SetActive(pageKey == key) end
    if windowFrame.ScrollTabIntoView then windowFrame.ScrollTabIntoView(key) end
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

    railRoot = CreateFrame("Frame", nil, pageCanvas)
    railRoot:SetPoint("TOPLEFT", Layout.RAIL_LEFT, Layout.HEADING_TOP)
    railRoot:SetPoint("BOTTOMLEFT", Layout.RAIL_LEFT, Layout.CONTENT_BOTTOM)
    railRoot:SetWidth(HISTORY_WIDTH)

    local label = W.CreateSectionLabel(railRoot, ns.L.HISTORY)
    label:SetPoint("TOPLEFT", 3, 0)

    local panel = W.CreatePanel(railRoot, colors.editor[1], colors.editor[2], colors.editor[3], 1)
    panel:SetPoint("TOPLEFT", 0, Layout.CONTENT_TOP - Layout.HEADING_TOP)
    panel:SetPoint("BOTTOMLEFT", 0, 0)
    panel:SetWidth(HISTORY_WIDTH)

    local scroll = W.CreateScrollArea(panel, 8, 8, 7, 8)
    local content = CreateFrame("Frame", nil, scroll)
    content:SetWidth(HISTORY_WIDTH - 26)
    content:SetHeight(1)
    scroll:SetScrollChild(content)

    local empty = panel:CreateFontString(nil, "OVERLAY", "GameFontDisableSmall")
    Theme.SetFont(empty, 12, Theme.textDim)
    empty:SetPoint("TOP", 0, -24)
    empty:SetText(ns.L.NO_HISTORY)

    local rail = {
        root = railRoot,
        label = label,
        panel = panel,
        scroll = scroll,
        content = content,
        empty = empty,
        rowHeight = 58,
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

    local frame = CreateFrame("Frame", WINDOW_NAME, UIParent)
    local function FitViewport()
        frame:SetSize(math.min(WINDOW_WIDTH,
            math.max(1, ViewportDimension("GetWidth", WINDOW_WIDTH + 24) - 24)),
            math.min(WINDOW_HEIGHT,
                math.max(1, ViewportDimension("GetHeight", WINDOW_HEIGHT + 24) - 24)))
        if frame.FitNavigation then frame.FitNavigation() end
        if frame.FitPageViewport then frame.FitPageViewport() end
    end
    frame.FitViewport = FitViewport
    FitViewport()
    frame:SetPoint("CENTER")
    frame:SetFrameStrata("DIALOG")
    frame:SetClampedToScreen(true)
    frame:SetMovable(true)
    frame:EnableMouse(true)
    frame:SetClipsChildren(true)
    frame:RegisterForDrag("LeftButton")
    frame:SetScript("OnDragStart", function(self)
        self:StartMoving()
    end)
    frame:SetScript("OnDragStop", function(self)
        self:StopMovingOrSizing()
    end)
    Theme.PaintRoundedPanel(frame)
    frame:Hide()
    tinsert(UISpecialFrames, frame:GetName())

    pageViewport = CreateFrame("ScrollFrame", nil, frame)
    pageViewport:SetPoint("TOPLEFT", SIDEBAR_WIDTH + 8, 0)
    -- The page owns its bottom actions. Its viewport and canvas must share
    -- the same height or the last 48 units become permanently unreachable.
    pageViewport:SetPoint("BOTTOMRIGHT", 0, 0)
    pageViewport:SetClipsChildren(true)
    pageViewport:EnableMouseWheel(true)
    local pageScrollChild = CreateFrame("Frame", nil, pageViewport)
    pageScrollChild:SetSize(PAGE_WIDTH, WINDOW_HEIGHT)
    pageViewport:SetScrollChild(pageScrollChild)
    pageCanvas = CreateFrame("Frame", nil, pageScrollChild)
    pageCanvas:SetSize(PAGE_WIDTH, WINDOW_HEIGHT)
    pageCanvas:SetPoint("TOPLEFT", pageScrollChild, "TOPLEFT", 0, 0)
    pageViewport:SetScript("OnMouseWheel", function(self, delta)
        if issecretvalue and issecretvalue(delta) then return end
        local range = math.max(0, WINDOW_HEIGHT - frame:GetHeight())
        self:SetVerticalScroll(math.max(0, math.min(range,
            (self:GetVerticalScroll() or 0) - delta * 36)))
    end)
    local pagePrevious = W.CreateButton(frame, 32, "<", "secondary")
    pagePrevious:SetPoint("TOPRIGHT", frame, "TOPRIGHT", -90, -10)
    local pageNext = W.CreateButton(frame, 32, ">", "secondary")
    pageNext:SetPoint("TOPRIGHT", frame, "TOPRIGHT", -50, -10)
    local function FitPageViewport()
        local range = math.max(0, PAGE_WIDTH - math.max(1, frame:GetWidth() - SIDEBAR_WIDTH - 8))
        local shown = range > 0 and not (secondaryRoot and secondaryRoot:IsShown())
        pagePrevious:SetShown(shown)
        pageNext:SetShown(shown)
        pageViewport:SetHorizontalScroll(math.min(
            pageViewport:GetHorizontalScroll() or 0, range))
        pageViewport:SetVerticalScroll(math.min(
            pageViewport:GetVerticalScroll() or 0,
            math.max(0, WINDOW_HEIGHT - frame:GetHeight())))
    end
    pagePrevious:SetScript("OnClick", function()
        pageViewport:SetHorizontalScroll(math.max(0,
            (pageViewport:GetHorizontalScroll() or 0) - 160))
    end)
    pageNext:SetScript("OnClick", function()
        pageViewport:SetHorizontalScroll(math.min(math.max(0,
            PAGE_WIDTH - math.max(1, frame:GetWidth() - SIDEBAR_WIDTH - 8)),
            (pageViewport:GetHorizontalScroll() or 0) + 160))
    end)
    frame.pageViewport, frame.pagePrevious, frame.pageNext =
        pageViewport, pagePrevious, pageNext
    frame.FitPageViewport = FitPageViewport
    FitPageViewport()

    local header = CreateFrame("Frame", nil, frame)
    header:SetPoint("TOPLEFT", 0, 0)
    header:SetPoint("BOTTOMLEFT", 0, 0)
    header:SetWidth(SIDEBAR_WIDTH)
    -- Flush to the outer contour: only the two outside corners are rounded.
    Theme.PaintRoundedPanel(header, Theme.sidebar, { squareRight = true })
    frame.sidebar = header

    local logo = header:CreateTexture(nil, "ARTWORK")
    logo:SetTexture(W.LOGO_TEXTURE)
    logo:SetTexCoord(0, 1, 0, 1)
    logo:SetSize(28, 28)
    logo:SetPoint("TOPLEFT", 14, -18)

    local title = header:CreateFontString(nil, "OVERLAY", "GameFontNormalLarge")
    Theme.SetFont(title, 12, Theme.text)
    title:SetPoint("LEFT", logo, "RIGHT", 7, 0)
    title:SetWidth(SIDEBAR_WIDTH - 60)
    title:SetJustifyH("LEFT")
    title:SetText(ns.L.ADDON_TITLE)

    local closeButton = W.CreateCloseButton(frame)
    closeButton:SetPoint("TOPRIGHT", -11, -10)
    closeButton:SetScript("OnClick", function()
        frame:Hide()
    end)

    secondaryRoot = CreateFrame("Frame", nil, frame)
    secondaryRoot:SetPoint("TOPLEFT", SIDEBAR_WIDTH + 8, 0)
    secondaryRoot:SetPoint("BOTTOMRIGHT", -14, 0)
    secondaryRoot:EnableMouse(true)
    secondaryRoot:Hide()


    backButton = W.CreateBackButton(frame)
    backButton:SetPoint("TOPLEFT", secondaryRoot, "TOPLEFT", 4, -8)
    backButton:SetScript("OnClick", HideSecondary)
    backButton:Hide()
    frame.backButton = backButton

    local navScroll = W.CreateScrollArea(header, 10, 86, 10, 16)
    local navContent = CreateFrame("Frame", nil, navScroll)
    navContent:SetSize(SIDEBAR_WIDTH - 32, #pageOrder * 44)
    navScroll:SetScrollChild(navContent)
    frame.navScroll = navScroll
    frame.FitNavigation = function() navScroll:UpdateScrollChildRect() end
    frame.pageTabs = {}
    for index = 1, #pageOrder do
        local key = pageOrder[index]
        local def = pageDefs[key]
        local tab = W.CreateNavTab(navContent, ns.L[def.titleKey] or key)
        tab:SetSize(SIDEBAR_WIDTH - 32, 40)
        tab:SetPoint("TOPLEFT", navContent, "TOPLEFT", 0, -((index - 1) * 44))
        tab.navTop = (index - 1) * 44
        tab:SetScript("OnClick", function() ActivatePage(key) end)
        frame.pageTabs[key] = tab
    end
    frame.FitNavigation()
    frame.ScrollTabIntoView = function(key)
        local tab = frame.pageTabs[key]
        if not tab then return end
        local offset = navScroll:GetVerticalScroll() or 0
        if tab.navTop < offset then
            navScroll:SetVerticalScroll(tab.navTop)
        elseif tab.navTop + tab:GetHeight() > offset + navScroll:GetHeight() then
            navScroll:SetVerticalScroll(tab.navTop + tab:GetHeight() - navScroll:GetHeight())
        end
    end

    exportController = ns.ExportUI.Create(frame)

    frame:SetScript("OnShow", function(self)
        self:RegisterEvent("DISPLAY_SIZE_CHANGED")
        self:RegisterEvent("UI_SCALE_CHANGED")
    end)
    frame:SetScript("OnEvent", function(self)
        self.FitViewport()
    end)
    frame:SetScript("OnHide", function(self)
        self:UnregisterAllEvents()
        RunShutdowns()
    end)
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
        frame.FitViewport()
        frame:Show()
        Theme.Reveal(frame)
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
        if frame:IsShown() then return true end
        frame.FitViewport()
        frame:Show()
        Theme.Reveal(frame)
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

    ShowSecondary = function(content)
        if not windowFrame or not content or not activeKey then return false end
        if secondaryRoot.content and secondaryRoot.content ~= content then
            secondaryRoot.content:Hide()
        end
        secondaryRoot.content = content
        secondarySource = activeKey
        local current = builtPages[activeKey]
        if current then current.container:Hide() end
        pageViewport:Hide()
        windowFrame.pagePrevious:Hide()
        windowFrame.pageNext:Hide()
        if railRoot then railRoot:Hide() end
        for _, tab in pairs(windowFrame.pageTabs) do tab:SetActive(false) end
        backButton:Show()
        content:Show()
        secondaryRoot:Show()
        return true
    end,

    HideSecondary = HideSecondary,
    GetSecondaryRoot = function() return secondaryRoot end,

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
