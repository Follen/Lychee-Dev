local ADDON_NAME, ns = ...
local L = ns.L

-- Run workbench page (key "runner"). Ports the legacy runner sections
-- (add-on/UI/MainWindow.lua runner paths, Core/Bootstrap.lua semantics) onto
-- the 2.0 workbench: Lua editor with explicit execution, bounded text/tree
-- result views, newest-first history rows in the shell-owned rail and a
-- run_result export through the single shared export hook. Everything is
-- constructed lazily on the first workbench open: no frames, events, hooks or
-- timers exist before the first /dev.

local Workbench = ns.Workbench
local Widgets = ns.Widgets
local TreeView = ns.TreeView
local Layout = Workbench.Layout

-- Layout constants (legacy geometry; the shell owns the 232 px rail).
local PREVIEW_MAX_CHARS = 22
local EDITOR_WIDTH = Layout.WINDOW_WIDTH - Layout.CONTENT_LEFT - 58
local TAB_INDENT = "    " -- Tab inserts 4 spaces

local READY_R, READY_G, READY_B = 0.55, 0.60, 0.65
local DONE_R, DONE_G, DONE_B = 0.42, 0.76, 0.43
local ACCENT_R, ACCENT_G, ACCENT_B = 0.847, 0.231, 0.306

local function IsSecret(value)
    return issecretvalue and issecretvalue(value)
end

-- Combat refusal at every action entry.
local function RefuseInCombat()
    if ns.Safety.IsCombatBlocked() then
        ns.Safety.PrintBlocked()
        return true
    end
    return false
end

