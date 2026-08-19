local ADDON_NAME, ns = ...
local L = ns.L

function ns.CreateExportController(parent, ui)
    local controller = {}
    local popup

    local function EnsurePopup()
        if popup then
            return popup
        end

        local overlay = ui.CreatePanel(parent, 0, 0, 0, 0.78)
        overlay:SetAllPoints(parent)
        overlay:SetFrameLevel(parent:GetFrameLevel() + 80)
        overlay:EnableMouse(true)

        local panel = ui.CreatePanel(overlay, ui.surfaceR, ui.surfaceG, ui.surfaceB, 1)
        panel:SetSize(570, 300)
        panel:SetPoint("CENTER")
        panel:SetFrameLevel(overlay:GetFrameLevel() + 1)

        local logo = panel:CreateTexture(nil, "ARTWORK")
        logo:SetTexture("Interface\\AddOns\\" .. ADDON_NAME .. "\\Media\\Logo.png")
        logo:SetTexCoord(0.18, 0.79, 0.17, 0.80)
        logo:SetSize(40, 40)
        logo:SetPoint("TOPLEFT", 20, -18)

        local title = panel:CreateFontString(nil, "OVERLAY", "GameFontNormalLarge")
        title:SetPoint("TOPLEFT", logo, "TOPRIGHT", 13, -1)
        title:SetText(L.EXPORT_SAVED_TITLE)

        local description = panel:CreateFontString(nil, "OVERLAY", "GameFontHighlightSmall")
        description:SetPoint("TOPLEFT", title, "BOTTOMLEFT", 0, -8)
        description:SetPoint("RIGHT", panel, "RIGHT", -20, 0)
        description:SetJustifyH("LEFT")
        description:SetText(L.EXPORT_SAVED_DESCRIPTION)
        description:SetTextColor(1, 1, 1, 0.62)

        local divider = panel:CreateTexture(nil, "ARTWORK")
        divider:SetColorTexture(1, 1, 1, 0.07)
        divider:SetPoint("TOPLEFT", 20, -82)
        divider:SetPoint("TOPRIGHT", -20, -82)
        divider:SetHeight(1)

        local ticketLabel = panel:CreateFontString(nil, "OVERLAY", "GameFontDisableSmall")
        ticketLabel:SetPoint("TOPLEFT", 20, -104)
        ticketLabel:SetText(L.EXPORT_TICKET)
        ticketLabel:SetTextColor(1, 1, 1, 0.38)

        local ticketPanel = ui.CreatePanel(panel, ui.editorR, ui.editorG, ui.editorB, 1)
        ticketPanel:SetPoint("TOPLEFT", ticketLabel, "BOTTOMLEFT", 0, -7)
        ticketPanel:SetPoint("TOPRIGHT", panel, "TOPRIGHT", -20, -126)
        ticketPanel:SetHeight(38)

        local selectButton = ui.CreateButton(ticketPanel, 118, L.SELECT_TICKET, false)
        selectButton:SetPoint("RIGHT", -5, 0)

        local ticketBox = CreateFrame("EditBox", nil, ticketPanel)
        ticketBox:SetPoint("TOPLEFT", 10, -1)
        ticketBox:SetPoint("BOTTOMRIGHT", selectButton, "BOTTOMLEFT", -10, 1)
        ticketBox:SetAutoFocus(false)
        ticketBox:SetFontObject(ChatFontNormal)
        ticketBox:SetTextColor(0.94, 0.95, 0.96)
        ticketBox:SetScript("OnEscapePressed", function(self) self:ClearFocus() end)
        ticketBox:SetScript("OnTextChanged", function(self)
            if not self.updatingText and self:GetText() ~= self.savedText then
                self.updatingText = true
                self:SetText(self.savedText or "")
                self.updatingText = nil
                self:HighlightText()
            end
        end)

        local hint = panel:CreateFontString(nil, "OVERLAY", "GameFontDisableSmall")
        hint:SetPoint("TOPLEFT", ticketPanel, "BOTTOMLEFT", 1, -10)
        hint:SetPoint("RIGHT", -20, 0)
        hint:SetJustifyH("LEFT")
        hint:SetText(L.EXPORT_TICKET_HELP)
        hint:SetTextColor(1, 1, 1, 0.40)

        local reloadHint = panel:CreateFontString(nil, "OVERLAY", "GameFontHighlightSmall")
        reloadHint:SetPoint("BOTTOMLEFT", 20, 61)
        reloadHint:SetPoint("RIGHT", -20, 0)
        reloadHint:SetJustifyH("LEFT")
        reloadHint:SetText(L.EXPORT_RELOAD_HINT)
        reloadHint:SetTextColor(1, 1, 1, 0.52)

        local reloadButton = ui.CreateButton(panel, 110, L.RELOAD_NOW, true)
        reloadButton:SetPoint("BOTTOMRIGHT", -20, 18)
        reloadButton:SetScript("OnClick", function()
            if ns.IsCombatBlocked() then
                ns.PrintCombatBlocked()
                return
            end
            ReloadUI()
        end)

        local laterButton = ui.CreateButton(panel, 96, L.RELOAD_LATER, false)
        laterButton:SetPoint("RIGHT", reloadButton, "LEFT", -8, 0)
        laterButton:SetScript("OnClick", function() overlay:Hide() end)

        selectButton:SetScript("OnClick", function()
            ui.SelectAllText(ticketBox)
            hint:SetText(L.EXPORT_TICKET_SELECTED)
        end)
        ticketBox:SetScript("OnMouseUp", function()
            ui.SelectAllText(ticketBox)
            hint:SetText(L.EXPORT_TICKET_SELECTED)
        end)

        popup = {
            overlay = overlay,
            ticketBox = ticketBox,
            hint = hint,
            copyButton = selectButton,
            selectButton = selectButton,
            laterButton = laterButton,
            reloadButton = reloadButton,
        }
        overlay:Hide()
        controller.popup = popup
        return popup
    end

    function controller:Save(kind, title, content, metadata)
        if type(content) == "function" then
            local succeeded, generated = pcall(content)
            if not succeeded then
                print("|cffd83b4eLychee Dev:|r " .. L.EXPORT_BUILD_FAILED .. tostring(generated))
                return nil
            end
            content = generated
        end
        local ticket, errorMessage = ns.AddExport(kind, title, content, metadata)
        if not ticket then
            print("|cffd83b4eLychee Dev:|r " .. (errorMessage or L.EXPORT_FAILED))
            return nil
        end

        local activePopup = EnsurePopup()
        activePopup.ticketBox.savedText = ticket
        activePopup.ticketBox.updatingText = true
        activePopup.ticketBox:SetText(ticket)
        activePopup.ticketBox.updatingText = nil
        activePopup.ticketBox:SetCursorPosition(0)
        activePopup.hint:SetText(L.EXPORT_TICKET_HELP)
        activePopup.overlay:Show()
        return ticket
    end

    function controller:Hide()
        if popup then
            popup.overlay:Hide()
        end
    end

    return controller
end
