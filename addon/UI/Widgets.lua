local ADDON_NAME, ns = ...

local Theme = ns.Theme
local ACCENT_R, ACCENT_G, ACCENT_B = unpack(Theme.accent)
local PANEL_R, PANEL_G, PANEL_B = unpack(Theme.window)
local SURFACE_R, SURFACE_G, SURFACE_B = unpack(Theme.surfaceSelected)
local EDITOR_R, EDITOR_G, EDITOR_B = unpack(Theme.field)
local BORDER_R, BORDER_G, BORDER_B = unpack(Theme.fieldBorder)

local BACKDROP = {
    bgFile = "Interface\\Buttons\\WHITE8X8",
    edgeFile = "Interface\\Buttons\\WHITE8X8",
    edgeSize = 1,
}

local function Clamp(value, minimum, maximum)
    return math.max(minimum, math.min(maximum, value))
end

local function SetBorderColor(frame, accent, alpha)
    if frame.fieldOutline then
        -- Resting fields are one continuous surface, like the adjacent lists.
        -- Only keyboard focus needs an outline.
        frame.fieldOutline:SetColor(accent and Theme.accent or Theme.field, alpha or 1)
    end
end

local function CreatePanel(parent, r, g, b, a)
    local panel = CreateFrame("Frame", nil, parent, "BackdropTemplate")
    Theme.PaintRoundedPanel(panel, {r, g, b, a}, { radius = Theme.controlRadius })
    return panel
end

local function CreateFieldPanel(parent, frameType)
    local panel = CreateFrame(frameType or "Frame", nil, parent)
    panel.fieldOutline = Theme.CreateRoundedSurface(panel, Theme.fieldBorder, { radius = Theme.controlRadius, sublevel = -2 })
    Theme.PaintRoundedPanel(panel, Theme.field, { radius = Theme.controlRadius - 1, inset = 1, sublevel = -1 })
    SetBorderColor(panel, false)
    return panel
end

