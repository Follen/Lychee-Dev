local ADDON_NAME, ns = ...
local L = ns.L

local ERROR_ROW_HEIGHT = 44
local PERF_ROW_HEIGHT = 28
local ERROR_LIST_WIDTH = 430
local VISIBLE_ROWS = 16

local function SetReadOnlyText(panel, text, selectAll)
    local editBox = panel.editBox
    editBox.savedText, editBox.updatingText = text or "", true
    editBox:SetText(text or "")
    editBox.updatingText = nil
    editBox:SetCursorPosition(0)
    panel.scroll:SetVerticalScroll(0)
    if selectAll then
        editBox:SetFocus()
        editBox:HighlightText()
    end
end

local function CreateLineInput(parent, ui)
    local panel = ui.CreatePanel(parent, ui.editorR, ui.editorG, ui.editorB, 1)
    local editBox = CreateFrame("EditBox", nil, panel)
    editBox:SetAutoFocus(false)
    editBox:SetFontObject(ChatFontNormal)
    editBox:SetTextColor(0.94, 0.95, 0.96)
    editBox:SetTextInsets(9, 9, 0, 0)
    editBox:SetPoint("TOPLEFT", 1, -1)
    editBox:SetPoint("BOTTOMRIGHT", -1, 1)
    editBox:SetScript("OnEscapePressed", function(self) self:ClearFocus() end)
    editBox:SetScript("OnEditFocusGained", function() ui.SetBorderColor(panel, true, 0.75) end)
    editBox:SetScript("OnEditFocusLost", function() ui.SetBorderColor(panel, false) end)
    panel.editBox = editBox
    return panel
end

