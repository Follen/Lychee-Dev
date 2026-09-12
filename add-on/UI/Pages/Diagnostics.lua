local ADDON_NAME, ns = ...
local L = ns.L

local ERROR_ROW_HEIGHT = 44
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

    local refreshButton = ui.CreateButton(page, 92, L.REFRESH, false)
    refreshButton:SetPoint("TOPRIGHT", -14, -84)
    local clearButton = ui.CreateButton(page, 178, L.CLEAR_ERRORS, false)
    clearButton:SetPoint("RIGHT", refreshButton, "LEFT", -8, 0)

    local status = page:CreateFontString(nil, "OVERLAY", "GameFontHighlightSmall")
    status:SetPoint("TOPLEFT", 17, -94)
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
    local exportReport = ui.CreateButton(errorsView, 92, L.SAVE_TO_DISK, false)
    exportReport:SetPoint("RIGHT", selectReport, "LEFT", -8, 0)
    page.currentScope = currentScope
    page.allScope = allScope
    page.refreshButton = refreshButton
    page.clearButton = clearButton
    page.selectReport = selectReport
    page.exportReport = exportReport

    local listLabel = ui.CreateSectionLabel(errorsView, L.ERROR_LIST)
    listLabel:SetPoint("TOPLEFT", 17, -48)
    local reportLabel = ui.CreateSectionLabel(errorsView, L.AGENT_REPORT)
    reportLabel:SetPoint("TOPLEFT", ERROR_LIST_WIDTH + 29, -48)

    local listPanel = ui.CreatePanel(errorsView, ui.editorR, ui.editorG, ui.editorB, 0.78)
    listPanel:SetPoint("TOPLEFT", 14, -68)
    listPanel:SetPoint("BOTTOMLEFT", 14, 54)
    listPanel:SetWidth(ERROR_LIST_WIDTH)
    local errorScroll = ui.CreateScrollArea(listPanel, 8, 8, 7, 8)
    local errorContent = CreateFrame("Frame", nil, errorScroll)
    errorContent:SetWidth(ERROR_LIST_WIDTH - 26)
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

    local errors = {}
    local selectedError
    local errorRows = {}
    local scope = "current"
    local confirmClear = false
    local callbackOwner = {}
    local callbackRegistered = false
    local refreshQueued = false

    local function ApplyErrorRowStyle(row)
        local selected = row.errorEntry == selectedError
        ui.SetListRowState(row, selected, row.isHovered)
    end

    local function CreateErrorRow(index)
        local row = ui.CreateListRow(errorContent, ERROR_ROW_HEIGHT)
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
            SetReadOnlyText(reportPanel, ns.Diagnostics.FormatAgentReport(selectedError))
            page:RefreshErrors()
        end)
        errorRows[index] = row
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
        ui.SetButtonEnabled(exportReport, #errors > 0)
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

    local function SetScope(newScope)
        scope = newScope
        ui.SetButtonVariant(currentScope, scope == "current" and "selected" or "secondary")
        ui.SetButtonVariant(allScope, scope == "all" and "selected" or "secondary")
        RefreshErrorsFromSource()
    end

    local function QueueErrorRefresh()
        if refreshQueued then return end
        refreshQueued = true
        C_Timer.After(0, function()
            refreshQueued = false
            if page:IsShown() then
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
    exportReport:SetScript("OnClick", function()
        if #errors > 0 then
            ui.ExportText("error_log", L.ERROR_LIST, function()
                return ns.SerializeForExport(errors)
            end, { scope = scope, query = searchPanel.editBox:GetText(), recordCount = #errors })
        else
            SetStatus(L.NO_ERRORS, true)
        end
    end)
    refreshButton:SetScript("OnClick", RefreshErrorsFromSource)
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
        RefreshErrorsFromSource()
    end
    function page:Stop()
        UnregisterErrorCallback()
    end

    ui.SetButtonEnabled(clearButton, false)
    ui.SetButtonEnabled(selectReport, false)
    ui.SetButtonEnabled(exportReport, false)
    SetStatus(L.READY, false)
    return page
end
