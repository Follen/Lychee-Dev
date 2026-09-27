local ADDON_NAME, ns = ...

-- About page. This is the reference example of the page API: it registers
-- itself at load time (no frames), builds lazily on first activation and keeps
-- its repository popup lazy, rewrite-guarded and pre-selected (copy = select +
-- Ctrl+C).
local REPOSITORY_URL = "https://github.com/Follen/Lychee-Dev"

local function CreateMetaItem(parent, labelText, valueText, x, y, width)
    local label = parent:CreateFontString(nil, "OVERLAY", "GameFontDisableSmall")
    ns.Theme.SetFont(label, 11, ns.Theme.textDim)
    label:SetPoint("TOPLEFT", x, y)
    label:SetWidth(width)
    label:SetJustifyH("LEFT")
    label:SetWordWrap(false)
    label:SetText(labelText)
    label:SetTextColor(unpack(ns.Theme.textDim))

    local value = parent:CreateFontString(nil, "OVERLAY", "GameFontHighlightSmall")
    ns.Theme.SetFont(value, 12, ns.Theme.text)
    value:SetPoint("TOPLEFT", x, y - 22)
    value:SetWidth(width)
    value:SetJustifyH("LEFT")
    value:SetWordWrap(false)
    value:SetText(valueText)
    value:SetTextColor(unpack(ns.Theme.text))
    return { label = label, value = value }
end

local function CreateEnvironmentRow(parent, labelText, valueText, detailText, y, accent)
    local label = parent:CreateFontString(nil, "OVERLAY", "GameFontDisableSmall")
    ns.Theme.SetFont(label, 11, ns.Theme.textDim)
    label:SetPoint("TOPLEFT", 17, y)
    label:SetWidth(160)
    label:SetJustifyH("LEFT")
    label:SetText(labelText)
    label:SetTextColor(unpack(ns.Theme.textDim))

    local value = parent:CreateFontString(nil, "OVERLAY", "GameFontHighlightSmall")
    ns.Theme.SetFont(value, 12, ns.Theme.text)
    value:SetPoint("TOPLEFT", 190, y + 1)
    value:SetPoint("RIGHT", -18, 0)
    value:SetJustifyH("LEFT")
    value:SetText(valueText)
    local valueColor = accent and ns.Theme.success or ns.Theme.text
    value:SetTextColor(unpack(valueColor))

    local detail = parent:CreateFontString(nil, "OVERLAY", "GameFontDisableSmall")
    ns.Theme.SetFont(detail, 11, ns.Theme.textDim)
    detail:SetPoint("TOPLEFT", value, "BOTTOMLEFT", 0, -7)
    detail:SetPoint("RIGHT", -18, 0)
    detail:SetJustifyH("LEFT")
    detail:SetText(detailText)
    detail:SetTextColor(unpack(ns.Theme.textDim))
end

local function BuildAboutPage(parent)
    local W = ns.Widgets
    local colors = W.Colors
    local L = ns.L

    local page = CreateFrame("Frame", nil, parent)
    page:SetAllPoints(parent)
    W.CreatePageHeading(page, L.TAB_ABOUT)

    local description = page:CreateFontString(nil, "OVERLAY", "GameFontNormalLarge")
    ns.Theme.SetFont(description, 16, ns.Theme.text)
    description:SetPoint("TOPLEFT", 17, -68)
    description:SetWidth(760)
    description:SetJustifyH("LEFT")
    description:SetText(L.ABOUT_DESCRIPTION)
    description:SetTextColor(unpack(ns.Theme.textMuted))

    -- The TOC is the source of truth; ns.Release only backs a metadata read
    -- failure so the page can never show an unrelated old number.
    local version = ns.Compat.GetAddOnMetadata(ADDON_NAME, "Version") or ns.Release

    local projectLabel = W.CreateSectionLabel(page, L.ABOUT_PROJECT)
    projectLabel:SetPoint("TOPLEFT", 17, -136)

    local metadata = CreateFrame("Frame", nil, page)
    metadata:SetPoint("TOPLEFT", 14, -160)
    metadata:SetPoint("TOPRIGHT", -14, -160)
    metadata:SetHeight(112)
    local metaItems = {
        version = CreateMetaItem(metadata, L.ABOUT_VERSION, version, 4, -12, 230),
        author = CreateMetaItem(metadata, L.ABOUT_AUTHOR, "Follen", 264, -12, 230),
        command = CreateMetaItem(metadata, L.ABOUT_COMMAND, "/dev", 524, -12, 230),
        clients = CreateMetaItem(metadata, L.ABOUT_CLIENT, L.ABOUT_CLIENT_VALUE, 4, -64, 750),
    }

    local environmentLabel = W.CreateSectionLabel(page, L.ABOUT_ENVIRONMENT)
    environmentLabel:SetPoint("TOPLEFT", 17, -329)

    local grabber = _G.BugGrabber
    local hasGrabber = not (issecretvalue and issecretvalue(grabber))
        and type(grabber) == "table" and type(grabber.GetDB) == "function"
    CreateEnvironmentRow(page, L.ABOUT_DEPENDENCY,
        hasGrabber and L.ABOUT_DEPENDENCY_STATUS or L.NOT_AVAILABLE,
        hasGrabber and L.ABOUT_DEPENDENCY_DETAIL or L.BUGGRABBER_UNAVAILABLE,
        -358, hasGrabber)

    CreateEnvironmentRow(page, L.ABOUT_SAFETY, L.ABOUT_SAFETY_STATUS,
        L.ABOUT_SAFETY_TEXT, -423, false)

    local githubButton = CreateFrame("Button", nil, page)
    githubButton:SetSize(32, 32)
    githubButton:SetPoint("BOTTOMLEFT", 17, 17)

    local githubIcon = githubButton:CreateTexture(nil, "ARTWORK")
    githubIcon:SetAllPoints()
    githubIcon:SetTexture(W.GITHUB_TEXTURE)
    githubIcon:SetAlpha(0.8)

    githubButton:SetScript("OnEnter", function()
        githubIcon:SetAlpha(1)
    end)
    githubButton:SetScript("OnLeave", function()
        githubIcon:SetAlpha(0.8)
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

        linkPopup = W.CreatePanel(UIParent, colors.surface[1], colors.surface[2], colors.surface[3], 0.98)
        linkPopup:SetSize(420, 54)
        linkPopup:SetFrameStrata("DIALOG")
        linkPopup:SetFrameLevel(500)
        linkPopup:EnableMouse(true)

        local urlPanel = W.CreateFieldPanel(linkPopup)
        urlPanel:SetPoint("TOPLEFT", 12, -10)
        urlPanel:SetPoint("BOTTOMRIGHT", -12, 10)

        local urlBox = CreateFrame("EditBox", nil, urlPanel)
        urlBox:SetPoint("TOPLEFT", 10, -1)
        urlBox:SetPoint("BOTTOMRIGHT", -10, 1)
        urlBox:SetAutoFocus(false)
        urlBox:SetFont(ns.Theme.font, 14, "")
        urlBox:SetJustifyH("CENTER")
        urlBox:SetTextColor(unpack(ns.Theme.text))
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
