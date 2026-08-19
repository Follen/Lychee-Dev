local ADDON_NAME, ns = ...
local L = ns.L

local REPOSITORY_URL = "https://github.com/Follen/Lychee-Dev"

local function CreateMetaItem(parent, labelText, valueText, x)
    local label = parent:CreateFontString(nil, "OVERLAY", "GameFontDisableSmall")
    label:SetPoint("TOPLEFT", x, -14)
    label:SetText(labelText)
    label:SetTextColor(1, 1, 1, 0.34)

    local value = parent:CreateFontString(nil, "OVERLAY", "GameFontHighlightSmall")
    value:SetPoint("TOPLEFT", x, -38)
    value:SetText(valueText)
    value:SetTextColor(0.94, 0.95, 0.96, 0.94)
end

local function CreateEnvironmentRow(parent, labelText, valueText, detailText, y, accent)
    local label = parent:CreateFontString(nil, "OVERLAY", "GameFontDisableSmall")
    label:SetPoint("TOPLEFT", 17, y)
    label:SetWidth(160)
    label:SetJustifyH("LEFT")
    label:SetText(labelText)
    label:SetTextColor(1, 1, 1, 0.36)

    local value = parent:CreateFontString(nil, "OVERLAY", "GameFontHighlightSmall")
    value:SetPoint("TOPLEFT", 190, y + 1)
    value:SetPoint("RIGHT", -18, 0)
    value:SetJustifyH("LEFT")
    value:SetText(valueText)
    value:SetTextColor(accent and 0.42 or 0.86, accent and 0.76 or 0.89, accent and 0.43 or 0.92, 0.94)

    local detail = parent:CreateFontString(nil, "OVERLAY", "GameFontDisableSmall")
    detail:SetPoint("TOPLEFT", value, "BOTTOMLEFT", 0, -7)
    detail:SetPoint("RIGHT", -18, 0)
    detail:SetJustifyH("LEFT")
    detail:SetText(detailText)
    detail:SetTextColor(1, 1, 1, 0.34)
end

