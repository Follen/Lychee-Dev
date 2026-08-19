local ADDON_NAME, ns = ...
local L = ns.L

local ROW_HEIGHT = 58
local LIST_WIDTH = 354

local KIND_LABELS = {
    run_result = L.EXPORT_KIND_RUN_RESULT,
    object_snapshot = L.EXPORT_KIND_OBJECT_SNAPSHOT,
    object_node = L.EXPORT_KIND_OBJECT_NODE,
    event_log = L.EXPORT_KIND_EVENT_LOG,
    function_trace = L.EXPORT_KIND_FUNCTION_TRACE,
    error_log = L.EXPORT_KIND_ERROR_LOG,
    performance_snapshot = L.EXPORT_KIND_PERFORMANCE,
    performance_health = L.EXPORT_KIND_PERFORMANCE,
    performance_capture = L.EXPORT_KIND_PERFORMANCE,
    performance_benchmark = L.EXPORT_KIND_PERFORMANCE,
    performance_storage = L.EXPORT_KIND_PERFORMANCE,
}

local function FormatBytes(bytes)
    bytes = math.max(0, tonumber(bytes) or 0)
    if bytes >= 1024 * 1024 then
        return string.format("%.2f MB", bytes / 1024 / 1024)
    elseif bytes >= 1024 then
        return string.format("%.1f KB", bytes / 1024)
    end
    return string.format("%d B", bytes)
end

local function FormatTime(timestamp)
    timestamp = tonumber(timestamp) or 0
    if timestamp <= 0 then
        return L.UNKNOWN
    end
    return date("%Y-%m-%d %H:%M:%S", timestamp)
end

local function GetKindLabel(kind)
    return KIND_LABELS[kind] or L.EXPORT_KIND_UNKNOWN
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

