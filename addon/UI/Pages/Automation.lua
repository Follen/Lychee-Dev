local ADDON_NAME, ns = ...

-- Automation workbench page: the in-game view and manual execute over the SAME
-- game-side probe machinery the CLI drives (design.md section 12). It lists
-- queue entries and execution history through Modules/AutomationView.lua,
-- which derives every status from ns.ProbeQueue / ns.ProbeRunner /
-- ns.ReportStore. No task registry, no /dev auto grammar and no parallel
-- executor exist here.
local L = ns.L
local W = ns.Widgets
local view = ns.AutomationView

local ROW_HEIGHT = 64
local LIST_WIDTH = 256
local VISIBLE_ROWS = 8

-- Mirrors the bridge record vocabulary exposed by the bridge modules (see
-- Modules/AutomationView.lua for the derivation). An unavailable record shows
-- its raw bridge error code instead of a made-up label.
local STATUS_LABELS = {
    queued = "AUTO_STATUS_QUEUED",
    loaded = "AUTO_STATUS_LOADED",
    running = "AUTO_STATUS_RUNNING",
    finalizing = "AUTO_STATUS_FINALIZING",
    reported = "AUTO_STATUS_REPORTED",
    acknowledged = "AUTO_STATUS_ACKNOWLEDGED",
    cleared = "AUTO_STATUS_CLEARED",
    unavailable = "AUTO_STATUS_UNAVAILABLE",
}

local KIND_LABELS = {
    lua = "AUTO_KIND_LUA",
    bug_snapshot = "AUTO_KIND_BUG",
}

local function Restricted(value)
    return issecretvalue and issecretvalue(value)
end

local function ShownText(value, fallback)
    if Restricted(value) then
        return "<secret>"
    end
    if value == nil then
        return fallback or ""
    end
    return tostring(value)
end

local function GetStatusText(record)
    if record.status == "unavailable" and record.errorCode then
        return ShownText(record.errorCode)
    end
    local key = STATUS_LABELS[record.status]
    return key and L[key] or ShownText(record.status, L.UNKNOWN)
end

local function GetKindLabel(kind)
    local key = KIND_LABELS[kind]
    return key and L[key] or L.EXPORT_KIND_UNKNOWN
end

local function FormatTime(timestamp)
    if Restricted(timestamp) then
        return "<secret>"
    end
    timestamp = tonumber(timestamp) or 0
    if timestamp <= 0 then
        return L.UNKNOWN
    end
    return date("%Y-%m-%d %H:%M:%S", timestamp)
end

local function FormatCode(record)
    if Restricted(record.codeBytes) or type(record.codeBytes) ~= "number" then
        return L.NOT_AVAILABLE
    end
    return string.format(L.AUTO_CODE_SUMMARY, record.codeBytes,
        ShownText(record.codeAdler32, "-"), ShownText(record.codeSHA256, "-"))
end

local function FormatRecorded(record)
    local text = FormatTime(record.observedAt)
    if type(record.sequence) == "number" and not Restricted(record.sequence) then
        text = text .. "  ·  #" .. tostring(record.sequence)
    end
    return text
end

local function CreateField(parent, labelText, x, y, width)
    local label = parent:CreateFontString(nil, "OVERLAY", "GameFontDisableSmall")
    ns.Theme.SetFont(label, 11, ns.Theme.textDim)
    label:SetPoint("TOPLEFT", x, y)
    label:SetText(labelText)
    label:SetTextColor(unpack(ns.Theme.textDim))

    local value = parent:CreateFontString(nil, "OVERLAY", "GameFontHighlightSmall")
    ns.Theme.SetFont(value, 12, ns.Theme.text)
    value:SetPoint("TOPLEFT", x, y - 20)
    value:SetWidth(width)
    value:SetJustifyH("LEFT")
    value:SetWordWrap(false)
    value:SetTextColor(unpack(ns.Theme.text))
    return value
end

