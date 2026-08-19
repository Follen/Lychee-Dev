local ADDON_NAME, ns = ...
local L = ns.L

local WINDOW_WIDTH = 1040
local WINDOW_HEIGHT = 720
local HISTORY_WIDTH = 232
local MAIN_LEFT = HISTORY_WIDTH + 36

local ACCENT_R, ACCENT_G, ACCENT_B = 0.847, 0.231, 0.306
local PANEL_R, PANEL_G, PANEL_B = 0.050, 0.070, 0.090
local SURFACE_R, SURFACE_G, SURFACE_B = 0.061, 0.095, 0.120
local EDITOR_R, EDITOR_G, EDITOR_B = 0.027, 0.035, 0.043
local BORDER_R, BORDER_G, BORDER_B = 0.34, 0.39, 0.44
local LOGO_TEXTURE = "Interface\\AddOns\\" .. ADDON_NAME .. "\\Media\\Logo.png"

local BACKDROP = {
    bgFile = "Interface\\Buttons\\WHITE8X8",
    edgeFile = "Interface\\Buttons\\WHITE8X8",
    edgeSize = 1,
}

local window
local selectedHistoryIndex
local historyButtons = {}
local activePage = "runner"
local resultMode = "text"
local historyTrees = setmetatable({}, { __mode = "k" })

local PAGE_DEFINITIONS = {
    { key = "runner", label = L.TAB_RUNNER },
    { key = "objects", label = L.TAB_OBJECTS },
    { key = "events", label = L.TAB_EVENTS },
    { key = "trace", label = L.TAB_TRACE },
    { key = "diagnostics", label = L.TAB_DIAGNOSTICS },
    { key = "about", label = L.TAB_ABOUT },
}

local function SetBorderColor(frame, accent, alpha)
    if accent then
        frame:SetBackdropBorderColor(ACCENT_R, ACCENT_G, ACCENT_B, alpha or 0.8)
    else
        frame:SetBackdropBorderColor(BORDER_R, BORDER_G, BORDER_B, alpha or 0.35)
    end
end

local function CreatePanel(parent, r, g, b, a)
    local panel = CreateFrame("Frame", nil, parent, "BackdropTemplate")
    panel:SetBackdrop(BACKDROP)
    panel:SetBackdropColor(r, g, b, a)
    SetBorderColor(panel, false)

    local innerHighlight = panel:CreateTexture(nil, "ARTWORK")
    innerHighlight:SetColorTexture(1, 1, 1, 0.035)
    innerHighlight:SetPoint("TOPLEFT", 1, -1)
    innerHighlight:SetPoint("TOPRIGHT", -1, -1)
    innerHighlight:SetHeight(1)
    return panel
end

local function Clamp(value, minimum, maximum)
    return math.max(minimum, math.min(maximum, value))
end

