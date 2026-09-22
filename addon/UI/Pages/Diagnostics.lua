local ADDON_NAME, ns = ...
local L = ns.L

-- Diagnostics workbench page: bounded !BugGrabber error browsing with a scope
-- toggle, keyword search, a two-click clear and the agent report pane. The
-- provider is a SOFT integration: nothing is fabricated when it is missing.
local ERROR_ROW_HEIGHT = 44
local ERROR_LIST_WIDTH = 430
local VISIBLE_ROWS = 16

-- The single built page instance; only read by the shell export hook.
local builtPage

local function IsSecret(value)
    return issecretvalue and issecretvalue(value)
end

local function SafeNumber(value)
    if value == nil or IsSecret(value) then
        return nil
    end
    return tonumber(value)
end

local function CreateLineInput(parent)
    local W = ns.Widgets
    local colors = W.Colors
    local panel = W.CreatePanel(parent, colors.editor[1], colors.editor[2], colors.editor[3], 1)
    local editBox = CreateFrame("EditBox", nil, panel)
    editBox:SetAutoFocus(false)
    editBox:SetFontObject(ChatFontNormal)
    editBox:SetTextColor(0.94, 0.95, 0.96)
    editBox:SetTextInsets(9, 9, 0, 0)
    editBox:SetPoint("TOPLEFT", 1, -1)
    editBox:SetPoint("BOTTOMRIGHT", -1, 1)
    editBox:SetScript("OnEscapePressed", function(self)
        self:ClearFocus()
    end)
    editBox:SetScript("OnEditFocusGained", function()
        W.SetBorderColor(panel, true, 0.75)
    end)
    editBox:SetScript("OnEditFocusLost", function()
        W.SetBorderColor(panel, false)
    end)
    panel.editBox = editBox
    return panel
end