function ns.CreateAutomationPage(parent)
    local colors = W.Colors
    local page = CreateFrame("Frame", nil, parent)
    page:SetAllPoints(parent)

    local selectedRequestId
    local rows = {}

    local heading = W.CreatePageHeading(page, L.TAB_AUTOMATION, L.AUTOMATION_PAGE_HELP)

    local countText = page:CreateFontString(nil, "OVERLAY", "GameFontDisableSmall")
    ns.Theme.SetFont(countText, 11, ns.Theme.textDim)
    countText:SetPoint("LEFT", heading, "RIGHT", 12, 0)
    countText:SetTextColor(unpack(ns.Theme.textDim))

    local clearButton = W.CreateConfirmButton(page, 118, L.AUTO_CLEAR_HISTORY,
        L.CONFIRM_CLEAR_CACHE, function()
            if ns.Safety.IsCombatBlocked() then
                ns.Safety.PrintBlocked()
                return
            end
            view.ClearRecords()
            selectedRequestId = nil
            page:Refresh()
        end, "secondary")
    clearButton:SetPoint("BOTTOMLEFT", 14, 14)

    local hideNoticeButton = W.CreateButton(page, 118, L.AUTO_HIDE_NOTICE, "secondary")
    hideNoticeButton:SetPoint("BOTTOMRIGHT", -14, 14)

    local showNoticeButton = W.CreateButton(page, 128, L.AUTO_SHOW_NOTICE, "secondary")
    showNoticeButton:SetPoint("RIGHT", hideNoticeButton, "LEFT", -8, 0)

    local executeButton = W.CreateButton(page, 96, L.AUTO_EXECUTE, "primary")
    executeButton:SetPoint("TOPRIGHT", -14, -84)

    local listPanel = W.CreatePanel(page, colors.editor[1], colors.editor[2], colors.editor[3], 1)
    listPanel:SetPoint("TOPLEFT", 14, -84)
    listPanel:SetPoint("BOTTOMLEFT", 14, 64)
    listPanel:SetWidth(LIST_WIDTH)

    local listScroll = W.CreateScrollArea(listPanel, 8, 8, 7, 8)
    local listContent = CreateFrame("Frame", nil, listScroll)
    listContent:SetWidth(LIST_WIDTH - 26)
    listContent:SetHeight(1)
    listScroll:SetScrollChild(listContent)

    local emptyTitle = listPanel:CreateFontString(nil, "OVERLAY", "GameFontNormal")
    ns.Theme.SetFont(emptyTitle, 14, ns.Theme.text)
    emptyTitle:SetPoint("CENTER", listPanel, "CENTER", 0, 14)
    emptyTitle:SetText(L.AUTO_NO_EXECUTIONS)
    emptyTitle:SetTextColor(unpack(ns.Theme.textMuted))

    local detailPanel = CreateFrame("Frame", nil, page)
    detailPanel:SetPoint("TOPLEFT", listPanel, "TOPRIGHT", 12, 0)
    detailPanel:SetPoint("BOTTOMRIGHT", -14, 64)

    local detailHeading = detailPanel:CreateFontString(nil, "OVERLAY", "GameFontNormal")
    ns.Theme.SetFont(detailHeading, 14, ns.Theme.text)
    detailHeading:SetPoint("TOPLEFT", 4, -4)
    detailHeading:SetPoint("TOPRIGHT", -110, -4)
    detailHeading:SetJustifyH("LEFT")
    detailHeading:SetWordWrap(false)
    detailHeading:SetText(L.AUTO_DETAIL)

    local detailState = detailPanel:CreateFontString(nil, "OVERLAY", "GameFontHighlightSmall")
    ns.Theme.SetFont(detailState, 12, ns.Theme.text)
    detailState:SetPoint("TOPLEFT", 4, -28)

    local metadata=CreateFrame("Frame",nil,detailPanel)
    metadata:SetPoint("TOPLEFT",0,-94);metadata:SetPoint("TOPRIGHT",0,-94);metadata:SetPoint("BOTTOMRIGHT",-4,4)
    metadata:Hide()
    local requestValue = CreateField(metadata, L.AUTO_FIELD_REQUEST, 4, 0, 492)
    local kindValue = CreateField(metadata, L.EXPORT_RECORD_KIND, 4, -52, 232)
    local statusValue = CreateField(metadata, L.AUTO_FIELD_STATUS, 254, -52, 240)
    local codeValue = CreateField(metadata, L.AUTO_FIELD_CODE, 4, -104, 492)
    local timeValue = CreateField(metadata, L.AUTO_FIELD_TIME, 4, -156, 232)
    local errorValue = CreateField(metadata, L.AUTO_FIELD_ERROR, 254, -156, 240)

    local viewReportButton = W.CreateModeTab(detailPanel, L.AUTO_VIEW_REPORT, nil)
    viewReportButton:SetWidth(112);viewReportButton:SetPoint("TOPLEFT",4,-56)
    local detailsButton=W.CreateModeTab(detailPanel,L.AUTO_EVIDENCE_DETAILS,nil)
    detailsButton:SetWidth(112);detailsButton:SetPoint("LEFT",viewReportButton,"RIGHT",4,0)
    viewReportButton:SetActive(true)

    local reportArea = W.CreateTextArea(detailPanel, true)
    reportArea:SetPoint("TOPLEFT", 4, -94)
    reportArea:SetPoint("BOTTOMRIGHT", -4, 4)
    W.SetReadOnlyText(reportArea, L.AUTO_SELECT_RECORD)
    detailsButton:SetScript("OnClick",function()
        metadata:Show();reportArea:Hide()
        detailsButton:SetActive(true)
        viewReportButton:SetActive(false)
    end)

    local function StatusColor(record)
        if record.status == "unavailable" or record.probeStatus == "failed" then
            return ns.Theme.danger[1], ns.Theme.danger[2], ns.Theme.danger[3], 1
        elseif record.status == "reported" or record.probeStatus == "completed" then
            return ns.Theme.success[1], ns.Theme.success[2], ns.Theme.success[3], 1
        elseif record.status == "queued" or record.status == "loaded" or record.status == "running" or record.status == "finalizing" then
            return ns.Theme.warning[1], ns.Theme.warning[2], ns.Theme.warning[3], 1
        end
        return ns.Theme.textDim[1], ns.Theme.textDim[2], ns.Theme.textDim[3], 1
    end

    local function ApplyRowState(row)
        local selected = row.requestId == selectedRequestId
        W.SetListRowState(row, selected, row.hovered)
        if selected then
            row.title:SetTextColor(unpack(ns.Theme.text))
        elseif row.hovered then
            row.title:SetTextColor(unpack(ns.Theme.text))
        else
            row.title:SetTextColor(unpack(ns.Theme.textMuted))
        end
    end

    local function RefreshDetail()
        local record = selectedRequestId and view.GetRecord(selectedRequestId) or nil
        local hasRecord = record ~= nil

        detailHeading:SetText(hasRecord and GetKindLabel(record.kind) or L.AUTO_DETAIL)
        detailState:SetText(hasRecord and GetStatusText(record) or "")
        if hasRecord then
            detailState:SetTextColor(StatusColor(record))
        else
            detailState:SetTextColor(unpack(ns.Theme.textDim))
        end
        requestValue:SetText(hasRecord and ShownText(record.requestId) or "")
        kindValue:SetText(hasRecord and GetKindLabel(record.kind) or "")
        statusValue:SetText(hasRecord and GetStatusText(record) or "")
        codeValue:SetText(hasRecord and FormatCode(record) or "")
        timeValue:SetText(hasRecord and FormatRecorded(record) or "")
        errorValue:SetText(hasRecord
            and ShownText(record.actionError or record.probeError or record.errorCode, L.NOT_AVAILABLE) or "")

        W.SetButtonEnabled(executeButton, hasRecord and (record.status == "queued" or record.status == "loaded"))
        W.SetButtonEnabled(showNoticeButton, hasRecord and record.receipt ~= nil)
        W.SetButtonEnabled(viewReportButton, hasRecord)
        W.SetButtonEnabled(detailsButton, hasRecord)
    end

    local function ShowSelectedReport()
        local text,failure=view.GetReportText(selectedRequestId)
        W.SetReadOnlyText(reportArea,text or ShownText(failure,L.AUTO_NO_REPORT))
    end

    local function SelectRecord(requestId)
        selectedRequestId = requestId
        ShowSelectedReport()
        for index = 1, #rows do
            ApplyRowState(rows[index])
        end
        RefreshDetail()
    end
    page.SelectRecord = SelectRecord

    local function AcquireRow(index)
        local row = rows[index]
        if row then
            return row
        end

        row = W.CreateListRow(listContent, ROW_HEIGHT)
        row:SetPoint("TOPLEFT", 0, 0)
        row:SetPoint("TOPRIGHT", 0, 0)
        row.hovered = false

        local title = row:CreateFontString(nil, "OVERLAY", "GameFontHighlightSmall")
        ns.Theme.SetFont(title, 12, ns.Theme.text)
        title:SetPoint("TOPLEFT", 12, -10)
        title:SetPoint("TOPRIGHT", -100, -10)
        title:SetJustifyH("LEFT")
        title:SetWordWrap(false)
        row.title = title

        local status = row:CreateFontString(nil, "OVERLAY", "GameFontDisableSmall")
        ns.Theme.SetFont(status, 11, ns.Theme.textDim)
        status:SetPoint("TOPRIGHT", -10, -10)
        status:SetJustifyH("RIGHT")
        row.status = status

        local meta = row:CreateFontString(nil, "OVERLAY", "GameFontDisableSmall")
        ns.Theme.SetFont(meta, 11, ns.Theme.textDim)
        meta:SetPoint("BOTTOMLEFT", 12, 9)
        meta:SetPoint("BOTTOMRIGHT", -10, 9)
        meta:SetJustifyH("LEFT")
        meta:SetWordWrap(false)
        meta:SetTextColor(unpack(ns.Theme.textDim))
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
            SelectRecord(self.requestId)
        end)
        row:SetScript("OnMouseWheel", listScroll.onMouseWheel)
        rows[index] = row
        return row
    end

    local function Refresh()
        view.Collect()
        local order = view.GetOrder()
        countText:SetText(string.format(L.AUTO_EXECUTION_COUNT, #order))
        emptyTitle:SetShown(#order == 0)
        W.SetButtonEnabled(clearButton, #order > 0)

        for index = 1, #rows do
            rows[index]:Hide()
        end

        local scrollOffset = listScroll:GetVerticalScroll()
        if Restricted(scrollOffset) then
            scrollOffset = 0
        end
        local firstIndex = math.floor((scrollOffset or 0) / ROW_HEIGHT) + 1
        local lastIndex = math.min(#order, firstIndex + VISIBLE_ROWS - 1)
        local poolIndex = 0
        for orderIndex = firstIndex, lastIndex do
            poolIndex = poolIndex + 1
            local record = view.GetRecord(order[orderIndex])
            local row = rows[poolIndex] or AcquireRow(poolIndex)
            row.requestId = record.requestId
            row:ClearAllPoints()
            row:SetPoint("TOPLEFT", 0, -((orderIndex - 1) * ROW_HEIGHT))
            row:SetPoint("TOPRIGHT", 0, -((orderIndex - 1) * ROW_HEIGHT))
            row.title:SetText(GetKindLabel(record.kind))
            row.status:SetText(GetStatusText(record))
            row.status:SetTextColor(StatusColor(record))
            row.meta:SetText(FormatTime(record.observedAt) .. " | " .. GetKindLabel(record.kind))
            ApplyRowState(row)
            row:Show()
        end

        listContent:SetHeight(math.max(1, #order * ROW_HEIGHT))
        listScroll:UpdateScrollChildRect()

        -- Newest record first: when the selection is gone, fall back to the
        -- head of the newest-first order.
        if selectedRequestId and not view.GetRecord(selectedRequestId) then
            selectedRequestId = nil
        end
        if not selectedRequestId and order[1] then
            selectedRequestId = order[1]
            ShowSelectedReport()
        end
        for index = 1, #rows do
            ApplyRowState(rows[index])
        end
        RefreshDetail()
    end
    page.Refresh = Refresh

    listScroll.onVerticalScrollChanged = function()
        page:Refresh()
    end

    executeButton:SetScript("OnClick", function()
        if ns.Safety.IsCombatBlocked() then
            ns.Safety.PrintBlocked()
            return
        end
        if not selectedRequestId then
            return
        end
        -- The one execute path: ProbeQueue.Load + ProbeRunner.Dispatch, the
        -- exact calls the /dev bridge load/run verbs make. Failures surface as
        -- their bridge error codes; nothing is invented here.
        local _, failure = view.Execute(selectedRequestId)
        local record = view.GetRecord(selectedRequestId)
        if record then
            -- Transient action error for the detail pane; the row status stays
            -- the derived bridge status refreshed below.
            record.actionError = failure
        end
        Refresh()
    end)

    showNoticeButton:SetScript("OnClick", function()
        if ns.Safety.IsCombatBlocked() then
            ns.Safety.PrintBlocked()
            return
        end
        if selectedRequestId then
            view.ShowNotice(selectedRequestId)
        end
    end)

    -- Hiding the notice is teardown and must stay available in every state.
    hideNoticeButton:SetScript("OnClick", function()
        view.HideNotice()
    end)

    viewReportButton:SetScript("OnClick", function()
        metadata:Hide();reportArea:Show()
        detailsButton:SetActive(false)
        viewReportButton:SetActive(true)
        if ns.Safety.IsCombatBlocked() then
            ns.Safety.PrintBlocked()
            return
        end
        if not selectedRequestId then
            return
        end
        local text, failure = view.GetReportText(selectedRequestId)
        W.SetReadOnlyText(reportArea, text or ShownText(failure, L.AUTO_NO_REPORT))
    end)

    page.rows = rows
    page.reportArea = reportArea
    page.requestValue = requestValue
    page.kindValue = kindValue
    page.statusValue = statusValue
    page.codeValue = codeValue
    page.timeValue = timeValue
    page.errorValue = errorValue
    page.executeButton = executeButton
    page.showNoticeButton = showNoticeButton
    page.hideNoticeButton = hideNoticeButton
    page.viewReportButton = viewReportButton
    page.clearButton = clearButton
    page.detailsButton,page.metadata=detailsButton,metadata
    function page:Activate()
        view.SetChangeHandler(Refresh)
        Refresh()
    end
    page:SetScript("OnHide",function() view.SetChangeHandler(nil) end)

    Refresh()
    return page
end

-- Workbench registration (zero cost before the first /dev; the page builds
-- lazily and owns no events or timers).
local pageInstance
local definition = {
    key = "automation",
    titleKey = "TAB_AUTOMATION",
    build = function(parent)
        pageInstance = ns.CreateAutomationPage(parent)
        return pageInstance
    end,
    activate = function()
        if pageInstance then
            pageInstance:Activate()
        end
    end,
    suspend = function()
    end,
    shutdown = function()
        if ns.AutomationView then
            ns.AutomationView.HideNotice()
        end
    end,
}

if ns.Workbench and type(ns.Workbench.RegisterPage) == "function" then
    ns.Workbench.RegisterPage(definition)
    if type(ns.Workbench.RegisterShutdown) == "function" then
        ns.Workbench.RegisterShutdown(definition.shutdown)
    end
end
ns.AutomationPageDefinition = definition