local function CreateScrollArea(parent, leftInset, topInset, rightInset, bottomInset, useNativeScrollFrame)
    local scroll = CreateFrame(useNativeScrollFrame and "ScrollFrame" or "Frame", nil, parent)
    scroll:SetPoint("TOPLEFT", leftInset, -topInset)
    scroll:SetPoint("BOTTOMRIGHT", -(rightInset + 11), bottomInset)
    scroll:SetClipsChildren(true)
    scroll:EnableMouseWheel(true)
    scroll.verticalOffset = 0
    scroll.verticalRange = 0

    local scrollbar = CreateFrame("Slider", nil, parent)
    scrollbar:SetOrientation("VERTICAL")
    scrollbar:SetPoint("TOPRIGHT", -rightInset, -topInset)
    scrollbar:SetPoint("BOTTOMRIGHT", -rightInset, bottomInset)
    scrollbar:SetWidth(7)
    scrollbar:SetMinMaxValues(0, 0)
    scrollbar:SetValue(0)
    scrollbar:SetValueStep(1)
    scrollbar:SetObeyStepOnDrag(false)
    scrollbar:SetHitRectInsets(-4, -4, 0, 0)
    scrollbar.syncing = false

    local track = scrollbar:CreateTexture(nil, "BACKGROUND")
    track:SetColorTexture(1, 1, 1, 0.07)
    track:SetPoint("TOP", 0, 0)
    track:SetPoint("BOTTOM", 0, 0)
    track:SetWidth(2)

    local thumb = scrollbar:CreateTexture(nil, "ARTWORK")
    thumb:SetColorTexture(0.50, 0.56, 0.61, 0.72)
    thumb:SetSize(7, 32)
    scrollbar:SetThumbTexture(thumb)
    scrollbar.thumb = thumb

    scrollbar:SetScript("OnEnter", function(self)
        self.thumb:SetColorTexture(ACCENT_R, ACCENT_G, ACCENT_B, 0.95)
    end)
    scrollbar:SetScript("OnLeave", function(self)
        self.thumb:SetColorTexture(0.50, 0.56, 0.61, 0.72)
    end)
    scrollbar:SetScript("OnValueChanged", function(self, value)
        if not self.syncing then
            scroll:SetVerticalScroll(value)
            if useNativeScrollFrame then
                scroll:SyncVerticalOffset(value)
            end
        end
    end)

    function scroll:SyncVerticalOffset(offset)
        if issecretvalue and issecretvalue(offset) then
            return
        end
        offset = Clamp(tonumber(offset) or 0, 0, self.verticalRange or 0)
        local changed = offset ~= self.verticalOffset
        self.verticalOffset = offset
        scrollbar.syncing = true
        scrollbar:SetValue(offset)
        scrollbar.syncing = false
        if changed and self.onVerticalScrollChanged then
            self.onVerticalScrollChanged(offset)
        end
    end

    if useNativeScrollFrame then
        scroll:SetScript("OnVerticalScroll", function(self, offset)
            self:SyncVerticalOffset(offset)
        end)
        scroll:SetScript("OnScrollRangeChanged", function(self, _, verticalRange)
            self:UpdateScrollChildRect(verticalRange)
        end)
    else
        function scroll:GetVerticalScroll()
            return self.verticalOffset or 0
        end

        function scroll:SetVerticalScroll(offset)
            self:SyncVerticalOffset(offset)
            if self.scrollChild then
                self.scrollChild:ClearAllPoints()
                self.scrollChild:SetPoint("TOPLEFT", self, "TOPLEFT", 0, self.verticalOffset or 0)
            end
        end
    end

    function scroll:UpdateScrollChildRect(nativeVerticalRange)
        local child = useNativeScrollFrame and self:GetScrollChild() or self.scrollChild
        local childHeight = child and child:GetHeight() or 0
        local viewHeight = self:GetHeight() or 0
        local nativeRangeIsSecret = nativeVerticalRange ~= nil
            and issecretvalue and issecretvalue(nativeVerticalRange)
        if issecretvalue and (issecretvalue(childHeight) or issecretvalue(viewHeight)
            or nativeRangeIsSecret) then
            scrollbar:Hide()
            return
        end

        local range
        if useNativeScrollFrame then
            range = nativeVerticalRange
            if range == nil then
                range = self:GetVerticalScrollRange()
            end
        else
            range = (childHeight or 0) - (viewHeight or 0)
        end
        if issecretvalue and issecretvalue(range) then
            scrollbar:Hide()
            return
        end
        range = math.max(0, tonumber(range) or 0)
        self.verticalRange = range
        scrollbar:SetMinMaxValues(0, range)
        local offset = useNativeScrollFrame and self:GetVerticalScroll() or self.verticalOffset
        if issecretvalue and issecretvalue(offset) then
            scrollbar:Hide()
            return
        end
        offset = Clamp(offset or 0, 0, range)
        self:SetVerticalScroll(offset)
        if useNativeScrollFrame then
            self:SyncVerticalOffset(offset)
        end
        if range <= 0 then
            scrollbar:Hide()
            return
        end

        local trackHeight = scrollbar:GetHeight()
        if not (issecretvalue and issecretvalue(trackHeight))
            and trackHeight and trackHeight > 0 and viewHeight > 0 then
            local contentHeight = math.max(viewHeight, viewHeight + range)
            scrollbar.thumb:SetHeight(math.max(28, math.floor(trackHeight * viewHeight / contentHeight + 0.5)))
        end
        scrollbar:Show()
    end

    if not useNativeScrollFrame then
        function scroll:SetScrollChild(child)
            self.scrollChild = child
            child:ClearAllPoints()
            child:SetPoint("TOPLEFT", self, "TOPLEFT", 0, 0)
            self:UpdateScrollChildRect()
        end
    end

    local function HandleMouseWheel(_, delta)
        if issecretvalue and issecretvalue(delta) then
            return
        end
        local target = Clamp((scroll.verticalOffset or 0) - delta * 36, 0, scroll.verticalRange or 0)
        scroll:SetVerticalScroll(target)
        if useNativeScrollFrame then
            scroll:SyncVerticalOffset(target)
        end
    end
    scroll:SetScript("OnMouseWheel", HandleMouseWheel)
    scroll.onMouseWheel = HandleMouseWheel
    scrollbar:EnableMouseWheel(true)
    scrollbar:SetScript("OnMouseWheel", HandleMouseWheel)
    scroll:SetScript("OnSizeChanged", function(self)
        self:UpdateScrollChildRect()
    end)

    scrollbar:Hide()
    scroll.scrollbar = scrollbar
    return scroll
end