local function BuildRunPage(parent)
    local colors = Widgets.Colors
    local page = CreateFrame("Frame", nil, parent)
    page:SetAllPoints(parent)

    -- Session-only live tree cache. Weak keys: restored rows fall back to the
    -- persisted stored tree once the live tree is gone.
    local historyTrees = setmetatable({}, { __mode = "k" })
    local historyButtons = {}
    local selectedHistoryIndex
    local resultMode = "text"
    local currentResultText = ""

    -- The shell owns the rail panel/scroll/empty label; the page renders rows.
    local rail = Workbench.GetHistoryRail()

    local inputLabel = Widgets.CreateSectionLabel(page, L.LUA_INPUT)
    inputLabel:SetPoint("TOPLEFT", Layout.CONTENT_LEFT, Layout.HEADING_TOP)

    local inputPanel = Widgets.CreateTextArea(page, false)
    inputPanel:SetPoint("TOPLEFT", Layout.CONTENT_LEFT, Layout.CONTENT_TOP)
    inputPanel:SetPoint("TOPRIGHT", Layout.CONTENT_RIGHT, Layout.CONTENT_TOP)
    inputPanel:SetHeight(210)
    inputPanel.editBox:SetWidth(EDITOR_WIDTH)
    inputPanel.editBox:SetScript("OnTabPressed", function(self)
        self:Insert(TAB_INDENT)
    end)

    local actionRow = CreateFrame("Frame", nil, page)
    actionRow:SetPoint("TOPLEFT", inputPanel, "BOTTOMLEFT", 0, -8)
    actionRow:SetPoint("TOPRIGHT", inputPanel, "BOTTOMRIGHT", 0, -8)
    actionRow:SetHeight(30)

    local runButton = Widgets.CreateButton(actionRow, 96, L.RUN, "primary")
    runButton:SetPoint("RIGHT", actionRow, "RIGHT", 0, 0)

    local clearInputButton = Widgets.CreateButton(actionRow, 108, L.CLEAR_INPUT, "secondary")
    clearInputButton:SetPoint("RIGHT", runButton, "LEFT", -8, 0)

    local statusDot = actionRow:CreateTexture(nil, "ARTWORK")
    statusDot:SetSize(5, 5)
    statusDot:SetPoint("LEFT", actionRow, "LEFT", 2, 0)

    local status = actionRow:CreateFontString(nil, "OVERLAY", "GameFontHighlightSmall")
    status:SetPoint("LEFT", statusDot, "RIGHT", 7, 0)

    local resultLabel = Widgets.CreateSectionLabel(page, L.RESULT)
    resultLabel:SetPoint("TOPLEFT", actionRow, "BOTTOMLEFT", 0, -13)

    local resultTextTab = Widgets.CreateNavTab(page, L.TEXT)
    Widgets.FitNavTab(resultTextTab, 58, 28)
    resultTextTab:SetPoint("TOPRIGHT", actionRow, "BOTTOMRIGHT", 0, -4)

    local resultTreeTab = Widgets.CreateNavTab(page, L.TREE)
    Widgets.FitNavTab(resultTreeTab, 58, 28)
    resultTreeTab:SetPoint("RIGHT", resultTextTab, "LEFT", -2, 0)

    local resultPanel = Widgets.CreateTextArea(page, true)
    resultPanel:SetPoint("TOPLEFT", actionRow, "BOTTOMLEFT", 0, -35)
    resultPanel:SetPoint("BOTTOMRIGHT", Layout.CONTENT_RIGHT, Layout.CONTENT_BOTTOM)
    resultPanel.editBox:SetWidth(EDITOR_WIDTH)
    resultPanel.editBox:SetScript("OnMouseUp", function(self)
        self:SetFocus()
    end)

    local treeView = TreeView.Create(page, { contentWidth = EDITOR_WIDTH + 30 })
    treeView.panel:SetPoint("TOPLEFT", actionRow, "BOTTOMLEFT", 0, -35)
    treeView.panel:SetPoint("BOTTOMRIGHT", Layout.CONTENT_RIGHT, Layout.CONTENT_BOTTOM)

    local selectButton = Widgets.CreateButton(page, 118, L.SELECT_RESULT, "secondary")
    selectButton:SetPoint("BOTTOMRIGHT", Layout.CONTENT_RIGHT, 14)

    local saveButton = Widgets.CreateButton(page, 92, L.SAVE_TO_DISK, "secondary")
    saveButton:SetPoint("RIGHT", selectButton, "LEFT", -8, 0)
    Widgets.SetButtonEnabled(saveButton, false)

    local clearHistoryButton = Widgets.CreateButton(page, Layout.RAIL_WIDTH, L.CLEAR_HISTORY, "secondary")
    clearHistoryButton:SetPoint("BOTTOMLEFT", Layout.RAIL_LEFT, 14)

    local function SetStatus(text, red, green, blue)
        status:SetText(text)
        status:SetTextColor(red, green, blue, 0.92)
        statusDot:SetColorTexture(red, green, blue, 0.92)
    end

    local function SetResult(text)
        if IsSecret(text) then
            text = L.TREE_SECRET
        end
        currentResultText = text or ""
        page.currentResultText = currentResultText
        Widgets.SetReadOnlyText(resultPanel, currentResultText)
        Widgets.SetButtonEnabled(saveButton, currentResultText ~= "")
    end

    local function SetResultMode(mode)
        resultMode = mode
        resultPanel:SetShown(mode == "text")
        treeView.panel:SetShown(mode == "tree")
        selectButton:Show()
        resultTextTab:SetActive(mode == "text")
        resultTreeTab:SetActive(mode == "tree")
        resultTreeTab:SetEnabled(true)
    end

    -- Displayed store values fail open to a visible marker instead of
    -- throwing on secret data.
    local function FormatHistoryEntry(entry)
        local stamp
        if IsSecret(entry.timestamp) then
            stamp = L.UNKNOWN_TIME
        else
            stamp = entry.timestamp and date("%m-%d %H:%M", entry.timestamp) or L.UNKNOWN_TIME
        end
        local firstLine
        if IsSecret(entry.code) then
            firstLine = L.TREE_SECRET
        else
            firstLine = (entry.code or ""):match("([^\r\n]+)") or L.EMPTY_INPUT
            if #firstLine > PREVIEW_MAX_CHARS then
                firstLine = firstLine:sub(1, PREVIEW_MAX_CHARS) .. "..."
            end
        end
        return stamp, firstLine
    end

    local function ApplyHistoryButtonStyle(button)
        local selected = button.entryIndex == selectedHistoryIndex
        Widgets.SetListRowState(button, selected, button.isHovered)
        if selected then
            button.time:SetTextColor(1, 1, 1, 0.96)
            button.preview:SetTextColor(1, 1, 1, 0.68)
        elseif button.isHovered then
            button.time:SetTextColor(1, 1, 1, 0.90)
            button.preview:SetTextColor(1, 1, 1, 0.58)
        else
            button.time:SetTextColor(1, 1, 1, 0.68)
            button.preview:SetTextColor(1, 1, 1, 0.42)
        end
    end

    local function RestoreHistory(index)
        local entry = (ns.Stores.History.Get() or {})[index]
        if not entry then
            return
        end
        selectedHistoryIndex = index
        local code = IsSecret(entry.code) and "" or (entry.code or "")
        inputPanel.editBox:SetText(code)
        SetResult(entry.result or "")
        local valueTree = historyTrees[entry] or entry.tree
        if IsSecret(valueTree) then
            valueTree = nil
        end
        treeView:SetTree(valueTree)
        SetResultMode(valueTree and "tree" or "text")
        local succeeded = not IsSecret(entry.succeeded) and entry.succeeded
        if succeeded then
            SetStatus(L.COMPLETED, DONE_R, DONE_G, DONE_B)
        else
            SetStatus(L.FAILED, ACCENT_R, ACCENT_G, ACCENT_B)
        end
    end

    local function CreateHistoryButton(index)
        local button = Widgets.CreateListRow(rail.content, rail.rowHeight)
        button:SetWidth(rail.rowWidth)
        button.entryIndex = index

        local timeLabel = button:CreateFontString(nil, "OVERLAY", "GameFontHighlightSmall")
        timeLabel:SetPoint("TOPLEFT", 12, -9)
        timeLabel:SetPoint("TOPRIGHT", -9, -9)
        timeLabel:SetJustifyH("LEFT")
        timeLabel:SetWordWrap(false)
        button.time = timeLabel

        local preview = button:CreateFontString(nil, "OVERLAY", "GameFontDisableSmall")
        preview:SetPoint("BOTTOMLEFT", 12, 8)
        preview:SetPoint("BOTTOMRIGHT", -9, 8)
        preview:SetJustifyH("LEFT")
        preview:SetWordWrap(false)
        button.preview = preview

        button:SetScript("OnEnter", function(self)
            self.isHovered = true
            ApplyHistoryButtonStyle(self)
        end)
        button:SetScript("OnLeave", function(self)
            self.isHovered = nil
            ApplyHistoryButtonStyle(self)
        end)
        button:SetScript("OnClick", function(self)
            RestoreHistory(self.entryIndex)
            for historyIndex = 1, #historyButtons do
                ApplyHistoryButtonStyle(historyButtons[historyIndex])
            end
        end)
        button:SetScript("OnMouseWheel", rail.scroll.onMouseWheel)

        historyButtons[index] = button
        return button
    end

    local function RefreshHistory()
        local history = ns.Stores.History.Get() or {}

        for index = 1, #historyButtons do
            historyButtons[index]:Hide()
        end

        for index = 1, #history do
            local button = historyButtons[index] or CreateHistoryButton(index)
            button.entryIndex = index
            button:ClearAllPoints()
            button:SetPoint("TOPLEFT", 0, -((index - 1) * rail.rowHeight))
            local stamp, preview = FormatHistoryEntry(history[index])
            button.time:SetText(stamp)
            button.preview:SetText(preview)
            ApplyHistoryButtonStyle(button)
            button:Show()
        end

        rail.content:SetHeight(math.max(1, #history * rail.rowHeight))
        rail.scroll:UpdateScrollChildRect()
        rail.empty:SetShown(#history == 0)
    end

    local function ApplyRunOutcome(succeeded, resultText, normalizedCode, valueTree, storedTree)
        SetResult(resultText)
        treeView:SetTree(valueTree)
        SetResultMode(valueTree and "tree" or "text")
        local entry = ns.Stores.History.Add(normalizedCode, resultText, succeeded and true or false, storedTree)
        if entry and valueTree then
            historyTrees[entry] = valueTree
        end
        selectedHistoryIndex = 1
        RefreshHistory()

        if succeeded then
            SetStatus(L.COMPLETED, DONE_R, DONE_G, DONE_B)
        else
            SetStatus(L.FAILED, ACCENT_R, ACCENT_G, ACCENT_B)
        end
    end

    local function RunInput()
        if RefuseInCombat() then
            return
        end
        local code = inputPanel.editBox:GetText() or ""
        if IsSecret(code) then
            return
        end
        if code:match("^%s*$") then
            -- Empty input is an error presented as data, never a thrown error.
            ApplyRunOutcome(false, L.EMPTY_INPUT, "", nil, nil)
            return
        end
        local succeeded, result, normalizedCode, valueTree, storedTree = ns.Execute(code)
        ApplyRunOutcome(succeeded, result, normalizedCode, valueTree, storedTree)
    end

    local function ClearInput()
        if RefuseInCombat() then
            return
        end
        inputPanel.editBox:SetText("")
        inputPanel.editBox:SetFocus()
        SetStatus(L.READY, READY_R, READY_G, READY_B)
    end

    local function SelectResult()
        SetResultMode("text")
        Widgets.SelectAllText(resultPanel)
    end

    -- Every Save goes through the single shared export hook (SaveFromHook
    -- resolves this page's payload provider and opens the ticket popup).
    local function SaveResult()
        if RefuseInCombat() then
            return
        end
        if currentResultText == "" then
            return
        end
        Workbench.SaveFromHook()
    end

    local function ClearHistory()
        if RefuseInCombat() then
            return
        end
        ns.Stores.History.Clear()
        wipe(historyTrees)
        selectedHistoryIndex = nil
        SetResult("")
        treeView:SetTree(nil)
        SetResultMode("text")
        SetStatus(L.READY, READY_R, READY_G, READY_B)
        RefreshHistory()
    end

    runButton:SetScript("OnClick", RunInput)
    clearInputButton:SetScript("OnClick", ClearInput)
    selectButton:SetScript("OnClick", SelectResult)
    saveButton:SetScript("OnClick", SaveResult)
    clearHistoryButton:SetScript("OnClick", ClearHistory)
    resultTextTab:SetScript("OnClick", function()
        SetResultMode("text")
    end)
    resultTreeTab:SetScript("OnClick", function()
        SetResultMode("tree")
    end)

    SetResult("")
    treeView:SetTree(nil)
    SetStatus(L.READY, READY_R, READY_G, READY_B)
    RefreshHistory()
    SetResultMode("text")

    page.inputPanel = inputPanel
    page.runButton = runButton
    page.clearInputButton = clearInputButton
    page.statusDot = statusDot
    page.status = status
    page.resultLabel = resultLabel
    page.resultTextTab = resultTextTab
    page.resultTreeTab = resultTreeTab
    page.resultPanel = resultPanel
    page.treeView = treeView
    page.selectButton = selectButton
    page.saveButton = saveButton
    page.exportResultButton = saveButton
    page.clearHistoryButton = clearHistoryButton
    page.historyButtons = historyButtons
    page.rail = rail
    page.RefreshHistory = RefreshHistory
    page.RunInput = RunInput
    page.SetResultMode = SetResultMode
    page.RestoreHistory = RestoreHistory
    function page:GetResultMode()
        return resultMode
    end
    function page:GetResultText()
        return currentResultText
    end
    function page:GetSelectedHistoryIndex()
        return selectedHistoryIndex
    end
    return page
end

-- The run_result payload provider for the one shared export hook.
Workbench.RegisterExportHook{
    key = "runner",
    GetPayload = function()
        local page = Workbench.GetPage("runner")
        if not page or page:GetResultText() == "" then
            return nil
        end
        return "run_result", L.RESULT, page:GetResultText(), {
            code = page.inputPanel.editBox:GetText(),
        }
    end,
}

Workbench.RegisterPage{
    key = "runner",
    titleKey = "TAB_RUNNER",
    rail = true,
    build = BuildRunPage,
    activate = function(page)
        page.RefreshHistory()
        page.inputPanel.editBox:SetFocus()
    end,
    suspend = function(page)
        page.inputPanel.editBox:ClearFocus()
    end,
    -- Window hide / combat teardown: release focus and transient presentation
    -- state. The page owns no events or timers.
    shutdown = function(page)
        page.inputPanel.editBox:ClearFocus()
        page.resultPanel.editBox:ClearFocus()
    end,
}
