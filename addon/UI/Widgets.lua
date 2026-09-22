local ADDON_NAME, ns = ...

-- The one shared visual system for the workbench: spacing, typography, colors,
-- borders and control sizes ported from the existing UI. Do not create a
-- second visual system; extend this kit instead.
local ACCENT_R, ACCENT_G, ACCENT_B = 0.847, 0.231, 0.306
local PANEL_R, PANEL_G, PANEL_B = 0.050, 0.070, 0.090
local SURFACE_R, SURFACE_G, SURFACE_B = 0.061, 0.095, 0.120
local EDITOR_R, EDITOR_G, EDITOR_B = 0.027, 0.035, 0.043
local BORDER_R, BORDER_G, BORDER_B = 0.34, 0.39, 0.44

local BACKDROP = {
    bgFile = "Interface\\Buttons\\WHITE8X8",
    edgeFile = "Interface\\Buttons\\WHITE8X8",
    edgeSize = 1,
}

local function Clamp(value, minimum, maximum)
    return math.max(minimum, math.min(maximum, value))
end

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
    SetBorderColor(panel, false, 0.26)

    local innerHighlight = panel:CreateTexture(nil, "ARTWORK")
    innerHighlight:SetColorTexture(1, 1, 1, 0.026)
    innerHighlight:SetPoint("TOPLEFT", 1, -1)
    innerHighlight:SetPoint("TOPRIGHT", -1, -1)
    innerHighlight:SetHeight(1)
    return panel
end

-- Custom scrollbar synced to the viewport; with useNativeScrollFrame the
-- viewport is a real ScrollFrame and stays the edit box's native scroll child.
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
        if variant == "ghost" then
            button:SetBackdropColor(0, 0, 0, 0)
            SetBorderColor(button, false, 0)
        else
            button:SetBackdropColor(SURFACE_R, SURFACE_G, SURFACE_B, 0.34)
            SetBorderColor(button, false, 0.18)
        end
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
        elseif variant == "ghost" then
            button:SetBackdropColor(SURFACE_R, SURFACE_G, SURFACE_B, 0.72)
        else
            button:SetBackdropColor(SURFACE_R * 0.82, SURFACE_G * 0.82, SURFACE_B * 0.82, 0.86)
        end
        SetBorderColor(button, emphasized or variant == "selected",
            variant == "ghost" and 0.28 or (emphasized and 1 or 0.64))
    elseif state == "hover" then
        if emphasized then
            button:SetBackdropColor(ACCENT_R, ACCENT_G, ACCENT_B, 1)
        elseif variant == "selected" then
            button:SetBackdropColor(ACCENT_R, ACCENT_G, ACCENT_B, 0.18)
        elseif variant == "ghost" then
            button:SetBackdropColor(SURFACE_R, SURFACE_G, SURFACE_B, 0.48)
        else
            button:SetBackdropColor(SURFACE_R * 1.22, SURFACE_G * 1.22, SURFACE_B * 1.22, 0.82)
        end
        SetBorderColor(button, emphasized or variant == "selected",
            variant == "ghost" and 0.20 or (emphasized and 1 or 0.7))
    else
        if emphasized then
            button:SetBackdropColor(ACCENT_R, ACCENT_G, ACCENT_B, 0.88)
        elseif variant == "selected" then
            button:SetBackdropColor(ACCENT_R, ACCENT_G, ACCENT_B, 0.12)
        elseif variant == "ghost" then
            button:SetBackdropColor(0, 0, 0, 0)
        else
            button:SetBackdropColor(SURFACE_R, SURFACE_G, SURFACE_B, 0.42)
        end
        SetBorderColor(button, emphasized or variant == "selected",
            variant == "ghost" and 0
                or (emphasized and 0.85 or (variant == "selected" and 0.52 or 0.30)))
    end

    local labelAlpha = 0.76
    if emphasized then
        labelAlpha = 1
    elseif variant == "selected" then
        labelAlpha = 0.94
    elseif variant == "ghost" then
        labelAlpha = state == "normal" and 0.68 or 0.92
    end
    button.label:SetTextColor(1, 1, 1, labelAlpha)
end

-- variant: "primary" / "danger" / "secondary" / "ghost" / "selected", or a
-- boolean for the historical primary=true shorthand.
local function CreateButton(parent, width, text, variant)
    local button = CreateFrame("Button", nil, parent, "BackdropTemplate")
    button:SetBackdrop(BACKDROP)
    button.variant = type(variant) == "string" and variant or (variant and "primary" or "secondary")
    button.primary = button.variant == "primary"

    local label = button:CreateFontString(nil, "OVERLAY", "GameFontHighlightSmall")
    label:SetPoint("CENTER")
    label:SetText(text)
    button.label = label
    button:SetSize(math.max(width, math.ceil(label:GetStringWidth()) + 24), 30)

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
    if button.RefreshEnabledState then
        button:RefreshEnabledState()
    else
        ApplyButtonState(button, "normal")
    end
end

local function SetButtonText(button, text)
    button.label:SetText(text or "")
end