local function CreateCloseButton(parent)
    local button = CreateFrame("Button", nil, parent, "BackdropTemplate")
    button:SetSize(28, 28)
    button:SetBackdrop(BACKDROP)

    local firstLine = button:CreateTexture(nil, "ARTWORK")
    firstLine:SetColorTexture(1, 1, 1, 0.72)
    firstLine:SetSize(13, 2)
    firstLine:SetPoint("CENTER")
    firstLine:SetRotation(0.785398)

    local secondLine = button:CreateTexture(nil, "ARTWORK")
    secondLine:SetColorTexture(1, 1, 1, 0.72)
    secondLine:SetSize(13, 2)
    secondLine:SetPoint("CENTER")
    secondLine:SetRotation(-0.785398)

    local function SetState(hovered, pressed)
        if pressed then
            button:SetBackdropColor(ACCENT_R * 0.72, ACCENT_G * 0.72, ACCENT_B * 0.72, 0.95)
            button:SetBackdropBorderColor(ACCENT_R, ACCENT_G, ACCENT_B, 1)
        elseif hovered then
            button:SetBackdropColor(ACCENT_R, ACCENT_G, ACCENT_B, 0.88)
            button:SetBackdropBorderColor(ACCENT_R, ACCENT_G, ACCENT_B, 1)
        else
            button:SetBackdropColor(SURFACE_R, SURFACE_G, SURFACE_B, 0.78)
            button:SetBackdropBorderColor(BORDER_R, BORDER_G, BORDER_B, 0.38)
        end
        local alpha = hovered and 1 or 0.72
        firstLine:SetColorTexture(1, 1, 1, alpha)
        secondLine:SetColorTexture(1, 1, 1, alpha)
    end

    button:SetScript("OnEnter", function(self)
        self.isHovered = true
        SetState(true, false)
    end)
    button:SetScript("OnLeave", function(self)
        self.isHovered = nil
        SetState(false, false)
    end)
    button:SetScript("OnMouseDown", function()
        SetState(true, true)
    end)
    button:SetScript("OnMouseUp", function(self)
        SetState(self.isHovered, false)
    end)

    SetState(false, false)
    return button
end

local function SetButtonLabelOffset(button, y)
    button.label:ClearAllPoints()
    button.label:SetPoint("CENTER", 0, y)
end

local function ApplyButtonState(button, state)
    local enabled = button:IsEnabled()
    local variant = button.variant or (button.primary and "primary" or "secondary")

    if not enabled then
        button:SetBackdropColor(SURFACE_R, SURFACE_G, SURFACE_B, 0.34)
        SetBorderColor(button, false, 0.18)
        button.label:SetTextColor(1, 1, 1, 0.28)
        SetButtonLabelOffset(button, 0)
        return
    end

    local emphasized = variant == "primary" or variant == "danger"
    if state == "pressed" then
        if emphasized then
            button:SetBackdropColor(ACCENT_R * 0.78, ACCENT_G * 0.78, ACCENT_B * 0.78, 1)
        elseif variant == "selected" then
            button:SetBackdropColor(ACCENT_R, ACCENT_G, ACCENT_B, 0.22)
        else
            button:SetBackdropColor(SURFACE_R * 0.72, SURFACE_G * 0.72, SURFACE_B * 0.72, 1)
        end
        SetBorderColor(button, emphasized or variant == "selected", emphasized and 1 or 0.64)
    elseif state == "hover" then
        if emphasized then
            button:SetBackdropColor(ACCENT_R, ACCENT_G, ACCENT_B, 1)
        elseif variant == "selected" then
            button:SetBackdropColor(ACCENT_R, ACCENT_G, ACCENT_B, 0.18)
        else
            button:SetBackdropColor(SURFACE_R * 1.28, SURFACE_G * 1.28, SURFACE_B * 1.28, 1)
        end
        SetBorderColor(button, emphasized or variant == "selected", emphasized and 1 or 0.7)
    else
        if emphasized then
            button:SetBackdropColor(ACCENT_R, ACCENT_G, ACCENT_B, 0.88)
        elseif variant == "selected" then
            button:SetBackdropColor(ACCENT_R, ACCENT_G, ACCENT_B, 0.12)
        else
            button:SetBackdropColor(SURFACE_R, SURFACE_G, SURFACE_B, 0.92)
        end
        SetBorderColor(button, emphasized or variant == "selected", emphasized and 0.85 or (variant == "selected" and 0.52 or 0.42))
    end

    button.label:SetTextColor(1, 1, 1, emphasized and 1 or (variant == "selected" and 0.94 or 0.76))
end

local function CreateButton(parent, width, text, primary)
    local button = CreateFrame("Button", nil, parent, "BackdropTemplate")
    button:SetSize(width, 28)
    button:SetBackdrop(BACKDROP)
    button.variant = type(primary) == "string" and primary or (primary and "primary" or "secondary")
    button.primary = button.variant == "primary"

    local label = button:CreateFontString(nil, "OVERLAY", "GameFontHighlightSmall")
    label:SetPoint("CENTER")
    label:SetText(text)
    button.label = label

    button:SetScript("OnEnter", function(self)
        self.isHovered = true
        ApplyButtonState(self, "hover")
    end)
    button:SetScript("OnLeave", function(self)
        self.isHovered = nil
        ApplyButtonState(self, "normal")
        SetButtonLabelOffset(self, 0)
    end)
    button:SetScript("OnMouseDown", function(self)
        ApplyButtonState(self, "pressed")
        SetButtonLabelOffset(self, -1)
    end)
    button:SetScript("OnMouseUp", function(self)
        ApplyButtonState(self, self.isHovered and "hover" or "normal")
        SetButtonLabelOffset(self, 0)
    end)

    ApplyButtonState(button, "normal")
    return button