local function CreateLineInput(parent)
    local panel = CreateFieldPanel(parent)
    local edit = CreateFrame("EditBox", nil, panel)
    edit:SetAutoFocus(false)
    edit:SetFont(Theme.font, 14, "")
    edit:SetTextColor(unpack(Theme.text))
    edit:SetTextInsets(12, 12, 0, 0)
    edit:SetPoint("TOPLEFT", panel, "TOPLEFT", 1, -1)
    edit:SetPoint("BOTTOMRIGHT", panel, "BOTTOMRIGHT", -1, 1)
    edit:SetScript("OnEscapePressed", function(self) self:ClearFocus() end)
    edit:SetScript("OnEditFocusGained", function() SetBorderColor(panel, true) end)
    edit:SetScript("OnEditFocusLost", function() SetBorderColor(panel, false) end)
    panel.editBox = edit
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
    track:SetColorTexture(Theme.textDim[1], Theme.textDim[2], Theme.textDim[3], 0.35)
    track:SetPoint("TOP", 0, 0)
    track:SetPoint("BOTTOM", 0, 0)
    track:SetWidth(2)

    local thumb = scrollbar:CreateTexture(nil, "ARTWORK")
    thumb:SetColorTexture(Theme.textDim[1], Theme.textDim[2], Theme.textDim[3], 0.72)
    thumb:SetSize(7, 32)
    scrollbar:SetThumbTexture(thumb)
    scrollbar.thumb = thumb

    scrollbar:SetScript("OnEnter", function(self)
        self.thumb:SetColorTexture(ACCENT_R, ACCENT_G, ACCENT_B, 0.95)
    end)
    scrollbar:SetScript("OnLeave", function(self)
        self.thumb:SetColorTexture(Theme.textDim[1], Theme.textDim[2], Theme.textDim[3], 0.72)
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
    local button = CreateFrame("Button", nil, parent)
    button:SetSize(Theme.iconHit, Theme.iconHit)

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

    local function SetState(hovered)
        local color = hovered and Theme.accentHover or Theme.text
        firstLine:SetColorTexture(color[1], color[2], color[3], 1)
        secondLine:SetColorTexture(color[1], color[2], color[3], 1)
    end

    button:SetScript("OnEnter", function(self)
        self.isHovered = true
        SetState(true)
    end)
    button:SetScript("OnLeave", function(self)
        self.isHovered = nil
        SetState(false)
    end)
    button:SetScript("OnMouseDown", function()
        SetState(true)
    end)
    button:SetScript("OnMouseUp", function(self)
        SetState(self.isHovered)
    end)

    SetState(false)
    return button
end

local function CreateTextNavButton(parent, labelText)
    local button = CreateFrame("Button", nil, parent)
    local label = button:CreateFontString(nil, "OVERLAY", "GameFontHighlightSmall")
    Theme.SetFont(label, 12, Theme.text)
    label:SetPoint("CENTER")
    label:SetText(labelText)
    button.label = label
    button:SetSize(math.max(64, math.ceil(label:GetStringWidth()) + 24), 36)
    local function Tint(hover)
        local c = hover and Theme.accentHover or Theme.text
        label:SetTextColor(c[1], c[2], c[3], 1)
    end
    button:SetScript("OnEnter", function() Tint(true) end)
    button:SetScript("OnLeave", function() Tint(false) end)
    Tint(false)
    return button
end

local function CreateBackButton(parent)
    return CreateTextNavButton(parent, ns.L.BACK or "Back")
end

local function CreateSettingsButton(parent)
    return CreateTextNavButton(parent, ns.L.SETTINGS or "Settings")
end

local function CreateToggle(parent, onChange)
    local button = CreateFrame("Button", nil, parent)
    button:SetSize(40, 32)
    -- Lychee's 32 x 18 pill and 14-unit round thumb, with a larger hit target.
    local track = CreateFrame("Frame", nil, button)
    track:SetSize(32, 18)
    track:SetPoint("CENTER")
    Theme.PaintRoundedPanel(track, Theme.disabled, { radius = 8 })
    local thumb = CreateFrame("Frame", nil, track)
    thumb:SetSize(14, 14)
    Theme.PaintRoundedPanel(thumb, Theme.text, { radius = 6.9 })
    thumb:SetFrameLevel(track:GetFrameLevel() + 1)
    button.track, button.thumb = track, thumb
    local position = 2
    local function place(x)
        position = x
        thumb:ClearAllPoints()
        thumb:SetPoint("LEFT", track, "LEFT", x, 0)
    end
    local elapsed, start, destination
    local function finish()
        button:SetScript("OnUpdate", nil)
        place(button.checked and 16 or 2)
    end
    local function advance(_, delta)
        elapsed = math.min(.14, elapsed + delta)
        place(start + (destination - start) * (1 - (1 - elapsed / .14) ^ 3))
        if elapsed == .14 then finish() end
    end
    local function SetChecked(self, checked, instant)
        self.checked = checked == true
        track.lycheeSurface:SetColor(self.checked and Theme.accent or Theme.disabled)
        if instant or Theme.reducedMotion then finish(); return end
        elapsed, start, destination = 0, position, self.checked and 16 or 2
        if start == destination then finish() else self:SetScript("OnUpdate", advance) end
    end
    button.SetChecked = SetChecked
    button:SetScript("OnHide", finish)
    button:SetScript("OnClick", function(self)
        self:SetChecked(not self.checked)
        if onChange then onChange(self.checked) end
        if Theme.reducedMotion then finish() end
    end)
    button:SetChecked(false, true)
    return button
end

local function SetButtonLabelOffset(button, y)
    button.label:ClearAllPoints()
    button.label:SetPoint("CENTER", 0, 0)
end

local function ApplyButtonState(button, state)
    local enabled = button:IsEnabled()
    local variant = button.variant or (button.primary and "primary" or "secondary")

    local selected = variant == "selected"
    local primary = variant == "primary" or variant == "danger"
    local field = variant == "field"
    local background = field and Theme.field or primary and Theme.action or selected and Theme.surfaceSelected or Theme.surfaceHover
    button.lycheeSurface:SetColor(background, (field or selected or primary or state == "hover" or state == "pressed") and 1 or 0)
    local color = not enabled and Theme.disabled
        or state == "hover" and Theme.accentHover
        or variant == "danger" and Theme.danger
        or primary and Theme.accentHover
        or selected and Theme.text
        or Theme.textMuted
    button.label:SetTextColor(color[1], color[2], color[3], 1)
end

-- variant: "primary" / "danger" / "secondary" / "ghost" / "selected", or a
-- boolean for the historical primary=true shorthand.
local function CreateButton(parent, width, text, variant)
    variant = type(variant) == "string" and variant or (variant and "primary" or "secondary")
    local button = variant == "field" and CreateFieldPanel(parent, "Button") or CreateFrame("Button", nil, parent)
    if not button.lycheeSurface then Theme.PaintRoundedPanel(button, Theme.window, { radius = Theme.controlRadius }) end
    button.variant = variant
    button.primary = button.variant == "primary"

    local label = button:CreateFontString(nil, "OVERLAY", "GameFontHighlightSmall")
    Theme.SetFont(label, 12, Theme.text)
    label:SetPoint("CENTER")
    label:SetText(text)
    button.label = label
    button:SetSize(math.max(width, math.ceil(label:GetStringWidth()) + 24), 36)

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

-- View choices belong to a compact toolbar, not the vertical page navigation.
local function CreateModeTab(parent, text)
    local button = CreateButton(parent, 58, text, "ghost")
    Theme.SetFont(button.label, 11, Theme.textMuted)
    button:SetHeight(30)
    local function refresh(self)
        self.variant = self.active and "selected" or "ghost"
        ApplyButtonState(self, self.isHovered and "hover" or "normal")
    end
    button.SetActive = function(self, active) self.active = active and true or false; refresh(self) end
    button.RefreshEnabledState = refresh
    return button
end

local function SetButtonVariant(button, variant)
    button.variant = variant or "secondary"
    button.primary = button.variant == "primary"
    ApplyButtonState(button, "normal")
end

-- A shortcut is a sequence of keys, not an editable text field. The focused
-- recorder replaces these caps with its instruction without moving the row.
local function CreateShortcutButton(parent)
    local button = CreateButton(parent, 220, "", "ghost")
    local group = CreateFrame("Frame", nil, button)
    group:SetHeight(28)
    group:SetPoint("RIGHT", button, "RIGHT", -8, 0)
    button.keycaps = {}
    for index = 1, 4 do
        local cap = CreateFrame("Frame", nil, group)
        cap:SetHeight(28)
        Theme.PaintRoundedPanel(cap, Theme.action, { radius = 4 })
        cap.label = cap:CreateFontString(nil, "OVERLAY", "GameFontHighlightSmall")
        Theme.SetFont(cap.label, 11, Theme.text)
        cap.label:SetPoint("CENTER")
        button.keycaps[index] = cap
    end
    function button:SetShortcut(chord, recording)
        self.label:SetText(chord)
        self.label:SetShown(recording)
        group:SetShown(not recording)
        if recording then return end
        local x, count = 0, 0
        for key in chord:gmatch("[^+]+") do
            count = count + 1
            local cap = self.keycaps[count]
            if not cap then break end
            cap.label:SetText(key)
            local width = math.max(26, math.ceil(cap.label:GetStringWidth()) + 16)
            cap:SetWidth(width)
            cap:ClearAllPoints(); cap:SetPoint("LEFT", group, "LEFT", x, 0)
            cap:Show()
            x = x + width + 4
        end
        for index = count + 1, 4 do self.keycaps[index]:Hide() end
        group:SetWidth(math.max(1, x - 4))
    end
    return button
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
    button:SetSize(math.max(width, math.ceil(math.max(measure, confirmWidth)) + 24), 36)

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
    Theme.SetFont(label, 14, Theme.text)
    label:SetText(text)
    return label
end

local function CreatePageHeading(parent, title, help)
    local label = CreateSectionLabel(parent, title)
    Theme.SetFont(label, 16, Theme.text)
    label:SetPoint("TOPLEFT", 17, -22)
    if help then
        local description = parent:CreateFontString(nil, "OVERLAY", "GameFontDisableSmall")
        Theme.SetFont(description, 11, Theme.textMuted)
        description:SetPoint("TOPLEFT", 17, -50)
        description:SetPoint("TOPRIGHT", -54, -50)
        description:SetJustifyH("LEFT")
        description:SetWordWrap(false)
        description:SetText(help)
    end
    parent.heading = label
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
    accent:SetPoint("LEFT", 0, 0)
    accent:SetSize(2, 22)
    accent:Hide()
    row.accent = accent

    row.divider = row:CreateTexture(nil, "ARTWORK")
    row.divider:SetColorTexture(0, 0, 0, 0)
    return row
end

local function SetListRowState(row, selected, hovered)
    if selected then
        row.background:SetColorTexture(unpack(Theme.surfaceSelected))
        row.accent:Show()
    elseif hovered then
        row.background:SetColorTexture(unpack(Theme.surfaceHover))
        row.accent:Hide()
    else
        row.background:SetColorTexture(0, 0, 0, 0)
        row.accent:Hide()
    end
end

local function CreateNavTab(parent, text)
    local button = CreateFrame("Button", nil, parent)
    button:SetSize(120, 40)

    local surface = button:CreateTexture(nil, "BACKGROUND")
    surface:SetAllPoints()
    surface:SetColorTexture(0, 0, 0, 0)
    button.surface = surface

    local label = button:CreateFontString(nil, "OVERLAY", "GameFontHighlightSmall")
    Theme.SetFont(label, 12, Theme.textMuted)
    label:SetPoint("LEFT", 14, 0)
    label:SetText(text)
    button.label = label

    local marker = button:CreateTexture(nil, "ARTWORK")
    marker:SetPoint("LEFT", 0, 0)
    marker:SetSize(2, 22)
    button.underline = marker

    local function ApplyState(self)
        if not self:IsEnabled() then
            self.label:SetTextColor(unpack(Theme.disabled))
            self.underline:SetColorTexture(0, 0, 0, 0)
            self.surface:SetColorTexture(0, 0, 0, 0)
        elseif self.active then
            self.label:SetTextColor(unpack(Theme.text))
            self.underline:SetColorTexture(ACCENT_R, ACCENT_G, ACCENT_B, 1)
            self.surface:SetColorTexture(unpack(Theme.surfaceSelected))
        elseif self.isHovered then
            self.label:SetTextColor(unpack(Theme.accentHover))
            self.underline:SetColorTexture(0, 0, 0, 0)
            self.surface:SetColorTexture(unpack(Theme.surfaceHover))
        else
            self.label:SetTextColor(unpack(Theme.textMuted))
            self.underline:SetColorTexture(0, 0, 0, 0)
            self.surface:SetColorTexture(0, 0, 0, 0)
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
        math.ceil(button.label:GetStringWidth()) + 22), height or 36)
