local ADDON_NAME, ns = ...

-- About page. This is the reference example of the page API: it registers
-- itself at load time (no frames), builds lazily on first activation and keeps
-- its repository popup lazy, rewrite-guarded and pre-selected (copy = select +
-- Ctrl+C).
local REPOSITORY_URL = "https://github.com/Follen/Lychee-Dev"

local function CreateMetaItem(parent, labelText, valueText, x, y, width)
    local label = parent:CreateFontString(nil, "OVERLAY", "GameFontDisableSmall")
    label:SetPoint("TOPLEFT", x, y)
    label:SetWidth(width)
    label:SetJustifyH("LEFT")
    label:SetWordWrap(false)
    label:SetText(labelText)
    label:SetTextColor(1, 1, 1, 0.34)

    local value = parent:CreateFontString(nil, "OVERLAY", "GameFontHighlightSmall")
    value:SetPoint("TOPLEFT", x, y - 22)
    value:SetWidth(width)
    value:SetJustifyH("LEFT")
    value:SetWordWrap(false)
    value:SetText(valueText)
    value:SetTextColor(0.94, 0.95, 0.96, 0.94)
    return { label = label, value = value }
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

local function BuildAboutPage(parent)
    local W = ns.Widgets
    local colors = W.Colors
    local L = ns.L

    local page = CreateFrame("Frame", nil, parent)
    page:SetAllPoints(parent)

    local description = page:CreateFontString(nil, "OVERLAY", "GameFontNormalLarge")
    description:SetPoint("TOPLEFT", 17, -101)
    description:SetWidth(930)
    description:SetJustifyH("LEFT")
    description:SetText(L.ABOUT_DESCRIPTION)
    description:SetTextColor(0.92, 0.94, 0.96, 0.92)

    -- The TOC is the source of truth; ns.Release only backs a metadata read
    -- failure so the page can never show an unrelated old number.
    local version = ns.Compat.GetAddOnMetadata(ADDON_NAME, "Version") or ns.Release

    local accent = page:CreateTexture(nil, "ARTWORK")
    accent:SetColorTexture(colors.accent[1], colors.accent[2], colors.accent[3], 0.92)
    accent:SetPoint("TOPLEFT", 17, -137)
    accent:SetSize(32, 2)

    local projectLabel = W.CreateSectionLabel(page, L.ABOUT_PROJECT)
    projectLabel:SetPoint("TOPLEFT", 17, -166)

    local metadata = CreateFrame("Frame", nil, page)
    metadata:SetPoint("TOPLEFT", 14, -188)
    metadata:SetPoint("TOPRIGHT", -14, -188)
    metadata:SetHeight(112)
    local metadataBackground = metadata:CreateTexture(nil, "BACKGROUND")
    metadataBackground:SetAllPoints()
    metadataBackground:SetColorTexture(colors.surface[1], colors.surface[2], colors.surface[3], 0.24)

    local metaItems = {
        version = CreateMetaItem(metadata, L.ABOUT_VERSION, version, 18, -12, 250),
        author = CreateMetaItem(metadata, L.ABOUT_AUTHOR, "Follen", 330, -12, 250),
        command = CreateMetaItem(metadata, L.ABOUT_COMMAND, "/dev", 642, -12, 250),
        clients = CreateMetaItem(metadata, L.ABOUT_CLIENT, L.ABOUT_CLIENT_VALUE, 18, -64, 920),
    }

    for _, x in ipairs({ 312, 624 }) do
        local separator = metadata:CreateTexture(nil, "ARTWORK")
        separator:SetColorTexture(1, 1, 1, 0.06)
        separator:SetPoint("TOPLEFT", x, -10)
        separator:SetPoint("BOTTOMLEFT", x, 58)
        separator:SetWidth(1)
    end

    local metadataDivider = metadata:CreateTexture(nil, "ARTWORK")
    metadataDivider:SetColorTexture(1, 1, 1, 0.06)
    metadataDivider:SetPoint("TOPLEFT", 18, -55)
    metadataDivider:SetPoint("TOPRIGHT", -18, -55)
    metadataDivider:SetHeight(1)

    local environmentLabel = W.CreateSectionLabel(page, L.ABOUT_ENVIRONMENT)
    environmentLabel:SetPoint("TOPLEFT", 17, -329)

    CreateEnvironmentRow(page, L.ABOUT_DEPENDENCY, L.ABOUT_DEPENDENCY_STATUS,
        L.ABOUT_DEPENDENCY_DETAIL, -358, true)

    local rowDivider = page:CreateTexture(nil, "ARTWORK")
    rowDivider:SetColorTexture(1, 1, 1, 0.06)
    rowDivider:SetPoint("TOPLEFT", 190, -403)
    rowDivider:SetPoint("TOPRIGHT", -18, -403)
    rowDivider:SetHeight(1)

    CreateEnvironmentRow(page, L.ABOUT_SAFETY, L.ABOUT_SAFETY_STATUS,
        L.ABOUT_SAFETY_TEXT, -423, false)

    local githubButton = CreateFrame("Button", nil, page)
    githubButton:SetSize(38, 38)
    githubButton:SetPoint("BOTTOMLEFT", 17, 17)

    local githubIcon = githubButton:CreateTexture(nil, "ARTWORK")
    githubIcon:SetAllPoints()
    githubIcon:SetTexture(W.GITHUB_TEXTURE)
    githubIcon:SetAlpha(0.38)

    githubButton:SetScript("OnEnter", function()
        githubIcon:SetAlpha(0.72)
    end)
    githubButton:SetScript("OnLeave", function()
        githubIcon:SetAlpha(0.38)
    end)

    local linkPopup
    local linkBackdrop

    local function HideLinkPopup()
        if linkPopup then
            linkPopup:Hide()
        end
        if linkBackdrop then
            linkBackdrop:Hide()
        end
    end

    local function EnsureLinkPopup()
        if linkPopup then
            return linkPopup
        end

        linkBackdrop = CreateFrame("Button", nil, UIParent)
        linkBackdrop:SetAllPoints(UIParent)
        linkBackdrop:SetFrameStrata("DIALOG")
        linkBackdrop:SetFrameLevel(499)
        linkBackdrop:RegisterForClicks("AnyUp")
        linkBackdrop:SetScript("OnClick", HideLinkPopup)
        local shade = linkBackdrop:CreateTexture(nil, "BACKGROUND")
        shade:SetAllPoints()
        shade:SetColorTexture(0, 0, 0, 0.24)

        linkPopup = W.CreatePanel(UIParent, colors.surface[1], colors.surface[2], colors.surface[3], 0.98)
        linkPopup:SetSize(420, 54)
        linkPopup:SetFrameStrata("DIALOG")
        linkPopup:SetFrameLevel(500)
        linkPopup:EnableMouse(true)

        local urlPanel = W.CreatePanel(linkPopup, colors.editor[1], colors.editor[2], colors.editor[3], 1)
        urlPanel:SetPoint("TOPLEFT", 12, -10)
        urlPanel:SetPoint("BOTTOMRIGHT", -12, 10)

        local urlBox = CreateFrame("EditBox", nil, urlPanel)
        urlBox:SetPoint("TOPLEFT", 10, -1)
        urlBox:SetPoint("BOTTOMRIGHT", -10, 1)
        urlBox:SetAutoFocus(false)
        urlBox:SetFontObject(ChatFontNormal)
        urlBox:SetJustifyH("CENTER")
        urlBox:SetTextColor(0.94, 0.95, 0.96)
        urlBox.savedText = REPOSITORY_URL
        urlBox:SetText(REPOSITORY_URL)
        urlBox:SetScript("OnEscapePressed", function(self)
            self:ClearFocus()
            HideLinkPopup()
        end)
        urlBox:SetScript("OnMouseUp", function(self)
            self:SetFocus()
            self:HighlightText()
        end)
        urlBox:SetScript("OnTextChanged", function(self)
            if not self.updatingText and self:GetText() ~= self.savedText then
                self.updatingText = true
                self:SetText(self.savedText)
                self.updatingText = nil
                self:HighlightText()
            end
        end)

        linkPopup:SetScript("OnMouseDown", function()
            urlBox:SetFocus()
            urlBox:HighlightText()
        end)
        linkPopup:Hide()
        linkBackdrop:Hide()

        linkPopup.urlBox = urlBox
        linkPopup.backdrop = linkBackdrop
        page.linkPopup = linkPopup
        page.linkBackdrop = linkBackdrop
        page.urlBox = urlBox
        return linkPopup
    end

    githubButton:SetScript("OnClick", function()
        local popup = EnsureLinkPopup()
        popup:ClearAllPoints()
        popup:SetPoint("BOTTOMLEFT", githubButton, "TOPLEFT", 0, 10)
        linkBackdrop:Show()
        popup:Show()
        popup.urlBox:SetFocus()
        popup.urlBox:HighlightText()
    end)
    page:SetScript("OnHide", HideLinkPopup)

    page.githubButton = githubButton
    page.githubIcon = githubIcon
    page.metaItems = metaItems
    page.HideLinkPopup = HideLinkPopup
    return page
end

ns.Workbench.RegisterPage({
    key = "about",
    titleKey = "TAB_ABOUT",
    build = BuildAboutPage,
})