end

local function SetButtonVariant(button, variant)
    button.variant = variant or "secondary"
    button.primary = button.variant == "primary"
    ApplyButtonState(button, "normal")
end

local function SetButtonPrimary(button, primary)
    SetButtonVariant(button, primary and "primary" or "secondary")
end

local function SetButtonEnabled(button, enabled)
    button:SetEnabled(enabled and true or false)
    if not enabled then
        button.isHovered = nil
    end
    ApplyButtonState(button, "normal")
end

local function SetButtonText(button, text)
    button.label:SetText(text or "")
end

local function CreateSectionLabel(parent, text)
    local label = parent:CreateFontString(nil, "OVERLAY", "GameFontNormal")
    label:SetText(text)
    label:SetTextColor(0.78, 0.82, 0.86, 1)
    return label
end

local function CreateNavTab(parent, text)
    local button = CreateFrame("Button", nil, parent)
    button:SetSize(82, 32)

    local label = button:CreateFontString(nil, "OVERLAY", "GameFontHighlightSmall")
    label:SetPoint("CENTER", 0, 1)
    label:SetText(text)
    button.label = label

    local underline = button:CreateTexture(nil, "ARTWORK")
    underline:SetPoint("BOTTOMLEFT", 7, 0)
    underline:SetPoint("BOTTOMRIGHT", -7, 0)
    underline:SetHeight(2)
    button.underline = underline

    local function ApplyState(self)
        if self.active then
            self.label:SetTextColor(1, 1, 1, 1)
            self.underline:SetColorTexture(ACCENT_R, ACCENT_G, ACCENT_B, 1)
        elseif self.isHovered then
            self.label:SetTextColor(1, 1, 1, 0.86)
            self.underline:SetColorTexture(ACCENT_R, ACCENT_G, ACCENT_B, 0.35)
        else
            self.label:SetTextColor(1, 1, 1, 0.46)
            self.underline:SetColorTexture(0, 0, 0, 0)
        end
    end

    button.SetActive = function(self, active)
        self.active = active and true or false
        ApplyState(self)
    end
    button:SetScript("OnEnter", function(self)
        self.isHovered = true
        ApplyState(self)
    end)
    button:SetScript("OnLeave", function(self)
        self.isHovered = nil
        ApplyState(self)
    end)
    button:SetActive(false)
    return button
end

local function CreateTextArea(parent, readOnly)
    local panel = CreatePanel(parent, EDITOR_R, EDITOR_G, EDITOR_B, 1)

    local scroll = CreateScrollArea(panel, 10, 10, 8, 10, true)
    panel:EnableMouseWheel(true)
    panel:SetScript("OnMouseWheel", function(_, delta)
        scroll.onMouseWheel(scroll, delta)
    end)

    local editBox = CreateFrame("EditBox", nil, scroll)
    editBox:SetPoint("TOPLEFT", scroll, "TOPLEFT", 0, 0)
    editBox:SetMultiLine(true)
    editBox:SetAutoFocus(false)
    editBox:SetCountInvisibleLetters(true)
    editBox:EnableMouseWheel(true)
    editBox:SetScript("OnMouseWheel", function(_, delta)
        scroll.onMouseWheel(scroll, delta)
    end)
    editBox:SetFontObject(ChatFontNormal)
    editBox:SetTextInsets(4, 4, 4, 4)
    editBox:SetWidth(500)
    editBox:SetHeight(1)
    editBox.savedText = readOnly and "" or nil

    editBox:SetScript("OnEscapePressed", function(self)
        self:ClearFocus()
    end)
    editBox:SetScript("OnEditFocusGained", function()
        SetBorderColor(panel, true, 0.75)
    end)
    editBox:SetScript("OnEditFocusLost", function()
        SetBorderColor(panel, false)
    end)
    editBox:SetScript("OnTextChanged", function(self)
        if readOnly and not self.updatingText and self:GetText() ~= self.savedText then
            self.updatingText = true
            self:SetText(self.savedText)
            self.updatingText = nil
            self:HighlightText()
            return
        end
        scroll:UpdateScrollChildRect()
    end)

    local function ScrollCursorIntoView()
        editBox.cursorScrollQueued = false
        scroll:UpdateScrollChildRect()
        local offset = scroll:GetVerticalScroll()
        local scrollHeight = scroll:GetHeight()
        local y = editBox.cursorOffset or 0
        local height = editBox.cursorHeight or 0
        if issecretvalue and (issecretvalue(offset) or issecretvalue(scrollHeight)
            or issecretvalue(y) or issecretvalue(height)) then
            return
        end
        if -y < offset then
            scroll:SetVerticalScroll(-y)
        elseif -y + height > offset + scrollHeight then
            scroll:SetVerticalScroll(-y + height - scrollHeight)
        end
    end
    editBox:SetScript("OnCursorChanged", function(self, _, y, _, height)
        self.cursorOffset = y
        self.cursorHeight = height
        if self.cursorScrollQueued then
            return
        end
        self.cursorScrollQueued = true
        C_Timer.After(0, ScrollCursorIntoView)
    end)
    scroll:SetScrollChild(editBox)
    scroll:UpdateScrollChildRect()
    scroll:EnableMouse(true)
    scroll:SetScript("OnMouseDown", function()
        editBox:SetFocus()
    end)

    if readOnly then
        editBox:SetTextColor(0.79, 0.83, 0.87)
    else
        editBox:SetTextColor(0.94, 0.95, 0.96)
    end

    panel.scroll = scroll
    panel.editBox = editBox
    panel.SelectAll = function(self)
        self.editBox:SetFocus()
        self.editBox:HighlightText()
    end
    return panel
