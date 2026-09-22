local ADDON_NAME, ns = ...

-- Events workbench page: catalog search and selection over the per-client
-- event catalog (Modules/Events/Catalog.lua) plus the bounded event monitor
-- (Modules/Events/Monitor.lua). All visuals come from ns.Widgets; the Save
-- action goes through the one export hook (ns.ExportUI).
local L = ns.L
local W = ns.Widgets

local EVENT_ROW_HEIGHT = 36
local EVENT_LIST_WIDTH = 540
local EVENT_VISIBLE_ROWS = 18
local SEARCH_RESULT_LIMIT = 8
local SEARCH_ROW_HEIGHT = 38
local SELECTED_ROW_HEIGHT = 28
local SELECTED_VISIBLE_ROWS = 3
-- Matches the workbench window content width; rows and the detail edit box
-- size against it so dynamic text never resizes the layout.
local PAGE_WIDTH = 1040

local function Restricted(value)
    return issecretvalue and issecretvalue(value)
end

-- Display boundary guard: a restricted value degrades to a harmless visible
-- token instead of leaking or throwing on format.
local function ShownText(value, fallback)
    if Restricted(value) then
        return "<secret>"
    end
    if value == nil then
        return fallback or ""
    end
    return tostring(value)
end

local function ShownNumber(value, format, fallback)
    if Restricted(value) or type(value) ~= "number" then
        return fallback or "<secret>"
    end
    return string.format(format, value)
end

local function CreateLineInput(parent)
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

local function CreateRemoveButton(parent)
    local button = CreateFrame("Button", nil, parent, "BackdropTemplate")
    button:SetSize(22, 22)
    button:SetBackdrop(W.Backdrop)

    local firstLine = button:CreateTexture(nil, "ARTWORK")
    firstLine:SetSize(9, 1)
    firstLine:SetPoint("CENTER")
    firstLine:SetRotation(0.785398)

    local secondLine = button:CreateTexture(nil, "ARTWORK")
    secondLine:SetSize(9, 1)
    secondLine:SetPoint("CENTER")
    secondLine:SetRotation(-0.785398)

    local colors = W.Colors
    local function ApplyState(hovered, pressed)
        if pressed then
            button:SetBackdropColor(colors.accent[1] * 0.72, colors.accent[2] * 0.72, colors.accent[3] * 0.72, 0.92)
            button:SetBackdropBorderColor(colors.accent[1], colors.accent[2], colors.accent[3], 0.9)
        elseif hovered then
            button:SetBackdropColor(colors.accent[1], colors.accent[2], colors.accent[3], 0.74)
            button:SetBackdropBorderColor(colors.accent[1], colors.accent[2], colors.accent[3], 0.92)
        else
            button:SetBackdropColor(colors.surface[1], colors.surface[2], colors.surface[3], 0.58)
            W.SetBorderColor(button, false, 0.24)
        end
        local alpha = hovered and 0.95 or 0.48
        firstLine:SetColorTexture(1, 1, 1, alpha)
        secondLine:SetColorTexture(1, 1, 1, alpha)
    end

    button:SetScript("OnEnter", function(self)
        self.isHovered = true
        ApplyState(true, false)
    end)
    button:SetScript("OnLeave", function(self)
        self.isHovered = nil
        ApplyState(false, false)
    end)
    button:SetScript("OnMouseDown", function()
        ApplyState(true, true)
    end)
    button:SetScript("OnMouseUp", function(self)
        ApplyState(self.isHovered, false)
    end)
    ApplyState(false, false)
    return button
end

-- The one export hook shared by every page. The window shell owns the shared
-- controller (ns.Workbench.SaveToDisk / GetExportController); the fallback
-- keeps standalone builds working without forking the export UI.
local sharedExportUI
local function ObtainExportUI(parent)
    if ns.Workbench and type(ns.Workbench.GetExportController) == "function" then
        local shared = ns.Workbench.GetExportController()
        if shared then
            return shared
        end
    end
    if not sharedExportUI then
        sharedExportUI = ns.SharedExportUI or ns.ExportUI.Create(parent)
        ns.SharedExportUI = sharedExportUI
    end
    return sharedExportUI
end

