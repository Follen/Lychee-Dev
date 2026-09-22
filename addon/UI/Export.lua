local ADDON_NAME, ns = ...

-- Shared "Save (落盘)" ticket popup: 520x224 modal that reports the committed
-- record with a pre-selected, rewrite-guarded ticket box (copy = select +
-- Ctrl+C, never a clipboard API) plus combat-guarded Reload UI / Later actions.
local POPUP_WIDTH = 520
local POPUP_HEIGHT = 224

ns.ExportUI = {}

function ns.ExportUI.Create(parent)
    local W = ns.Widgets
    local colors = W.Colors
    local controller = {}
    local popup

    local function EnsurePopup()
        if popup then
            return popup
        end

        local overlay = W.CreatePanel(parent, 0, 0, 0, 0.78)
        overlay:SetAllPoints(parent)
        overlay:SetFrameLevel(parent:GetFrameLevel() + 80)
        overlay:EnableMouse(true)

        local panel = W.CreatePanel(overlay, colors.surface[1], colors.surface[2], colors.surface[3], 1)
        panel:SetSize(POPUP_WIDTH, POPUP_HEIGHT)
        panel:SetPoint("CENTER")
        panel:SetFrameLevel(overlay:GetFrameLevel() + 1)

        local logo = panel:CreateTexture(nil, "ARTWORK")
        logo:SetTexture(W.LOGO_TEXTURE)
        logo:SetTexCoord(0.18, 0.79, 0.17, 0.80)
        logo:SetSize(34, 34)
        logo:SetPoint("TOPLEFT", 20, -17)

        local title = panel:CreateFontString(nil, "OVERLAY", "GameFontNormalLarge")
        title:SetPoint("LEFT", logo, "RIGHT", 12, 1)
        title:SetText(ns.L.EXPORT_SAVED_TITLE)

        local status = panel:CreateFontString(nil, "OVERLAY", "GameFontHighlightSmall")
        status:SetPoint("TOPRIGHT", -20, -27)
        status:SetText(ns.L.EXPORT_STATUS_PENDING)
        status:SetTextColor(colors.accent[1], colors.accent[2], colors.accent[3], 0.94)

        local divider = panel:CreateTexture(nil, "ARTWORK")
        divider:SetColorTexture(1, 1, 1, 0.07)
        divider:SetPoint("TOPLEFT", 20, -66)
        divider:SetPoint("TOPRIGHT", -20, -66)
        divider:SetHeight(1)

        local ticketLabel = panel:CreateFontString(nil, "OVERLAY", "GameFontDisableSmall")
        ticketLabel:SetPoint("TOPLEFT", 20, -86)
        ticketLabel:SetText(ns.L.EXPORT_TICKET)
        ticketLabel:SetTextColor(1, 1, 1, 0.38)

        local ticketPanel = W.CreatePanel(panel, colors.editor[1], colors.editor[2], colors.editor[3], 1)
        ticketPanel:SetPoint("TOPLEFT", ticketLabel, "BOTTOMLEFT", 0, -7)
        ticketPanel:SetPoint("TOPRIGHT", panel, "TOPRIGHT", -20, -108)
        ticketPanel:SetHeight(42)

        local ticketBox = CreateFrame("EditBox", nil, ticketPanel)
        ticketBox:SetPoint("TOPLEFT", 12, -1)
        ticketBox:SetPoint("BOTTOMRIGHT", -12, 1)
        ticketBox:SetAutoFocus(false)
        ticketBox:SetFontObject(ChatFontNormal)
        ticketBox:SetTextColor(0.94, 0.95, 0.96)
        ticketBox:SetScript("OnEscapePressed", function()
            overlay:Hide()
        end)
        ticketBox:SetScript("OnTextChanged", function(self)
            if not self.updatingText and self:GetText() ~= self.savedText then
                self.updatingText = true
                self:SetText(self.savedText or "")
                self.updatingText = nil
                self:HighlightText()
            end
        end)
        ticketBox:SetScript("OnMouseUp", function()
            W.SelectAllText(ticketBox)
        end)

        local hint = panel:CreateFontString(nil, "OVERLAY", "GameFontDisableSmall")
        hint:SetPoint("TOPLEFT", ticketPanel, "BOTTOMLEFT", 1, -9)
        hint:SetPoint("RIGHT", -20, 0)
        hint:SetJustifyH("LEFT")
        hint:SetText(ns.L.EXPORT_TICKET_HELP)
        hint:SetTextColor(1, 1, 1, 0.48)

        local reloadButton = W.CreateButton(panel, 110, ns.L.RELOAD_NOW, true)
        reloadButton:SetPoint("BOTTOMRIGHT", -20, 16)
        reloadButton:SetScript("OnClick", function()
            if ns.Safety.IsCombatBlocked() then
                ns.Safety.PrintBlocked()
                return
            end
            ReloadUI()
        end)

        local laterButton = W.CreateButton(panel, 96, ns.L.RELOAD_LATER, false)
        laterButton:SetPoint("RIGHT", reloadButton, "LEFT", -8, 0)
        laterButton:SetScript("OnClick", function()
            overlay:Hide()
        end)

        popup = {
            overlay = overlay,
            panel = panel,
            ticketBox = ticketBox,
            status = status,
            hint = hint,
            laterButton = laterButton,
            reloadButton = reloadButton,
        }
        overlay:Hide()
        controller.popup = popup
        return popup
    end

    local function ShowTicket(id)
        local activePopup = EnsurePopup()
        activePopup.ticketBox.savedText = id
        activePopup.ticketBox.updatingText = true
        activePopup.ticketBox:SetText(id)
        activePopup.ticketBox.updatingText = nil
        local state = ns.Stores.Exports.GetState(id)
        activePopup.status:SetText(state == "saved" and ns.L.EXPORT_STATUS_SAVED or ns.L.EXPORT_STATUS_PENDING)
        activePopup.hint:SetText(ns.L.EXPORT_TICKET_HELP)
        activePopup.overlay:Show()
        W.SelectAllText(activePopup.ticketBox)
    end

    -- content may be a string or a function producing one (built lazily so a
    -- failing generator never commits a partial record).
    function controller:Save(kind, title, content, metadata)
        if type(content) == "function" then
            local succeeded, generated = pcall(content)
            if not succeeded then
                print("|cffd83b4eLychee Dev:|r " .. ns.L.EXPORT_BUILD_FAILED .. tostring(generated))
                return nil
            end
            content = generated
        end
        local id, errorMessage = ns.Stores.Exports.Add(kind, title, content, metadata)
        if not id then
            print("|cffd83b4eLychee Dev:|r " .. (errorMessage or ns.L.EXPORT_FAILED))
            return nil
        end

        ShowTicket(id)
        return id
    end

    function controller:Show(id)
        if type(id) ~= "string" or not ns.Stores.Exports.Get(id) then
            return false
        end
        ShowTicket(id)
        return true
    end

    function controller:Hide()
        if popup then
            popup.overlay:Hide()
        end
    end

    return controller
end
