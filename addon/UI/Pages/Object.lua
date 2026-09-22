local ADDON_NAME, ns = ...
local L = ns.L

-- Objects workbench page: bounded path inspection over _G with incremental
-- text streaming, a live value tree, ranked field search and the mouse picker.
-- Everything here is read-only toward inspected and Blizzard-owned objects.
local ROW_HEIGHT = 38
local VISIBLE_ROWS = 16
local LIST_WIDTH = 420
local TEXT_CHUNK_BYTES = 44000
local TEXT_LOAD_THRESHOLD = 80
local NODE_POPUP_WIDTH = 760
local NODE_POPUP_HEIGHT = 520

-- The single built page instance; only read by the shell export hook.
local builtPage

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

-- Bounded incremental text: one 44 KB chunk is appended whenever the viewport
-- scrolls within 80 px of the bottom. Close/teardown releases the stream.
local function BindIncrementalText(panel, textStream, onProgress)
    local W = ns.Widgets
    panel.serializationStream = textStream
    panel.loadingSerialization = false

    local function LoadNextChunk()
        local stream = panel.serializationStream
        if panel.loadingSerialization or not stream or stream:IsFinished() then
            return
        end
        panel.loadingSerialization = true
        local chunk, finished = stream:ReadChunk(TEXT_CHUNK_BYTES)
        if chunk ~= "" then
            W.AppendReadOnlyText(panel, chunk)
        end
        panel.loadingSerialization = false
        if onProgress then
            onProgress(#panel.editBox.savedText, finished, stream:WasLimited())
        end
    end

    panel.scroll.onVerticalScrollChanged = function(offset)
        local range = panel.scroll.verticalRange or 0
        if range - (tonumber(offset) or 0) <= TEXT_LOAD_THRESHOLD then
            LoadNextChunk()
        end
    end
    panel.LoadNextChunk = LoadNextChunk
end

local function BuildObjectPage(parent)
    local W = ns.Widgets
    local colors = W.Colors
    local layout = ns.Workbench.Layout
    local accent = colors.accent

    local page = CreateFrame("Frame", nil, parent)
    page:SetAllPoints(parent)

    local mouseButton = W.CreateButton(page, 118, L.CAPTURE_MOUSE)
    mouseButton:SetPoint("TOPRIGHT", -14, -84)
    local searchButton = W.CreateButton(page, 82, L.SEARCH)
    searchButton:SetPoint("RIGHT", mouseButton, "LEFT", -8, 0)
    local inspectButton = W.CreateButton(page, 82, L.INSPECT, "primary")
    inspectButton:SetPoint("RIGHT", searchButton, "LEFT", -8, 0)

    local pathPanel = CreateLineInput(page)
    pathPanel:SetPoint("TOPLEFT", 14, -84)
    pathPanel:SetPoint("TOPRIGHT", inspectButton, "TOPLEFT", -8, 0)
    pathPanel:SetHeight(30)
    pathPanel.editBox:SetText("_G")

    local inputHint = pathPanel:CreateFontString(nil, "OVERLAY", "GameFontDisableSmall")
    inputHint:SetPoint("LEFT", 10, 0)
    inputHint:SetText(L.OBJECT_INPUT_HINT)
    inputHint:SetTextColor(1, 1, 1, 0.3)
    inputHint:Hide()

    local statusDot = page:CreateTexture(nil, "ARTWORK")
    statusDot:SetSize(5, 5)
    statusDot:SetPoint("TOPLEFT", 17, -135)
    local status = page:CreateFontString(nil, "OVERLAY", "GameFontHighlightSmall")
    status:SetPoint("LEFT", statusDot, "RIGHT", 7, 0)
    status:SetPoint("RIGHT", -14, 0)
    status:SetJustifyH("LEFT")
    local function SetStatus(text, r, g, b)
        status:SetText(text)
        status:SetTextColor(r, g, b, 0.92)
        statusDot:SetColorTexture(r, g, b, 0.92)
    end

    local listLabel = W.CreateSectionLabel(page, L.SEARCH_RESULTS)
    listLabel:SetPoint("TOPLEFT", 17, -161)
    local detailLabel = W.CreateSectionLabel(page, L.OBJECT_SNAPSHOT)
    detailLabel:SetPoint("TOPLEFT", LIST_WIDTH + 29, -161)

    local treeHint = page:CreateFontString(nil, "OVERLAY", "GameFontDisableSmall")
    treeHint:SetPoint("LEFT", detailLabel, "RIGHT", 10, 0)
    treeHint:SetText(L.TREE_CONTEXT_HINT)
    treeHint:SetTextColor(1, 1, 1, 0.32)

    local listPanel = W.CreatePanel(page, colors.editor[1], colors.editor[2], colors.editor[3], 0.78)
    listPanel:SetPoint("TOPLEFT", 14, -181)
    listPanel:SetPoint("BOTTOMLEFT", 14, 54)
    listPanel:SetWidth(LIST_WIDTH)
    local listScroll = W.CreateScrollArea(listPanel, 8, 8, 7, 8)
    local listContent = CreateFrame("Frame", nil, listScroll)
    listContent:SetWidth(LIST_WIDTH - 26)
    listContent:SetHeight(1)
    listScroll:SetScrollChild(listContent)
    local empty = listPanel:CreateFontString(nil, "OVERLAY", "GameFontDisableSmall")
    empty:SetPoint("TOP", 0, -24)
    empty:SetText(L.NO_SEARCH_RESULTS)
    empty:SetTextColor(1, 1, 1, 0.32)

    local treeView = ns.TreeView.Create(page, {
        contentWidth = layout.WINDOW_WIDTH - LIST_WIDTH - 54,
    })
    treeView.panel:SetPoint("TOPLEFT", LIST_WIDTH + 26, -181)
    treeView.panel:SetPoint("BOTTOMRIGHT", -14, 54)

    local textView = W.CreateTextArea(page, true)
    textView:SetPoint("TOPLEFT", LIST_WIDTH + 26, -181)
    textView:SetPoint("BOTTOMRIGHT", -14, 54)
    textView.editBox:SetWidth(layout.WINDOW_WIDTH - LIST_WIDTH - 78)
    textView.editBox:SetScript("OnMouseUp", function(self)
        self:SetFocus()
    end)

    local textModeButton = W.CreateNavTab(page, L.TEXT)
    W.FitNavTab(textModeButton, 58, 28)
    textModeButton:SetPoint("TOPRIGHT", -14, -150)
    local treeModeButton = W.CreateNavTab(page, L.TREE)
    W.FitNavTab(treeModeButton, 58, 28)
    treeModeButton:SetPoint("RIGHT", textModeButton, "LEFT", -2, 0)
    local snapshotMode = "tree"
    local function SetSnapshotMode(mode)
        snapshotMode = mode == "text" and "text" or "tree"
        treeView.panel:SetShown(snapshotMode == "tree")
        textView:SetShown(snapshotMode == "text")
        treeModeButton:SetActive(snapshotMode == "tree")
        textModeButton:SetActive(snapshotMode == "text")
        treeHint:SetShown(snapshotMode == "tree")
    end
    treeModeButton:SetScript("OnClick", function() SetSnapshotMode("tree") end)
    textModeButton:SetScript("OnClick", function() SetSnapshotMode("text") end)

    local selectSnapshot = W.CreateButton(page, 132, L.SELECT_LOADED_TEXT, false)
    selectSnapshot:SetPoint("BOTTOMRIGHT", -14, 14)
    selectSnapshot:SetScript("OnClick", function()
        SetSnapshotMode("text")
        W.SelectAllText(textView)
    end)

    local exportSnapshot = W.CreateButton(page, 92, L.SAVE_TO_DISK, false)
    exportSnapshot:SetPoint("RIGHT", selectSnapshot, "LEFT", -8, 0)
    page.inspectButton = inspectButton
    page.searchButton = searchButton
    page.mouseButton = mouseButton
    page.selectSnapshot = selectSnapshot
    page.exportSnapshot = exportSnapshot
    page.treeView = treeView
    page.textView = textView
    page.pathPanel = pathPanel
    page.listPanel = listPanel

    local pickerDock
    local pickerPrompt
    local pickerActive = false
    local StopPicker
    local ShowInspection
    local currentValue
    local currentLabel
    local searchRootLabel
    local nodePopup

    local function GetNodePath(node)
        local labels = {}
        while node and node.parent do
            labels[#labels + 1] = node.label or "?"
            node = node.parent
        end
        local path = currentLabel or "?"
        for index = #labels, 1, -1 do
            path = path .. " / " .. labels[index]
        end
        return path
    end

    local function EnsureNodePopup()
        if nodePopup then
            return nodePopup
        end

        local overlay = W.CreatePanel(page, 0, 0, 0, 0.76)
        overlay:SetAllPoints(page)
        overlay:SetFrameLevel(page:GetFrameLevel() + 40)
        overlay:EnableMouse(true)

        local panel = W.CreatePanel(overlay, colors.surface[1], colors.surface[2], colors.surface[3], 1)
        panel:SetSize(NODE_POPUP_WIDTH, NODE_POPUP_HEIGHT)
        panel:SetPoint("CENTER")
        panel:SetFrameLevel(overlay:GetFrameLevel() + 1)

        local title = panel:CreateFontString(nil, "OVERLAY", "GameFontNormalLarge")
        title:SetPoint("TOPLEFT", 18, -17)
        title:SetText(L.NODE_TEXT_TITLE)

        local path = panel:CreateFontString(nil, "OVERLAY", "GameFontDisableSmall")
        path:SetPoint("TOPLEFT", title, "BOTTOMLEFT", 0, -8)
        path:SetPoint("RIGHT", -18, 0)
        path:SetJustifyH("LEFT")
        path:SetWordWrap(false)
        path:SetTextColor(1, 1, 1, 0.42)

        local textPanel = W.CreateTextArea(panel, true)
        textPanel:SetPoint("TOPLEFT", 14, -66)
        textPanel:SetPoint("BOTTOMRIGHT", -14, 56)
        textPanel.editBox:SetWidth(NODE_POPUP_WIDTH - 48)

        local progress = panel:CreateFontString(nil, "OVERLAY", "GameFontDisableSmall")
        progress:SetPoint("BOTTOMLEFT", 18, 21)
        progress:SetTextColor(1, 1, 1, 0.36)

        local closeButton = W.CreateButton(panel, 82, L.CLOSE, false)
        closeButton:SetPoint("BOTTOMRIGHT", -14, 14)
        closeButton:SetScript("OnClick", function()
            overlay:Hide()
            -- Closing releases the incremental stream immediately.
            BindIncrementalText(textPanel, nil)
        end)

        local selectButton = W.CreateButton(panel, 118, L.SELECT_LOADED_TEXT, false)
        selectButton:SetPoint("RIGHT", closeButton, "LEFT", -8, 0)
        selectButton:SetScript("OnClick", function()
            W.SelectAllText(textPanel)
        end)

        local exportButton = W.CreateButton(panel, 92, L.SAVE_TO_DISK, false)
        exportButton:SetPoint("RIGHT", selectButton, "LEFT", -8, 0)
        exportButton:SetScript("OnClick", function()
            if ns.Safety.IsCombatBlocked() then
                ns.Safety.PrintBlocked()
                return
            end
            local nodePath = path:GetText()
            ns.Workbench.SaveToDisk("object_node", nodePath, function()
                local text = ns.Serializer.SerializeForExport(nodePopup.source)
                return text
            end, {
                path = nodePath,
                valueType = type(nodePopup.source),
            })
        end)

        nodePopup = {
            overlay = overlay,
            path = path,
            progress = progress,
            textPanel = textPanel,
            closeButton = closeButton,
            exportButton = exportButton,
        }
        page.nodePopup = nodePopup
        overlay:Hide()
        return nodePopup
    end

    local function OpenNodePopup(node)
        if ns.Safety.IsCombatBlocked() then
            ns.Safety.PrintBlocked()
            return
        end
        if not node or not node.exportable then
            return
        end
        local popup = EnsureNodePopup()
        local nodePath = GetNodePath(node)
        popup.source = node.source
        local stream = ns.Serializer.CreateStream(node.source)
        local serialized, finished = stream:ReadChunk(TEXT_CHUNK_BYTES)
        popup.path:SetText(nodePath)
        W.SetReadOnlyText(popup.textPanel,
            string.format(L.OBJECT_TEXT_HEADER, nodePath, type(node.source)) .. "\n" .. serialized)
        local function UpdateProgress(bytes, isFinished, limited)
            popup.progress:SetText(string.format(isFinished and L.TEXT_LOADED_COMPLETE or L.TEXT_LOADED_MORE,
                math.floor(bytes / 1024 + 0.5)))
            if isFinished and limited then
                popup.progress:SetText(L.TEXT_LIMIT_REACHED)
            end
        end
        BindIncrementalText(popup.textPanel, stream, UpdateProgress)
        UpdateProgress(#popup.textPanel.editBox.savedText, finished, stream:WasLimited())
        popup.overlay:Show()
    end

    treeView:SetOnNodeContext(OpenNodePopup)

    exportSnapshot:SetScript("OnClick", function()
        if ns.Safety.IsCombatBlocked() then
            ns.Safety.PrintBlocked()
            return
        end
        local kind, title, content, metadata = page.GetSnapshotPayload()
        if not kind then
            return
        end
        ns.Workbench.SaveToDisk(kind, title, content, metadata)
    end)

    local function CompletePicker()
        local succeeded, inspection, errorMessage = ns.ObjectInspector.CaptureMouseFocus()
        StopPicker(false)
        if ns.Safety.IsCombatBlocked() then
            return
        end
        ns.Workbench.Open()
        if succeeded then
            pathPanel.editBox:SetText("")
            ShowInspection(inspection)
        else
            SetStatus(errorMessage or L.NO_MOUSE_FOCUS, accent[1], accent[2], accent[3])
        end
    end

    local function EnsurePickerDock()
        if pickerDock then
            return
        end

        pickerDock = CreateFrame("Button", "LycheeToolkitPickerDock", UIParent, "BackdropTemplate")
        pickerDock:SetSize(48, 48)
        pickerDock:SetPoint("TOP", UIParent, "TOP", 0, -36)
        pickerDock:SetFrameStrata("TOOLTIP")
        pickerDock:SetClampedToScreen(true)

        local logo = pickerDock:CreateTexture(nil, "ARTWORK")
        logo:SetTexture(W.LOGO_TEXTURE)
        logo:SetTexCoord(0.18, 0.79, 0.17, 0.80)
        logo:SetPoint("TOPLEFT", 5, -5)
        logo:SetPoint("BOTTOMRIGHT", -5, 5)

        pickerPrompt = W.CreatePanel(pickerDock, colors.editor[1], colors.editor[2], colors.editor[3], 0.98)
        pickerPrompt:SetPoint("TOP", pickerDock, "BOTTOM", 0, -7)
        pickerPrompt:SetSize(220, 28)
        pickerPrompt:SetFrameLevel(pickerDock:GetFrameLevel() + 1)
        local promptText = pickerPrompt:CreateFontString(nil, "OVERLAY", "GameFontHighlightSmall")
        promptText:SetPoint("CENTER")
        promptText:SetText(L.PICKER_PROMPT)
        promptText:SetTextColor(1, 1, 1, 0.86)
        pickerPrompt:Hide()

        pickerDock:SetScript("OnClick", function()
            StopPicker(true, L.PICKER_CANCELLED)
        end)
        pickerDock:Hide()
    end

    StopPicker = function(restoreWindow, statusText)
        pickerActive = false
        if pickerDock then
            pickerDock:SetScript("OnKeyDown", nil)
            if not ns.Safety.IsCombatBlocked() then
                pickerDock:EnableKeyboard(false)
            end
            pickerDock:Hide()
            pickerPrompt:Hide()
        end
        if restoreWindow and not ns.Safety.IsCombatBlocked() then
            ns.Workbench.Open()
            SetStatus(statusText or L.PICKER_CANCELLED, 0.55, 0.60, 0.65)
        end
    end

    local function StartPicker()
        if ns.Safety.IsCombatBlocked() then
            SetStatus(L.COMBAT_BLOCKED, accent[1], accent[2], accent[3])
            return
        end
        EnsurePickerDock()
        SetStatus(L.PICKER_ACTIVE, 0.42, 0.76, 0.43)
        -- Hiding the window runs the shared teardown first (see the picker
        -- restart below); the picker itself is set up afterwards so it
        -- survives that teardown exactly like the legacy picker did.
        ns.Workbench.Close()
        pickerActive = true
        pickerDock:EnableKeyboard(true)
        pickerDock:SetScript("OnKeyDown", function(self, key)
            if key == "F" or key == "ENTER" then
                self:SetPropagateKeyboardInput(false)
                CompletePicker()
            elseif key == "ESCAPE" then
                self:SetPropagateKeyboardInput(false)
                StopPicker(true, L.PICKER_CANCELLED)
            else
                self:SetPropagateKeyboardInput(true)
            end
        end)
        pickerDock:Show()
        pickerPrompt:Show()
        print("|cffd83b4eLychee Dev:|r " .. L.PICKER_HELP)
    end

    local results = {}
    local rows = {}
    page.resultRows = rows
    ShowInspection = function(inspection)
        currentValue = inspection.value
        currentLabel = inspection.label
        treeView:SetTree(inspection.tree)
        W.SetReadOnlyText(textView, inspection.text)
        if inspection.textStream then
            BindIncrementalText(textView, inspection.textStream, function(bytes, finished, limited)
                if finished and limited then
                    SetStatus(L.TEXT_LIMIT_REACHED, accent[1], accent[2], accent[3])
                elseif finished then
                    SetStatus(string.format(L.OBJECT_READY, inspection.label, inspection.valueType),
                        0.42, 0.76, 0.43)
                else
                    SetStatus(string.format(L.OBJECT_LOADING_PROGRESS, inspection.label,
                        inspection.valueType, math.floor(bytes / 1024 + 0.5)), 0.42, 0.76, 0.43)
                end
            end)
        else
            BindIncrementalText(textView, nil)
        end
        SetSnapshotMode("tree")
        W.SetButtonEnabled(selectSnapshot, true)
        W.SetButtonEnabled(exportSnapshot, true)
        local readyText = L.OBJECT_READY
        if inspection.textStream and not inspection.textStream:IsFinished() then
            readyText = L.OBJECT_READY_MORE
        end
        SetStatus(string.format(readyText, inspection.label, inspection.valueType), 0.42, 0.76, 0.43)
    end

    local function InspectPath(path)
        local succeeded, inspection, errorMessage = ns.ObjectInspector.InspectPath(path)
        if succeeded then
            pathPanel.editBox:SetText(path)
            ShowInspection(inspection)
        else
            SetStatus(errorMessage or L.OBJECT_NOT_FOUND, accent[1], accent[2], accent[3])
        end
    end

    local function ResultPath(root, result)
        local path = result.path or result.key
        if path:sub(1, 1) == "[" then
            return root .. path
        end
        return root .. "." .. path
    end

    local function CreateRow(index)
        local row = W.CreateListRow(listContent, ROW_HEIGHT)
        row:SetWidth(LIST_WIDTH - 26)
        local name = row:CreateFontString(nil, "OVERLAY", "GameFontHighlightSmall")
        name:SetPoint("TOPLEFT", 9, -6)
        name:SetPoint("TOPRIGHT", -9, -6)
        name:SetJustifyH("LEFT")
        name:SetWordWrap(false)
        row.name = name
        local preview = row:CreateFontString(nil, "OVERLAY", "GameFontDisableSmall")
        preview:SetPoint("BOTTOMLEFT", 9, 5)
        preview:SetPoint("BOTTOMRIGHT", -9, 5)
        preview:SetJustifyH("LEFT")
        preview:SetWordWrap(false)
        preview:SetTextColor(1, 1, 1, 0.38)
        row.preview = preview
        row:SetScript("OnEnter", function(self)
            self.isHovered = true
            W.SetListRowState(self, false, true)
        end)
        row:SetScript("OnLeave", function(self)
            self.isHovered = nil
            W.SetListRowState(self, false, false)
        end)
        row:SetScript("OnClick", function(self)
            if ns.Safety.IsCombatBlocked() then
                ns.Safety.PrintBlocked()
                return
            end
            if not self.result then
                return
            end
            local label = ResultPath(searchRootLabel or currentLabel or "_G", self.result)
            local succeeded, inspection, errorMessage = ns.ObjectInspector.InspectValue(self.result.value, label)
            if succeeded then
                pathPanel.editBox:SetText("")
                ShowInspection(inspection)
            else
                SetStatus(errorMessage or L.OBJECT_NOT_FOUND, accent[1], accent[2], accent[3])
            end
        end)
        rows[index] = row
        return row
    end

    local function RefreshResults()
        for index = 1, #rows do
            rows[index]:Hide()
        end
        local offset = listScroll:GetVerticalScroll()
        if issecretvalue and issecretvalue(offset) then
            offset = 0
        end
        local first = math.floor((offset or 0) / ROW_HEIGHT) + 1
        local last = math.min(#results, first + VISIBLE_ROWS - 1)
        local pool = 0
        for resultIndex = first, last do
            pool = pool + 1
            local result = results[resultIndex]
            local row = rows[pool] or CreateRow(pool)
            row.result = result
            row:ClearAllPoints()
            row:SetPoint("TOPLEFT", 0, -((resultIndex - 1) * ROW_HEIGHT))
            row.name:SetText(result.path or result.key)
            row.preview:SetText(result.valueType .. "  " .. result.preview)
            W.SetListRowState(row, false, row.isHovered)
            row:Show()
        end
        listContent:SetHeight(math.max(1, #results * ROW_HEIGHT))
        listScroll:UpdateScrollChildRect()
        empty:SetShown(#results == 0)
    end
    listScroll.onVerticalScrollChanged = RefreshResults

    inspectButton:SetScript("OnClick", function()
        if ns.Safety.IsCombatBlocked() then
            SetStatus(L.COMBAT_BLOCKED, accent[1], accent[2], accent[3])
            return
        end
        InspectPath(pathPanel.editBox:GetText())
    end)
    mouseButton:SetScript("OnClick", StartPicker)

    local function Search()
        if ns.Safety.IsCombatBlocked() then
            SetStatus(L.COMBAT_BLOCKED, accent[1], accent[2], accent[3])
            return
        end
        local query = pathPanel.editBox:GetText()
        local succeeded, searchResult, errorMessage
        local searchedGlobal = currentValue == nil
        if searchedGlobal then
            succeeded, searchResult, errorMessage = ns.ObjectInspector.SearchGlobal(query)
        else
            succeeded, searchResult, errorMessage = ns.ObjectInspector.SearchValue(currentValue, query)
            if succeeded and searchResult.totalMatches == 0 then
                succeeded, searchResult, errorMessage = ns.ObjectInspector.SearchGlobal(query)
                searchedGlobal = succeeded
            end
        end
        if succeeded then
            results = searchResult.results
            searchRootLabel = searchedGlobal and "_G" or currentLabel
            RefreshResults()
            local message = searchedGlobal
                and string.format(L.GLOBAL_MATCH_COUNT, searchResult.totalMatches)
                or string.format(L.MATCH_COUNT, searchResult.totalMatches)
            SetStatus(message, 0.55, 0.60, 0.65)
        else
            SetStatus(errorMessage, accent[1], accent[2], accent[3])
        end
    end
    searchButton:SetScript("OnClick", Search)
    pathPanel.editBox:SetScript("OnEnterPressed", function(self)
        if currentValue ~= nil and self:GetText() ~= "_G" then
            Search()
        else
            InspectPath(self:GetText())
        end
    end)
    pathPanel.editBox:SetScript("OnTextChanged", function(self)
        inputHint:SetShown(self:GetText() == "" and not self:HasFocus())
    end)
    pathPanel.editBox:SetScript("OnEditFocusGained", function()
        inputHint:Hide()
        W.SetBorderColor(pathPanel, true, 0.75)
    end)
    pathPanel.editBox:SetScript("OnEditFocusLost", function(self)
        inputHint:SetShown(self:GetText() == "")
        W.SetBorderColor(pathPanel, false)
    end)

    function page:GetSelection()
        return currentValue, currentLabel
    end

    -- One payload builder shared by the page Save button and the shell export
    -- hook; both funnel into the single export UI. The serialized payload is
    -- the full captured value, not the visible text.
    function page.GetSnapshotPayload()
        if currentValue == nil then
            return nil
        end
        local value, label = currentValue, currentLabel or ""
        return "object_snapshot", currentLabel or L.OBJECT_SNAPSHOT, function()
            local text = ns.Serializer.SerializeForExport(value)
            return text
        end, { path = label, valueType = type(value) }
    end

    function page.HandleActivate()
        if pickerActive then
            StopPicker(false)
        end
        pathPanel.editBox:SetFocus()
    end

    function page.HandleShutdown()
        StopPicker(false)
        BindIncrementalText(textView, nil)
        if nodePopup then
            nodePopup.overlay:Hide()
            BindIncrementalText(nodePopup.textPanel, nil)
        end
    end

    SetStatus(L.READY, 0.55, 0.60, 0.65)
    treeView:SetTree(nil)
    W.SetReadOnlyText(textView, "")
    W.SetButtonEnabled(selectSnapshot, false)
    W.SetButtonEnabled(exportSnapshot, false)
    SetSnapshotMode("tree")
    RefreshResults()
    builtPage = page
    return page
end

ns.Workbench.RegisterPage({
    key = "objects",
    titleKey = "TAB_OBJECTS",
    build = BuildObjectPage,
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

-- Shell-level Save provider: the captured snapshot. The node text popup owns
-- its own object_node Save. Every Save funnels through the one export UI.
ns.Workbench.RegisterExportHook({
    key = "objects",
    GetPayload = function()
        if not builtPage then
            return nil
        end
        return builtPage.GetSnapshotPayload()
    end,
})
