local ADDON_NAME, ns = ...
local L = ns.L

local ROW_HEIGHT = 58
local LIST_WIDTH = 354
local REPORT_DISPLAY_BYTES = 48 * 1024

local STATUS_LABELS = {
    running = "AUTO_STATUS_RUNNING",
    finalizing = "AUTO_STATUS_FINALIZING",
    succeeded = "AUTO_STATUS_SUCCEEDED",
    failed = "AUTO_STATUS_FAILED",
    cancelled = "AUTO_STATUS_CANCELLED",
    interrupted = "AUTO_STATUS_INTERRUPTED",
}

local KIND_LABELS = {
    lua = "AUTO_KIND_LUA",
    bug_snapshot = "AUTO_KIND_BUG",
}

local function GetStatusKey(status)
    return STATUS_LABELS[status] and L[STATUS_LABELS[status]] or tostring(status or L.UNKNOWN)
end

local function GetKindLabel(kind)
    local key = KIND_LABELS[kind]
    return key and L[key] or L.EXPORT_KIND_UNKNOWN
end

local function FormatTime(timestamp)
    timestamp = tonumber(timestamp) or 0
    if timestamp <= 0 then
        return L.UNKNOWN
    end
    return date("%Y-%m-%d %H:%M:%S", timestamp)
end

local RESULT_STATE_LABELS = {
    pending = "AUTO_PENDING_DISK",
    flushed = "AUTO_LOADED_SAVED",
    received = "AUTO_ACK_RECEIVED",
    failed = "AUTO_ACK_FAILED",
    expired = "AUTO_RESULT_EXPIRED",
}

-- The life cycle lives in one place (Core/Database.lua); the page only renders
-- it, so the notice, the list and the detail pane cannot disagree.
local function GetResultState(record)
    local state = ns.AutomationResultState(record)
    local key = RESULT_STATE_LABELS[state]
    return key and L[key] or "-"
end

local function CreateField(parent, labelText, x, y, width)
    local label = parent:CreateFontString(nil, "OVERLAY", "GameFontDisableSmall")
    label:SetPoint("TOPLEFT", x, y)
    label:SetText(labelText)
    label:SetTextColor(1, 1, 1, 0.34)

    local value = parent:CreateFontString(nil, "OVERLAY", "GameFontHighlightSmall")
    value:SetPoint("TOPLEFT", x, y - 20)
    value:SetWidth(width)
    value:SetJustifyH("LEFT")
    value:SetWordWrap(false)
    value:SetTextColor(0.91, 0.93, 0.95, 0.92)
    return value
end

