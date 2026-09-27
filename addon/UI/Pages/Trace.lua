local ADDON_NAME, ns = ...
local L = ns.L

-- Trace workbench page: one bounded function trace at a time with a 300-record
-- call list and a per-call argument detail pane.
local ROW_HEIGHT = 44
local LIST_WIDTH = 292
local VISIBLE_ROWS = 18

-- The single built page instance; only read by the shell export hook.
local builtPage

local CreateLineInput = ns.Widgets.CreateLineInput

local function BuildTracePage(parent)
    local W = ns.Widgets
    local colors = W.Colors
    local layout = ns.Workbench.Layout
    local accent = colors.accent

    local page = CreateFrame("Frame", nil, parent)
    page:SetAllPoints(parent)
    W.CreatePageHeading(page, L.TAB_TRACE, L.TRACE_PAGE_HELP)

    local input = CreateLineInput(page)
    input:SetPoint("TOPLEFT", 14, -84)
    input:SetPoint("TOPRIGHT", -250, -84)
    input:SetHeight(36)
    input.editBox:SetText("C_AddOns.GetAddOnInfo")

    local traceButton = W.CreateButton(page, 112, L.START_TRACE, "primary")
    traceButton:SetPoint("TOPRIGHT", -14, -84)
    local clearButton = W.CreateButton(page, 100, L.CLEAR_LOG, false)
    clearButton:SetPoint("RIGHT", traceButton, "LEFT", -8, 0)
    page.traceButton = traceButton
    page.clearButton = clearButton
    local dot = page:CreateTexture(nil, "ARTWORK")
    dot:SetSize(5, 5)
    dot:SetPoint("TOPLEFT", input, "BOTTOMLEFT", 2, -15)
    local status = page:CreateFontString(nil, "OVERLAY", "GameFontHighlightSmall")
    ns.Theme.SetFont(status, 12, ns.Theme.text)
    status:SetPoint("LEFT", dot, "RIGHT", 7, 0)
    local function SetStatus(text, r, g, b)
        status:SetText(text)
        status:SetTextColor(r, g, b, 0.92)
        dot:SetColorTexture(r, g, b, 0.92)
    end

    local listLabel = W.CreateSectionLabel(page, L.CALL_RECORDS)
    listLabel:SetPoint("TOPLEFT", 17, -152)
    local detailLabel = W.CreateSectionLabel(page, L.CALL_ARGUMENTS)
    detailLabel:SetPoint("TOPLEFT", LIST_WIDTH + 29, -152)

    local listPanel = W.CreatePanel(page, colors.editor[1], colors.editor[2], colors.editor[3], 1)
    listPanel:SetPoint("TOPLEFT", 14, -172)
    listPanel:SetPoint("BOTTOMLEFT", 14, 64)
    listPanel:SetWidth(LIST_WIDTH)
    local scroll = W.CreateScrollArea(listPanel, 8, 8, 7, 8)
    local content = CreateFrame("Frame", nil, scroll)
    content:SetWidth(LIST_WIDTH - 26)
    content:SetHeight(1)
    scroll:SetScrollChild(content)
    local empty = listPanel:CreateFontString(nil, "OVERLAY", "GameFontDisableSmall")
    ns.Theme.SetFont(empty, 11, ns.Theme.textDim)
    empty:SetPoint("TOP", 0, -24)
    empty:SetText(L.NO_CALLS_CAPTURED)
    empty:SetTextColor(unpack(ns.Theme.textDim))

    local selected
    local detail = W.CreateTextArea(page, true)
    detail:SetPoint("TOPLEFT", LIST_WIDTH + 26, -172)
    detail:SetPoint("BOTTOMRIGHT", -14, 64)
    detail.editBox:SetWidth(layout.WINDOW_WIDTH - LIST_WIDTH - 78)
    W.SetReadOnlyText(detail, L.SELECT_CALL_DETAIL)

    local exportDetail = W.CreateButton(page, 92, L.SAVE_TO_DISK, false)
    exportDetail:SetPoint("BOTTOMRIGHT", -14, 14)
    exportDetail:SetScript("OnClick", function()
        if ns.Safety.IsCombatBlocked() then
            ns.Safety.PrintBlocked()
            return
        end
        local kind, title, content, metadata = page.GetTracePayload()
        if not kind then
            return
        end
        ns.Workbench.SaveToDisk(kind, title, content, metadata)
    end)
    page.exportDetail = exportDetail
    page.listPanel = listPanel
    page.detailPanel = detail

    local rows, refreshQueued = {}, false
    local RefreshList
    page.callRows = rows
    local function SyncTraceButton()
        local running = ns.FunctionTrace.IsRunning()
        W.SetButtonText(traceButton, running and L.STOP_TRACE or L.START_TRACE)
        W.SetButtonVariant(traceButton, running and "danger" or "primary")
    end

    local function Format(record)
        if not record then
            return L.SELECT_CALL_DETAIL
        end
        local lines = {
            string.format(L.TRACE_PATH, record.path),
            string.format(L.ELAPSED_DETAIL, record.elapsed),
            "",
        }
        if #record.arguments == 0 then
            lines[#lines + 1] = L.NO_ARGUMENTS
        end
        for index = 1, #record.arguments do
            lines[#lines + 1] = "[" .. index .. "] = " .. record.arguments[index]
        end
        return table.concat(lines, "\n")
    end

    local function CreateRow(index)
        local row = W.CreateListRow(content, ROW_HEIGHT)
        row:SetWidth(LIST_WIDTH - 26)
        local elapsed = row:CreateFontString(nil, "OVERLAY", "GameFontDisableSmall")
        ns.Theme.SetFont(elapsed, 11, ns.Theme.textDim)
        elapsed:SetPoint("LEFT", 8, 0)
        elapsed:SetWidth(62)
        elapsed:SetJustifyH("LEFT")
        elapsed:SetTextColor(unpack(ns.Theme.textDim))
        row.elapsed = elapsed
        local name = row:CreateFontString(nil, "OVERLAY", "GameFontHighlightSmall")
        ns.Theme.SetFont(name, 12, ns.Theme.text)
        name:SetPoint("TOPLEFT", 72, -5)
        name:SetPoint("TOPRIGHT", -8, -5)
        name:SetJustifyH("LEFT")
        name:SetWordWrap(false)
        row.name = name
        local summary = row:CreateFontString(nil, "OVERLAY", "GameFontDisableSmall")
        ns.Theme.SetFont(summary, 11, ns.Theme.textDim)
        summary:SetPoint("BOTTOMLEFT", 72, 5)
        summary:SetPoint("BOTTOMRIGHT", -8, 5)
        summary:SetJustifyH("LEFT")
        summary:SetWordWrap(false)
        summary:SetTextColor(unpack(ns.Theme.textDim))
        row.summary = summary
        row:SetScript("OnEnter", function(self)
            self.isHovered = true
            W.SetListRowState(self, self.record == selected, true)
        end)
        row:SetScript("OnLeave", function(self)
            self.isHovered = nil
            W.SetListRowState(self, self.record == selected, false)
        end)
        row:SetScript("OnClick", function(self)
            selected = self.record
            W.SetReadOnlyText(detail, Format(selected))
            RefreshList()
        end)
        rows[index] = row
        return row
    end

    RefreshList = function()
        for index = 1, #rows do
            rows[index]:Hide()
        end
        local recordCount = ns.FunctionTrace.GetCount()
        W.SetButtonEnabled(clearButton, recordCount > 0)
        W.SetButtonEnabled(exportDetail, recordCount > 0)
        local offset = scroll:GetVerticalScroll()
        if issecretvalue and issecretvalue(offset) then
            offset = 0
        end
        local first = math.floor((offset or 0) / ROW_HEIGHT) + 1
        local last = math.min(recordCount, first + VISIBLE_ROWS - 1)
        local pool = 0
        for recordIndex = first, last do
            pool = pool + 1
            local record = ns.FunctionTrace.GetRecord(recordIndex)
            local row = rows[pool] or CreateRow(pool)
            row.record = record
            row:ClearAllPoints()
            row:SetPoint("TOPLEFT", 0, -((recordIndex - 1) * ROW_HEIGHT))
            row.elapsed:SetText(string.format("+%.2fs", record.elapsed))
            row.name:SetText(record.path)
            row.summary:SetText(record.summary ~= "" and record.summary or L.NO_ARGUMENTS)
            W.SetListRowState(row, record == selected, row.isHovered)
            row:Show()
        end
        content:SetHeight(math.max(1, recordCount * ROW_HEIGHT))
        scroll:UpdateScrollChildRect()
        empty:SetShown(recordCount == 0)
    end

    scroll.onVerticalScrollChanged = function()
        RefreshList()
    end

    local function QueueRefresh()
        if refreshQueued then
            return
        end
        refreshQueued = true
        C_Timer.After(0, function()
            refreshQueued = false
            if page:IsShown() then
                RefreshList()
            end
        end)
    end

    traceButton:SetScript("OnClick", function()
        if ns.FunctionTrace.IsRunning() then
            ns.FunctionTrace.Stop()
            SetStatus(L.STOPPED, unpack(ns.Theme.textMuted))
            SyncTraceButton()
            return
        end

        local succeeded, errorMessage = ns.FunctionTrace.Start(input.editBox:GetText(), QueueRefresh)
        if succeeded then
            SetStatus(string.format(L.TRACING_FUNCTION, ns.FunctionTrace.GetActivePath()), unpack(ns.Theme.success))
        else
            SetStatus(errorMessage, accent[1], accent[2], accent[3])
        end
        SyncTraceButton()
    end)
    clearButton:SetScript("OnClick", function()
        if ns.Safety.IsCombatBlocked() then
            ns.Safety.PrintBlocked()
            return
        end
        ns.FunctionTrace.Clear()
        selected = nil
        W.SetReadOnlyText(detail, L.SELECT_CALL_DETAIL)
        RefreshList()
    end)
    input.editBox:SetScript("OnEnterPressed", function()
        traceButton:Click()
    end)

    function page.HandleActivate()
        input.editBox:SetFocus()
        SyncTraceButton()
        RefreshList()
    end

    -- One payload builder shared by the page Save button and the shell export
    -- hook; both funnel into the single export UI. Records are newest first
    -- and bounded by the trace ring (300 records).
    function page.GetTracePayload()
        local recordCount = ns.FunctionTrace.GetCount()
        if recordCount <= 0 then
            return nil
        end
        return "function_trace", L.CALL_RECORDS, function()
            local records = {}
            for index = 1, recordCount do
                records[index] = ns.FunctionTrace.GetRecord(index)
            end
            local text = ns.Serializer.SerializeForExport(records)
            return text
        end, { recordCount = recordCount }
    end

    -- The page stop lives on window hide / combat shutdown: recording is
    -- disabled there and the hook drops back to its inert guard.
    function page.HandleShutdown()
        ns.FunctionTrace.Stop()
        SetStatus(L.STOPPED, unpack(ns.Theme.textMuted))
        SyncTraceButton()
    end

    SetStatus(L.STOPPED, unpack(ns.Theme.textMuted))
    SyncTraceButton()
    RefreshList()
    builtPage = page
    return page
end

ns.Workbench.RegisterPage({
    key = "trace",
    titleKey = "TAB_TRACE",
    build = BuildTracePage,
    activate = function(page)
        if page and page.HandleActivate then
            page:HandleActivate()
        end
    end,
    shutdown = function(page)
        if page and page.HandleShutdown then
            page:HandleShutdown()
        end
    end,
})

ns.Workbench.RegisterExportHook({
    key = "trace",
    GetPayload = function()
        if not builtPage then
            return nil
        end
        return builtPage.GetTracePayload()
    end,
})
