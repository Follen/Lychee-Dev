local _, ns = ...

-- Presentation has no input authority. The receiver owns the short input
-- shield; this independent, click-through companion owns only task feedback.
local frame, receiving, requestId, startedAt, failed
local function create()
    frame = CreateFrame("Frame", nil, UIParent)
    frame:Hide()
    frame:SetSize(120, 116)
    frame:SetFrameStrata("DIALOG")
    frame:EnableMouse(false)
    frame:SetPoint("TOPLEFT", UIParent, "TOPLEFT", 240, -20)
    local anchor = CreateFrame("Frame", nil, frame)
    anchor:SetSize(96, 96)
    anchor:SetPoint("TOP", frame, "TOP", 0, 0)
    anchor:EnableMouse(false)
    frame.anchor = anchor
    frame.logo = frame:CreateTexture(nil, "ARTWORK")
    frame.logo:SetTexture(ns.Widgets.LOGO_TEXTURE)
    frame.logo:SetTexCoord(0, 1, 0, 1)
    frame.label = frame:CreateFontString(nil, "OVERLAY", "GameFontHighlightSmall")
    ns.Theme.SetFont(frame.label, 13, ns.Theme.text)
    frame.label:SetPoint("TOP", anchor, "BOTTOM", 0, 0)
    frame:SetScript("OnHide", function(self) self:SetScript("OnUpdate", nil) end)
end
local function anchor()
    if frame and not failed then ns.ReceiptView.AnchorCompanion(frame) end
end
local function render()
    if not receiving and not requestId then
        if frame then frame:SetScript("OnUpdate", nil); frame:Hide() end
        startedAt = nil
        return
    end
    if not frame then create() end
    startedAt = startedAt or GetTime()
    frame.label:SetText(receiving and ns.L.ACTIVITY_CONNECTING or ns.L.ACTIVITY_PROBE)
    anchor()
    local current = ns.Persistence.Current()
    local reduced = current and current.options and current.options.reducedMotion == true
    ns.Theme.DrawConnectionBounce(frame.logo, frame.anchor, 96, 0)
    frame:SetScript("OnUpdate", not reduced and function(self)
        ns.Theme.DrawConnectionBounce(self.logo, self.anchor, 96, math.max(0, GetTime()-startedAt))
    end or nil)
    frame:Show()
end
local function update()
    if failed then return end
    local ok = pcall(render)
    if not ok then
        -- Cosmetic failure cannot interrupt execution or retain an input
        -- shield. Fence repeated construction attempts for this runtime.
        failed = true
        if frame then
            if frame.SetScript then pcall(frame.SetScript,frame,"OnUpdate",nil) end
            if frame.Hide then pcall(frame.Hide,frame) end
        end
    end
end
ns.ActivityView = {
    Receiving = function(value) receiving = value == true; update() end,
    Begin = function(id) requestId = id; update() end,
    Finish = function(id)
        if requestId == id then requestId = nil; update() end
    end,
    Stop = function() receiving, requestId = false, nil; update() end,
    Refresh = update,
    Anchor = anchor,
    Current = function() return receiving and "connecting" or requestId and "probe" or nil, requestId end,
}