function ns.CreateDiagnosticsPage(parent, ui)
    local page = CreateFrame("Frame", nil, parent)
    page:SetAllPoints(parent)

    local errorsTab = ui.CreateViewTab(page, L.ERRORS)
    errorsTab:SetSize(72, 30)
    errorsTab:SetPoint("TOPLEFT", 14, -84)
    local performanceTab = ui.CreateViewTab(page, L.PERFORMANCE)
    performanceTab:SetSize(72, 30)
    performanceTab:SetPoint("LEFT", errorsTab, "RIGHT", 2, 0)
    local refreshButton = ui.CreateButton(page, 92, L.REFRESH, false)
    refreshButton:SetPoint("TOPRIGHT", -14, -84)
    local clearButton = ui.CreateButton(page, 112, L.CLEAR_ERRORS, false)
    clearButton:SetPoint("RIGHT", refreshButton, "LEFT", -8, 0)

    local status = page:CreateFontString(nil, "OVERLAY", "GameFontHighlightSmall")
    status:SetPoint("LEFT", performanceTab, "RIGHT", 14, 0)
    status:SetPoint("RIGHT", clearButton, "LEFT", -10, 0)
    status:SetJustifyH("LEFT")
    local function SetStatus(text, errorState)
        local r = errorState and ui.accentR or 0.55
        local g = errorState and ui.accentG or 0.60
        local b = errorState and ui.accentB or 0.65
        status:SetText(text or "")
        status:SetTextColor(r, g, b, 0.92)
    end

    local errorsView = CreateFrame("Frame", nil, page)
    errorsView:SetPoint("TOPLEFT", 0, -126)
    errorsView:SetPoint("BOTTOMRIGHT")

    local currentScope = ui.CreateButton(errorsView, 104, L.CURRENT_SESSION, "selected")
    currentScope:SetPoint("TOPLEFT", 14, 0)
    local allScope = ui.CreateButton(errorsView, 104, L.ALL_SESSIONS, false)
    allScope:SetPoint("LEFT", currentScope, "RIGHT", 8, 0)
    local searchPanel = CreateLineInput(errorsView, ui)
    searchPanel:SetPoint("TOPLEFT", allScope, "TOPRIGHT", 14, 0)
    searchPanel:SetWidth(260)
    searchPanel:SetHeight(28)
    local searchButton = ui.CreateButton(errorsView, 76, L.SEARCH, false)
    searchButton:SetPoint("LEFT", searchPanel, "RIGHT", 8, 0)
    local selectReport = ui.CreateButton(errorsView, 110, L.SELECT_REPORT, false)
    selectReport:SetPoint("TOPRIGHT", -14, 0)
    page.errorsTab = errorsTab
    page.performanceTab = performanceTab
    page.currentScope = currentScope
    page.allScope = allScope
    page.refreshButton = refreshButton
    page.clearButton = clearButton
    page.selectReport = selectReport

    local listLabel = ui.CreateSectionLabel(errorsView, L.ERROR_LIST)
    listLabel:SetPoint("TOPLEFT", 17, -48)
    local reportLabel = ui.CreateSectionLabel(errorsView, L.AGENT_REPORT)
    reportLabel:SetPoint("TOPLEFT", ERROR_LIST_WIDTH + 29, -48)

    local listPanel = ui.CreatePanel(errorsView, ui.editorR, ui.editorG, ui.editorB, 0.9)
    listPanel:SetPoint("TOPLEFT", 14, -68)
    listPanel:SetPoint("BOTTOMLEFT", 14, 54)
    listPanel:SetWidth(ERROR_LIST_WIDTH)
    local errorScroll = ui.CreateScrollArea(listPanel, 8, 8, 7, 8)
    local errorContent = CreateFrame("Frame", nil, errorScroll)
    errorContent:SetWidth(ERROR_LIST_WIDTH - 34)
    errorContent:SetHeight(1)
    errorScroll:SetScrollChild(errorContent)
    local errorEmpty = listPanel:CreateFontString(nil, "OVERLAY", "GameFontDisableSmall")
    errorEmpty:SetPoint("TOP", 0, -24)
    errorEmpty:SetText(L.NO_ERRORS)
    errorEmpty:SetTextColor(1, 1, 1, 0.32)

    local reportPanel = ui.CreateTextArea(errorsView, true)
    reportPanel:SetPoint("TOPLEFT", ERROR_LIST_WIDTH + 26, -68)
    reportPanel:SetPoint("BOTTOMRIGHT", -14, 54)
    reportPanel.editBox:SetWidth(ui.windowWidth - ERROR_LIST_WIDTH - 78)
    reportPanel.editBox:SetScript("OnMouseUp", function(self)
        self:SetFocus()
    end)
    SetReadOnlyText(reportPanel, L.SELECT_ERROR_DETAIL)

    local performanceView = ui.CreatePanel(page, ui.editorR, ui.editorG, ui.editorB, 0.9)
    performanceView:SetPoint("TOPLEFT", 14, -126)
    performanceView:SetPoint("BOTTOMRIGHT", -14, 54)
    local overview = performanceView:CreateFontString(nil, "OVERLAY", "GameFontHighlightSmall")
    overview:SetPoint("TOPLEFT", 17, -12)
    overview:SetPoint("TOPRIGHT", -17, -12)
    overview:SetJustifyH("LEFT")
    overview:SetTextColor(1, 1, 1, 0.58)
    local perfScroll = ui.CreateScrollArea(performanceView, 8, 52, 7, 8)
    local perfContent = CreateFrame("Frame", nil, perfScroll)
    perfContent:SetWidth(ui.windowWidth - 56)
    perfContent:SetHeight(1)
    perfScroll:SetScrollChild(perfContent)
    local addonHeader = performanceView:CreateFontString(nil, "OVERLAY", "GameFontDisableSmall")
    addonHeader:SetPoint("TOPLEFT", 17, -35)
    addonHeader:SetText(L.ADDON_COLUMN)
    addonHeader:SetTextColor(1, 1, 1, 0.42)
    local memoryHeader = performanceView:CreateFontString(nil, "OVERLAY", "GameFontDisableSmall")
    memoryHeader:SetPoint("TOPRIGHT", -130, -35)
    memoryHeader:SetText(L.MEMORY_COLUMN)
    memoryHeader:SetTextColor(1, 1, 1, 0.42)
    local cpuHeader = performanceView:CreateFontString(nil, "OVERLAY", "GameFontDisableSmall")
    cpuHeader:SetPoint("TOPRIGHT", -28, -35)
    cpuHeader:SetText(L.CPU_COLUMN)
    cpuHeader:SetTextColor(1, 1, 1, 0.42)

    local errors = {}
    local addons = {}
    local selectedError
    local errorRows, perfRows = {}, {}
    local mode = "errors"
    local scope = "current"
    local confirmClear = false
    local callbackOwner = {}
    local callbackRegistered = false
    local refreshQueued = false

    local function FormatMemory(kb)
        return kb >= 1024 and string.format("%.1f MB", kb / 1024) or string.format("%.0f KB", kb)
    end

    local function ApplyErrorRowStyle(row)
        local selected = row.errorEntry == selectedError
        local alpha = selected and 0.82 or (row.isHovered and 0.72 or 0.44)
        row:SetBackdropColor(ui.surfaceR, ui.surfaceG, ui.surfaceB, alpha)
        ui.SetBorderColor(row, selected, selected and 0.42 or (row.isHovered and 0.30 or 0.18))
    end

    local function CreateErrorRow(index)
        local row = CreateFrame("Button", nil, errorContent, "BackdropTemplate")
        row:SetSize(ERROR_LIST_WIDTH - 34, ERROR_ROW_HEIGHT - 2)
        row:SetBackdrop(ui.backdrop)
        local message = row:CreateFontString(nil, "OVERLAY", "GameFontHighlightSmall")
        message:SetPoint("TOPLEFT", 9, -6)
        message:SetPoint("RIGHT", -54, 0)
        message:SetJustifyH("LEFT")
        message:SetWordWrap(false)
        row.message = message
        local count = row:CreateFontString(nil, "OVERLAY", "GameFontDisableSmall")
        count:SetPoint("TOPRIGHT", -9, -6)
        count:SetTextColor(1, 1, 1, 0.42)
        row.count = count
        local metadata = row:CreateFontString(nil, "OVERLAY", "GameFontDisableSmall")
        metadata:SetPoint("BOTTOMLEFT", 9, 5)
        metadata:SetPoint("RIGHT", -9, 0)
        metadata:SetJustifyH("LEFT")
        metadata:SetWordWrap(false)
        metadata:SetTextColor(1, 1, 1, 0.34)
        row.metadata = metadata
        row:SetScript("OnEnter", function(self)
            self.isHovered = true
            ApplyErrorRowStyle(self)
        end)
        row:SetScript("OnLeave", function(self)
            self.isHovered = nil
            ApplyErrorRowStyle(self)
        end)
        row:SetScript("OnClick", function(self)
            selectedError = self.errorEntry
            SetReadOnlyText(reportPanel, ns.Diagnostics.FormatAgentReport(selectedError))
            page:RefreshErrors()
        end)
        errorRows[index] = row
        return row
    end

    local function CreatePerfRow(index)
        local row = CreateFrame("Frame", nil, perfContent)
        row:SetSize(ui.windowWidth - 56, PERF_ROW_HEIGHT)
        local background = row:CreateTexture(nil, "BACKGROUND")
        background:SetAllPoints()
        background:SetColorTexture(ui.surfaceR, ui.surfaceG, ui.surfaceB, index % 2 == 0 and 0.38 or 0.18)
        local name = row:CreateFontString(nil, "OVERLAY", "GameFontHighlightSmall")
        name:SetPoint("LEFT", 9, 0)
        name:SetPoint("RIGHT", -230, 0)
        name:SetJustifyH("LEFT")
        name:SetWordWrap(false)
        row.name = name
        local memory = row:CreateFontString(nil, "OVERLAY", "GameFontHighlightSmall")
        memory:SetPoint("RIGHT", -116, 0)
        memory:SetWidth(100)
        memory:SetJustifyH("RIGHT")
        row.memory = memory
        local cpu = row:CreateFontString(nil, "OVERLAY", "GameFontHighlightSmall")
        cpu:SetPoint("RIGHT", -12, 0)
        cpu:SetWidth(92)
        cpu:SetJustifyH("RIGHT")
        row.cpu = cpu
        perfRows[index] = row
        return row
    end

    function page:RefreshErrors()
        for index = 1, #errorRows do errorRows[index]:Hide() end
        local offset = errorScroll:GetVerticalScroll()
        if issecretvalue and issecretvalue(offset) then offset = 0 end
        local first = math.floor((offset or 0) / ERROR_ROW_HEIGHT) + 1
        local last = math.min(#errors, first + VISIBLE_ROWS - 1)
        local pool = 0
        for itemIndex = first, last do
            pool = pool + 1
            local entry = errors[itemIndex]
            local row = errorRows[pool] or CreateErrorRow(pool)
            row.errorEntry = entry
            row:ClearAllPoints()
            row:SetPoint("TOPLEFT", 0, -((itemIndex - 1) * ERROR_ROW_HEIGHT))
            row.message:SetText((tostring(entry.message or "")):match("([^\r\n]+)") or L.UNKNOWN_ERROR)
            row.count:SetText("x" .. (tonumber(entry.counter) or 1))
            row.metadata:SetText(string.format(L.ERROR_ROW_META,
                tonumber(entry.session) or -1,
                tonumber(entry.time) and date("%m-%d %H:%M:%S", entry.time) or L.UNKNOWN_TIME))
            ApplyErrorRowStyle(row)
            row:Show()
        end
        errorContent:SetHeight(math.max(1, #errors * ERROR_ROW_HEIGHT))
        errorScroll:UpdateScrollChildRect()
        errorEmpty:SetShown(#errors == 0)
        ui.SetButtonEnabled(clearButton, #errors > 0)
        ui.SetButtonEnabled(selectReport, selectedError ~= nil)
    end

    local function RefreshPerformanceRows()
        for index = 1, #perfRows do perfRows[index]:Hide() end
        local offset = perfScroll:GetVerticalScroll()
        if issecretvalue and issecretvalue(offset) then offset = 0 end
        local first = math.floor((offset or 0) / PERF_ROW_HEIGHT) + 1
        local last = math.min(#addons, first + VISIBLE_ROWS - 1)
        local pool = 0
        for itemIndex = first, last do
            pool = pool + 1
            local entry = addons[itemIndex]
            local row = perfRows[pool] or CreatePerfRow(pool)
            row:ClearAllPoints()
            row:SetPoint("TOPLEFT", 0, -((itemIndex - 1) * PERF_ROW_HEIGHT))
            row.name:SetText(entry.title or entry.name)
            row.memory:SetText(FormatMemory(entry.memory or 0))
            row.cpu:SetText(entry.cpu and string.format("%.2f ms", entry.cpu) or "--")
            row:Show()
        end
        perfContent:SetHeight(math.max(1, #addons * PERF_ROW_HEIGHT))
        perfScroll:UpdateScrollChildRect()
    end

    local function RefreshErrorsFromSource()
        confirmClear = false
        ui.SetButtonText(clearButton, L.CLEAR_ERRORS)
        ui.SetButtonVariant(clearButton, "secondary")
        local succeeded, snapshot, errorMessage = ns.Diagnostics.GetErrors(scope, searchPanel.editBox:GetText())
        if not succeeded then
            SetStatus(errorMessage, true)
            return
        end
        errors = snapshot.errors
        selectedError = nil
        SetReadOnlyText(reportPanel, L.SELECT_ERROR_DETAIL)
        SetStatus(string.format(scope == "current" and L.CURRENT_ERRORS_STATUS or L.ALL_ERRORS_STATUS, #errors), false)
        page:RefreshErrors()
    end

    local function RefreshPerformance()
        local succeeded, snapshot, errorMessage = ns.Diagnostics.CollectSystem()
        if not succeeded then
            SetStatus(errorMessage, true)
            return
        end
        addons = snapshot.addons
        overview:SetText(string.format(L.DIAGNOSTIC_SUMMARY,
            snapshot.version,
            snapshot.locale,
            snapshot.scriptErrors and L.ON or L.OFF,
            snapshot.scriptProfile and L.ON or L.OFF,
            snapshot.bugGrabberPaused and L.YES or L.NO))
        SetStatus(L.PERFORMANCE_UPDATED, false)
        RefreshPerformanceRows()
    end

    local function SetScope(newScope)
        scope = newScope
        ui.SetButtonVariant(currentScope, scope == "current" and "selected" or "secondary")
        ui.SetButtonVariant(allScope, scope == "all" and "selected" or "secondary")
        RefreshErrorsFromSource()
    end

    local function SetMode(newMode)
        mode = newMode
        errorsView:SetShown(mode == "errors")
        performanceView:SetShown(mode == "performance")
        clearButton:SetShown(mode == "errors")
        errorsTab:SetActive(mode == "errors")
        performanceTab:SetActive(mode == "performance")
        if mode == "errors" then RefreshErrorsFromSource() else RefreshPerformance() end
    end

    local function QueueErrorRefresh()
        if refreshQueued or mode ~= "errors" then return end
        refreshQueued = true
        C_Timer.After(0, function()
            refreshQueued = false
            if page:IsShown() and mode == "errors" then
                RefreshErrorsFromSource()
            end
        end)
    end

    local function RegisterErrorCallback()
        if callbackRegistered or not EventRegistry then return end
        EventRegistry:RegisterCallback("BugGrabber.BugGrabbed", QueueErrorRefresh, callbackOwner)
        callbackRegistered = true
    end

    local function UnregisterErrorCallback()
        if not callbackRegistered or not EventRegistry then return end
        EventRegistry:UnregisterCallback("BugGrabber.BugGrabbed", callbackOwner)
        callbackRegistered = false
    end

    errorScroll.onVerticalScrollChanged = function() page:RefreshErrors() end
    perfScroll.onVerticalScrollChanged = RefreshPerformanceRows
    currentScope:SetScript("OnClick", function() SetScope("current") end)
    allScope:SetScript("OnClick", function() SetScope("all") end)
    searchButton:SetScript("OnClick", RefreshErrorsFromSource)
    searchPanel.editBox:SetScript("OnEnterPressed", RefreshErrorsFromSource)
    selectReport:SetScript("OnClick", function()
        if selectedError then
            SetReadOnlyText(reportPanel, ns.Diagnostics.FormatAgentReport(selectedError))
            reportPanel:SelectAll()
        else
            SetStatus(L.SELECT_ERROR_FIRST, true)
        end
    end)
    errorsTab:SetScript("OnClick", function() SetMode("errors") end)
    performanceTab:SetScript("OnClick", function() SetMode("performance") end)
    refreshButton:SetScript("OnClick", function()
        if mode == "errors" then RefreshErrorsFromSource() else RefreshPerformance() end
    end)
    clearButton:SetScript("OnClick", function()
        if not confirmClear then
            confirmClear = true
            ui.SetButtonText(clearButton, L.CONFIRM_CLEAR_ERRORS)
            ui.SetButtonVariant(clearButton, "danger")
            SetStatus(L.CONFIRM_CLEAR_ERRORS, true)
            return
        end
        local succeeded, message = ns.Diagnostics.ResetErrors()
        SetStatus(message, not succeeded)
        if succeeded then RefreshErrorsFromSource() end
    end)

    page:SetScript("OnShow", RegisterErrorCallback)
    page:SetScript("OnHide", UnregisterErrorCallback)

    function page:Activate()
        SetMode(mode)
    end
    function page:Stop()
        UnregisterErrorCallback()
    end

    performanceView:Hide()
    errorsTab:SetActive(true)
    performanceTab:SetActive(false)
    ui.SetButtonEnabled(clearButton, false)
    ui.SetButtonEnabled(selectReport, false)
    SetStatus(L.READY, false)
    return page
end
