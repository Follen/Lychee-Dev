local ADDON_NAME, ns = ...
local L = ns.L

local ROW_HEIGHT = 38
local VISIBLE_ROWS = 16
local LIST_WIDTH = 420

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

function ns.CreateObjectPage(parent, ui)
    local page = CreateFrame("Frame", nil, parent)
    page:SetAllPoints(parent)

    local title = ui.CreateSectionLabel(page, L.OBJECT_INSPECTOR)
    title:SetPoint("TOPLEFT", 17, -84)

    local mouseButton = ui.CreateButton(page, 118, L.CAPTURE_MOUSE)
    mouseButton:SetPoint("TOPRIGHT", -14, -104)
    local searchButton = ui.CreateButton(page, 82, L.SEARCH)
    searchButton:SetPoint("RIGHT", mouseButton, "LEFT", -8, 0)
    local inspectButton = ui.CreateButton(page, 82, L.INSPECT)
    inspectButton:SetPoint("RIGHT", searchButton, "LEFT", -8, 0)

    local pathPanel = CreateLineInput(page, ui)
    pathPanel:SetPoint("TOPLEFT", 14, -104)
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
    statusDot:SetPoint("TOPLEFT", 17, -155)
    local status = page:CreateFontString(nil, "OVERLAY", "GameFontHighlightSmall")
    status:SetPoint("LEFT", statusDot, "RIGHT", 7, 0)
    status:SetPoint("RIGHT", -14, 0)
    status:SetJustifyH("LEFT")
    local function SetStatus(text, r, g, b)
        status:SetText(text)
        status:SetTextColor(r, g, b, 0.92)
        statusDot:SetColorTexture(r, g, b, 0.92)
    end

    local listLabel = ui.CreateSectionLabel(page, L.SEARCH_RESULTS)
    listLabel:SetPoint("TOPLEFT", 17, -181)
    local detailLabel = ui.CreateSectionLabel(page, L.OBJECT_SNAPSHOT)
    detailLabel:SetPoint("TOPLEFT", LIST_WIDTH + 29, -181)

    local listPanel = ui.CreatePanel(page, ui.editorR, ui.editorG, ui.editorB, 0.9)
    listPanel:SetPoint("TOPLEFT", 14, -201)
    listPanel:SetPoint("BOTTOMLEFT", 14, 54)
    listPanel:SetWidth(LIST_WIDTH)
    local listScroll = ui.CreateScrollArea(listPanel, 8, 8, 7, 8)
    local listContent = CreateFrame("Frame", nil, listScroll)
    listContent:SetWidth(LIST_WIDTH - 34)
    listContent:SetHeight(1)
    listScroll:SetScrollChild(listContent)
    local empty = listPanel:CreateFontString(nil, "OVERLAY", "GameFontDisableSmall")
    empty:SetPoint("TOP", 0, -24)
    empty:SetText(L.NO_SEARCH_RESULTS)
    empty:SetTextColor(1, 1, 1, 0.32)

    local treeView = ns.CreateTreeView(page, {
        CreatePanel = ui.CreatePanel,
        CreateScrollArea = ui.CreateScrollArea,
        editorR = ui.editorR, editorG = ui.editorG, editorB = ui.editorB,
        surfaceR = ui.surfaceR, surfaceG = ui.surfaceG, surfaceB = ui.surfaceB,
        accentR = ui.accentR, accentG = ui.accentG, accentB = ui.accentB,
        contentWidth = ui.windowWidth - LIST_WIDTH - 54,
    })
    treeView.panel:SetPoint("TOPLEFT", LIST_WIDTH + 26, -201)
    treeView.panel:SetPoint("BOTTOMRIGHT", -14, 54)

    local textView = ui.CreateTextArea(page, true)
    textView:SetPoint("TOPLEFT", LIST_WIDTH + 26, -201)
    textView:SetPoint("BOTTOMRIGHT", -14, 54)
    textView.editBox:SetWidth(ui.windowWidth - LIST_WIDTH - 78)
    textView.editBox:SetScript("OnMouseUp", function(self) self:SetFocus() end)

    local textModeButton = ui.CreateViewTab(page, L.TEXT)
    textModeButton:SetSize(58, 28)
    textModeButton:SetPoint("TOPRIGHT", -14, -170)
    local treeModeButton = ui.CreateViewTab(page, L.TREE)
    treeModeButton:SetSize(58, 28)
    treeModeButton:SetPoint("RIGHT", textModeButton, "LEFT", -2, 0)
    local snapshotMode = "tree"
    local function SetSnapshotMode(mode)
        snapshotMode = mode == "text" and "text" or "tree"
        treeView.panel:SetShown(snapshotMode == "tree")
        textView:SetShown(snapshotMode == "text")
        treeModeButton:SetActive(snapshotMode == "tree")
        textModeButton:SetActive(snapshotMode == "text")
    end
    treeModeButton:SetScript("OnClick", function() SetSnapshotMode("tree") end)
    textModeButton:SetScript("OnClick", function() SetSnapshotMode("text") end)

    local selectSnapshot = ui.CreateButton(page, 118, L.SELECT_SNAPSHOT, false)
    selectSnapshot:SetPoint("BOTTOMRIGHT", -14, 14)
    selectSnapshot:SetScript("OnClick", function()
        SetSnapshotMode("text")
        textView:SelectAll()
    end)

    local pickerDock
    local pickerTooltip
    local pickerPrompt
    local pickerActive = false
    local pickerElapsed = 0
    local StopPicker
    local ShowInspection
    local currentValue
    local currentLabel
    local searchRootLabel

    local function CompletePicker()
        local succeeded, inspection, errorMessage = ns.ObjectInspector.CaptureMouseFocus()
        StopPicker(false)
        if ns.IsCombatBlocked() then
            return
        end
        parent:Show()
        if succeeded then
            pathPanel.editBox:SetText("")
            ShowInspection(inspection)
        else
            SetStatus(errorMessage or L.NO_MOUSE_FOCUS, ui.accentR, ui.accentG, ui.accentB)
        end
    end

    local function EnsurePickerDock()
        if pickerDock then
            return
        end

        pickerDock = CreateFrame("Button", "LycheeDevPickerDock", UIParent, "BackdropTemplate")
        pickerDock:SetSize(48, 48)
        pickerDock:SetPoint("TOP", UIParent, "TOP", 0, -36)
        pickerDock:SetFrameStrata("TOOLTIP")
        pickerDock:SetClampedToScreen(true)
        pickerDock:SetBackdrop(ui.backdrop)
        pickerDock:SetBackdropColor(ui.editorR, ui.editorG, ui.editorB, 0.96)
        pickerDock:SetBackdropBorderColor(ui.accentR, ui.accentG, ui.accentB, 0.9)

        local logo = pickerDock:CreateTexture(nil, "ARTWORK")
        logo:SetTexture("Interface\\AddOns\\" .. ADDON_NAME .. "\\Media\\Logo.png")
        logo:SetTexCoord(0.18, 0.79, 0.17, 0.80)
        logo:SetPoint("TOPLEFT", 5, -5)
        logo:SetPoint("BOTTOMRIGHT", -5, 5)

        pickerTooltip = ui.CreatePanel(pickerDock, ui.editorR, ui.editorG, ui.editorB, 0.98)
        pickerTooltip:SetPoint("TOP", pickerDock, "BOTTOM", 0, -8)
        pickerTooltip:SetSize(310, 58)
        pickerTooltip:SetFrameLevel(pickerDock:GetFrameLevel() + 2)
        local target = pickerTooltip:CreateFontString(nil, "OVERLAY", "GameFontHighlightSmall")
        target:SetPoint("TOPLEFT", 10, -9)
        target:SetPoint("TOPRIGHT", -10, -9)
        target:SetJustifyH("CENTER")
        target:SetWordWrap(false)
        target:SetTextColor(1, 1, 1, 0.9)
        pickerTooltip.target = target
        local help = pickerTooltip:CreateFontString(nil, "OVERLAY", "GameFontDisableSmall")
        help:SetPoint("BOTTOMLEFT", 10, 9)
        help:SetPoint("BOTTOMRIGHT", -10, 9)
        help:SetJustifyH("CENTER")
        help:SetText(L.PICKER_HELP)
        help:SetTextColor(1, 1, 1, 0.46)
        pickerTooltip:Hide()

        pickerPrompt = ui.CreatePanel(pickerDock, ui.editorR, ui.editorG, ui.editorB, 0.98)
        pickerPrompt:SetPoint("TOP", pickerDock, "BOTTOM", 0, -7)
        pickerPrompt:SetSize(220, 28)
        pickerPrompt:SetFrameLevel(pickerDock:GetFrameLevel() + 1)
        local promptText = pickerPrompt:CreateFontString(nil, "OVERLAY", "GameFontHighlightSmall")
        promptText:SetPoint("CENTER")
        promptText:SetText(L.PICKER_PROMPT)
        promptText:SetTextColor(1, 1, 1, 0.86)
        pickerPrompt:Hide()

        pickerTooltip:ClearAllPoints()
        pickerTooltip:SetPoint("TOP", pickerPrompt, "BOTTOM", 0, -5)

        pickerDock:SetScript("OnEnter", function()
            pickerTooltip:Show()
        end)
        pickerDock:SetScript("OnLeave", function()
            pickerTooltip:Hide()
        end)
        pickerDock:SetScript("OnClick", function()
            StopPicker(true, L.PICKER_CANCELLED)
        end)
        pickerDock:Hide()
    end

    StopPicker = function(restoreWindow, statusText)
        pickerActive = false
        pickerElapsed = 0
        if pickerDock then
            pickerDock:SetScript("OnUpdate", nil)
            pickerDock:SetScript("OnKeyDown", nil)
            if not ns.IsCombatBlocked() then
                pickerDock:EnableKeyboard(false)
            end
            pickerDock:Hide()
            pickerTooltip:Hide()
            pickerPrompt:Hide()
        end
        if restoreWindow and not ns.IsCombatBlocked() then
            parent:Show()
            SetStatus(statusText or L.PICKER_CANCELLED, 0.55, 0.60, 0.65)
        end
    end

    local function StartPicker()
        if ns.IsCombatBlocked() then
            SetStatus(L.COMBAT_BLOCKED, ui.accentR, ui.accentG, ui.accentB)
            return
        end
        EnsurePickerDock()
        SetStatus(L.PICKER_ACTIVE, 0.42, 0.76, 0.43)
        parent:Hide()
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
        pickerDock:SetScript("OnUpdate", function(_, elapsed)
            pickerElapsed = pickerElapsed + elapsed
            if pickerElapsed < 0.05 then
                return
            end
            pickerElapsed = 0
            local succeeded, label, objectType = ns.ObjectInspector.GetMouseFocusLabel()
            if succeeded then
                pickerTooltip.target:SetText(string.format(L.PICKER_TARGET, label, objectType))
                pickerDock:SetBackdropBorderColor(ui.accentR, ui.accentG, ui.accentB, 0.95)
            else
                pickerTooltip.target:SetText(L.NO_MOUSE_FOCUS)
                pickerDock:SetBackdropBorderColor(0.46, 0.51, 0.56, 0.42)
            end
        end)
        pickerDock:Show()
        pickerPrompt:Show()
        print("|cffd83b4eLychee Dev:|r " .. L.PICKER_HELP)
    end

    local results = {}
    local rows = {}
    ShowInspection = function(inspection)
        currentValue = inspection.value
        currentLabel = inspection.label
        treeView:SetTree(inspection.tree)
        ui.SetReadOnlyText(textView, inspection.text)
        SetSnapshotMode("tree")
        SetStatus(string.format(L.OBJECT_READY, inspection.label, inspection.valueType), 0.42, 0.76, 0.43)
    end

    local function InspectPath(path)
        local succeeded, inspection, errorMessage = ns.ObjectInspector.InspectPath(path)
        if succeeded then
            pathPanel.editBox:SetText(path)
            ShowInspection(inspection)
        else
            SetStatus(errorMessage or L.OBJECT_NOT_FOUND, ui.accentR, ui.accentG, ui.accentB)
        end
    end

    local function ChildPath(root, key)
        if key:match("^[_%a][_%w]*$") then return root .. "." .. key end
        return root .. "[" .. string.format("%q", key) .. "]"
    end

    local function ResultPath(root, result)
        local path = result.path or result.key
        if path:sub(1, 1) == "[" then return root .. path end
        return root .. "." .. path
    end

    local function CreateRow(index)
        local row = CreateFrame("Button", nil, listContent, "BackdropTemplate")
        row:SetSize(LIST_WIDTH - 34, ROW_HEIGHT - 2)
        row:SetBackdrop(ui.backdrop)
        local name = row:CreateFontString(nil, "OVERLAY", "GameFontHighlightSmall")
        name:SetPoint("TOPLEFT", 9, -6)
        name:SetPoint("RIGHT", -9, 0)
        name:SetJustifyH("LEFT")
        row.name = name
        local preview = row:CreateFontString(nil, "OVERLAY", "GameFontDisableSmall")
        preview:SetPoint("BOTTOMLEFT", 9, 5)
        preview:SetPoint("RIGHT", -9, 0)
        preview:SetJustifyH("LEFT")
        preview:SetWordWrap(false)
        preview:SetTextColor(1, 1, 1, 0.38)
        row.preview = preview
        row:SetScript("OnEnter", function(self)
            self:SetBackdropColor(ui.surfaceR, ui.surfaceG, ui.surfaceB, 0.9)
            ui.SetBorderColor(self, false, 0.42)
        end)
        row:SetScript("OnLeave", function(self)
            self:SetBackdropColor(ui.surfaceR, ui.surfaceG, ui.surfaceB, 0.44)
            ui.SetBorderColor(self, false, 0.18)
        end)
        row:SetScript("OnClick", function(self)
            if not self.result then return end
            local label = ResultPath(searchRootLabel or currentLabel or "_G", self.result)
            local succeeded, inspection, errorMessage = ns.ObjectInspector.InspectValue(self.result.value, label)
            if succeeded then
                pathPanel.editBox:SetText("")
                ShowInspection(inspection)
            else
                SetStatus(errorMessage or L.OBJECT_NOT_FOUND, ui.accentR, ui.accentG, ui.accentB)
            end
        end)
        rows[index] = row
        return row
    end

    local function RefreshResults()
        for index = 1, #rows do rows[index]:Hide() end
        local offset = listScroll:GetVerticalScroll()
        if issecretvalue and issecretvalue(offset) then offset = 0 end
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
            row:SetBackdropColor(ui.surfaceR, ui.surfaceG, ui.surfaceB, 0.44)
            ui.SetBorderColor(row, false, 0.18)
            row:Show()
        end
        listContent:SetHeight(math.max(1, #results * ROW_HEIGHT))
        listScroll:UpdateScrollChildRect()
        empty:SetShown(#results == 0)
    end
    listScroll.onVerticalScrollChanged = RefreshResults

    inspectButton:SetScript("OnClick", function() InspectPath(pathPanel.editBox:GetText()) end)
    mouseButton:SetScript("OnClick", StartPicker)
    local function Search()
        if currentValue == nil then
            SetStatus(L.INSPECT_OR_CAPTURE_FIRST, ui.accentR, ui.accentG, ui.accentB)
            return
        end
        local succeeded, searchResult, errorMessage = ns.ObjectInspector.SearchValue(currentValue, pathPanel.editBox:GetText())
        if succeeded then
            results = searchResult.results
            searchRootLabel = currentLabel
            RefreshResults()
            SetStatus(string.format(L.MATCH_COUNT, searchResult.totalMatches), 0.55, 0.60, 0.65)
        else
            SetStatus(errorMessage, ui.accentR, ui.accentG, ui.accentB)
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
        ui.SetBorderColor(pathPanel, true, 0.75)
    end)
    pathPanel.editBox:SetScript("OnEditFocusLost", function(self)
        inputHint:SetShown(self:GetText() == "")
        ui.SetBorderColor(pathPanel, false)
    end)

    function page:Activate()
        if pickerActive then
            StopPicker(false)
        end
        pathPanel.editBox:SetFocus()
    end
    function page:Stop()
        StopPicker(false)
    end
    SetStatus(L.READY, 0.55, 0.60, 0.65)
    treeView:SetTree(nil)
    ui.SetReadOnlyText(textView, "")
    SetSnapshotMode("tree")
    RefreshResults()
    return page
end