end

local function CreateTextArea(parent, readOnly)
    local panel = CreateFieldPanel(parent)

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
    editBox:SetFont(Theme.font, 14, "")
    editBox:SetTextInsets(4, 4, 4, 4)
    editBox:SetWidth(500)
    editBox:SetHeight(1)
    editBox.savedText = readOnly and "" or nil

    editBox:SetScript("OnEscapePressed", function(self)
        self:ClearFocus()
    end)
    editBox:SetScript("OnEditFocusGained", function()
        SetBorderColor(panel, true)
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
        if panel.placeholder then panel.placeholder:SetShown(self:GetText() == "") end
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
        editBox:SetTextColor(unpack(Theme.textMuted))
    else
        editBox:SetTextColor(unpack(Theme.text))
    end

    panel.scroll = scroll
    panel.editBox = editBox
    panel.SetPlaceholder = function(self, text)
        if not self.placeholder then
            self.placeholder = self:CreateFontString(nil, "OVERLAY", "GameFontDisableSmall")
            Theme.SetFont(self.placeholder, 11, Theme.textDim)
            self.placeholder:SetPoint("TOPLEFT", self, "TOPLEFT", 16, -16)
            self.placeholder:SetPoint("TOPRIGHT", self, "TOPRIGHT", -16, -16)
            self.placeholder:SetJustifyH("LEFT")
            self.placeholder:SetWordWrap(true)
        end
        self.placeholder:SetText(text)
        self.placeholder:SetShown(self.editBox:GetText() == "")
    end
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
        accent = Theme.accent,
        panel = Theme.window,
        surface = Theme.surfaceSelected,
        editor = Theme.field,
        border = Theme.fieldBorder,
        text = Theme.text,
        textMuted = Theme.textMuted,
        textDim = Theme.textDim,
        success = Theme.success,
        warning = Theme.warning,
        danger = Theme.danger,
    },
    Backdrop = BACKDROP,
    LOGO_TEXTURE = "Interface\\AddOns\\" .. ADDON_NAME .. "\\Media\\Logo.png",
    GITHUB_TEXTURE = "Interface\\AddOns\\" .. ADDON_NAME .. "\\Media\\GitHub.png",

    SetBorderColor = SetBorderColor,
    CreatePanel = CreatePanel,
    CreateFieldPanel = CreateFieldPanel,
    CreateLineInput = CreateLineInput,
    CreateShortcutButton = CreateShortcutButton,
    CreatePageHeading = CreatePageHeading,
    CreateScrollArea = CreateScrollArea,
    CreateCloseButton = CreateCloseButton,
    CreateBackButton = CreateBackButton,
    CreateSettingsButton = CreateSettingsButton,
    CreateToggle = CreateToggle,
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
    CreateModeTab = CreateModeTab,
    FitNavTab = FitNavTab,
    CreateTextArea = CreateTextArea,
    SetReadOnlyText = SetReadOnlyText,
    AppendReadOnlyText = AppendReadOnlyText,
    SelectAllText = SelectAllText,
}
