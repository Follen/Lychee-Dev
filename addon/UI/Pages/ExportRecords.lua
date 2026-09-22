local ADDON_NAME, ns = ...
local L = ns.L

-- Saved Records workbench page (key "exports"). Ports
-- add-on/UI/Pages/ExportRecords.lua: a newest-first record list with
-- pending/saved badges, storage stats, a detail pane with a rewrite-guarded
-- pre-selected ticket box (copy = select + Ctrl+C) and delete / reload /
-- cache-clear actions over the shared export store. Constructed lazily on the
-- first workbench open.

local Workbench = ns.Workbench
local Widgets = ns.Widgets
local Layout = Workbench.Layout

local ROW_HEIGHT = 58
local LIST_WIDTH = 354

local KIND_LABELS = {
    run_result = L.EXPORT_KIND_RUN_RESULT,
    object_snapshot = L.EXPORT_KIND_OBJECT_SNAPSHOT,
    object_node = L.EXPORT_KIND_OBJECT_NODE,
    event_log = L.EXPORT_KIND_EVENT_LOG,
    function_trace = L.EXPORT_KIND_FUNCTION_TRACE,
    error_log = L.EXPORT_KIND_ERROR_LOG,
    automation_result = L.EXPORT_KIND_AUTOMATION_RESULT,
    -- Historical evidence remains readable after the tool that produced it is
    -- gone; unknown legacy kinds fall back to the generic label below.
    performance_snapshot = L.EXPORT_KIND_PERFORMANCE,
    performance_health = L.EXPORT_KIND_PERFORMANCE,
    performance_capture = L.EXPORT_KIND_PERFORMANCE,
    performance_benchmark = L.EXPORT_KIND_PERFORMANCE,
    performance_storage = L.EXPORT_KIND_PERFORMANCE,
}

local function IsSecret(value)
    return issecretvalue and issecretvalue(value)
end

-- Displayed values coming from stored records fail open to a visible marker
-- instead of throwing on secret data.
local function SafeText(value, fallback)
    if value == nil or IsSecret(value) then
        return fallback
    end
    return tostring(value)
end

-- Combat refusal at every action entry.
local function RefuseInCombat()
    if ns.Safety.IsCombatBlocked() then
        ns.Safety.PrintBlocked()
        return true
    end
    return false
end

local function FormatBytes(bytes)
    if IsSecret(bytes) then
        return L.UNKNOWN
    end
    bytes = math.max(0, tonumber(bytes) or 0)
    if bytes >= 1024 * 1024 then
        return string.format("%.2f MB", bytes / 1024 / 1024)
    elseif bytes >= 1024 then
        return string.format("%.1f KB", bytes / 1024)
    end
    return string.format("%d B", bytes)
end

local function FormatTime(timestamp)
    if IsSecret(timestamp) then
        return L.UNKNOWN
    end
    timestamp = tonumber(timestamp) or 0
    if timestamp <= 0 then
        return L.UNKNOWN
    end
    return date("%Y-%m-%d %H:%M:%S", timestamp)
end

local function GetKindLabel(kind)
    if IsSecret(kind) then
        return L.EXPORT_KIND_UNKNOWN
    end
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