-- Save payload for the captured event log: SerializeForExport over at most
-- MAX_RECORDS records of {elapsed, event, arguments, summary}.
local function BuildEventLogPayload()
    local recordCount = ns.EventMonitor.GetCount()
    if recordCount == 0 then
        return nil
    end
    return "event_log", L.CAPTURED_EVENTS, function()
        local exportRecords = {}
        for index = 1, recordCount do
            local record = ns.EventMonitor.GetRecord(index)
            exportRecords[index] = {
                elapsed = record.elapsed,
                event = record.event,
                arguments = record.arguments,
                summary = record.summary,
            }
        end
        local text = ns.Serializer.SerializeForExport(exportRecords)
        return text
    end, { recordCount = recordCount }
end

function ns.CreateEventsPage(parent)
    local colors = W.Colors
    local page = CreateFrame("Frame", nil, parent)
    page:SetAllPoints(parent)

    local inputLabel = W.CreateSectionLabel(page, L.FIND_EVENT)
    inputLabel:SetPoint("TOPLEFT", 17, -84)

    local catalogCount = page:CreateFontString(nil, "OVERLAY", "GameFontDisableSmall")
    catalogCount:SetPoint("LEFT", inputLabel, "RIGHT", 9, 0)
    catalogCount:SetText(string.format(L.CATALOG_COUNT, ns.EventCatalog.GetCount()))
    catalogCount:SetTextColor(1, 1, 1, 0.34)

    local inputPanel = CreateLineInput(page)
    inputPanel:SetPoint("TOPLEFT", 14, -104)
    inputPanel:SetPoint("TOPRIGHT", -278, -104)
    inputPanel:SetHeight(30)
    page.inputPanel = inputPanel

    local inputHint = inputPanel:CreateFontString(nil, "OVERLAY", "GameFontDisableSmall")
    inputHint:SetPoint("LEFT", 10, 0)
    inputHint:SetText(L.EVENT_SEARCH_HINT)
    inputHint:SetTextColor(1, 1, 1, 0.30)
    page.inputHint = inputHint

    local monitorButton = W.CreateButton(page, 140, L.START_MONITORING, "primary")
    monitorButton:SetPoint("TOPRIGHT", -14, -104)

    local clearButton = W.CreateButton(page, 100, L.CLEAR_LOG, "secondary")
    clearButton:SetPoint("RIGHT", monitorButton, "LEFT", -8, 0)
    page.monitorButton = monitorButton
    page.clearButton = clearButton

    local statusDot = page:CreateTexture(nil, "ARTWORK")
    statusDot:SetSize(5, 5)
    statusDot:SetPoint("TOPLEFT", inputPanel, "BOTTOMLEFT", 2, -15)

    local status = page:CreateFontString(nil, "OVERLAY", "GameFontHighlightSmall")
    status:SetPoint("LEFT", statusDot, "RIGHT", 7, 0)

    local function SetStatus(text, r, g, b)
        status:SetText(text)
        status:SetTextColor(r, g, b, 0.92)
        statusDot:SetColorTexture(r, g, b, 0.92)
    end

    local selection = ns.EventCatalog.CreateSelection()
    local SyncMonitorControls
    local function SetSelectionStatus()
        local count = selection:GetCount()
        if count > 0 then
            SetStatus(string.format(L.EVENTS_SELECTED_STATUS, count), 0.55, 0.60, 0.65)
        else
            SetStatus(L.EVENT_READY, 0.55, 0.60, 0.65)
        end
    end

    local selectedLabel = W.CreateSectionLabel(page, L.SELECTED_EVENTS)
    selectedLabel:SetPoint("TOPLEFT", 17, -169)

    local selectedCount = page:CreateFontString(nil, "OVERLAY", "GameFontDisableSmall")
    selectedCount:SetPoint("LEFT", selectedLabel, "RIGHT", 9, 0)
    selectedCount:SetTextColor(1, 1, 1, 0.34)

    local selectedPanel = W.CreatePanel(page, colors.editor[1], colors.editor[2], colors.editor[3], 0.78)
    selectedPanel:SetPoint("TOPLEFT", 14, -189)
    selectedPanel:SetPoint("TOPRIGHT", -14, -189)
    selectedPanel:SetHeight(42)
    page.selectedPanel = selectedPanel

    local selectedScroll = W.CreateScrollArea(selectedPanel, 7, 7, 7, 7)
    local selectedContent = CreateFrame("Frame", nil, selectedScroll)
    selectedContent:SetWidth(PAGE_WIDTH - 56)
    selectedContent:SetHeight(1)
    selectedScroll:SetScrollChild(selectedContent)

    local selectedEmpty = selectedPanel:CreateFontString(nil, "OVERLAY", "GameFontDisableSmall")
    selectedEmpty:SetPoint("CENTER")
    selectedEmpty:SetText(L.NO_EVENTS_SELECTED)
    selectedEmpty:SetTextColor(1, 1, 1, 0.32)

    local logLabel = W.CreateSectionLabel(page, L.CAPTURED_EVENTS)
    logLabel:SetPoint("TOPLEFT", selectedPanel, "BOTTOMLEFT", 3, -22)

    local detailLabel = W.CreateSectionLabel(page, L.PAYLOAD)
    detailLabel:SetPoint("TOPLEFT", selectedPanel, "BOTTOMLEFT", EVENT_LIST_WIDTH + 15, -22)

    local logPanel = W.CreatePanel(page, colors.editor[1], colors.editor[2], colors.editor[3], 0.78)
    logPanel:SetPoint("TOPLEFT", selectedPanel, "BOTTOMLEFT", 0, -42)
    logPanel:SetPoint("BOTTOMLEFT", 14, 54)
    logPanel:SetWidth(EVENT_LIST_WIDTH)

    local logScroll = W.CreateScrollArea(logPanel, 8, 8, 7, 8)
    local logContent = CreateFrame("Frame", nil, logScroll)
    logContent:SetWidth(EVENT_LIST_WIDTH - 26)
    logContent:SetHeight(1)
    logScroll:SetScrollChild(logContent)

    local empty = logPanel:CreateFontString(nil, "OVERLAY", "GameFontDisableSmall")
    empty:SetPoint("TOP", 0, -24)
    empty:SetText(L.NO_EVENTS_CAPTURED)
    empty:SetTextColor(1, 1, 1, 0.32)

    local selectedRecord
    local FormatDetails
    local detailPanel = W.CreateTextArea(page, true)
    detailPanel:SetPoint("TOPLEFT", selectedPanel, "BOTTOMLEFT", EVENT_LIST_WIDTH + 12, -42)
    detailPanel:SetPoint("BOTTOMRIGHT", -14, 54)
    detailPanel.editBox:SetWidth(PAGE_WIDTH - EVENT_LIST_WIDTH - 78)
    W.SetReadOnlyText(detailPanel, L.SELECT_EVENT_DETAIL)

    local exportDetail = W.CreateButton(page, 92, L.SAVE_TO_DISK, "secondary")
    exportDetail:SetPoint("BOTTOMRIGHT", -14, 14)
    exportDetail:SetScript("OnClick", function()
        if ns.Safety.IsCombatBlocked() then
            ns.Safety.PrintBlocked()
            return
        end
        local kind, title, content, metadata = BuildEventLogPayload()
        if not kind then
            return
        end
        if ns.Workbench and type(ns.Workbench.SaveToDisk) == "function" then
            ns.Workbench.SaveToDisk(kind, title, content, metadata)
        else
            ObtainExportUI(parent):Save(kind, title, content, metadata)
        end
    end)
    page.exportDetail = exportDetail

    local rows = {}
    local selectedRows = {}
    local searchRows = {}
    local searchResults = {}
    local highlightedSearchIndex = 1
    local refreshQueued
    local dirty

    local searchPanel = W.CreatePanel(page, colors.editor[1], colors.editor[2], colors.editor[3], 0.99)
    searchPanel:SetPoint("TOPLEFT", inputPanel, "BOTTOMLEFT", 0, -4)
    searchPanel:SetPoint("TOPRIGHT", inputPanel, "BOTTOMRIGHT", 0, -4)
    searchPanel:SetFrameLevel(page:GetFrameLevel() + 30)
    searchPanel:Hide()

    -- While monitoring runs, the selection surface is frozen: input disabled
    -- and unfocused, the dropdown hidden and every remove button disabled.
    SyncMonitorControls = function()
        local running = ns.EventMonitor.IsRunning()
        W.SetButtonText(monitorButton, running and L.STOP_MONITORING or L.START_MONITORING)
        W.SetButtonVariant(monitorButton, running and "danger" or "primary")
        W.SetButtonEnabled(monitorButton, running or selection:GetCount() > 0)
        inputPanel.editBox:SetEnabled(not running)
        inputPanel.editBox:SetTextColor(0.94, 0.95, 0.96, running and 0.38 or 1)
        inputHint:SetAlpha(running and 0.22 or 1)
        if running then
            inputPanel.editBox:ClearFocus()
            searchPanel:Hide()
        end
        for index = 1, #selectedRows do
            local removeButton = selectedRows[index].removeButton
            removeButton:SetEnabled(not running)
            removeButton:SetAlpha(running and 0.24 or 1)
        end
    end

    local function FormatCatalogPayload(eventName, signature)
        if eventName == "ALL" then
            return L.ALL_EVENTS
        end
        return signature ~= "" and (L.PAYLOAD_PREFIX .. signature) or L.NO_PAYLOAD
    end

    local function ApplySearchRowStyle(row)
        W.SetListRowState(row, row.resultIndex == highlightedSearchIndex, row.isHovered)
        if row.resultIndex == highlightedSearchIndex then
            row.name:SetTextColor(1, 1, 1, 0.96)
        elseif row.isHovered then
            row.name:SetTextColor(1, 1, 1, 0.9)
        else
            row.name:SetTextColor(1, 1, 1, 0.72)
        end
    end

    local RefreshSelected
    local function AddSearchResult(resultIndex)
        if ns.EventMonitor.IsRunning() then
            return
        end
        local catalogIndex = searchResults[resultIndex]
        if not catalogIndex then
            return
        end

        local succeeded, errorMessage = selection:Add(catalogIndex)
        if succeeded then
            inputPanel.editBox:SetText("")
            inputHint:Show()
            searchPanel:Hide()
            RefreshSelected()
            SetSelectionStatus()
        else
            SetStatus(errorMessage or L.COULD_NOT_SELECT_EVENT, colors.accent[1], colors.accent[2], colors.accent[3])
        end
    end

    local function CreateSearchRow(index)
        local row = W.CreateListRow(searchPanel, SEARCH_ROW_HEIGHT)
        row:SetPoint("TOPLEFT", 3, -3 - ((index - 1) * SEARCH_ROW_HEIGHT))
        row:SetPoint("TOPRIGHT", -3, -3 - ((index - 1) * SEARCH_ROW_HEIGHT))
        row.resultIndex = index
        row.isHovered = false

        local name = row:CreateFontString(nil, "OVERLAY", "GameFontHighlightSmall")
        name:SetPoint("TOPLEFT", 9, -5)
        name:SetPoint("TOPRIGHT", -9, -5)
        name:SetJustifyH("LEFT")
        name:SetWordWrap(false)
        row.name = name

        local payload = row:CreateFontString(nil, "OVERLAY", "GameFontDisableSmall")
        payload:SetPoint("BOTTOMLEFT", 9, 5)
        payload:SetPoint("BOTTOMRIGHT", -9, 5)
        payload:SetJustifyH("LEFT")
        payload:SetWordWrap(false)
        payload:SetTextColor(1, 1, 1, 0.38)
        row.payload = payload

        row:SetScript("OnEnter", function(self)
            searchPanel.isHovered = true
            self.isHovered = true
            highlightedSearchIndex = self.resultIndex
            for rowIndex = 1, #searchRows do
                ApplySearchRowStyle(searchRows[rowIndex])
            end
        end)
        row:SetScript("OnLeave", function(self)
            searchPanel.isHovered = nil
            self.isHovered = nil
            ApplySearchRowStyle(self)
        end)
        row:SetScript("OnClick", function(self)
            AddSearchResult(self.resultIndex)
        end)

        searchRows[index] = row
        return row
    end

    local function RefreshSearch()
        local query = inputPanel.editBox:GetText()
        wipe(searchResults)
        if not ns.EventMonitor.IsRunning() and query ~= "" then
            ns.EventCatalog.Search(query, SEARCH_RESULT_LIMIT, searchResults)
        end
        highlightedSearchIndex = math.min(math.max(highlightedSearchIndex, 1), math.max(#searchResults, 1))

        for index = 1, #searchRows do
            searchRows[index]:Hide()
        end
        for index = 1, #searchResults do
            local row = searchRows[index] or CreateSearchRow(index)
            local eventName, signature = ns.EventCatalog.Get(searchResults[index])
            row.name:SetText(ShownText(eventName))
            row.payload:SetText(FormatCatalogPayload(eventName, signature))
            ApplySearchRowStyle(row)
            row:Show()
        end

        searchPanel:SetHeight(math.max(1, #searchResults * SEARCH_ROW_HEIGHT + 6))
        searchPanel:SetShown(#searchResults > 0 and inputPanel.editBox:HasFocus())
    end

    local function ApplySelectedRowStyle(row)
        W.SetListRowState(row, false, row.isHovered)
    end

    local function CreateSelectedRow(index)
        local row = W.CreateListRow(selectedContent, SELECTED_ROW_HEIGHT)
        row:SetWidth(PAGE_WIDTH - 56)
        row:EnableMouse(true)
        row.isHovered = false

        local name = row:CreateFontString(nil, "OVERLAY", "GameFontHighlightSmall")
        name:SetPoint("LEFT", 9, 0)
        name:SetWidth(330)
        name:SetJustifyH("LEFT")
        name:SetWordWrap(false)
        name:SetTextColor(0.93, 0.94, 0.96, 0.9)
        row.name = name

        local payload = row:CreateFontString(nil, "OVERLAY", "GameFontDisableSmall")
        payload:SetPoint("LEFT", 348, 0)
        payload:SetPoint("RIGHT", -35, 0)
        payload:SetJustifyH("LEFT")
        payload:SetWordWrap(false)
        payload:SetTextColor(1, 1, 1, 0.36)
        row.payload = payload

        local removeButton = CreateRemoveButton(row)
        removeButton:SetPoint("RIGHT", -3, 0)
        removeButton:SetScript("OnClick", function()
            if not ns.EventMonitor.IsRunning() and row.eventName and selection:Remove(row.eventName) then
                RefreshSelected()
                SetSelectionStatus()
            end
        end)
        row.removeButton = removeButton

        row:SetScript("OnEnter", function(self)
            self.isHovered = true
            ApplySelectedRowStyle(self)
        end)
        row:SetScript("OnLeave", function(self)
            self.isHovered = nil
            ApplySelectedRowStyle(self)
        end)

        selectedRows[index] = row
        return row
    end

    RefreshSelected = function()
        for index = 1, #selectedRows do
            selectedRows[index]:Hide()
        end

        local selectionCount = selection:GetCount()
        local scrollOffset = selectedScroll:GetVerticalScroll()
        if Restricted(scrollOffset) then
            scrollOffset = 0
        end
        local firstIndex = math.floor((scrollOffset or 0) / SELECTED_ROW_HEIGHT) + 1
        local lastIndex = math.min(selectionCount, firstIndex + SELECTED_VISIBLE_ROWS - 1)
        local poolIndex = 0
        for selectionIndex = firstIndex, lastIndex do
            poolIndex = poolIndex + 1
            local row = selectedRows[poolIndex] or CreateSelectedRow(poolIndex)
            local eventName, signature = selection:Get(selectionIndex)
            row.eventName = eventName
            row:ClearAllPoints()
            row:SetPoint("TOPLEFT", 0, -((selectionIndex - 1) * SELECTED_ROW_HEIGHT))
            row.name:SetText(ShownText(eventName))
            row.payload:SetText(FormatCatalogPayload(eventName, signature))
            ApplySelectedRowStyle(row)
            row:Show()
        end

        selectedCount:SetText(string.format(L.SELECTED_COUNT, selectionCount))
        selectedPanel:SetHeight(math.max(42,
            math.min(selectionCount, SELECTED_VISIBLE_ROWS) * SELECTED_ROW_HEIGHT + 14))
        selectedContent:SetHeight(math.max(1, selectionCount * SELECTED_ROW_HEIGHT))
        selectedScroll:UpdateScrollChildRect()
        selectedEmpty:SetShown(selectionCount == 0)
        SyncMonitorControls()
    end

    selectedScroll.onVerticalScrollChanged = RefreshSelected

    inputPanel.editBox:SetScript("OnTextChanged", function(self)
        inputHint:SetShown(self:GetText() == "")
        highlightedSearchIndex = 1
        RefreshSearch()
    end)
    inputPanel.editBox:SetScript("OnEditFocusGained", function()
        W.SetBorderColor(inputPanel, true, 0.75)
        RefreshSearch()
    end)
    inputPanel.editBox:SetScript("OnEditFocusLost", function()
        W.SetBorderColor(inputPanel, false)
        if not searchPanel.isHovered then
            searchPanel:Hide()
        end
    end)
    inputPanel.editBox:SetScript("OnArrowPressed", function(_, key)
        if not searchPanel:IsShown() or #searchResults == 0 then
            return
        end
        if key == "UP" then
            highlightedSearchIndex = highlightedSearchIndex > 1 and highlightedSearchIndex - 1 or #searchResults
        elseif key == "DOWN" then
            highlightedSearchIndex = highlightedSearchIndex < #searchResults and highlightedSearchIndex + 1 or 1
        end
        for index = 1, #searchRows do
            ApplySearchRowStyle(searchRows[index])
        end
    end)
    inputPanel.editBox:SetScript("OnEnterPressed", function()
        AddSearchResult(highlightedSearchIndex)
    end)
    inputPanel.editBox:SetScript("OnEscapePressed", function(self)
        if searchPanel:IsShown() then
            searchPanel:Hide()
        else
            self:ClearFocus()
        end
    end)

    FormatDetails = function(record)
        if not record then
            return L.SELECT_EVENT_DETAIL
        end

        local lines = {
            string.format(L.EVENT_DETAIL, ShownText(record.event)),
            ShownNumber(record.elapsed, L.ELAPSED_DETAIL, L.UNKNOWN_TIME),
            "",
        }
        if #record.arguments == 0 then
            lines[#lines + 1] = L.EVENT_HAS_NO_PAYLOAD
        else
            for index = 1, #record.arguments do
                lines[#lines + 1] = "[" .. index .. "] = " .. ShownText(record.arguments[index])
            end
        end
        return table.concat(lines, "\n")
    end

    local function ApplyEventRowStyle(row)
        W.SetListRowState(row, row.record == selectedRecord, row.isHovered)
    end

    local function CreateEventRow(index)
        local row = W.CreateListRow(logContent, EVENT_ROW_HEIGHT)
        row:SetWidth(EVENT_LIST_WIDTH - 26)
        row.isHovered = false

        local timeLabel = row:CreateFontString(nil, "OVERLAY", "GameFontDisableSmall")
        timeLabel:SetPoint("LEFT", 8, 0)
        timeLabel:SetWidth(62)
        timeLabel:SetJustifyH("LEFT")
        timeLabel:SetTextColor(1, 1, 1, 0.38)
        row.timeLabel = timeLabel

        local eventLabel = row:CreateFontString(nil, "OVERLAY", "GameFontHighlightSmall")
        eventLabel:SetPoint("LEFT", 72, 7)
        eventLabel:SetPoint("RIGHT", -8, 7)
        eventLabel:SetJustifyH("LEFT")
        eventLabel:SetWordWrap(false)
        eventLabel:SetTextColor(0.93, 0.94, 0.96, 0.9)
        row.eventLabel = eventLabel

        local summary = row:CreateFontString(nil, "OVERLAY", "GameFontDisableSmall")
        summary:SetPoint("LEFT", 72, -8)
        summary:SetPoint("RIGHT", -8, -8)
        summary:SetJustifyH("LEFT")
        summary:SetWordWrap(false)
        summary:SetTextColor(1, 1, 1, 0.38)
        row.summary = summary

        row:SetScript("OnEnter", function(self)
            self.isHovered = true
            ApplyEventRowStyle(self)
        end)
        row:SetScript("OnLeave", function(self)
            self.isHovered = nil
            ApplyEventRowStyle(self)
        end)
        row:SetScript("OnClick", function(self)
            selectedRecord = self.record
            W.SetReadOnlyText(detailPanel, FormatDetails(selectedRecord))
            page:Refresh()
        end)

        rows[index] = row
        return row
    end

    function page:Refresh()
        dirty = nil
        local recordCount = ns.EventMonitor.GetCount()
        W.SetButtonEnabled(clearButton, recordCount > 0)
        W.SetButtonEnabled(exportDetail, recordCount > 0)
        for index = 1, #rows do
            rows[index]:Hide()
        end

        local scrollOffset = logScroll:GetVerticalScroll()
        if Restricted(scrollOffset) then
            scrollOffset = 0
        end
        local firstIndex = math.floor((scrollOffset or 0) / EVENT_ROW_HEIGHT) + 1
        local lastIndex = math.min(recordCount, firstIndex + EVENT_VISIBLE_ROWS - 1)
        local poolIndex = 0
        for recordIndex = firstIndex, lastIndex do
            poolIndex = poolIndex + 1
            local record = ns.EventMonitor.GetRecord(recordIndex)
            local row = rows[poolIndex] or CreateEventRow(poolIndex)
            row.record = record
            row:ClearAllPoints()
            row:SetPoint("TOPLEFT", 0, -((recordIndex - 1) * EVENT_ROW_HEIGHT))
            row.timeLabel:SetText(ShownNumber(record.elapsed, "+%.2fs", "<secret>"))
            row.eventLabel:SetText(ShownText(record.event))
            row.summary:SetText(record.summary ~= "" and ShownText(record.summary) or L.NO_PAYLOAD)
            ApplyEventRowStyle(row)
            row:Show()
        end

        logContent:SetHeight(math.max(1, recordCount * EVENT_ROW_HEIGHT))
        logScroll:UpdateScrollChildRect()
        empty:SetShown(recordCount == 0)
        if selectedRecord then
            W.SetReadOnlyText(detailPanel, FormatDetails(selectedRecord))
        end
    end

    logScroll.onVerticalScrollChanged = function()
        page:Refresh()
    end

    local function FlushRefresh()
        refreshQueued = nil
        if page:IsShown() then
            page:Refresh()
        else
            dirty = true
        end
    end

    local function QueueRefresh()
        if refreshQueued then
            return
        end
        refreshQueued = true
        C_Timer.After(0, FlushRefresh)
    end
    page.QueueRefresh = QueueRefresh

    monitorButton:SetScript("OnClick", function()
        if ns.Safety.IsCombatBlocked() then
            ns.Safety.PrintBlocked()
            return
        end
        if ns.EventMonitor.IsRunning() then
            ns.EventMonitor.Stop()
            SetStatus(L.STOPPED, 0.55, 0.60, 0.65)
            SyncMonitorControls()
            return
        end

        local succeeded, errorMessage = ns.EventMonitor.Start(selection:GetNames(), QueueRefresh)
        if succeeded then
            local listeningText = ns.EventMonitor.IsMonitoringAllEvents()
                and L.MONITORING_ALL_EVENTS
                or string.format(L.MONITORING_EVENTS, ns.EventMonitor.GetActiveEventCount())
            SetStatus(listeningText, 0.42, 0.76, 0.43)
            inputPanel.editBox:ClearFocus()
            searchPanel:Hide()
        else
            SetStatus(errorMessage or L.COULD_NOT_START, colors.accent[1], colors.accent[2], colors.accent[3])
        end
        SyncMonitorControls()
    end)

    clearButton:SetScript("OnClick", function()
        if ns.Safety.IsCombatBlocked() then
            ns.Safety.PrintBlocked()
            return
        end
        ns.EventMonitor.Clear()
        selectedRecord = nil
        W.SetReadOnlyText(detailPanel, L.SELECT_EVENT_DETAIL)
        page:Refresh()
    end)

    function page:FocusInput()
        inputPanel.editBox:SetFocus()
    end

    function page:StopMonitor()
        ns.EventMonitor.Stop()
        SetSelectionStatus()
        SyncMonitorControls()
    end

    function page:RefreshIfDirty()
        if dirty then
            self:Refresh()
        end
    end

    SetSelectionStatus()
    RefreshSelected()
    page:Refresh()
    return page
end

-- Workbench registration: zero cost before the first /dev (build runs lazily);
-- shutdown stops the monitor so a hidden window or combat entry never leaves
-- events registered.
local pageInstance
local definition = {
    key = "events",
    titleKey = "TAB_EVENTS",
    build = function(parent)
        pageInstance = ns.CreateEventsPage(parent)
        return pageInstance
    end,
    activate = function()
        if pageInstance then
            pageInstance:RefreshIfDirty()
        end
    end,
    suspend = function()
    end,
    shutdown = function()
        if pageInstance then
            pageInstance:StopMonitor()
        elseif ns.EventMonitor then
            ns.EventMonitor.Stop()
        end
    end,
}

if ns.Workbench and type(ns.Workbench.RegisterPage) == "function" then
    ns.Workbench.RegisterPage(definition)
    if type(ns.Workbench.RegisterShutdown) == "function" then
        ns.Workbench.RegisterShutdown(definition.shutdown)
    end
    if type(ns.Workbench.RegisterExportHook) == "function" then
        ns.Workbench.RegisterExportHook({ key = "events", GetPayload = BuildEventLogPayload })
    end
end
ns.EventsPageDefinition = definition
