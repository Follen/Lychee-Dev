local ADDON_NAME, ns = ...

local frame, strips, lastBytes, lastReady, lastGeneration, lastScale
local focusWatch
local focusEvent = "ChatFrame.OnEditBoxFocusGained"
-- The white receipt card matches the legacy automation notice: anchored to the
-- top-left corner, at most 480 UI units on a side, with module size derived
-- from physical pixels so high-resolution screens do not grow giant symbols.
local MAX_CARD_UI_SIZE = 480
local function restricted(value)
    return issecretvalue and issecretvalue(value)
end
local function stopFocusWatch()
    local watch = focusWatch
    focusWatch = nil
    if watch then watch.registry:UnregisterCallback(focusEvent, watch) end
end
local function hide()
    stopFocusWatch()
    if ns.Session and type(ns.Session.CancelInputWait) == "function" then ns.Session.CancelInputWait() end
    if ns.Identity and type(ns.Identity.CancelWait) == "function" then ns.Identity.CancelWait() end
    lastBytes, lastReady, lastGeneration, lastScale = nil, nil, nil, nil
    if frame then
        frame:Hide()
        frame:UnregisterAllEvents()
        frame:SetScript("OnEvent", nil)
    end
end
local function create()
    frame = CreateFrame("Frame", nil, UIParent)
    frame:Hide()
    frame:EnableMouse(false)
    frame:SetFrameStrata("DIALOG")
    frame:SetPoint("TOPLEFT", UIParent, "TOPLEFT", 16, -16)
    local background = frame:CreateTexture(nil, "BACKGROUND")
    background:SetAllPoints(frame)
    background:SetColorTexture(1, 1, 1, 1)
    strips = {}
end
local function payload(value)
    if restricted(value) or type(value) ~= "string" or #value == 0 or #value > 2048 then
        return nil, "receipt_invalid_payload"
    end
    return value
end
-- One visual system for every receipt kind. The generation key only memoizes
-- identical redisplays; it is never an identity or an input permission.
-- Module size is max(2, ceil(3 * physical pixel)) UI units, every symbol keeps
-- a 4-module quiet zone, and the whole card is clamped to MAX_CARD_UI_SIZE.
local function cardGeometry(sizes)
    local unit = 1
    if ns.Compat and type(ns.Compat.GetPhysicalPixelSize) == "function" then
        local measured = ns.Compat.GetPhysicalPixelSize()
        if not (issecretvalue and issecretvalue(measured)) and type(measured) == "number"
            and measured == measured and measured > 0 and measured < math.huge then
            unit = measured
        end
    end
    local across, tall = 8, 8
    for index, size in ipairs(sizes) do
        across = across + size
        if index > 1 then across = across + 8 end
        if size + 8 > tall then tall = size + 8 end
    end
    local modules = math.max(2, math.ceil(3 * unit))
    modules = math.min(modules,
        math.max(1, math.floor(MAX_CARD_UI_SIZE / across)),
        math.max(1, math.floor(MAX_CARD_UI_SIZE / tall)))
    return modules, across, tall
end

local function display(receipt, readiness, generation)
    local valid, failure = payload(receipt)
    if not valid then hide(); return nil, failure end
    if readiness ~= nil then
        local auxiliary, auxiliaryFailure = payload(readiness)
        if not auxiliary then hide(); return nil, auxiliaryFailure end
    end
    if restricted(EventRegistry) or type(EventRegistry) ~= "table"
        or type(EventRegistry.RegisterCallback) ~= "function"
        or type(EventRegistry.UnregisterCallback) ~= "function" then
        hide(); return nil, "receipt_events_unavailable"
    end
    local scale = UIParent:GetEffectiveScale()
    if restricted(scale) or type(scale) ~= "number" or scale ~= scale or scale <= 0 or scale == math.huge then
        hide(); return nil, "receipt_invalid_scale"
    end
    if lastBytes == receipt and lastReady == readiness and lastGeneration == generation and lastScale == scale then return true end
    -- Merge horizontal black runs instead of allocating a texture per cell.
    -- Matrix coordinates are [x][y]. Every symbol owns a 4-module quiet zone;
    -- both share invalidation and one frame.
    local runs, sizes = {}, {}
    local origin = 4
    for _, bytes in ipairs({receipt, readiness}) do
        local matrix, reason = ns.MatrixSymbol.Encode(bytes)
        if not matrix then hide(); return nil, reason end
        local size = #matrix
        if size < 21 or size > 177 then hide(); return nil, "receipt_invalid_symbol" end
        sizes[#sizes + 1] = size
        local count = 0
        for y = 1, size do
            local x = 1
            while x <= size do
                if matrix[x][y] > 0 then
                    local start = x
                    repeat x = x + 1 until x > size or matrix[x][y] <= 0
                    runs[#runs + 1] = { origin + start, y, x - start }
                    count = count + 1
                    if count > 8192 then hide(); return nil, "receipt_texture_limit" end
                else x = x + 1 end
            end
        end
        origin = origin + size + 8
    end
    local modules, across, tall = cardGeometry(sizes)
    if not frame then create() end
    frame:Hide()
    frame:SetSize(across * modules, tall * modules)
    for index, run in ipairs(runs) do
        local texture = strips[index]
        if not texture then
            texture = frame:CreateTexture(nil, "ARTWORK")
            texture:SetColorTexture(0, 0, 0, 1)
            strips[index] = texture
        end
        texture:ClearAllPoints()
        texture:SetPoint("TOPLEFT", frame, "TOPLEFT", (run[1] - 1) * modules, -(run[2] + 3) * modules)
        texture:SetSize(run[3] * modules, modules)
        texture:Show()
    end
    for index = #runs + 1, #strips do strips[index]:Hide() end
    frame:SetScript("OnEvent", hide)
    frame:RegisterEvent("PLAYER_LEAVING_WORLD")
    frame:RegisterEvent("PLAYER_REGEN_DISABLED")
    -- Visible receipts are point-in-time observations, not latched input
    -- permission. Invalidate on known focus/combat changes without polling.
    stopFocusWatch()
    local watch = { registry = EventRegistry }
    focusWatch = watch
    watch.registry:RegisterCallback(focusEvent, function()
        if focusWatch == watch then hide() end
    end, watch)
    lastBytes, lastReady, lastGeneration, lastScale = receipt, readiness, generation, scale
    frame:Show()
    return true
end

ns.ReceiptView = {
    Hide = hide,
    Show = function(receipt, readiness)
        local session, failure = ns.Session.Current()
        if not session then hide(); return nil, failure end
        return display(receipt, readiness, session.generation)
    end,
    -- Identity markers bind no session. They share the exact display,
    -- invalidation and focus/combat rules; only the memo key differs.
    ShowIdentity = function(receipt)
        return display(receipt, nil, 0)
    end,
}