function ns.CreateAutomationPage(parent, ui)
    local page = CreateFrame("Frame", nil, parent)
    page:SetAllPoints(parent)

    local selectedExecutionId
    local rows = {}
    local clearConfirmed = false

    local heading = ui.CreateSectionLabel(page, L.TAB_AUTOMATION)
    heading:SetPoint("TOPLEFT", 17, -84)

    local countText = page:CreateFontString(nil, "OVERLAY", "GameFontDisableSmall")
    countText:SetPoint("LEFT", heading, "RIGHT", 12, 0)
    countText:SetTextColor(1, 1, 1, 0.36)

    local taskLabel = page:CreateFontString(nil, "OVERLAY", "GameFontDisableSmall")
    taskLabel:SetPoint("TOPRIGHT", -14, -84)
    taskLabel:SetText(L.AUTO_FIELD_TASK)
    taskLabel:SetTextColor(1, 1, 1, 0.34)

    local taskPanel = ui.CreatePanel(page, ui.editorR, ui.editorG, ui.editorB, 1)
    taskPanel:SetPoint("TOPRIGHT", -14, -104)
    taskPanel:SetWidth(220)
    taskPanel:SetHeight(30)

    local taskInput = CreateFrame("EditBox", nil, taskPanel)
    taskInput:SetPoint("TOPLEFT", 10, -1)
    taskInput:SetPoint("BOTTOMRIGHT", -10, 1)
    taskInput:SetAutoFocus(false)
    taskInput:SetFontObject(ChatFontNormal)
    taskInput:SetTextColor(0.94, 0.95, 0.96)
    taskInput:SetScript("OnEscapePressed", function(self) self:ClearFocus() end)
    taskInput:SetScript("OnEnterPressed", function(self)
        self:ClearFocus()
        if ns.Automation then
            ns.Automation.Run(self:GetText())
        end
    end)

    local runButton = ui.CreateButton(page, 96, L.AUTO_RUN, true)
    runButton:SetPoint("RIGHT", taskPanel, "LEFT", -8, 0)
    runButton:SetScript("OnClick", function()
        if ns.Automation then
            ns.Automation.Run(taskInput:GetText())
        end
    end)

    local cancelButton = ui.CreateButton(page, 110, L.AUTO_CANCEL, false)
    cancelButton:SetPoint("RIGHT", runButton, "LEFT", -8, 0)

    local showNoticeButton = ui.CreateButton(page, 128, L.AUTO_SHOW_NOTICE, false)
    showNoticeButton:SetPoint("RIGHT", cancelButton, "LEFT", -8, 0)

    local hideNoticeButton = ui.CreateButton(page, 118, L.AUTO_HIDE_NOTICE, false)
    hideNoticeButton:SetPoint("RIGHT", showNoticeButton, "LEFT", -8, 0)

    local clearButton = ui.CreateButton(page, 118, L.AUTO_CLEAR_HISTORY, false)
    clearButton:SetPoint("RIGHT", hideNoticeButton, "LEFT", -8, 0)

    local listPanel = ui.CreatePanel(page, ui.editorR, ui.editorG, ui.editorB, 0.78)
    listPanel:SetPoint("TOPLEFT", 14, -144)
    listPanel:SetPoint("BOTTOMLEFT", 14, 54)
    listPanel:SetWidth(LIST_WIDTH)

    local listScroll = ui.CreateScrollArea(listPanel, 8, 8, 7, 8)
    local listContent = CreateFrame("Frame", nil, listScroll)
    listContent:SetWidth(LIST_WIDTH - 26)
    listContent:SetHeight(1)
    listScroll:SetScrollChild(listContent)

    local emptyTitle = listPanel:CreateFontString(nil, "OVERLAY", "GameFontNormal")
    emptyTitle:SetPoint("CENTER", listPanel, "CENTER", 0, 14)
    emptyTitle:SetText(L.AUTO_NO_EXECUTIONS)
    emptyTitle:SetTextColor(1, 1, 1, 0.58)

    local detailPanel = CreateFrame("Frame", nil, page)
    detailPanel:SetPoint("TOPLEFT", listPanel, "TOPRIGHT", 12, 0)
    detailPanel:SetPoint("BOTTOMRIGHT", -14, 54)

    local detailHeading = detailPanel:CreateFontString(nil, "OVERLAY", "GameFontNormal")
    detailHeading:SetPoint("TOPLEFT", 4, -4)
    detailHeading:SetText(L.AUTO_DETAIL)

    local detailState = detailPanel:CreateFontString(nil, "OVERLAY", "GameFontHighlightSmall")
    detailState:SetPoint("TOPRIGHT", -4, -4)

    local taskValue = CreateField(detailPanel, L.AUTO_FIELD_TASK, 4, -48, 282)
    local kindValue = CreateField(detailPanel, L.EXPORT_RECORD_KIND, 314, -48, 272)
    local requestValue = CreateField(detailPanel, L.AUTO_FIELD_REQUEST, 4, -106, 282)
    local statusValue = CreateField(detailPanel, L.AUTO_FIELD_STATUS, 314, -106, 272)
    local startedValue = CreateField(detailPanel, L.AUTO_FIELD_STARTED, 4, -164, 282)
    local finishedValue = CreateField(detailPanel, L.AUTO_FIELD_FINISHED, 314, -164, 272)
    local revisionValue = CreateField(detailPanel, L.AUTO_FIELD_REVISION, 4, -222, 282)
    local errorValue = CreateField(detailPanel, L.AUTO_FIELD_ERROR, 314, -222, 272)

    local ticketLabel = detailPanel:CreateFontString(nil, "OVERLAY", "GameFontDisableSmall")
    ticketLabel:SetPoint("TOPLEFT", 4, -284)
    ticketLabel:SetText(L.EXPORT_TICKET)
    ticketLabel:SetTextColor(1, 1, 1, 0.34)

    local ticketPanel = ui.CreatePanel(detailPanel, ui.editorR, ui.editorG, ui.editorB, 1)
    ticketPanel:SetPoint("TOPLEFT", 4, -305)
    ticketPanel:SetPoint("TOPRIGHT", -140, -305)
    ticketPanel:SetHeight(36)

    local ticketBox = CreateFrame("EditBox", nil, ticketPanel)
    ticketBox:SetPoint("TOPLEFT", 10, -1)
    ticketBox:SetPoint("BOTTOMRIGHT", -10, 1)
    ticketBox:SetAutoFocus(false)
    ticketBox:SetFontObject(ChatFontNormal)
    ticketBox:SetTextColor(0.94, 0.95, 0.96)
    ticketBox:SetScript("OnEscapePressed", function(self) self:ClearFocus() end)

    local viewReportButton = ui.CreateButton(detailPanel, 118, L.AUTO_VIEW_REPORT, false)
    viewReportButton:SetPoint("TOPRIGHT", -4, -305)

    local reportArea = ui.CreateTextArea(detailPanel, true)
    reportArea:SetPoint("TOPLEFT", 4, -352)
    reportArea:SetPoint("BOTTOMRIGHT", -4, 4)

    local function ApplyRowState(row)
        local selected = row.executionId == selectedExecutionId
        ui.SetListRowState(row, selected, row.hovered)
        if selected then
            row.title:SetTextColor(1, 1, 1, 0.96)
        elseif row.hovered then
            row.title:SetTextColor(1, 1, 1, 0.90)
        else
            row.title:SetTextColor(1, 1, 1, 0.76)
        end
    end

    local function RefreshDetail()
        local automation = ns.GetAutomationIndex()
        local record = selectedExecutionId and automation
            and automation.records[selectedExecutionId] or nil
        local hasRecord = record ~= nil
        local resultState = GetResultState(record)
        local hasResult = hasRecord and record.ticket and ns.GetExport(record.ticket) ~= nil

        detailHeading:SetText(hasRecord and record.taskId or L.AUTO_DETAIL)
        detailState:SetText(hasRecord and resultState or "")
        detailState:SetTextColor(1, 1, 1, 0.36)
        taskValue:SetText(hasRecord and record.taskId or "")
        kindValue:SetText(hasRecord and GetKindLabel(record.kind) or "")
        requestValue:SetText(hasRecord and record.executionId or "")
        statusValue:SetText(hasRecord and GetStatusKey(record.status) or "")
        startedValue:SetText(hasRecord and FormatTime(record.startedAt) or "")
        finishedValue:SetText(hasRecord and FormatTime(record.finishedAt) or "")
        revisionValue:SetText(hasRecord and (record.revision or L.NOT_AVAILABLE) or "")
        errorValue:SetText(hasRecord and (record.errorCode or L.NOT_AVAILABLE) or "")

        ticketBox.savedText = hasResult and record.ticket or ""
        ticketBox.updatingText = true
        ticketBox:SetText(ticketBox.savedText)
        ticketBox.updatingText = nil
        ui.SetButtonEnabled(showNoticeButton, hasResult and not (ns.Automation and ns.Automation.IsBusy()))
        ui.SetButtonEnabled(viewReportButton, hasResult)

        if not hasResult then
            ui.SetReadOnlyText(reportArea, "")
        end
    end

    local function SelectRecord(executionId)
        selectedExecutionId = executionId
        for index = 1, #rows do
            ApplyRowState(rows[index])
        end
        RefreshDetail()
    end

    local function AcquireRow(index)
        local row = rows[index]
        if row then
            return row
        end

        row = ui.CreateListRow(listContent, ROW_HEIGHT)
        row:SetPoint("TOPLEFT", 0, -(index - 1) * ROW_HEIGHT)
        row:SetPoint("TOPRIGHT", 0, -(index - 1) * ROW_HEIGHT)
        row:EnableMouseWheel(true)

        local title = row:CreateFontString(nil, "OVERLAY", "GameFontHighlightSmall")
        title:SetPoint("TOPLEFT", 12, -10)
        title:SetPoint("TOPRIGHT", -100, -10)
        title:SetJustifyH("LEFT")
        title:SetWordWrap(false)
        row.title = title

        local status = row:CreateFontString(nil, "OVERLAY", "GameFontDisableSmall")
        status:SetPoint("TOPRIGHT", -10, -10)
        status:SetJustifyH("RIGHT")
        row.status = status

        local meta = row:CreateFontString(nil, "OVERLAY", "GameFontDisableSmall")
        meta:SetPoint("BOTTOMLEFT", 12, 9)
        meta:SetPoint("BOTTOMRIGHT", -10, 9)
        meta:SetJustifyH("LEFT")
        meta:SetWordWrap(false)
        meta:SetTextColor(1, 1, 1, 0.34)
        row.meta = meta

        row:SetScript("OnEnter", function(self)
            self.hovered = true
            ApplyRowState(self)
        end)
        row:SetScript("OnLeave", function(self)
            self.hovered = nil
            ApplyRowState(self)
        end)
        row:SetScript("OnClick", function(self)
            SelectRecord(self.executionId)
        end)
        row:SetScript("OnMouseWheel", listScroll.onMouseWheel)
        rows[index] = row
        return row
    end

    local function Refresh()
        local automation = ns.GetAutomationIndex()
        local order = automation and automation.order or {}
        countText:SetText(string.format(L.AUTO_EXECUTION_COUNT, #order))
        emptyTitle:SetShown(#order == 0)
        ui.SetButtonEnabled(clearButton, #order > 0)

        if clearConfirmed then
            clearConfirmed = false
            ui.SetButtonText(clearButton, L.AUTO_CLEAR_HISTORY)
            ui.SetButtonVariant(clearButton, "secondary")
        end

        for index = 1, #order do
            local record = automation.records[order[index]]
            local row = AcquireRow(index)
            row.executionId = order[index]
            row.title:SetText(record.taskId)
            local state = GetResultState(record)
            row.status:SetText(GetStatusKey(record.status))
            if record.status == "succeeded" then
                row.status:SetTextColor(0.42, 0.76, 0.43, 0.92)
            elseif record.status == "running" or record.status == "finalizing" then
                row.status:SetTextColor(ui.accentR, ui.accentG, ui.accentB, 0.92)
            else
                row.status:SetTextColor(1, 1, 1, 0.30)
            end
            row.meta:SetText(FormatTime(record.startedAt) .. " | " .. GetKindLabel(record.kind)
                .. " | " .. state)
            row:Show()
        end
        for index = #order + 1, #rows do
            rows[index]:Hide()
        end

        listContent:SetHeight(math.max(1, #order * ROW_HEIGHT))
        listScroll:UpdateScrollChildRect()
        if selectedExecutionId and not (automation and automation.records[selectedExecutionId]) then
            selectedExecutionId = nil
        end
        if not selectedExecutionId and order[1] then
            selectedExecutionId = order[1]
        end
        for index = 1, #rows do
            ApplyRowState(rows[index])
        end
        RefreshDetail()
    end

    ticketBox:SetScript("OnTextChanged", function(self)
        if not self.updatingText and self:GetText() ~= self.savedText then
            self.updatingText = true
            self:SetText(self.savedText or "")
            self.updatingText = nil
            self:HighlightText()
        end
    end)
    ticketBox:SetScript("OnMouseUp", function()
        if ticketBox.savedText ~= "" then
            ui.SelectAllText(ticketBox)
        end
    end)

    cancelButton:SetScript("OnClick", function()
        if selectedExecutionId then
            local automation = ns.GetAutomationIndex()
            local record = automation and automation.records[selectedExecutionId] or nil
            if record and ns.Automation then
                ns.Automation.Cancel(record.taskId)
            end
        end
    end)

    showNoticeButton:SetScript("OnClick", function()
        if ns.Automation then
            ns.Automation.Show(ticketBox.savedText or "")
        end
    end)

    hideNoticeButton:SetScript("OnClick", function()
        if ns.Automation then
            ns.Automation.Stop()
        end
    end)

    clearButton:SetScript("OnClick", function()
        if not clearConfirmed then
            clearConfirmed = true
            ui.SetButtonText(clearButton, L.CONFIRM_CLEAR_CACHE)
            ui.SetButtonVariant(clearButton, "danger")
            return
        end
        ns.ClearAutomationHistory()
        selectedExecutionId = nil
        Refresh()
    end)

    viewReportButton:SetScript("OnClick", function()
        local entry = ticketBox.savedText ~= "" and ns.GetExport(ticketBox.savedText) or nil
        if not entry or not entry.payload then
            return
        end
        local content = entry.payload.content or ""
        if #content > REPORT_DISPLAY_BYTES then
            ui.SetReadOnlyText(reportArea, content:sub(1, REPORT_DISPLAY_BYTES)
                .. "\n... " .. string.format(L.AUTO_REPORT_DISPLAY_LIMIT, REPORT_DISPLAY_BYTES / 1024))
        else
            ui.SetReadOnlyText(reportArea, content)
        end
    end)

    page.Refresh = Refresh
    page.SelectRecord = SelectRecord
    page.ticketBox = ticketBox
    page.taskInput = taskInput
    page.rows = rows
    page.reportArea = reportArea
    page.runButton = runButton
    page.cancelButton = cancelButton
    page.showNoticeButton = showNoticeButton
    page.hideNoticeButton = hideNoticeButton
    page.viewReportButton = viewReportButton
    page.clearButton = clearButton
    function page:Activate()
        Refresh()
    end
    Refresh()
    return page
end
