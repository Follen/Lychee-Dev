local ADDON_NAME, ns = ...
local L = ns.L

local REPOSITORY_URL = "https://github.com/Follen/Lychee-Dev"

local function CreateMetaItem(parent, labelText, valueText, x)
    local label = parent:CreateFontString(nil, "OVERLAY", "GameFontDisableSmall")
    label:SetPoint("TOPLEFT", x, -15)
    label:SetText(labelText)
    label:SetTextColor(1, 1, 1, 0.38)

    local value = parent:CreateFontString(nil, "OVERLAY", "GameFontHighlightSmall")
    value:SetPoint("TOPLEFT", x, -39)
    value:SetText(valueText)
    value:SetTextColor(0.92, 0.94, 0.96, 0.94)
end

local function CreateStatusRow(parent, text, y, r, g, b)
    local dot = parent:CreateTexture(nil, "ARTWORK")
    dot:SetSize(5, 5)
    dot:SetPoint("TOPLEFT", 18, y - 5)
    dot:SetColorTexture(r, g, b, 0.92)

    local label = parent:CreateFontString(nil, "OVERLAY", "GameFontHighlightSmall")
    label:SetPoint("TOPLEFT", dot, "TOPRIGHT", 10, 5)
    label:SetPoint("RIGHT", -18, 0)
    label:SetJustifyH("LEFT")
    label:SetText(text)
    label:SetTextColor(0.72, 0.77, 0.81, 0.88)
end

function ns.CreateAboutPage(parent, ui)
    local page = CreateFrame("Frame", nil, parent)
    page:SetAllPoints(parent)

    local sectionTitle = ui.CreateSectionLabel(page, L.ABOUT_TITLE)
    sectionTitle:SetPoint("TOPLEFT", 17, -84)

    local logo = page:CreateTexture(nil, "ARTWORK")
    logo:SetTexture("Interface\\AddOns\\" .. ADDON_NAME .. "\\Media\\Logo.png")
    logo:SetTexCoord(0.18, 0.79, 0.17, 0.80)
    logo:SetSize(58, 58)
    logo:SetPoint("TOPLEFT", 16, -116)

    local productName = page:CreateFontString(nil, "OVERLAY", "GameFontNormalLarge")
    productName:SetPoint("TOPLEFT", logo, "TOPRIGHT", 15, -4)
    productName:SetText(L.ADDON_TITLE)
    productName:SetTextColor(1, 1, 1, 1)

    local description = page:CreateFontString(nil, "OVERLAY", "GameFontHighlightSmall")
    description:SetPoint("TOPLEFT", productName, "BOTTOMLEFT", 0, -9)
    description:SetPoint("RIGHT", -18, 0)
    description:SetJustifyH("LEFT")
    description:SetText(L.ABOUT_DESCRIPTION)
    description:SetTextColor(1, 1, 1, 0.48)

    local divider = page:CreateTexture(nil, "ARTWORK")
    divider:SetColorTexture(1, 1, 1, 0.09)
    divider:SetPoint("TOPLEFT", 14, -195)
    divider:SetPoint("TOPRIGHT", -14, -195)
    divider:SetHeight(1)

    local version = "0.4.5"
    if C_AddOns and C_AddOns.GetAddOnMetadata then
        version = C_AddOns.GetAddOnMetadata(ADDON_NAME, "Version") or version
    end

    local metadata = CreateFrame("Frame", nil, page)
    metadata:SetPoint("TOPLEFT", 14, -218)
    metadata:SetPoint("TOPRIGHT", -14, -218)
    metadata:SetHeight(70)
    local metadataBackground = metadata:CreateTexture(nil, "BACKGROUND")
    metadataBackground:SetAllPoints()
    metadataBackground:SetColorTexture(ui.surfaceR, ui.surfaceG, ui.surfaceB, 0.42)

    CreateMetaItem(metadata, L.ABOUT_VERSION, version, 18)
    CreateMetaItem(metadata, L.ABOUT_AUTHOR, "Follen", 252)
    CreateMetaItem(metadata, L.ABOUT_CLIENT, L.ABOUT_CLIENT_VALUE, 486)
    CreateMetaItem(metadata, L.ABOUT_COMMAND, "/dev", 720)

    for index = 1, 3 do
        local separator = metadata:CreateTexture(nil, "ARTWORK")
        separator:SetColorTexture(1, 1, 1, 0.07)
        separator:SetPoint("TOPLEFT", index * 234, -12)
        separator:SetPoint("BOTTOMLEFT", index * 234, 12)
        separator:SetWidth(1)
    end

    local githubLabel = ui.CreateSectionLabel(page, L.ABOUT_GITHUB)
    githubLabel:SetPoint("TOPLEFT", 17, -326)

    local urlPanel = ui.CreatePanel(page, ui.editorR, ui.editorG, ui.editorB, 1)
    urlPanel:SetPoint("TOPLEFT", 14, -350)
    urlPanel:SetPoint("TOPRIGHT", -136, -350)
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
        if not self.updatingText and self:GetText() ~= self.savedText then
            self.updatingText = true
            self:SetText(self.savedText)
            self.updatingText = nil
            self:HighlightText()
        end
    end)
    urlBox:SetScript("OnMouseUp", function(self)
        ui.SelectAllText(self)
    end)

    local selectButton = ui.CreateButton(page, 114, L.SELECT_ADDRESS, true)
    selectButton:SetPoint("TOPRIGHT", -14, -350)
    selectButton:SetScript("OnClick", function()
        ui.SelectAllText(urlBox)
    end)

    local copyHint = page:CreateFontString(nil, "OVERLAY", "GameFontDisableSmall")
    copyHint:SetPoint("TOPLEFT", 17, -393)
    copyHint:SetText(L.ABOUT_COPY_HINT)
    copyHint:SetTextColor(1, 1, 1, 0.32)

    local environmentLabel = ui.CreateSectionLabel(page, L.ABOUT_ENVIRONMENT)
    environmentLabel:SetPoint("TOPLEFT", 17, -444)

    CreateStatusRow(page, string.format(L.ABOUT_DEPENDENCY_STATUS, L.ABOUT_DEPENDENCY_VALUE), -474,
        0.42, 0.76, 0.43)
    CreateStatusRow(page, L.ABOUT_SAFETY_TEXT, -508, ui.accentR, ui.accentG, ui.accentB)

    return page
end