local function BuildExportRecordsPage(parent)
    local colors = Widgets.Colors
    local page = CreateFrame("Frame", nil, parent)
    page:SetAllPoints(parent)

    local selectedTicket
    local rows = {}
    -- Forward declarations: the actions, refreshers and row factory reference
    -- each other through closures.
    local ConfirmDelete
    local ConfirmClear
    local ApplyRowState
    local RefreshDetail
    local Refresh
    local SelectRecord
    local AcquireRow

    local heading = Widgets.CreateSectionLabel(page, L.EXPORT_RECORDS)
    heading:SetPoint("TOPLEFT", Layout.PAGE_LEFT, Layout.HEADING_TOP)

    local countText = page:CreateFontString(nil, "OVERLAY", "GameFontDisableSmall")
    countText:SetPoint("LEFT", heading, "RIGHT", 12, 0)
    countText:SetTextColor(1, 1, 1, 0.36)

    local pendingText = page:CreateFontString(nil, "OVERLAY", "GameFontDisableSmall")
    pendingText:SetPoint("LEFT", countText, "RIGHT", 12, 0)
    pendingText:SetTextColor(colors.accent[1], colors.accent[2], colors.accent[3], 0.92)

    local usageText = page:CreateFontString(nil, "OVERLAY", "GameFontDisableSmall")
    usageText:SetPoint("TOPRIGHT", Layout.CONTENT_RIGHT - 132, Layout.HEADING_TOP + 3)
    usageText:SetTextColor(1, 1, 1, 0.34)

    local listPanel = Widgets.CreatePanel(page, colors.editor[1], colors.editor[2], colors.editor[3], 0.78)
    listPanel:SetPoint("TOPLEFT", Layout.RAIL_LEFT, Layout.CONTENT_TOP - 4)
    listPanel:SetPoint("BOTTOMLEFT", Layout.RAIL_LEFT, Layout.CONTENT_BOTTOM)
    listPanel:SetWidth(LIST_WIDTH)

    local listScroll = Widgets.CreateScrollArea(listPanel, 8, 8, 7, 8)
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
    detailPanel:SetPoint("BOTTOMRIGHT", Layout.CONTENT_RIGHT, Layout.CONTENT_BOTTOM)

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

    local ticketPanel = Widgets.CreatePanel(detailPanel, colors.editor[1], colors.editor[2], colors.editor[3], 1)
    ticketPanel:SetPoint("TOPLEFT", 4, -247)
    ticketPanel:SetPoint("TOPRIGHT", -4, -247)
    ticketPanel:SetHeight(40)

    local ticketBox = CreateFrame("EditBox", nil, ticketPanel)
    ticketBox:SetPoint("TOPLEFT", 10, -1)
    ticketBox:SetPoint("BOTTOMRIGHT", -10, 1)
    ticketBox:SetAutoFocus(false)
    ticketBox:SetFontObject(ChatFontNormal)
    ticketBox:SetTextColor(0.94, 0.95, 0.96)
    ticketBox.savedText = ""
    ticketBox.updatingText = false
    ticketBox:SetScript("OnEscapePressed", function(self)
        self:ClearFocus()
    end)

    local ticketHint = detailPanel:CreateFontString(nil, "OVERLAY", "GameFontDisableSmall")
    ticketHint:SetPoint("TOPLEFT", ticketPanel, "BOTTOMLEFT", 1, -10)
    ticketHint:SetPoint("RIGHT", -4, 0)
    ticketHint:SetJustifyH("LEFT")
    ticketHint:SetText(L.EXPORT_TICKET_HELP)
    ticketHint:SetTextColor(1, 1, 1, 0.36)

    ConfirmDelete = function()
        if RefuseInCombat() then
            return
        end
        if not selectedTicket or IsSecret(selectedTicket) then
            return
        end
        ns.Stores.Exports.Delete(selectedTicket)
        selectedTicket = nil
        Refresh()
    end

    ConfirmClear = function()
        if RefuseInCombat() then
            return
        end
        ns.Stores.Exports.Clear()
        selectedTicket = nil
        Refresh()
    end

    -- Two-click confirmation widgets: the first click arms (confirm label +
    -- danger variant), the second runs the destructive action.
    local deleteButton = Widgets.CreateConfirmButton(detailPanel, 128,
        L.EXPORT_RECORD_DELETE, L.EXPORT_RECORD_DELETE_CONFIRM, ConfirmDelete, "secondary")
    deleteButton:SetPoint("BOTTOMLEFT", 4, 4)

    local clearButton = Widgets.CreateConfirmButton(page, 120,
        L.CLEAR_EXPORT_CACHE, L.CONFIRM_CLEAR_CACHE, ConfirmClear, "secondary")
    clearButton:SetPoint("TOPRIGHT", Layout.CONTENT_RIGHT, Layout.HEADING_TOP + 7)

    local reloadButton = Widgets.CreateButton(detailPanel, 110, L.RELOAD_NOW, "primary")
    reloadButton:SetPoint("BOTTOMRIGHT", -4, 4)

    ApplyRowState = function(row)
        local selected = not IsSecret(row.ticket) and row.ticket == selectedTicket
        Widgets.SetListRowState(row, selected, row.hovered)
        if selected then
            row.title:SetTextColor(1, 1, 1, 0.96)
        elseif row.hovered then
            row.title:SetTextColor(1, 1, 1, 0.90)
        else
            row.title:SetTextColor(1, 1, 1, 0.76)
        end
    end

    Refresh = function()
        local order = ns.Stores.Exports.GetOrder() or {}
        local count, bytes, maximum = ns.Stores.Exports.GetStats()
        local pendingCount = ns.Stores.Exports.GetPendingCount()
        countText:SetText(string.format(L.EXPORT_RECORD_COUNT, count))
        pendingText:SetText(pendingCount > 0 and string.format(L.EXPORT_PENDING_COUNT, pendingCount) or "")
        usageText:SetText(string.format(L.EXPORT_STORAGE_USAGE,
            bytes / 1024, maximum / 1024 / 1024))
        emptyTitle:SetShown(count == 0)
        emptyHelp:SetShown(count == 0)
        Widgets.SetButtonEnabled(clearButton, count > 0)
        clearButton:ResetConfirm()

        for index = 1, #order do
            local ticket = order[index]
            if IsSecret(ticket) then
                ticket = nil
            end
            local entry = ticket and ns.Stores.Exports.Get(ticket) or nil
            local source = entry and entry.source or nil
            local payload = entry and entry.payload or nil
            local row = rows[index] or AcquireRow(index)
            row.ticket = ticket
            local title = source and SafeText(source.title, "") or ""
            row.title:SetText(title ~= "" and title or SafeText(ticket, L.UNKNOWN))
            local pending = ticket ~= nil and ns.Stores.Exports.IsPending(ticket)
            row.status:SetText(pending and L.EXPORT_STATUS_PENDING or L.EXPORT_STATUS_SAVED)
            if pending then
                row.status:SetTextColor(colors.accent[1], colors.accent[2], colors.accent[3], 0.92)
            else
                row.status:SetTextColor(1, 1, 1, 0.30)
            end
            row.meta:SetText(FormatTime(entry and entry.createdAt) .. " | "
                .. GetKindLabel(source and source.kind)
                .. " | " .. FormatBytes(payload and payload.byteCount))
            row:Show()
        end
        for index = #order + 1, #rows do
            rows[index]:Hide()
        end

        listContent:SetHeight(math.max(1, #order * ROW_HEIGHT))
        listScroll:UpdateScrollChildRect()
        Widgets.SetButtonEnabled(reloadButton, pendingCount > 0)
        local newest = order[1]
        if not selectedTicket or not ns.Stores.Exports.Get(selectedTicket) then
            selectedTicket = not IsSecret(newest) and newest or nil
        end
        for index = 1, #rows do
            ApplyRowState(rows[index])
        end
        RefreshDetail()
    end

    RefreshDetail = function()
        local entry = selectedTicket and ns.Stores.Exports.Get(selectedTicket) or nil
        local hasEntry = entry ~= nil
        local source = hasEntry and entry.source or nil
        local payload = hasEntry and entry.payload or nil
        local environment = hasEntry and entry.environment or nil
        local title = SafeText(source and source.title, "")
        local kind = source and source.kind or "unknown"
        detailHeading:SetText(hasEntry and (title ~= "" and title or L.EXPORT_RECORD_DETAIL)
            or L.EXPORT_RECORD_DETAIL)
        local pending = hasEntry and ns.Stores.Exports.IsPending(selectedTicket)
        detailStatus:SetText(hasEntry and (pending and L.EXPORT_STATUS_PENDING or L.EXPORT_STATUS_SAVED) or "")
        if pending then
            detailStatus:SetTextColor(colors.accent[1], colors.accent[2], colors.accent[3], 0.94)
        else
            detailStatus:SetTextColor(1, 1, 1, 0.36)
        end
        nameValue:SetText(hasEntry and (title ~= "" and title or L.UNKNOWN) or "")
        kindValue:SetText(hasEntry and GetKindLabel(kind) or "")
        timeValue:SetText(hasEntry and FormatTime(entry.createdAt) or "")
        sizeValue:SetText(hasEntry and FormatBytes(payload and payload.byteCount) or "")
        if hasEntry and environment then
            local version = SafeText(environment.version, L.UNKNOWN)
            local build = environment.build and (" (" .. SafeText(environment.build, L.UNKNOWN) .. ")") or ""
            clientValue:SetText(version .. build)
        else
            clientValue:SetText("")
        end
        local path = hasEntry and source and source.path
        pathValue:SetText(path and not IsSecret(path) and tostring(path) or L.NOT_AVAILABLE)
        ticketBox.savedText = hasEntry and SafeText(selectedTicket, L.TREE_SECRET) or ""
        ticketBox.updatingText = true
        ticketBox:SetText(ticketBox.savedText)
        ticketBox.updatingText = false
        ticketHint:SetText(L.EXPORT_TICKET_HELP)
        if hasEntry then
            Widgets.SelectAllText(ticketBox)
        else
            ticketBox:ClearFocus()
        end
        Widgets.SetButtonEnabled(deleteButton, hasEntry)
        deleteButton:ResetConfirm()
    end

    SelectRecord = function(ticket)
        if IsSecret(ticket) then
            return
        end
        selectedTicket = ns.Stores.Exports.Get(ticket) and ticket or nil
        for index = 1, #rows do
            ApplyRowState(rows[index])
        end
        RefreshDetail()
    end

    AcquireRow = function(index)
        local row = Widgets.CreateListRow(listContent, ROW_HEIGHT)
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

    ticketBox:SetScript("OnTextChanged", function(self)
        if not self.updatingText and self:GetText() ~= self.savedText then
            self.updatingText = true
            self:SetText(self.savedText or "")
            self.updatingText = false
            self:HighlightText()
        end
    end)
    ticketBox:SetScript("OnMouseUp", function()
        if selectedTicket and not IsSecret(selectedTicket) then
            Widgets.SelectAllText(ticketBox)
        end
    end)
    reloadButton:SetScript("OnClick", function()
        if RefuseInCombat() then
            return
        end
        ReloadUI()
    end)

    page.listScroll = listScroll
    page.rows = rows
    page.ticketBox = ticketBox
    page.ticketHint = ticketHint
    page.pendingText = pendingText
    page.countText = countText
    page.usageText = usageText
    page.emptyTitle = emptyTitle
    page.emptyHelp = emptyHelp
    page.detailHeading = detailHeading
    page.detailStatus = detailStatus
    page.nameValue = nameValue
    page.kindValue = kindValue
    page.timeValue = timeValue
    page.sizeValue = sizeValue
    page.clientValue = clientValue
    page.pathValue = pathValue
    page.deleteButton = deleteButton
    page.clearButton = clearButton
    page.reloadButton = reloadButton
    page.Refresh = Refresh
    page.SelectRecord = SelectRecord
    function page:GetSelectedTicket()
        return selectedTicket
    end
    function page:Activate()
        Refresh()
    end

    Refresh()
    return page
end

Workbench.RegisterPage{
    key = "exports",
    titleKey = "TAB_EXPORTS",
    build = BuildExportRecordsPage,
    activate = function(page)
        page:Activate()
    end,
    -- Window hide / combat teardown: disarm both two-click confirmations so a
    -- later click never lands on an armed destructive action.
    shutdown = function(page)
        page.deleteButton:ResetConfirm()
        page.clearButton:ResetConfirm()
    end,
}
