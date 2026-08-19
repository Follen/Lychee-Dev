local ADDON_NAME, ns = ...
local L = ns.L

local TREE_ROW_HEIGHT = 22
local TREE_ROW_POOL_SIZE = 32
local EVENT_ROW_HEIGHT = 36
local EVENT_LIST_WIDTH = 540
local EVENT_VISIBLE_ROWS = 18
local SEARCH_RESULT_LIMIT = 8
local SEARCH_ROW_HEIGHT = 38
local SELECTED_ROW_HEIGHT = 28
local SELECTED_VISIBLE_ROWS = 3

local function SetReadOnlyText(panel, text)
    text = text or ""
    local editBox = panel.editBox
    editBox.savedText = text
    editBox.updatingText = true
    editBox:SetText(text)
    editBox.updatingText = nil
    editBox:SetCursorPosition(0)
    panel.scroll:SetVerticalScroll(0)
end

local function ValueColor(kind)
    if kind == "string" then
        return 0.62, 0.78, 0.64
    elseif kind == "number" then
        return 0.65, 0.74, 0.91
    elseif kind == "boolean" then
        return 0.90, 0.63, 0.42
    elseif kind == "table" then
        return 0.62, 0.67, 0.72
    elseif kind == "load_more" then
        return 0.86, 0.47, 0.55
    elseif kind == "marker" then
        return 0.85, 0.42, 0.48
    end
    return 0.70, 0.73, 0.76
end