end

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

local function SelectAllText(target)
    if target.SelectAll then
        target:SelectAll()
        return
    end
    target:SetFocus()
    target:HighlightText()
end

local function SetResult(text)
    SetReadOnlyText(window.resultPanel, text)
end

local function SetStatus(text, r, g, b)
    window.status:SetText(text)
    window.status:SetTextColor(r, g, b, 0.92)
    window.statusDot:SetColorTexture(r, g, b, 0.92)
end

local function SetResultMode(mode)
    if not window then
        return
    end

    resultMode = mode
    window.resultPanel:SetShown(mode == "text")
    window.treeView.panel:SetShown(mode == "tree")
    window.selectButton:Show()
    window.resultTextTab:SetActive(mode == "text")
    window.resultTreeTab:SetActive(mode == "tree")
    window.resultTreeTab:SetEnabled(true)
    window.resultTreeTab:SetAlpha(1)
end

local function SetActivePage(pageName)
    if not window or not window.pages[pageName] then
        pageName = "runner"
    end
    activePage = pageName

    for key, page in pairs(window.pages) do
        page:SetShown(key == pageName)
        window.pageTabs[key]:SetActive(key == pageName)
    end

    local page = window.pages[pageName]
    if pageName == "runner" then
        window.inputPanel.editBox:SetFocus()
    elseif pageName == "events" then
        page:RefreshIfDirty()
        page:FocusInput()
    elseif page.Activate then
        page:Activate()
    end
end

local function StopRuntimeTools()
    if window and window.objectPage then
        window.objectPage:Stop()
    end

    if window and window.eventsPage then
        window.eventsPage:StopMonitor()
    elseif ns.EventMonitor then
        ns.EventMonitor.Stop()
    end

    if window and window.tracePage then
        window.tracePage:Stop()
    elseif ns.FunctionTrace then
        ns.FunctionTrace.Stop()
    end

end

local function FormatHistoryLabel(entry)
    local stamp = entry.timestamp and date("%m-%d %H:%M", entry.timestamp) or L.UNKNOWN_TIME
    local firstLine = (entry.code or ""):match("([^\r\n]+)") or L.EMPTY_INPUT
    if #firstLine > 22 then
        firstLine = firstLine:sub(1, 22) .. "..."
    end
    return stamp .. "\n" .. firstLine
end

local function ApplyHistoryButtonStyle(button)
    local selected = button.entryIndex == selectedHistoryIndex
    if selected then
        button:SetBackdropColor(ACCENT_R, ACCENT_G, ACCENT_B, 0.13)
        button:SetBackdropBorderColor(ACCENT_R, ACCENT_G, ACCENT_B, 0.35)
        button.accent:Show()
        button.text:SetTextColor(1, 1, 1, 0.96)
    elseif button.isHovered then
        button:SetBackdropColor(SURFACE_R, SURFACE_G, SURFACE_B, 0.92)
        SetBorderColor(button, false, 0.48)
        button.accent:Hide()
        button.text:SetTextColor(1, 1, 1, 0.9)
    else
        button:SetBackdropColor(SURFACE_R, SURFACE_G, SURFACE_B, 0.48)
        SetBorderColor(button, false, 0.2)
        button.accent:Hide()
        button.text:SetTextColor(1, 1, 1, 0.62)
    end
end

