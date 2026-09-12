local ADDON_NAME, ns = ...
local L = ns.L

-- Screen-level automation status overlay. Everything is created lazily on the
-- first Show call: while disabled this file creates no frames, registers no
-- events and runs no scripts.
--
-- Status blocks: three 4-physical-pixel squares in RGB order at the top-left
-- corner while an automation request runs. Error state lights only red.
-- Completion notice: one static QR code at the top-right, encoded once per
-- result and kept visible until the output-side reload, an explicit hide or a
-- safe cleanup. Dark modules come from a bounded reuse pool.

local POOL_LIMIT = 3000
local MAX_NOTICE_UI_SIZE = 480

local statusFrame
local noticeFrame
local statusBlocks
local noticeTextures = {}
local noticeTextureCount = 0
local lastNoticeJson

local function PhysicalUnit()
    if ns.Client and ns.Client.GetPhysicalPixelSize then
        return ns.Client.GetPhysicalPixelSize()
    end
    return 1
end

local function EnsureOverlay()
    if statusFrame then
        return
    end

    statusFrame = CreateFrame("Frame", "LycheeDevAutomationStatus", UIParent)
    statusFrame:SetFrameStrata("FULLSCREEN_DIALOG")
    statusFrame:SetFrameLevel(10000)
    statusFrame:SetPoint("TOPLEFT", UIParent, "TOPLEFT", 16, -16)
    statusFrame:Hide()

    statusBlocks = {
        statusFrame:CreateTexture(nil, "OVERLAY"),
        statusFrame:CreateTexture(nil, "OVERLAY"),
        statusFrame:CreateTexture(nil, "OVERLAY"),
    }
    statusBlocks[1]:SetColorTexture(0.85, 0.13, 0.15, 1)
    statusBlocks[2]:SetColorTexture(0.20, 0.78, 0.33, 1)
    statusBlocks[3]:SetColorTexture(0.24, 0.47, 0.95, 1)

    noticeFrame = CreateFrame("Frame", "LycheeDevAutomationNotice", UIParent)
    noticeFrame:SetFrameStrata("FULLSCREEN_DIALOG")
    noticeFrame:SetFrameLevel(10000)
    -- Top-left corner, matching the status blocks. The notice replaces them when
    -- it appears, so the two never share the corner at the same time.
    noticeFrame:SetPoint("TOPLEFT", UIParent, "TOPLEFT", 16, -16)
    local background = noticeFrame:CreateTexture(nil, "BACKGROUND")
    background:SetAllPoints()
    background:SetColorTexture(1, 1, 1, 1)
    noticeFrame:Hide()
end

local function LayoutStatusBlocks(mode)
    local unit = PhysicalUnit()
    local size = math.max(1, math.floor(4 * unit + 0.5))
    local gap = math.max(1, math.floor(unit + 0.5))
    for index = 1, 3 do
        local block = statusBlocks[index]
        block:ClearAllPoints()
        block:SetSize(size, size)
        block:SetPoint("TOPLEFT", statusFrame, "TOPLEFT", (index - 1) * (size + gap), 0)
        block:SetShown(mode == "running" or (mode == "error" and index == 1))
    end
    statusFrame:SetSize(size * 3 + gap * 2, size)
    statusFrame:Show()
end

local function CountDarkModules(matrix)
    local size = #matrix
    local dark = 0
    for y = 1, size do
        local column = matrix[y]
        for x = 1, size do
            if column[x] and column[x] > 0 then
                dark = dark + 1
            end
        end
    end
    return dark, size
end

local function RenderMatrix(matrix)
    local dark, size = CountDarkModules(matrix)
    if dark > POOL_LIMIT then
        return false
    end

    local unit = PhysicalUnit()
    local moduleSize = math.max(2, math.ceil(3 * unit))
    local quiet = moduleSize * 4
    if size * moduleSize + quiet * 2 > MAX_NOTICE_UI_SIZE then
        moduleSize = math.max(1, math.floor((MAX_NOTICE_UI_SIZE / (size + 8)) + 0.5))
        quiet = moduleSize * 4
    end
    local total = size * moduleSize + quiet * 2
    noticeFrame:SetSize(total, total)

    local used = 0
    for y = 1, size do
        local column = matrix[y]
        for x = 1, size do
            if column[x] and column[x] > 0 then
                used = used + 1
                local texture = noticeTextures[used]
                if not texture then
                    texture = noticeFrame:CreateTexture(nil, "OVERLAY")
                    texture:SetColorTexture(0, 0, 0, 1)
                    noticeTextures[used] = texture
                    noticeTextureCount = used
                end
                texture:SetSize(moduleSize, moduleSize)
                texture:ClearAllPoints()
                texture:SetPoint("TOPLEFT", noticeFrame, "TOPLEFT",
                    quiet + (x - 1) * moduleSize, -(quiet + (y - 1) * moduleSize))
                texture:Show()
            end
        end
    end
    for index = used + 1, noticeTextureCount do
        noticeTextures[index]:Hide()
    end
    return true
end

local Overlay = {}

function Overlay.ShowRunning()
    EnsureOverlay()
    LayoutStatusBlocks("running")
end

function Overlay.ShowError()
    EnsureOverlay()
    LayoutStatusBlocks("error")
end

function Overlay.HideStatus()
    if statusFrame then
        statusFrame:Hide()
    end
end

function Overlay.ShowNotice(json)
    EnsureOverlay()
    if json == lastNoticeJson and noticeFrame:IsShown() then
        -- Same static notice: keep the rendered matrix, never re-encode.
        return true
    end
    local matrix, encodeError = ns.AutomationQR.Encode(json, 2)
    if not matrix or not RenderMatrix(matrix) then
        noticeFrame:Hide()
        lastNoticeJson = nil
        Overlay.ShowError()
        return false, encodeError or "render_failed"
    end
    lastNoticeJson = json
    -- The notice now shares the top-left corner with the status blocks, so the
    -- blocks step aside; the caller has already stopped the run they described.
    if statusFrame then
        statusFrame:Hide()
    end
    noticeFrame:Show()
    return true
end

function Overlay.HideNotice()
    if noticeFrame then
        noticeFrame:Hide()
    end
    lastNoticeJson = nil
end

ns.AutomationOverlay = Overlay