local function BuildDiagnosticsPage(parent)
    local W = ns.Widgets
    local colors = W.Colors
    local layout = ns.Workbench.Layout
    local accent = colors.accent

    local page = CreateFrame("Frame", nil, parent)
    page:SetAllPoints(parent)

    local refreshButton = W.CreateButton(page, 92, L.REFRESH, false)
    refreshButton:SetPoint("TOPRIGHT", -14, -84)
    local clearButton = W.CreateButton(page, 178, L.CLEAR_ERRORS, false)
    clearButton:SetPoint("RIGHT", refreshButton, "LEFT", -8, 0)

    local status = page:CreateFontString(nil, "OVERLAY", "GameFontHighlightSmall")
    status:SetPoint("TOPLEFT", 17, -94)
    status:SetPoint("RIGHT", clearButton, "LEFT", -10, 0)
    status:SetJustifyH("LEFT")
    local function SetStatus(text, errorState)
        local r = errorState and accent[1] or 0.55
        local g = errorState and accent[2] or 0.60
        local b = errorState and accent[3] or 0.65
        status:SetText(text or "")
        status:SetTextColor(r, g, b, 0.92)
    end

    local errorsView = CreateFrame("Frame", nil, page)
    errorsView:SetPoint("TOPLEFT", 0, -126)
    errorsView:SetPoint("BOTTOMRIGHT")

    local currentScope = W.CreateButton(errorsView, 104, L.CURRENT_SESSION, "selected")
    currentScope:SetPoint("TOPLEFT", 14, 0)
    local allScope = W.CreateButton(errorsView, 104, L.ALL_SESSIONS, "secondary")
    allScope:SetPoint("LEFT", currentScope, "RIGHT", 8, 0)
    local searchPanel = CreateLineInput(errorsView)
    searchPanel:SetPoint("TOPLEFT", allScope, "TOPRIGHT", 14, 0)
    searchPanel:SetWidth(260)
    searchPanel:SetHeight(28)
    local searchButton = W.CreateButton(errorsView, 76, L.SEARCH, false)
    searchButton:SetPoint("LEFT", searchPanel, "RIGHT", 8, 0)
    local selectReport = W.CreateButton(errorsView, 110, L.SELECT_REPORT, false)
    selectReport:SetPoint("TOPRIGHT", -14, 0)
    local exportReport = W.CreateButton(errorsView, 92, L.SAVE_TO_DISK, false)
    exportReport:SetPoint("RIGHT", selectReport, "LEFT", -8, 0)
    page.currentScope = currentScope
    page.allScope = allScope
    page.refreshButton = refreshButton
    page.clearButton = clearButton
    page.selectReport = selectReport
    page.exportReport = exportReport
    page.searchPanel = searchPanel
    page.searchButton = searchButton

    local listLabel = W.CreateSectionLabel(errorsView, L.ERROR_LIST)
    listLabel:SetPoint("TOPLEFT", 17, -48)
    local reportLabel = W.CreateSectionLabel(errorsView, L.AGENT_REPORT)
    reportLabel:SetPoint("TOPLEFT", ERROR_LIST_WIDTH + 29, -48)

    local listPanel = W.CreatePanel(errorsView, colors.editor[1], colors.editor[2], colors.editor[3], 0.78)
    listPanel:SetPoint("TOPLEFT", 14, -68)
    listPanel:SetPoint("BOTTOMLEFT", 14, 54)
    listPanel:SetWidth(ERROR_LIST_WIDTH)
    page.listPanel = listPanel
    local errorScroll = W.CreateScrollArea(listPanel, 8, 8, 7, 8)
    local errorContent = CreateFrame("Frame", nil, errorScroll)
    errorContent:SetWidth(ERROR_LIST_WIDTH - 26)
    errorContent:SetHeight(1)
    errorScroll:SetScrollChild(errorContent)
    local errorEmpty = listPanel:CreateFontString(nil, "OVERLAY", "GameFontDisableSmall")
    errorEmpty:SetPoint("TOP", 0, -24)
    errorEmpty:SetText(L.NO_ERRORS)
    errorEmpty:SetTextColor(1, 1, 1, 0.32)

    local reportPanel = W.CreateTextArea(errorsView, true)
    reportPanel:SetPoint("TOPLEFT", ERROR_LIST_WIDTH + 26, -68)
    reportPanel:SetPoint("BOTTOMRIGHT", -14, 54)
    reportPanel.editBox:SetWidth(layout.WINDOW_WIDTH - ERROR_LIST_WIDTH - 78)
    reportPanel.editBox:SetScript("OnMouseUp", function(self)
        self:SetFocus()
    end)
    W.SetReadOnlyText(reportPanel, L.SELECT_ERROR_DETAIL)
    page.reportPanel = reportPanel

    local errors = {}
    local selectedError
    local errorRows = {}
    page.errorRows = errorRows
    local scope = "current"
    local confirmClear = false
    local callbackOwner = {}
    local callbackRegistered = false
    local refreshQueued = false
    local pageActive = false

    local function ResetClearConfirm()
        confirmClear = false
        W.SetButtonText(clearButton, L.CLEAR_ERRORS)
        W.SetButtonVariant(clearButton, "secondary")
    end

    local function ApplyErrorRowStyle(row)
        local selected = row.errorEntry == selectedError
        W.SetListRowState(row, selected, row.isHovered)
    end

    local RefreshErrors
    local RefreshErrorsFromSource

    local function CreateErrorRow(index)
        local row = W.CreateListRow(errorContent, ERROR_ROW_HEIGHT)
        row:SetWidth(ERROR_LIST_WIDTH - 26)
        local message = row:CreateFontString(nil, "OVERLAY", "GameFontHighlightSmall")
        message:SetPoint("TOPLEFT", 9, -6)
        message:SetPoint("TOPRIGHT", -54, -6)
        message:SetJustifyH("LEFT")
        message:SetWordWrap(false)
        row.message = message
        local count = row:CreateFontString(nil, "OVERLAY", "GameFontDisableSmall")
        count:SetPoint("TOPRIGHT", -9, -6)
        count:SetTextColor(1, 1, 1, 0.42)
        row.count = count
        local metadata = row:CreateFontString(nil, "OVERLAY", "GameFontDisableSmall")
        metadata:SetPoint("BOTTOMLEFT", 9, 5)
        metadata:SetPoint("BOTTOMRIGHT", -9, 5)
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
            W.SetReadOnlyText(reportPanel, ns.Diagnostics.FormatAgentReport(selectedError))
            RefreshErrors()
        end)
        errorRows[index] = row
        return row
    end

    RefreshErrors = function()
        for index = 1, #errorRows do
            errorRows[index]:Hide()
        end
        local offset = errorScroll:GetVerticalScroll()
        if issecretvalue and issecretvalue(offset) then
            offset = 0
        end
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
            row.message:SetText(ns.Diagnostics.SafeText(entry.message):match("([^\r\n]+)") or L.UNKNOWN_ERROR)
            row.count:SetText("x" .. (SafeNumber(entry.counter) or 1))
            row.metadata:SetText(string.format(L.ERROR_ROW_META,
                SafeNumber(entry.session) or -1,
                SafeNumber(entry.time) and date("%m-%d %H:%M:%S", SafeNumber(entry.time)) or L.UNKNOWN_TIME))
            ApplyErrorRowStyle(row)
            row:Show()
        end
        errorContent:SetHeight(math.max(1, #errors * ERROR_ROW_HEIGHT))
        errorScroll:UpdateScrollChildRect()
        errorEmpty:SetShown(#errors == 0)
        W.SetButtonEnabled(clearButton, #errors > 0)
        W.SetButtonEnabled(selectReport, selectedError ~= nil)
        W.SetButtonEnabled(exportReport, #errors > 0)
    end

    RefreshErrorsFromSource = function()
        ResetClearConfirm()
        local succeeded, snapshot, errorMessage = ns.Diagnostics.GetErrors(scope, searchPanel.editBox:GetText())
        if not succeeded then
            errors = {}
            selectedError = nil
            SetStatus(errorMessage, true)
            RefreshErrors()
            return
        end
        errors = snapshot.errors
        selectedError = nil
        W.SetReadOnlyText(reportPanel, L.SELECT_ERROR_DETAIL)
        SetStatus(string.format(scope == "current" and L.CURRENT_ERRORS_STATUS or L.ALL_ERRORS_STATUS, #errors), false)
        RefreshErrors()
    end

    local function SetScope(newScope)
        scope = newScope
        W.SetButtonVariant(currentScope, scope == "current" and "selected" or "secondary")
        W.SetButtonVariant(allScope, scope == "all" and "selected" or "secondary")
        RefreshErrorsFromSource()
    end

    local function QueueErrorRefresh()
        if refreshQueued then
            return
        end
        refreshQueued = true
        C_Timer.After(0, function()
            refreshQueued = false
            if pageActive then
                RefreshErrorsFromSource()
            end
        end)
    end

    local function RegisterErrorCallback()
        if callbackRegistered or not EventRegistry then
            return
        end
        EventRegistry:RegisterCallback("BugGrabber.BugGrabbed", QueueErrorRefresh, callbackOwner)
        callbackRegistered = true
    end

    local function UnregisterErrorCallback()
        if not callbackRegistered or not EventRegistry then
            return
        end
        EventRegistry:UnregisterCallback("BugGrabber.BugGrabbed", callbackOwner)
        callbackRegistered = false
    end

    errorScroll.onVerticalScrollChanged = function()
        RefreshErrors()
    end
    currentScope:SetScript("OnClick", function()
        SetScope("current")
    end)
    allScope:SetScript("OnClick", function()
        SetScope("all")
    end)
    searchButton:SetScript("OnClick", RefreshErrorsFromSource)
    searchPanel.editBox:SetScript("OnEnterPressed", RefreshErrorsFromSource)
    selectReport:SetScript("OnClick", function()
        if selectedError then
            W.SetReadOnlyText(reportPanel, ns.Diagnostics.FormatAgentReport(selectedError))
            W.SelectAllText(reportPanel)
        else
            SetStatus(L.SELECT_ERROR_FIRST, true)
        end
    end)
    exportReport:SetScript("OnClick", function()
        if ns.Safety.IsCombatBlocked() then
            ns.Safety.PrintBlocked()
            return
        end
        local kind, title, content, metadata = page.GetErrorLogPayload()
        if not kind then
            SetStatus(L.NO_ERRORS, true)
            return
        end
        ns.Workbench.SaveToDisk(kind, title, content, metadata)
    end)
    refreshButton:SetScript("OnClick", RefreshErrorsFromSource)

    -- Two-click destructive confirm; the arm state is also announced in the
    -- status line and disarmed by every refresh.
    clearButton:SetScript("OnClick", function()
        if ns.Safety.IsCombatBlocked() then
            ns.Safety.PrintBlocked()
            return
        end
        if not confirmClear then
            confirmClear = true
            W.SetButtonText(clearButton, L.CONFIRM_CLEAR_ERRORS)
            W.SetButtonVariant(clearButton, "danger")
            SetStatus(L.CONFIRM_CLEAR_ERRORS, true)
            return
        end
        ResetClearConfirm()
        local succeeded, message = ns.Diagnostics.ResetErrors()
        SetStatus(message, not succeeded)
        if succeeded then
            RefreshErrorsFromSource()
        end
    end)

    function page.HandleActivate()
        pageActive = true
        RegisterErrorCallback()
        RefreshErrorsFromSource()
    end

    -- One payload builder shared by the page Save button and the shell export
    -- hook; both funnel into the single export UI.
    function page.GetErrorLogPayload()
        if #errors == 0 then
            return nil
        end
        local captured = errors
        return "error_log", L.ERROR_LIST, function()
            local text = ns.Serializer.SerializeForExport(captured)
            return text
        end, { scope = scope, query = searchPanel.editBox:GetText(), recordCount = #captured }
    end

    function page.HandleSuspend()
        pageActive = false
        UnregisterErrorCallback()
    end

    function page.HandleShutdown()
        pageActive = false
        UnregisterErrorCallback()
    end

    W.SetButtonEnabled(clearButton, false)
    W.SetButtonEnabled(selectReport, false)
    W.SetButtonEnabled(exportReport, false)
    SetStatus(L.READY, false)
    builtPage = page
    return page
end

ns.Workbench.RegisterPage({
    key = "diagnostics",
    titleKey = "TAB_DIAGNOSTICS",
    build = BuildDiagnosticsPage,
    activate = function(page)
        if page and page.HandleActivate then
            page:HandleActivate()
        end
    end,
    suspend = function(page)
        if page and page.HandleSuspend then
            page:HandleSuspend()
        end
    end,
    shutdown = function(page)
        if page and page.HandleShutdown then
            page:HandleShutdown()
        end
    end,
})

ns.Workbench.RegisterExportHook({
    key = "diagnostics",
    GetPayload = function()
        if not builtPage then
            return nil
        end
        return builtPage.GetErrorLogPayload()
    end,
})
