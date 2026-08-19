local ADDON_NAME, ns = ...
local L = ns.L

local REPOSITORY_URL = "https://github.com/Follen/Lychee-Dev"

local function CreateInfoRow(parent, labelText, valueText, y)
    local label = parent:CreateFontString(nil, "OVERLAY", "GameFontDisableSmall")
    label:SetPoint("TOPLEFT", 16, y)
    label:SetWidth(150)
    label:SetJustifyH("LEFT")
    label:SetText(labelText)
    label:SetTextColor(1, 1, 1, 0.4)

    local value = parent:CreateFontString(nil, "OVERLAY", "GameFontHighlightSmall")
    value:SetPoint("LEFT", label, "RIGHT", 12, 0)
    value:SetPoint("RIGHT", -16, 0)
    value:SetJustifyH("LEFT")
    value:SetText(valueText)
    value:SetTextColor(0.9, 0.92, 0.94, 0.92)
end

function ns.CreateAboutPage(parent, ui)
    local page = CreateFrame("Frame", nil, parent)
    page:SetAllPoints(parent)

    local title = ui.CreateSectionLabel(page, L.ABOUT_TITLE)
    title:SetPoint("TOPLEFT", 17, -84)

    local logo = page:CreateTexture(nil, "ARTWORK")
    logo:SetTexture("Interface\\AddOns\\" .. ADDON_NAME .. "\\Media\\Logo.png")
    logo:SetTexCoord(0.18, 0.79, 0.17, 0.80)
    logo:SetSize(68, 68)
    logo:SetPoint("TOPLEFT", 14, -116)

    local productName = page:CreateFontString(nil, "OVERLAY", "GameFontNormalLarge")
    productName:SetPoint("TOPLEFT", logo, "TOPRIGHT", 14, -5)
    productName:SetText(L.ADDON_TITLE)
    productName:SetTextColor(1, 1, 1, 0.98)

    local description = page:CreateFontString(nil, "OVERLAY", "GameFontHighlightSmall")
    description:SetPoint("TOPLEFT", productName, "BOTTOMLEFT", 0, -10)
    description:SetPoint("RIGHT", -14, 0)
    description:SetJustifyH("LEFT")
    description:SetText(L.ABOUT_DESCRIPTION)
    description:SetTextColor(1, 1, 1, 0.5)

    local projectLabel = ui.CreateSectionLabel(page, L.ABOUT_PROJECT)
    projectLabel:SetPoint("TOPLEFT", 17, -216)

    local projectPanel = ui.CreatePanel(page, ui.editorR, ui.editorG, ui.editorB, 0.72)
    projectPanel:SetPoint("TOPLEFT", 14, -240)
    projectPanel:SetPoint("TOPRIGHT", -14, -240)
    projectPanel:SetHeight(156)

    local version = "0.4.4"
    if C_AddOns and C_AddOns.GetAddOnMetadata then
        version = C_AddOns.GetAddOnMetadata(ADDON_NAME, "Version") or version
    end
    CreateInfoRow(projectPanel, L.ABOUT_VERSION, version, -17)
    CreateInfoRow(projectPanel, L.ABOUT_AUTHOR, "Follen", -47)
    CreateInfoRow(projectPanel, L.ABOUT_CLIENT, L.ABOUT_CLIENT_VALUE, -77)
    CreateInfoRow(projectPanel, L.ABOUT_COMMAND, "/dev", -107)
    CreateInfoRow(projectPanel, L.ABOUT_DEPENDENCY, L.ABOUT_DEPENDENCY_VALUE, -137)

    local githubLabel = ui.CreateSectionLabel(page, L.ABOUT_GITHUB)
    githubLabel:SetPoint("TOPLEFT", 17, -426)

    local urlPanel = ui.CreatePanel(page, ui.editorR, ui.editorG, ui.editorB, 1)
    urlPanel:SetPoint("TOPLEFT", 14, -450)
    urlPanel:SetPoint("TOPRIGHT", -136, -450)
    urlPanel:SetHeight(34)

    local urlBox = CreateFrame("EditBox", nil, urlPanel)
    urlBox:SetAutoFocus(false)
    urlBox:SetFontObject(ChatFontNormal)
    urlBox:SetTextColor(0.86, 0.89, 0.92)
    urlBox:SetTextInsets(10, 10, 0, 0)
    urlBox:SetPoint("TOPLEFT", 1, -1)
    urlBox:SetPoint("BOTTOMRIGHT", -1, 1)
    urlBox:SetText(REPOSITORY_URL)
    urlBox.savedText = REPOSITORY_URL
    urlBox:SetScript("OnEscapePressed", function(self) self:ClearFocus() end)
    urlBox:SetScript("OnEditFocusGained", function() ui.SetBorderColor(urlPanel, true, 0.75) end)
    urlBox:SetScript("OnEditFocusLost", function() ui.SetBorderColor(urlPanel, false) end)
    urlBox:SetScript("OnTextChanged", function(self)
        if self:GetText() ~= self.savedText then
            self:SetText(self.savedText)
            self:HighlightText()
        end
    end)
    urlBox:SetScript("OnMouseUp", function(self)
        self:SetFocus()
    end)

    local selectButton = ui.CreateButton(page, 114, L.SELECT_ADDRESS, true)
    selectButton:SetPoint("TOPRIGHT", -14, -450)
    selectButton:SetScript("OnClick", function()
        ui.SelectAllText(urlBox)
    end)

    local copyHint = page:CreateFontString(nil, "OVERLAY", "GameFontDisableSmall")
    copyHint:SetPoint("TOPLEFT", 17, -493)
    copyHint:SetText(L.ABOUT_COPY_HINT)
    copyHint:SetTextColor(1, 1, 1, 0.34)

    local safetyLabel = ui.CreateSectionLabel(page, L.ABOUT_SAFETY)
    safetyLabel:SetPoint("TOPLEFT", 17, -538)

    local safetyPanel = ui.CreatePanel(page, ui.editorR, ui.editorG, ui.editorB, 0.72)
    safetyPanel:SetPoint("TOPLEFT", 14, -562)
    safetyPanel:SetPoint("TOPRIGHT", -14, -562)
    safetyPanel:SetHeight(60)

    local safetyText = safetyPanel:CreateFontString(nil, "OVERLAY", "GameFontHighlightSmall")
    safetyText:SetPoint("LEFT", 16, 0)
    safetyText:SetPoint("RIGHT", -16, 0)
    safetyText:SetJustifyH("LEFT")
    safetyText:SetText(L.ABOUT_SAFETY_TEXT)
    safetyText:SetTextColor(0.72, 0.76, 0.8, 0.82)

    return page
end