function ns.CreateExportRecordsPage(parent, ui)
    local page = CreateFrame("Frame", nil, parent)
    page:SetAllPoints(parent)

    local selectedTicket
    local rows = {}
    local deleteConfirmed = false
    local clearConfirmed = false

    local heading = ui.CreateSectionLabel(page, L.EXPORT_RECORDS)
    heading:SetPoint("TOPLEFT", 17, -84)

    local countText = page:CreateFontString(nil, "OVERLAY", "GameFontDisableSmall")
    countText:SetPoint("LEFT", heading, "RIGHT", 12, 0)
    countText:SetTextColor(1, 1, 1, 0.36)

    local pendingText = page:CreateFontString(nil, "OVERLAY", "GameFontDisableSmall")
    pendingText:SetPoint("LEFT", countText, "RIGHT", 12, 0)
    pendingText:SetTextColor(ui.accentR, ui.accentG, ui.accentB, 0.92)

    local clearButton = ui.CreateButton(page, 120, L.CLEAR_EXPORT_CACHE, false)
    clearButton:SetPoint("TOPRIGHT", -14, -77)

    local usageText = page:CreateFontString(nil, "OVERLAY", "GameFontDisableSmall")
    usageText:SetPoint("RIGHT", clearButton, "LEFT", -12, 0)
    usageText:SetTextColor(1, 1, 1, 0.34)

    local listPanel = ui.CreatePanel(page, ui.editorR, ui.editorG, ui.editorB, 0.78)
    listPanel:SetPoint("TOPLEFT", 14, -108)
    listPanel:SetPoint("BOTTOMLEFT", 14, 54)
    listPanel:SetWidth(LIST_WIDTH)

    local listScroll = ui.CreateScrollArea(listPanel, 8, 8, 7, 8)
    local listContent = CreateFrame("Frame", nil, listScroll)
    listContent:SetWidth(LIST_WIDTH - 26)
    listContent:SetHeight(1)
    listScroll:SetScrollChild(listContent)

    local emptyTitle = listPanel:CreateFontString(nil, "OVERLAY", "GameFontNormal")
    emptyTitle:SetPoint("CENTER", listPanel, "CENTER", 0, 14)
    emptyTitle:SetText(L.EXPORT_RECORDS_EMPTY)
    emptyTitle:SetTextColor(1, 1, 1, 0.58)

    local emptyHelp = listPanel:CreateFontString(nil, "OVERLAY", "GameFontDisableSmall")
    emptyHelp:SetPoint("TOP", emptyTitle, "BOTTOM", 0, -9)
    emptyHelp:SetWidth(270)
    emptyHelp:SetJustifyH("CENTER")
    emptyHelp:SetText(L.EXPORT_RECORDS_EMPTY_HELP)
    emptyHelp:SetTextColor(1, 1, 1, 0.32)

    local detailPanel = CreateFrame("Frame", nil, page)
    detailPanel:SetPoint("TOPLEFT", listPanel, "TOPRIGHT", 12, 0)
    detailPanel:SetPoint("BOTTOMRIGHT", -14, 54)

    local detailHeading = detailPanel:CreateFontString(nil, "OVERLAY", "GameFontNormal")
    detailHeading:SetPoint("TOPLEFT", 4, -4)
    detailHeading:SetText(L.EXPORT_RECORD_DETAIL)

    local detailStatus = detailPanel:CreateFontString(nil, "OVERLAY", "GameFontHighlightSmall")
    detailStatus:SetPoint("TOPRIGHT", -4, -4)

    local nameValue = CreateField(detailPanel, L.EXPORT_RECORD_NAME, 4, -48, 282)
    local kindValue = CreateField(detailPanel, L.EXPORT_RECORD_KIND, 314, -48, 272)
    local timeValue = CreateField(detailPanel, L.EXPORT_RECORD_TIME, 4, -106, 282)
    local sizeValue = CreateField(detailPanel, L.EXPORT_RECORD_SIZE, 314, -106, 272)
    local clientValue = CreateField(detailPanel, L.EXPORT_RECORD_CLIENT, 4, -164, 282)
    local pathValue = CreateField(detailPanel, L.EXPORT_RECORD_PATH, 314, -164, 272)

    local ticketLabel = detailPanel:CreateFontString(nil, "OVERLAY", "GameFontDisableSmall")
    ticketLabel:SetPoint("TOPLEFT", 4, -226)
    ticketLabel:SetText(L.EXPORT_TICKET)
    ticketLabel:SetTextColor(1, 1, 1, 0.34)

    local ticketPanel = ui.CreatePanel(detailPanel, ui.editorR, ui.editorG, ui.editorB, 1)
    ticketPanel:SetPoint("TOPLEFT", 4, -247)
    ticketPanel:SetPoint("TOPRIGHT", -4, -247)
    ticketPanel:SetHeight(40)

    local selectTicket = ui.CreateButton(ticketPanel, 118, L.SELECT_TICKET, false)
    selectTicket:SetPoint("RIGHT", -5, 0)

    local ticketBox = CreateFrame("EditBox", nil, ticketPanel)
    ticketBox:SetPoint("TOPLEFT", 10, -1)
    ticketBox:SetPoint("BOTTOMRIGHT", selectTicket, "BOTTOMLEFT", -10, 1)
    ticketBox:SetAutoFocus(false)
    ticketBox:SetFontObject(ChatFontNormal)
    ticketBox:SetTextColor(0.94, 0.95, 0.96)
    ticketBox:SetScript("OnEscapePressed", function(self) self:ClearFocus() end)

    local ticketHint = detailPanel:CreateFontString(nil, "OVERLAY", "GameFontDisableSmall")
    ticketHint:SetPoint("TOPLEFT", ticketPanel, "BOTTOMLEFT", 1, -10)
    ticketHint:SetPoint("RIGHT", -4, 0)
    ticketHint:SetJustifyH("LEFT")
    ticketHint:SetText(L.EXPORT_TICKET_HELP)
    ticketHint:SetTextColor(1, 1, 1, 0.36)

    local deleteButton = ui.CreateButton(detailPanel, 128, L.EXPORT_RECORD_DELETE, false)
    deleteButton:SetPoint("BOTTOMLEFT", 4, 4)

    local reloadButton = ui.CreateButton(detailPanel, 110, L.RELOAD_NOW, true)
    reloadButton:SetPoint("BOTTOMRIGHT", -4, 4)

    local function ApplyRowState(row)
        local selected = row.ticket == selectedTicket
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
        local entry = selectedTicket and ns.GetExport(selectedTicket) or nil
        local hasEntry = entry ~= nil
        detailHeading:SetText(hasEntry and (entry.title ~= "" and entry.title or L.EXPORT_RECORD_DETAIL)
            or L.EXPORT_RECORD_DETAIL)
        local pending = hasEntry and ns.IsExportPending(selectedTicket)
        detailStatus:SetText(hasEntry and (pending and L.EXPORT_STATUS_PENDING or L.EXPORT_STATUS_SAVED) or "")
        if pending then
            detailStatus:SetTextColor(ui.accentR, ui.accentG, ui.accentB, 0.94)
        else
            detailStatus:SetTextColor(1, 1, 1, 0.36)
        end
        nameValue:SetText(hasEntry and (entry.title ~= "" and entry.title or L.UNKNOWN) or "")
        kindValue:SetText(hasEntry and GetKindLabel(entry.kind) or "")
        timeValue:SetText(hasEntry and FormatTime(entry.createdAt) or "")
        sizeValue:SetText(hasEntry and FormatBytes(entry.byteCount) or "")
        if hasEntry and entry.client then
            local version = entry.client.version or L.UNKNOWN
            local build = entry.client.build and (" (" .. tostring(entry.client.build) .. ")") or ""
            clientValue:SetText(version .. build)
        else
            clientValue:SetText("")
        end
        local path = hasEntry and entry.metadata and entry.metadata.path
        pathValue:SetText(path and tostring(path) or L.NOT_AVAILABLE)
        ticketBox.savedText = hasEntry and selectedTicket or ""
        ticketBox.updatingText = true
        ticketBox:SetText(ticketBox.savedText)
        ticketBox.updatingText = nil
        ticketBox:SetCursorPosition(0)
        ticketHint:SetText(L.EXPORT_TICKET_HELP)
        ui.SetButtonEnabled(selectTicket, hasEntry)
        ui.SetButtonEnabled(deleteButton, hasEntry)
        deleteConfirmed = false
        ui.SetButtonText(deleteButton, L.EXPORT_RECORD_DELETE)
        ui.SetButtonVariant(deleteButton, "secondary")
    end

    local function SelectRecord(ticket)
        selectedTicket = ns.GetExport(ticket) and ticket or nil
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
        title:SetPoint("TOPRIGHT", -76, -10)
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
            SelectRecord(self.ticket)
        end)
        row:SetScript("OnMouseWheel", listScroll.onMouseWheel)
        rows[index] = row
        return row
    end

    local function Refresh()
        local exports = ns.GetExports()
        local order = exports and exports.order or {}
        local count, bytes, maximum = ns.GetExportStats()
        local pendingCount = ns.GetPendingExportCount()
        countText:SetText(string.format(L.EXPORT_RECORD_COUNT, count))
        pendingText:SetText(pendingCount > 0 and string.format(L.EXPORT_PENDING_COUNT, pendingCount) or "")
        usageText:SetText(string.format(L.EXPORT_STORAGE_USAGE,
            bytes / 1024, maximum / 1024 / 1024))
        emptyTitle:SetShown(count == 0)
        emptyHelp:SetShown(count == 0)
        ui.SetButtonEnabled(clearButton, count > 0)

        if clearConfirmed then
            clearConfirmed = false
            ui.SetButtonText(clearButton, L.CLEAR_EXPORT_CACHE)
            ui.SetButtonVariant(clearButton, "secondary")
        end

        for index = 1, #order do
            local ticket = order[index]
            local entry = ns.GetExport(ticket)
            local row = AcquireRow(index)
            row.ticket = ticket
            row.title:SetText(entry.title ~= "" and entry.title or ticket)
            local pending = ns.IsExportPending(ticket)
            row.status:SetText(pending and L.EXPORT_STATUS_PENDING or L.EXPORT_STATUS_SAVED)
            if pending then
                row.status:SetTextColor(ui.accentR, ui.accentG, ui.accentB, 0.92)
            else
                row.status:SetTextColor(1, 1, 1, 0.30)
            end
            row.meta:SetText(FormatTime(entry.createdAt) .. " | " .. GetKindLabel(entry.kind)
                .. " | " .. FormatBytes(entry.byteCount))
            row:Show()
        end
        for index = #order + 1, #rows do
            rows[index]:Hide()
        end

        listContent:SetHeight(math.max(1, #order * ROW_HEIGHT))
        listScroll:UpdateScrollChildRect()
        ui.SetButtonEnabled(reloadButton, pendingCount > 0)
        if not selectedTicket or not ns.GetExport(selectedTicket) then
            selectedTicket = order[1]
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
        if selectedTicket then
            ui.SelectAllText(ticketBox)
            ticketHint:SetText(L.EXPORT_TICKET_SELECTED)
        end
    end)
    selectTicket:SetScript("OnClick", function()
        ui.SelectAllText(ticketBox)
        ticketHint:SetText(L.EXPORT_TICKET_SELECTED)
    end)
    reloadButton:SetScript("OnClick", function()
        if ns.IsCombatBlocked() then
            ns.PrintCombatBlocked()
            return
        end
        ReloadUI()
    end)
    deleteButton:SetScript("OnClick", function()
        if not selectedTicket then
            return
        end
        if not deleteConfirmed then
            deleteConfirmed = true
            ui.SetButtonText(deleteButton, L.EXPORT_RECORD_DELETE_CONFIRM)
            ui.SetButtonVariant(deleteButton, "danger")
            return
        end
        ns.DeleteExport(selectedTicket)
        selectedTicket = nil
        Refresh()
    end)
    clearButton:SetScript("OnClick", function()
        if not clearConfirmed then
            clearConfirmed = true
            ui.SetButtonText(clearButton, L.CONFIRM_CLEAR_CACHE)
            ui.SetButtonVariant(clearButton, "danger")
            return
        end
        ns.ClearExports()
        selectedTicket = nil
        Refresh()
    end)

    page.listScroll = listScroll
    page.rows = rows
    page.ticketBox = ticketBox
    page.ticketHint = ticketHint
    page.pendingText = pendingText
    page.detailStatus = detailStatus
    page.selectTicket = selectTicket
    page.deleteButton = deleteButton
    page.clearButton = clearButton
    page.reloadButton = reloadButton
    page.Refresh = Refresh
    page.SelectRecord = SelectRecord
    function page:Activate()
        Refresh()
    end
    Refresh()
    return page
end