function ns.CreateTreeView(parent, ui)
    local panel = ui.CreatePanel(parent, ui.editorR, ui.editorG, ui.editorB, 1)
    local scroll = ui.CreateScrollArea(panel, 8, 8, 7, 8)
    local content = CreateFrame("Frame", nil, scroll)
    content:SetWidth(ui.contentWidth)
    content:SetHeight(1)
    scroll:SetScrollChild(content)

    local empty = panel:CreateFontString(nil, "OVERLAY", "GameFontDisableSmall")
    empty:SetPoint("TOP", 0, -24)
    empty:SetText(L.TREE_EMPTY)
    empty:SetTextColor(1, 1, 1, 0.32)

    local rows = {}
    local visibleNodes = {}
    local tree
    local selectedNode
    local selectionChanged
    local nodeContext
    local view = { panel = panel }

    local function FlattenNodes(nodes, depth, visible)
        for index = 1, #nodes do
            local node = nodes[index]
            visible[#visible + 1] = { node = node, depth = depth }
            if node.kind == "table" and node.expanded and node.children then
                FlattenNodes(node.children, depth + 1, visible)
            end
        end
    end

    local function ApplyRowStyle(row)
        if row.node == selectedNode then
            row.hover:SetColorTexture(ui.accentR, ui.accentG, ui.accentB, 0.16)
        elseif row.isHovered then
            row.hover:SetColorTexture(ui.surfaceR, ui.surfaceG, ui.surfaceB, 0.72)
        else
            row.hover:SetColorTexture(0, 0, 0, 0)
        end
    end

    local function CreateRow(index)
        local row = CreateFrame("Button", nil, content)
        row:SetSize(ui.contentWidth, TREE_ROW_HEIGHT)
        row:RegisterForClicks("LeftButtonUp", "RightButtonUp")

        local hover = row:CreateTexture(nil, "BACKGROUND")
        hover:SetAllPoints()
        hover:SetColorTexture(0, 0, 0, 0)
        row.hover = hover

        local toggle = row:CreateFontString(nil, "OVERLAY", "GameFontHighlightSmall")
        toggle:SetJustifyH("CENTER")
        toggle:SetTextColor(ui.accentR, ui.accentG, ui.accentB, 0.95)
        row.toggle = toggle

        local key = row:CreateFontString(nil, "OVERLAY", "GameFontHighlightSmall")
        key:SetJustifyH("LEFT")
        key:SetTextColor(0.91, 0.93, 0.95, 0.92)
        row.key = key

        local value = row:CreateFontString(nil, "OVERLAY", "GameFontHighlightSmall")
        value:SetPoint("LEFT", 250, 0)
        value:SetPoint("RIGHT", -8, 0)
        value:SetJustifyH("LEFT")
        value:SetWordWrap(false)
        row.value = value

        row:SetScript("OnEnter", function(self)
            self.isHovered = true
            ApplyRowStyle(self)
        end)
        row:SetScript("OnLeave", function(self)
            self.isHovered = nil
            ApplyRowStyle(self)
        end)
        row:SetScript("OnClick", function(self, mouseButton)
            local node = self.node
            if not node then
                return
            elseif mouseButton ~= "RightButton" and node.kind == "load_more" and node.owner then
                ns.LoadMoreValueTreeNode(node.owner)
                view:Refresh()
                return
            end

            if node.exportable then
                selectedNode = node
                if selectionChanged then
                    selectionChanged(node)
                end
            end
            if mouseButton == "RightButton" then
                if node.exportable and nodeContext then
                    nodeContext(node)
                end
                view:Refresh()
                return
            end
            if node.kind == "table" then
                if node.expanded then
                    node.expanded = false
                else
                    if not node.loaded then
                        ns.LoadMoreValueTreeNode(node)
                    end
                    node.expanded = true
                end
            end
            view:Refresh()
        end)

        rows[index] = row
        return row
    end

    function view:HasTree()
        return tree and tree.roots and #tree.roots > 0
    end

    function view:GetSelectedNode()
        return selectedNode
    end

    function view:SetOnSelectionChanged(callback)
        selectionChanged = type(callback) == "function" and callback or nil
    end

    function view:SetOnNodeContext(callback)
        nodeContext = type(callback) == "function" and callback or nil
    end

    function view:SetTree(newTree)
        tree = newTree
        selectedNode = tree and tree.roots and tree.roots[1] and tree.roots[1].exportable
            and tree.roots[1] or nil
        scroll:SetVerticalScroll(0)
        self:Refresh()
        if selectionChanged then
            selectionChanged(selectedNode)
        end
    end

    function view:RenderRows()
        for index = 1, #rows do
            rows[index]:Hide()
        end

        local offset = scroll:GetVerticalScroll() or 0
        if issecretvalue and issecretvalue(offset) then
            return
        end
        local firstIndex = math.floor(math.max(0, offset) / TREE_ROW_HEIGHT) + 1
        local lastIndex = math.min(#visibleNodes, firstIndex + TREE_ROW_POOL_SIZE - 1)

        for visibleIndex = firstIndex, lastIndex do
            local poolIndex = visibleIndex - firstIndex + 1
            local item = visibleNodes[visibleIndex]
            local node = item.node
            local row = rows[poolIndex] or CreateRow(poolIndex)
            local indent = item.depth * 15
            row.node = node
            row:ClearAllPoints()
            row:SetPoint("TOPLEFT", 0, -((visibleIndex - 1) * TREE_ROW_HEIGHT))

            row.toggle:ClearAllPoints()
            row.toggle:SetPoint("LEFT", 6 + indent, 0)
            row.toggle:SetSize(14, TREE_ROW_HEIGHT)
            if node.kind == "table" and (not node.loaded or #node.children > 0) then
                row.toggle:SetText(node.expanded and "-" or "+")
            elseif node.kind == "load_more" then
                row.toggle:SetText("+")
            else
                row.toggle:SetText("")
            end

            row.key:ClearAllPoints()
            row.key:SetPoint("LEFT", 24 + indent, 0)
            row.key:SetWidth(math.max(70, 218 - indent))
            row.key:SetText(node.label or "")

            local r, g, b = ValueColor(node.kind)
            row.value:SetTextColor(r, g, b, 0.92)
            row.value:SetText(node.value or "")
            ApplyRowStyle(row)
            row:Show()
        end
    end

    function view:Refresh()
        wipe(visibleNodes)
        if tree and tree.roots then
            FlattenNodes(tree.roots, 0, visibleNodes)
        end

        content:SetHeight(math.max(1, #visibleNodes * TREE_ROW_HEIGHT))
        scroll:UpdateScrollChildRect()
        empty:SetShown(#visibleNodes == 0)
        self:RenderRows()
    end

    scroll.onVerticalScrollChanged = function()
        view:RenderRows()
    end

    view.content = content
    view.rows = rows
    view.scroll = scroll
    return view
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
    editBox:SetScript("OnEscapePressed", function(self)
        self:ClearFocus()
    end)
    editBox:SetScript("OnEditFocusGained", function()
        ui.SetBorderColor(panel, true, 0.75)
    end)
    editBox:SetScript("OnEditFocusLost", function()
        ui.SetBorderColor(panel, false)
    end)
    panel.editBox = editBox
    return panel
end

local function CreateRemoveButton(parent, ui)
    local button = CreateFrame("Button", nil, parent, "BackdropTemplate")
    button:SetSize(22, 22)
    button:SetBackdrop(ui.backdrop)

    local firstLine = button:CreateTexture(nil, "ARTWORK")
    firstLine:SetSize(9, 1)
    firstLine:SetPoint("CENTER")
    firstLine:SetRotation(0.785398)

    local secondLine = button:CreateTexture(nil, "ARTWORK")
    secondLine:SetSize(9, 1)
    secondLine:SetPoint("CENTER")
    secondLine:SetRotation(-0.785398)

    local function ApplyState(hovered, pressed)
        if pressed then
            button:SetBackdropColor(ui.accentR * 0.72, ui.accentG * 0.72, ui.accentB * 0.72, 0.92)
            button:SetBackdropBorderColor(ui.accentR, ui.accentG, ui.accentB, 0.9)
        elseif hovered then
            button:SetBackdropColor(ui.accentR, ui.accentG, ui.accentB, 0.74)
            button:SetBackdropBorderColor(ui.accentR, ui.accentG, ui.accentB, 0.92)
        else
            button:SetBackdropColor(ui.surfaceR, ui.surfaceG, ui.surfaceB, 0.58)
            ui.SetBorderColor(button, false, 0.24)
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

function ns.CreateEventsPage(parent, ui)
    local page = CreateFrame("Frame", nil, parent)
    page:SetAllPoints(parent)

    local inputLabel = ui.CreateSectionLabel(page, L.FIND_EVENT)
    inputLabel:SetPoint("TOPLEFT", 17, -84)

    local catalogCount = page:CreateFontString(nil, "OVERLAY", "GameFontDisableSmall")
    catalogCount:SetPoint("LEFT", inputLabel, "RIGHT", 9, 0)
    catalogCount:SetText(string.format(L.CATALOG_COUNT, ns.EventCatalog.GetCount()))
    catalogCount:SetTextColor(1, 1, 1, 0.34)

    local inputPanel = CreateLineInput(page, ui)
    inputPanel:SetPoint("TOPLEFT", 14, -104)
    inputPanel:SetPoint("TOPRIGHT", -250, -104)
    inputPanel:SetHeight(30)
    page.inputPanel = inputPanel

    local inputHint = inputPanel:CreateFontString(nil, "OVERLAY", "GameFontDisableSmall")
    inputHint:SetPoint("LEFT", 10, 0)
    inputHint:SetText(L.EVENT_SEARCH_HINT)
    inputHint:SetTextColor(1, 1, 1, 0.30)
    page.inputHint = inputHint

    local monitorButton = ui.CreateButton(page, 112, L.START_MONITORING, true)
    monitorButton:SetPoint("TOPRIGHT", -14, -104)

    local clearButton = ui.CreateButton(page, 100, L.CLEAR_LOG, false)
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

    local selectedLabel = ui.CreateSectionLabel(page, L.SELECTED_EVENTS)
    selectedLabel:SetPoint("TOPLEFT", 17, -169)

    local selectedCount = page:CreateFontString(nil, "OVERLAY", "GameFontDisableSmall")
    selectedCount:SetPoint("LEFT", selectedLabel, "RIGHT", 9, 0)
    selectedCount:SetTextColor(1, 1, 1, 0.34)

    local selectedPanel = ui.CreatePanel(page, ui.editorR, ui.editorG, ui.editorB, 0.9)
    selectedPanel:SetPoint("TOPLEFT", 14, -189)
    selectedPanel:SetPoint("TOPRIGHT", -14, -189)
    selectedPanel:SetHeight(42)
    page.selectedPanel = selectedPanel

    local selectedScroll = ui.CreateScrollArea(selectedPanel, 7, 7, 7, 7)
    local selectedContent = CreateFrame("Frame", nil, selectedScroll)
    selectedContent:SetWidth(ui.windowWidth - 56)
    selectedContent:SetHeight(1)
    selectedScroll:SetScrollChild(selectedContent)

    local selectedEmpty = selectedPanel:CreateFontString(nil, "OVERLAY", "GameFontDisableSmall")
    selectedEmpty:SetPoint("CENTER")
    selectedEmpty:SetText(L.NO_EVENTS_SELECTED)
    selectedEmpty:SetTextColor(1, 1, 1, 0.32)

    local logLabel = ui.CreateSectionLabel(page, L.CAPTURED_EVENTS)
    logLabel:SetPoint("TOPLEFT", selectedPanel, "BOTTOMLEFT", 3, -22)

    local detailLabel = ui.CreateSectionLabel(page, L.PAYLOAD)
    detailLabel:SetPoint("TOPLEFT", selectedPanel, "BOTTOMLEFT", EVENT_LIST_WIDTH + 15, -22)

    local logPanel = ui.CreatePanel(page, ui.editorR, ui.editorG, ui.editorB, 0.9)
    logPanel:SetPoint("TOPLEFT", selectedPanel, "BOTTOMLEFT", 0, -42)
    logPanel:SetPoint("BOTTOMLEFT", 14, 54)
    logPanel:SetWidth(EVENT_LIST_WIDTH)

    local logScroll = ui.CreateScrollArea(logPanel, 8, 8, 7, 8)
    local logContent = CreateFrame("Frame", nil, logScroll)
    logContent:SetWidth(EVENT_LIST_WIDTH - 34)
    logContent:SetHeight(1)
    logScroll:SetScrollChild(logContent)

    local empty = logPanel:CreateFontString(nil, "OVERLAY", "GameFontDisableSmall")
    empty:SetPoint("TOP", 0, -24)
    empty:SetText(L.NO_EVENTS_CAPTURED)
    empty:SetTextColor(1, 1, 1, 0.32)

    local selectedRecord
    local FormatDetails
    local detailPanel = ui.CreateTextArea(page, true)
    detailPanel:SetPoint("TOPLEFT", selectedPanel, "BOTTOMLEFT", EVENT_LIST_WIDTH + 12, -42)
    detailPanel:SetPoint("BOTTOMRIGHT", -14, 54)
    detailPanel.editBox:SetWidth(ui.windowWidth - EVENT_LIST_WIDTH - 78)
    SetReadOnlyText(detailPanel, L.SELECT_EVENT_DETAIL)

    local exportDetail = ui.CreateButton(page, 92, L.SAVE_TO_DISK, false)
    exportDetail:SetPoint("BOTTOMRIGHT", -14, 14)
    exportDetail:SetScript("OnClick", function()
        local recordCount = ns.EventMonitor.GetCount()
        if recordCount > 0 then
            ui.ExportText("event_log", L.CAPTURED_EVENTS, function()
                local records = {}
                for index = 1, recordCount do
                    records[index] = ns.EventMonitor.GetRecord(index)
                end
                return ns.SerializeForExport(records)
            end, { recordCount = recordCount })
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

    local searchPanel = ui.CreatePanel(page, ui.editorR, ui.editorG, ui.editorB, 0.99)
    searchPanel:SetPoint("TOPLEFT", inputPanel, "BOTTOMLEFT", 0, -4)
    searchPanel:SetPoint("TOPRIGHT", inputPanel, "BOTTOMRIGHT", 0, -4)
    searchPanel:SetFrameLevel(page:GetFrameLevel() + 30)
    searchPanel:Hide()

    SyncMonitorControls = function()
        local running = ns.EventMonitor.IsRunning()
        ui.SetButtonText(monitorButton, running and L.STOP_MONITORING or L.START_MONITORING)
        ui.SetButtonVariant(monitorButton, running and "danger" or "primary")
        ui.SetButtonEnabled(monitorButton, running or selection:GetCount() > 0)
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
        if row.resultIndex == highlightedSearchIndex then
            row:SetBackdropColor(ui.accentR, ui.accentG, ui.accentB, 0.14)
            row:SetBackdropBorderColor(ui.accentR, ui.accentG, ui.accentB, 0.32)
            row.name:SetTextColor(1, 1, 1, 0.96)
        elseif row.isHovered then
            row:SetBackdropColor(ui.surfaceR, ui.surfaceG, ui.surfaceB, 0.94)
            ui.SetBorderColor(row, false, 0.42)
            row.name:SetTextColor(1, 1, 1, 0.9)
        else
            row:SetBackdropColor(ui.surfaceR, ui.surfaceG, ui.surfaceB, 0.48)
            ui.SetBorderColor(row, false, 0.18)
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
            SetStatus(errorMessage or L.COULD_NOT_SELECT_EVENT, ui.accentR, ui.accentG, ui.accentB)
        end
    end

    local function CreateSearchRow(index)
        local row = CreateFrame("Button", nil, searchPanel, "BackdropTemplate")
        row:SetPoint("TOPLEFT", 3, -3 - ((index - 1) * SEARCH_ROW_HEIGHT))
        row:SetPoint("TOPRIGHT", -3, -3 - ((index - 1) * SEARCH_ROW_HEIGHT))
        row:SetHeight(SEARCH_ROW_HEIGHT - 2)
        row:SetBackdrop(ui.backdrop)
        row.resultIndex = index

        local name = row:CreateFontString(nil, "OVERLAY", "GameFontHighlightSmall")
        name:SetPoint("TOPLEFT", 9, -5)
        name:SetPoint("RIGHT", -9, 0)
        name:SetJustifyH("LEFT")
        name:SetWordWrap(false)
        row.name = name

        local payload = row:CreateFontString(nil, "OVERLAY", "GameFontDisableSmall")
        payload:SetPoint("BOTTOMLEFT", 9, 5)
        payload:SetPoint("RIGHT", -9, 0)
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
            row.name:SetText(eventName)
            row.payload:SetText(FormatCatalogPayload(eventName, signature))
            ApplySearchRowStyle(row)
            row:Show()
        end

        searchPanel:SetHeight(math.max(1, #searchResults * SEARCH_ROW_HEIGHT + 6))
        searchPanel:SetShown(#searchResults > 0 and inputPanel.editBox:HasFocus())
    end

    local function ApplySelectedRowStyle(row)
        if row.isHovered then
            row:SetBackdropColor(ui.surfaceR, ui.surfaceG, ui.surfaceB, 0.88)
            ui.SetBorderColor(row, false, 0.36)
        else
            row:SetBackdropColor(ui.surfaceR, ui.surfaceG, ui.surfaceB, 0.42)
            ui.SetBorderColor(row, false, 0.16)
        end
    end

    local function CreateSelectedRow(index)
        local row = CreateFrame("Frame", nil, selectedContent, "BackdropTemplate")
        row:SetSize(ui.windowWidth - 56, SELECTED_ROW_HEIGHT - 2)
        row:SetBackdrop(ui.backdrop)
        row:EnableMouse(true)

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

        local removeButton = CreateRemoveButton(row, ui)
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
        if issecretvalue and issecretvalue(scrollOffset) then
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
            row.name:SetText(eventName)
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
        ui.SetBorderColor(inputPanel, true, 0.75)
        RefreshSearch()
    end)
    inputPanel.editBox:SetScript("OnEditFocusLost", function()
        ui.SetBorderColor(inputPanel, false)
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
            string.format(L.EVENT_DETAIL, record.event),
            string.format(L.ELAPSED_DETAIL, record.elapsed),
            "",
        }
        if #record.arguments == 0 then
            lines[#lines + 1] = L.EVENT_HAS_NO_PAYLOAD
        else
            for index = 1, #record.arguments do
                lines[#lines + 1] = "[" .. index .. "] = " .. record.arguments[index]
            end
        end
        return table.concat(lines, "\n")
    end

    local function ApplyEventRowStyle(row)
        if row.record == selectedRecord then
            row:SetBackdropColor(ui.accentR, ui.accentG, ui.accentB, 0.13)
            row:SetBackdropBorderColor(ui.accentR, ui.accentG, ui.accentB, 0.35)
        elseif row.isHovered then
            row:SetBackdropColor(ui.surfaceR, ui.surfaceG, ui.surfaceB, 0.9)
            ui.SetBorderColor(row, false, 0.45)
        else
            row:SetBackdropColor(ui.surfaceR, ui.surfaceG, ui.surfaceB, 0.44)
            ui.SetBorderColor(row, false, 0.18)
        end
    end

    local function CreateEventRow(index)
        local row = CreateFrame("Button", nil, logContent, "BackdropTemplate")
        row:SetSize(EVENT_LIST_WIDTH - 34, EVENT_ROW_HEIGHT - 2)
        row:SetBackdrop(ui.backdrop)

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
            SetReadOnlyText(detailPanel, FormatDetails(selectedRecord))
            page:Refresh()
        end)

        rows[index] = row
        return row
    end

    function page:Refresh()
        dirty = nil
        local recordCount = ns.EventMonitor.GetCount()
        ui.SetButtonEnabled(clearButton, recordCount > 0)
        ui.SetButtonEnabled(exportDetail, recordCount > 0)
        for index = 1, #rows do
            rows[index]:Hide()
        end

        local scrollOffset = logScroll:GetVerticalScroll()
        if issecretvalue and issecretvalue(scrollOffset) then
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
            row.timeLabel:SetText(string.format("+%.2fs", record.elapsed))
            row.eventLabel:SetText(record.event)
            row.summary:SetText(record.summary ~= "" and record.summary or L.NO_PAYLOAD)
            ApplyEventRowStyle(row)
            row:Show()
        end

        logContent:SetHeight(math.max(1, recordCount * EVENT_ROW_HEIGHT))
        logScroll:UpdateScrollChildRect()
        empty:SetShown(recordCount == 0)
        if selectedRecord then
            SetReadOnlyText(detailPanel, FormatDetails(selectedRecord))
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

    monitorButton:SetScript("OnClick", function()
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
            SetStatus(errorMessage or L.COULD_NOT_START, ui.accentR, ui.accentG, ui.accentB)
        end
        SyncMonitorControls()
    end)

    clearButton:SetScript("OnClick", function()
        ns.EventMonitor.Clear()
        selectedRecord = nil
        SetReadOnlyText(detailPanel, L.SELECT_EVENT_DETAIL)
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
