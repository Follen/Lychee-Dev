local ADDON_NAME, ns = ...
local L = ns.L

local ROW_HEIGHT = 38
local LIST_WIDTH = 520
local VISIBLE_ROWS = 18

local function SetReadOnlyText(panel, text)
    local editBox = panel.editBox
    editBox.savedText, editBox.updatingText = text or "", true
    editBox:SetText(text or "")
    editBox.updatingText = nil
    editBox:SetCursorPosition(0)
    panel.scroll:SetVerticalScroll(0)
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
    panel.editBox = editBox
    return panel
end

function ns.CreateTracePage(parent, ui)
    local page = CreateFrame("Frame", nil, parent)
    page:SetAllPoints(parent)
    local title = ui.CreateSectionLabel(page, L.FUNCTION_TRACE)
    title:SetPoint("TOPLEFT", 17, -84)
    local input = CreateLineInput(page, ui)
    input:SetPoint("TOPLEFT", 14, -104)
    input:SetPoint("TOPRIGHT", -330, -104)
    input:SetHeight(30)
    input.editBox:SetText("C_AddOns.GetAddOnInfo")
    local start = ui.CreateButton(page, 92, L.START, true)
    start:SetPoint("TOPRIGHT", -14, -104)
    local stop = ui.CreateButton(page, 92, L.STOP, false)
    stop:SetPoint("RIGHT", start, "LEFT", -8, 0)
    local clear = ui.CreateButton(page, 100, L.CLEAR_LOG, false)
    clear:SetPoint("RIGHT", stop, "LEFT", -8, 0)

    local dot = page:CreateTexture(nil, "ARTWORK")
    dot:SetSize(5, 5)
    dot:SetPoint("TOPLEFT", input, "BOTTOMLEFT", 2, -15)
    local status = page:CreateFontString(nil, "OVERLAY", "GameFontHighlightSmall")
    status:SetPoint("LEFT", dot, "RIGHT", 7, 0)
    local function SetStatus(text, r, g, b)
        status:SetText(text) status:SetTextColor(r, g, b, 0.92) dot:SetColorTexture(r, g, b, 0.92)
    end

    local listLabel = ui.CreateSectionLabel(page, L.CALL_RECORDS)
    listLabel:SetPoint("TOPLEFT", 17, -172)
    local detailLabel = ui.CreateSectionLabel(page, L.CALL_ARGUMENTS)
    detailLabel:SetPoint("TOPLEFT", LIST_WIDTH + 29, -172)
    local listPanel = ui.CreatePanel(page, ui.editorR, ui.editorG, ui.editorB, 0.9)
    listPanel:SetPoint("TOPLEFT", 14, -192)
    listPanel:SetPoint("BOTTOMLEFT", 14, 54)
    listPanel:SetWidth(LIST_WIDTH)
    local scroll = ui.CreateScrollArea(listPanel, 8, 8, 7, 8)
    local content = CreateFrame("Frame", nil, scroll)
    content:SetWidth(LIST_WIDTH - 34) content:SetHeight(1) scroll:SetScrollChild(content)
    local empty = listPanel:CreateFontString(nil, "OVERLAY", "GameFontDisableSmall")
    empty:SetPoint("TOP", 0, -24) empty:SetText(L.NO_CALLS_CAPTURED) empty:SetTextColor(1, 1, 1, 0.32)
    local detail = ui.CreateTextArea(page, true)
    detail:SetPoint("TOPLEFT", LIST_WIDTH + 26, -192)
    detail:SetPoint("BOTTOMRIGHT", -14, 54)
    detail.editBox:SetWidth(ui.windowWidth - LIST_WIDTH - 78)
    SetReadOnlyText(detail, L.SELECT_CALL_DETAIL)

    local rows, selected, refreshQueued = {}, nil, false
    local function Format(record)
        if not record then return L.SELECT_CALL_DETAIL end
        local lines = { string.format(L.TRACE_PATH, record.path), string.format(L.ELAPSED_DETAIL, record.elapsed), "" }
        if #record.arguments == 0 then lines[#lines + 1] = L.NO_ARGUMENTS end
        for index = 1, #record.arguments do lines[#lines + 1] = "[" .. index .. "] = " .. record.arguments[index] end
        return table.concat(lines, "\n")
    end
    local function CreateRow(index)
        local row = CreateFrame("Button", nil, content, "BackdropTemplate")
        row:SetSize(LIST_WIDTH - 34, ROW_HEIGHT - 2) row:SetBackdrop(ui.backdrop)
        local elapsed = row:CreateFontString(nil, "OVERLAY", "GameFontDisableSmall")
        elapsed:SetPoint("LEFT", 8, 0) elapsed:SetWidth(62) elapsed:SetJustifyH("LEFT") elapsed:SetTextColor(1, 1, 1, 0.38) row.elapsed = elapsed
        local name = row:CreateFontString(nil, "OVERLAY", "GameFontHighlightSmall")
        name:SetPoint("TOPLEFT", 72, -5) name:SetPoint("RIGHT", -8, 0) name:SetJustifyH("LEFT") row.name = name
        local summary = row:CreateFontString(nil, "OVERLAY", "GameFontDisableSmall")
        summary:SetPoint("BOTTOMLEFT", 72, 5) summary:SetPoint("RIGHT", -8, 0) summary:SetJustifyH("LEFT") summary:SetWordWrap(false) summary:SetTextColor(1, 1, 1, 0.38) row.summary = summary
        row:SetScript("OnClick", function(self) selected = self.record SetReadOnlyText(detail, Format(selected)) page:Refresh() end)
        rows[index] = row return row
    end
    function page:Refresh()
        for index = 1, #rows do rows[index]:Hide() end
        local recordCount = ns.FunctionTrace.GetCount()
        local offset = scroll:GetVerticalScroll() if issecretvalue and issecretvalue(offset) then offset = 0 end
        local first = math.floor((offset or 0) / ROW_HEIGHT) + 1
        local last, pool = math.min(recordCount, first + VISIBLE_ROWS - 1), 0
        for recordIndex = first, last do
            pool = pool + 1
            local record = ns.FunctionTrace.GetRecord(recordIndex)
            local row = rows[pool] or CreateRow(pool)
            row.record = record row:ClearAllPoints() row:SetPoint("TOPLEFT", 0, -((recordIndex - 1) * ROW_HEIGHT))
            row.elapsed:SetText(string.format("+%.2fs", record.elapsed)) row.name:SetText(record.path) row.summary:SetText(record.summary ~= "" and record.summary or L.NO_ARGUMENTS)
            row:SetBackdropColor(ui.surfaceR, ui.surfaceG, ui.surfaceB, record == selected and 0.8 or 0.44) ui.SetBorderColor(row, record == selected, record == selected and 0.35 or 0.18) row:Show()
        end
        content:SetHeight(math.max(1, recordCount * ROW_HEIGHT)) scroll:UpdateScrollChildRect() empty:SetShown(recordCount == 0)
    end
    scroll.onVerticalScrollChanged = function() page:Refresh() end
    local function QueueRefresh()
        if refreshQueued then return end
        refreshQueued = true
        C_Timer.After(0, function() refreshQueued = false if page:IsShown() then page:Refresh() end end)
    end
    start:SetScript("OnClick", function()
        local succeeded, errorMessage = ns.FunctionTrace.Start(input.editBox:GetText(), QueueRefresh)
        SetStatus(succeeded and string.format(L.TRACING_FUNCTION, ns.FunctionTrace.GetActivePath()) or errorMessage, succeeded and 0.42 or ui.accentR, succeeded and 0.76 or ui.accentG, succeeded and 0.43 or ui.accentB)
    end)
    stop:SetScript("OnClick", function() ns.FunctionTrace.Stop() SetStatus(L.STOPPED, 0.55, 0.60, 0.65) end)
    clear:SetScript("OnClick", function() ns.FunctionTrace.Clear() selected = nil SetReadOnlyText(detail, L.SELECT_CALL_DETAIL) page:Refresh() end)
    input.editBox:SetScript("OnEnterPressed", function() start:Click() end)
    function page:Activate() input.editBox:SetFocus() page:Refresh() end
    function page:Stop()
        ns.FunctionTrace.Stop()
        SetStatus(L.STOPPED, 0.55, 0.60, 0.65)
    end
    SetStatus(L.STOPPED, 0.55, 0.60, 0.65) page:Refresh()
    return page
end