local function CreateHistoryButton(index)
    local button = CreateFrame("Button", nil, window.historyContent, "BackdropTemplate")
    button:SetSize(HISTORY_WIDTH - 34, 48)
    button:SetBackdrop(BACKDROP)
    button.entryIndex = index

    local accent = button:CreateTexture(nil, "ARTWORK")
    accent:SetColorTexture(ACCENT_R, ACCENT_G, ACCENT_B, 1)
    accent:SetPoint("TOPLEFT", 0, -1)
    accent:SetPoint("BOTTOMLEFT", 0, 1)
    accent:SetWidth(2)
    accent:Hide()
    button.accent = accent

    local label = button:CreateFontString(nil, "OVERLAY", "GameFontHighlightSmall")
    label:SetPoint("LEFT", 10, 0)
    label:SetPoint("RIGHT", -7, 0)
    label:SetJustifyH("LEFT")
    label:SetJustifyV("MIDDLE")
    button.text = label

    button:SetScript("OnEnter", function(self)
        self.isHovered = true
        ApplyHistoryButtonStyle(self)
    end)
    button:SetScript("OnLeave", function(self)
        self.isHovered = nil
        ApplyHistoryButtonStyle(self)
    end)
    button:SetScript("OnClick", function(self)
        selectedHistoryIndex = self.entryIndex
        local entry = (ns.GetHistory() or {})[selectedHistoryIndex]
        if entry then
            window.inputPanel.editBox:SetText(entry.code or "")
            SetResult(entry.result or "")
            local valueTree = historyTrees[entry] or entry.tree
            window.treeView:SetTree(valueTree)
            SetResultMode(valueTree and "tree" or "text")
            SetStatus(entry.succeeded and L.COMPLETED or L.FAILED,
                entry.succeeded and 0.42 or ACCENT_R,
                entry.succeeded and 0.76 or ACCENT_G,
                entry.succeeded and 0.43 or ACCENT_B)
        end
        for historyIndex = 1, #historyButtons do
            ApplyHistoryButtonStyle(historyButtons[historyIndex])
        end
    end)

    historyButtons[index] = button
    return button
end