-- Two-click confirmation: the first click arms the button (label switches to
-- confirmText, variant to danger), the second click runs onConfirm and resets.
-- No timers; callers can force a reset with button:ResetConfirm().
local function CreateConfirmButton(parent, width, text, confirmText, onConfirm, variant)
    local restVariant = variant or "secondary"
    local button = CreateButton(parent, width, text, restVariant)
    -- Size once for the longer of the two labels so the arm/disarm state
    -- change never resizes or shifts the control.
    local measure = button.label:GetStringWidth()
    button.label:SetText(confirmText)
    local confirmWidth = button.label:GetStringWidth()
    button.label:SetText(text)
    button:SetSize(math.max(width, math.ceil(math.max(measure, confirmWidth)) + 24), 30)

    local armed = false

    local function Reset()
        armed = false
        SetButtonText(button, text)
        SetButtonVariant(button, restVariant)
    end

    button.ResetConfirm = Reset
    button.IsConfirmArmed = function()
        return armed
    end
    button:SetScript("OnClick", function(self)
        if not armed then
            armed = true
            SetButtonText(self, confirmText)
            SetButtonVariant(self, "danger")
            return
        end
        Reset()
        if onConfirm then
            onConfirm(self)
        end
    end)
    return button
end

local function CreateSectionLabel(parent, text)
    local label = parent:CreateFontString(nil, "OVERLAY", "GameFontHighlightSmall")
    label:SetText(text)
    label:SetTextColor(0.70, 0.75, 0.79, 0.92)
    return label
end

local function CreateListRow(parent, height)
    local row = CreateFrame("Button", nil, parent)
    row:SetHeight(height)

    local background = row:CreateTexture(nil, "BACKGROUND")
    background:SetAllPoints()
    background:SetColorTexture(0, 0, 0, 0)
    row.background = background

    local accent = row:CreateTexture(nil, "ARTWORK")
    accent:SetColorTexture(ACCENT_R, ACCENT_G, ACCENT_B, 1)
    accent:SetPoint("TOPLEFT", 0, 0)
    accent:SetPoint("BOTTOMLEFT", 0, 0)
    accent:SetWidth(2)
    accent:Hide()
    row.accent = accent

    local divider = row:CreateTexture(nil, "ARTWORK")
    divider:SetColorTexture(1, 1, 1, 0.055)
    divider:SetPoint("BOTTOMLEFT", 0, 0)
    divider:SetPoint("BOTTOMRIGHT", 0, 0)
    divider:SetHeight(1)
    row.divider = divider
    return row
end

local function SetListRowState(row, selected, hovered)
    if selected then
        row.background:SetColorTexture(ACCENT_R, ACCENT_G, ACCENT_B, 0.095)
        row.accent:Show()
        row.divider:SetColorTexture(ACCENT_R, ACCENT_G, ACCENT_B, 0.28)
    elseif hovered then
        row.background:SetColorTexture(SURFACE_R, SURFACE_G, SURFACE_B, 0.68)
        row.accent:Hide()
        row.divider:SetColorTexture(1, 1, 1, 0.08)
    else
        row.background:SetColorTexture(0, 0, 0, 0)
        row.accent:Hide()
        row.divider:SetColorTexture(1, 1, 1, 0.055)
    end
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
        if not self:IsEnabled() then
            self.label:SetTextColor(1, 1, 1, 0.24)
            self.underline:SetColorTexture(0, 0, 0, 0)
        elseif self.active then
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

    button.RefreshEnabledState = ApplyState
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

-- Label-fit sizing: rendered label width + 22 px, never below the minimum.
local function FitNavTab(button, minimumWidth, height)
    button:SetSize(math.max(minimumWidth or 48,
        math.ceil(button.label:GetStringWidth()) + 22), height or 30)
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
    -- Rewrite-on-change guard keeps read-only text areas immutable while still
    -- letting users select and copy their content.
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

local function AppendReadOnlyText(panel, text)
    if not text or text == "" then
        return
    end
    local editBox = panel.editBox
    local offset = panel.scroll:GetVerticalScroll()
    editBox.savedText = (editBox.savedText or "") .. text
    editBox.updatingText = true
    editBox:SetText(editBox.savedText)
    editBox.updatingText = nil
    panel.scroll:UpdateScrollChildRect()
    panel.scroll:SetVerticalScroll(offset)
end

-- Copy support is always "select + Ctrl+C"; no clipboard API is used.
local function SelectAllText(target)
    if target.SelectAll then
        target:SelectAll()
        return
    end
    target:SetFocus()
    target:HighlightText()
end

ns.Widgets = {
    Colors = {
        accent = { ACCENT_R, ACCENT_G, ACCENT_B },
        panel = { PANEL_R, PANEL_G, PANEL_B },
        surface = { SURFACE_R, SURFACE_G, SURFACE_B },
        editor = { EDITOR_R, EDITOR_G, EDITOR_B },
        border = { BORDER_R, BORDER_G, BORDER_B },
    },
    Backdrop = BACKDROP,
    LOGO_TEXTURE = "Interface\\AddOns\\" .. ADDON_NAME .. "\\Media\\Logo.png",
    GITHUB_TEXTURE = "Interface\\AddOns\\" .. ADDON_NAME .. "\\Media\\GitHub.png",

    SetBorderColor = SetBorderColor,
    CreatePanel = CreatePanel,
    CreateScrollArea = CreateScrollArea,
    CreateCloseButton = CreateCloseButton,
    CreateButton = CreateButton,
    CreateConfirmButton = CreateConfirmButton,
    SetButtonVariant = SetButtonVariant,
    SetButtonPrimary = SetButtonPrimary,
    SetButtonEnabled = SetButtonEnabled,
    SetButtonText = SetButtonText,
    CreateSectionLabel = CreateSectionLabel,
    CreateListRow = CreateListRow,
    SetListRowState = SetListRowState,
    CreateNavTab = CreateNavTab,
    FitNavTab = FitNavTab,
    CreateTextArea = CreateTextArea,
    SetReadOnlyText = SetReadOnlyText,
    AppendReadOnlyText = AppendReadOnlyText,
    SelectAllText = SelectAllText,
}