function ns.CreateAboutPage(parent, ui)
    local page = CreateFrame("Frame", nil, parent)
    page:SetAllPoints(parent)

    local sectionTitle = ui.CreateSectionLabel(page, L.ABOUT_TITLE)
    sectionTitle:SetPoint("TOPLEFT", 17, -84)

    local logo = page:CreateTexture(nil, "ARTWORK")
    logo:SetTexture("Interface\\AddOns\\" .. ADDON_NAME .. "\\Media\\Logo.png")
    logo:SetTexCoord(0.18, 0.79, 0.17, 0.80)
    logo:SetSize(72, 72)
    logo:SetPoint("TOPLEFT", 16, -116)

    local productName = page:CreateFontString(nil, "OVERLAY", "GameFontNormalLarge")
    productName:SetPoint("TOPLEFT", logo, "TOPRIGHT", 18, -5)
    productName:SetText(L.ADDON_TITLE)
    productName:SetTextColor(1, 1, 1, 1)

    local description = page:CreateFontString(nil, "OVERLAY", "GameFontHighlightSmall")
    description:SetPoint("TOPLEFT", productName, "BOTTOMLEFT", 0, -10)
    description:SetPoint("RIGHT", -18, 0)
    description:SetJustifyH("LEFT")
    description:SetText(L.ABOUT_DESCRIPTION)
    description:SetTextColor(1, 1, 1, 0.50)

    local version = "0.5.0"
    if C_AddOns and C_AddOns.GetAddOnMetadata then
        version = C_AddOns.GetAddOnMetadata(ADDON_NAME, "Version") or version
    end

    local versionText = page:CreateFontString(nil, "OVERLAY", "GameFontDisableSmall")
    versionText:SetPoint("TOPLEFT", description, "BOTTOMLEFT", 0, -10)
    versionText:SetText(string.format(L.ABOUT_VERSION_TEXT, version))
    versionText:SetTextColor(1, 1, 1, 0.32)

    local divider = page:CreateTexture(nil, "ARTWORK")
    divider:SetColorTexture(1, 1, 1, 0.08)
    divider:SetPoint("TOPLEFT", 14, -211)
    divider:SetPoint("TOPRIGHT", -14, -211)
    divider:SetHeight(1)

    local projectLabel = ui.CreateSectionLabel(page, L.ABOUT_PROJECT)
    projectLabel:SetPoint("TOPLEFT", 17, -231)

    local metadata = CreateFrame("Frame", nil, page)
    metadata:SetPoint("TOPLEFT", 14, -253)
    metadata:SetPoint("TOPRIGHT", -14, -253)
    metadata:SetHeight(67)
    local metadataBackground = metadata:CreateTexture(nil, "BACKGROUND")
    metadataBackground:SetAllPoints()
    metadataBackground:SetColorTexture(ui.surfaceR, ui.surfaceG, ui.surfaceB, 0.34)

    CreateMetaItem(metadata, L.ABOUT_VERSION, version, 18)
    CreateMetaItem(metadata, L.ABOUT_AUTHOR, "Follen", 252)
    CreateMetaItem(metadata, L.ABOUT_CLIENT, L.ABOUT_CLIENT_VALUE, 486)
    CreateMetaItem(metadata, L.ABOUT_COMMAND, "/dev", 720)

    for index = 1, 3 do
        local separator = metadata:CreateTexture(nil, "ARTWORK")
        separator:SetColorTexture(1, 1, 1, 0.06)
        separator:SetPoint("TOPLEFT", index * 234, -11)
        separator:SetPoint("BOTTOMLEFT", index * 234, 11)
        separator:SetWidth(1)
    end

    local githubLabel = ui.CreateSectionLabel(page, L.ABOUT_GITHUB)
    githubLabel:SetPoint("TOPLEFT", 17, -346)

    local githubPanel = ui.CreatePanel(page, ui.editorR, ui.editorG, ui.editorB, 1)
    githubPanel:SetPoint("TOPLEFT", 14, -370)
    githubPanel:SetPoint("TOPRIGHT", -14, -370)
    githubPanel:SetHeight(62)

    local githubIcon = githubPanel:CreateTexture(nil, "ARTWORK")
    githubIcon:SetTexture("Interface\\AddOns\\" .. ADDON_NAME .. "\\Media\\GitHub.png")
    githubIcon:SetSize(28, 28)
    githubIcon:SetPoint("LEFT", 15, 0)
    githubIcon:SetAlpha(0.82)

    local repositoryLabel = githubPanel:CreateFontString(nil, "OVERLAY", "GameFontDisableSmall")
    repositoryLabel:SetPoint("TOPLEFT", githubIcon, "TOPRIGHT", 13, -2)
    repositoryLabel:SetText(L.ABOUT_REPOSITORY)
    repositoryLabel:SetTextColor(1, 1, 1, 0.34)

    local urlBox = CreateFrame("EditBox", nil, githubPanel)
    urlBox:SetAutoFocus(false)
    urlBox:SetFontObject(ChatFontNormal)
    urlBox:SetTextColor(0.90, 0.92, 0.94)
    urlBox:SetPoint("TOPLEFT", repositoryLabel, "BOTTOMLEFT", 0, -4)
    urlBox:SetPoint("TOPRIGHT", githubPanel, "TOPRIGHT", -148, -28)
    urlBox:SetHeight(22)
    urlBox:SetText(REPOSITORY_URL)
    urlBox.savedText = REPOSITORY_URL

    local selectButton = ui.CreateButton(githubPanel, 116, L.SELECT_ADDRESS, false)
    selectButton:SetPoint("RIGHT", -14, 0)

    local copyHint = page:CreateFontString(nil, "OVERLAY", "GameFontDisableSmall")
    copyHint:SetPoint("TOPLEFT", 17, -442)
    copyHint:SetText(L.ABOUT_COPY_HINT)
    copyHint:SetTextColor(1, 1, 1, 0.32)

    local function ResetSelectionFeedback()
        ui.SetButtonText(selectButton, L.SELECT_ADDRESS)
        ui.SetButtonVariant(selectButton, "secondary")
        copyHint:SetText(L.ABOUT_COPY_HINT)
        copyHint:SetTextColor(1, 1, 1, 0.32)
        ui.SetBorderColor(githubPanel, false)
    end

    local function SelectAddress()
        ui.SelectAllText(urlBox)
        ui.SetButtonText(selectButton, L.ADDRESS_SELECTED)
        ui.SetButtonVariant(selectButton, "selected")
        copyHint:SetText(L.ABOUT_SELECTED_HINT)
        copyHint:SetTextColor(0.72, 0.84, 0.76, 0.78)
        ui.SetBorderColor(githubPanel, true, 0.58)
    end

    urlBox:SetScript("OnEscapePressed", function(self) self:ClearFocus() end)
    urlBox:SetScript("OnEditFocusLost", ResetSelectionFeedback)
    urlBox:SetScript("OnTextChanged", function(self)
        if not self.updatingText and self:GetText() ~= self.savedText then
            self.updatingText = true
            self:SetText(self.savedText)
            self.updatingText = nil
            SelectAddress()
        end
    end)
    urlBox:SetScript("OnMouseUp", SelectAddress)
    selectButton:SetScript("OnClick", SelectAddress)

    local environmentLabel = ui.CreateSectionLabel(page, L.ABOUT_ENVIRONMENT)
    environmentLabel:SetPoint("TOPLEFT", 17, -484)

    CreateEnvironmentRow(page, L.ABOUT_DEPENDENCY, L.ABOUT_DEPENDENCY_STATUS,
        L.ABOUT_DEPENDENCY_DETAIL, -513, true)

    local rowDivider = page:CreateTexture(nil, "ARTWORK")
    rowDivider:SetColorTexture(1, 1, 1, 0.06)
    rowDivider:SetPoint("TOPLEFT", 190, -558)
    rowDivider:SetPoint("TOPRIGHT", -18, -558)
    rowDivider:SetHeight(1)

    CreateEnvironmentRow(page, L.ABOUT_SAFETY, L.ABOUT_SAFETY_STATUS,
        L.ABOUT_SAFETY_TEXT, -578, false)

    page.urlBox = urlBox
    page.selectAddressButton = selectButton
    page.copyHint = copyHint
    return page
end