local function RefreshHistory()
    local history = ns.GetHistory() or {}

    for index = 1, #historyButtons do
        historyButtons[index]:Hide()
    end

    for index = 1, #history do
        local button = historyButtons[index] or CreateHistoryButton(index)
        button.entryIndex = index
        button:ClearAllPoints()
        button:SetPoint("TOPLEFT", 0, -((index - 1) * 52))
        button.text:SetText(FormatHistoryLabel(history[index]))
        ApplyHistoryButtonStyle(button)
        button:Show()
    end

    window.historyContent:SetHeight(math.max(1, #history * 52))
    window.historyScroll:UpdateScrollChildRect()
    window.historyEmpty:SetShown(#history == 0)
end

local function RunInput()
    local succeeded, result, normalizedCode, valueTree, storedTree = ns.Execute(window.inputPanel.editBox:GetText())
    SetResult(result)
    window.treeView:SetTree(valueTree)
    SetResultMode(valueTree and "tree" or "text")
    local historyEntry = ns.AddHistory(normalizedCode, result, succeeded, storedTree)
    if historyEntry and valueTree then
        historyTrees[historyEntry] = valueTree
    end
    selectedHistoryIndex = 1
    RefreshHistory()

    if succeeded then
        SetStatus(L.COMPLETED, 0.42, 0.76, 0.43)
    else
        SetStatus(L.FAILED, ACCENT_R, ACCENT_G, ACCENT_B)
    end
end

local function CreateWindow()
    local frame = CreateFrame("Frame", "LycheeDevWindow", UIParent, "BackdropTemplate")
    frame:SetSize(WINDOW_WIDTH, WINDOW_HEIGHT)
    frame:SetPoint("CENTER")
    frame:SetFrameStrata("DIALOG")
    frame:SetClampedToScreen(true)
    frame:SetMovable(true)
    frame:EnableMouse(true)
    frame:RegisterForDrag("LeftButton")
    frame:SetScript("OnDragStart", function(self)
        self:StartMoving()
    end)
    frame:SetScript("OnDragStop", function(self)
        self:StopMovingOrSizing()
    end)
    frame:SetBackdrop(BACKDROP)
    frame:SetBackdropColor(PANEL_R, PANEL_G, PANEL_B, 0.985)
    frame:SetBackdropBorderColor(0.46, 0.51, 0.56, 0.48)
    frame:Hide()
    tinsert(UISpecialFrames, frame:GetName())

    local header = frame:CreateTexture(nil, "BACKGROUND")
    header:SetColorTexture(SURFACE_R, SURFACE_G, SURFACE_B, 0.52)
    header:SetPoint("TOPLEFT", 1, -1)
    header:SetPoint("TOPRIGHT", -1, -1)
    header:SetHeight(66)

    local logo = frame:CreateTexture(nil, "ARTWORK")
    logo:SetTexture(LOGO_TEXTURE)
    logo:SetTexCoord(0.18, 0.79, 0.17, 0.80)
    logo:SetSize(46, 46)
    logo:SetPoint("TOPLEFT", 14, -10)

    local title = frame:CreateFontString(nil, "OVERLAY", "GameFontNormalLarge")
    title:SetPoint("LEFT", logo, "RIGHT", 10, 0)
    title:SetText(L.ADDON_TITLE)
    title:SetTextColor(1, 1, 1, 1)

    local closeButton = CreateCloseButton(frame)
    closeButton:SetPoint("TOPRIGHT", -14, -14)
    closeButton:SetScript("OnClick", function()
        frame:Hide()
    end)

    frame.pageTabs = {}
    local previousTab
    for index = 1, #PAGE_DEFINITIONS do
        local definition = PAGE_DEFINITIONS[index]
        local tab = CreateNavTab(frame, definition.label)
        tab:SetSize(70, 32)
        if previousTab then
            tab:SetPoint("LEFT", previousTab, "RIGHT", 2, 0)
        else
            tab:SetPoint("BOTTOMLEFT", header, "BOTTOMLEFT", 348, 8)
        end
        tab:SetScript("OnClick", function()
            SetActivePage(definition.key)
        end)
        frame.pageTabs[definition.key] = tab
        previousTab = tab
    end

    local runnerPage = CreateFrame("Frame", nil, frame)
    runnerPage:SetAllPoints(frame)
    frame.runnerPage = runnerPage

    local historyLabel = CreateSectionLabel(runnerPage, L.HISTORY)
    historyLabel:SetPoint("TOPLEFT", 17, -84)

    local historyPanel = CreatePanel(runnerPage, EDITOR_R, EDITOR_G, EDITOR_B, 0.72)
    historyPanel:SetPoint("TOPLEFT", 14, -104)
    historyPanel:SetPoint("BOTTOMLEFT", 14, 54)
    historyPanel:SetWidth(HISTORY_WIDTH)

    local historyScroll = CreateScrollArea(historyPanel, 8, 8, 7, 8)

    local historyContent = CreateFrame("Frame", nil, historyScroll)
    historyContent:SetWidth(HISTORY_WIDTH - 34)
    historyContent:SetHeight(1)
    historyScroll:SetScrollChild(historyContent)
    frame.historyContent = historyContent
    frame.historyScroll = historyScroll

    local historyEmpty = historyPanel:CreateFontString(nil, "OVERLAY", "GameFontDisableSmall")
    historyEmpty:SetPoint("TOP", 0, -24)
    historyEmpty:SetText(L.NO_HISTORY)
    historyEmpty:SetTextColor(1, 1, 1, 0.32)
    frame.historyEmpty = historyEmpty

    local inputLabel = CreateSectionLabel(runnerPage, L.LUA_INPUT)
    inputLabel:SetPoint("TOPLEFT", MAIN_LEFT, -84)

    local inputPanel = CreateTextArea(runnerPage, false)
    inputPanel:SetPoint("TOPLEFT", MAIN_LEFT, -104)
    inputPanel:SetPoint("TOPRIGHT", -14, -104)
    inputPanel:SetHeight(210)
    inputPanel.editBox:SetWidth(WINDOW_WIDTH - MAIN_LEFT - 58)
    frame.inputPanel = inputPanel

    local actionRow = CreateFrame("Frame", nil, runnerPage)
    actionRow:SetPoint("TOPLEFT", inputPanel, "BOTTOMLEFT", 0, -8)
    actionRow:SetPoint("TOPRIGHT", inputPanel, "BOTTOMRIGHT", 0, -8)
    actionRow:SetHeight(30)

    local runButton = CreateButton(actionRow, 96, L.RUN, true)
    runButton:SetPoint("RIGHT", actionRow, "RIGHT", 0, 0)
    runButton:SetScript("OnClick", RunInput)
    frame.runButton = runButton

    local clearInputButton = CreateButton(actionRow, 108, L.CLEAR_INPUT, false)
    clearInputButton:SetPoint("RIGHT", runButton, "LEFT", -8, 0)
    clearInputButton:SetScript("OnClick", function()
        frame.inputPanel.editBox:SetText("")
        frame.inputPanel.editBox:SetFocus()
        SetStatus(L.READY, 0.55, 0.60, 0.65)
    end)

    local statusDot = actionRow:CreateTexture(nil, "ARTWORK")
    statusDot:SetSize(5, 5)
    statusDot:SetPoint("LEFT", actionRow, "LEFT", 2, 0)
    frame.statusDot = statusDot

    local status = actionRow:CreateFontString(nil, "OVERLAY", "GameFontHighlightSmall")
    status:SetPoint("LEFT", statusDot, "RIGHT", 7, 0)
    frame.status = status

    local resultLabel = CreateSectionLabel(runnerPage, L.RESULT)
    resultLabel:SetPoint("TOPLEFT", actionRow, "BOTTOMLEFT", 0, -13)

    local resultTextTab = CreateNavTab(runnerPage, L.TEXT)
    resultTextTab:SetSize(58, 28)
    resultTextTab:SetPoint("TOPRIGHT", actionRow, "BOTTOMRIGHT", 0, -4)
    frame.resultTextTab = resultTextTab

    local resultTreeTab = CreateNavTab(runnerPage, L.TREE)
    resultTreeTab:SetSize(58, 28)
    resultTreeTab:SetPoint("RIGHT", resultTextTab, "LEFT", -2, 0)
    frame.resultTreeTab = resultTreeTab

    local resultPanel = CreateTextArea(runnerPage, true)
    resultPanel:SetPoint("TOPLEFT", actionRow, "BOTTOMLEFT", 0, -35)
    resultPanel:SetPoint("BOTTOMRIGHT", -14, 54)
    resultPanel.editBox:SetWidth(WINDOW_WIDTH - MAIN_LEFT - 58)
    resultPanel.editBox:SetScript("OnMouseUp", function(self) self:SetFocus() end)
    frame.resultPanel = resultPanel

    local treeView = ns.CreateTreeView(runnerPage, {
        CreatePanel = CreatePanel,
        CreateScrollArea = CreateScrollArea,
        editorR = EDITOR_R,
        editorG = EDITOR_G,
        editorB = EDITOR_B,
        surfaceR = SURFACE_R,
        surfaceG = SURFACE_G,
        surfaceB = SURFACE_B,
        accentR = ACCENT_R,
        accentG = ACCENT_G,
        accentB = ACCENT_B,
        contentWidth = WINDOW_WIDTH - MAIN_LEFT - 28,
    })
    treeView.panel:SetPoint("TOPLEFT", actionRow, "BOTTOMLEFT", 0, -35)
    treeView.panel:SetPoint("BOTTOMRIGHT", -14, 54)
    frame.treeView = treeView

    local selectButton = CreateButton(runnerPage, 118, L.SELECT_RESULT, false)
    selectButton:SetPoint("BOTTOMRIGHT", -14, 14)
    selectButton:SetScript("OnClick", function()
        SetResultMode("text")
        SelectAllText(frame.resultPanel)
    end)
    frame.selectButton = selectButton

    local clearHistoryButton = CreateButton(runnerPage, HISTORY_WIDTH, L.CLEAR_HISTORY, false)
    clearHistoryButton:SetPoint("BOTTOMLEFT", 14, 14)
    clearHistoryButton:SetScript("OnClick", function()
        ns.ClearHistory()
        wipe(historyTrees)
        selectedHistoryIndex = nil
        RefreshHistory()
    end)

    inputPanel.editBox:SetScript("OnTabPressed", function(self)
        self:Insert("    ")
    end)

    window = frame
    local featureUI = {
        CreatePanel = CreatePanel,
        CreateScrollArea = CreateScrollArea,
        CreateTextArea = CreateTextArea,
        CreateButton = CreateButton,
        CreateViewTab = CreateNavTab,
        CreateSectionLabel = CreateSectionLabel,
        SetBorderColor = SetBorderColor,
        SetButtonEnabled = SetButtonEnabled,
        SetButtonPrimary = SetButtonPrimary,
        SetButtonText = SetButtonText,
        SetButtonVariant = SetButtonVariant,
        SetReadOnlyText = SetReadOnlyText,
        SelectAllText = SelectAllText,
        backdrop = BACKDROP,
        editorR = EDITOR_R,
        editorG = EDITOR_G,
        editorB = EDITOR_B,
        surfaceR = SURFACE_R,
        surfaceG = SURFACE_G,
        surfaceB = SURFACE_B,
        accentR = ACCENT_R,
        accentG = ACCENT_G,
        accentB = ACCENT_B,
        windowWidth = WINDOW_WIDTH,
    }
    frame.objectPage = ns.CreateObjectPage(frame, featureUI)
    frame.eventsPage = ns.CreateEventsPage(frame, featureUI)
    frame.tracePage = ns.CreateTracePage(frame, featureUI)
    frame.diagnosticsPage = ns.CreateDiagnosticsPage(frame, featureUI)
    frame.aboutPage = ns.CreateAboutPage(frame, featureUI)
    frame.historyButtons = historyButtons
    frame.pages = {
        runner = runnerPage,
        objects = frame.objectPage,
        events = frame.eventsPage,
        trace = frame.tracePage,
        diagnostics = frame.diagnosticsPage,
        about = frame.aboutPage,
    }
    for key, page in pairs(frame.pages) do
        if key ~= "runner" then
            page:Hide()
        end
    end

    frame:SetScript("OnHide", StopRuntimeTools)
    resultTextTab:SetScript("OnClick", function()
        SetResultMode("text")
    end)
    resultTreeTab:SetScript("OnClick", function()
        SetResultMode("tree")
    end)

    SetResult("")
    treeView:SetTree(nil)
    SetStatus(L.READY, 0.55, 0.60, 0.65)
    RefreshHistory()
    SetResultMode("text")
    SetActivePage("runner")
    return frame
end

function ns.ToggleWindow()
    if ns.IsCombatBlocked() then
        ns.PrintCombatBlocked()
        return
    end
    local frame = window or CreateWindow()
    if frame:IsShown() then
        frame:Hide()
    else
        frame:Show()
        SetActivePage(activePage)
    end
end

ns.RegisterCombatShutdown(function()
    if window and window:IsShown() then
        window:Hide()
    else
        StopRuntimeTools()
    end
end)
