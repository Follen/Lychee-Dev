local ADDON_NAME, ns = ...

local frame, strips, lastBytes, lastReady, lastGeneration, lastScale, lastRefresh
local focusWatch
local revision = 0
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
    revision = revision + 1
    stopFocusWatch()
    if ns.Session and type(ns.Session.CancelInputWait) == "function" then ns.Session.CancelInputWait() end
    if ns.Identity and type(ns.Identity.CancelWait) == "function" then ns.Identity.CancelWait() end
    lastBytes, lastReady, lastGeneration, lastScale = nil, nil, nil, nil
    lastRefresh = nil
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

local display
-- Focus invalidates pixels, not the pending result. Restore only through a
-- producer which observes current state; never latch an old inputReady=true.
local function suspend(refresh, generation)
    hide()
    if type(refresh) ~= "function" then return end
    local expected = revision
    local source = generation == 0 and ns.Identity or ns.Session
    if not source or type(source.WhenInputReady) ~= "function" then return end
    -- Explicit Hide, replacement, opt-out and leaving the world invalidate this
    -- one-shot wait. No frame is allocated until the first displayed receipt.
    frame:RegisterEvent("PLAYER_LEAVING_WORLD")
    frame:RegisterEvent("LOADING_SCREEN_ENABLED")
    frame:SetScript("OnEvent", hide)
    local waiting = source.WhenInputReady(function(ready)
        if revision ~= expected then return end
        if not ready then hide(); return end
        if generation ~= 0 then
            local current = ns.Session.Current()
            if not current or current.generation ~= generation then hide(); return end
        end
        local ok, receipt, readiness = pcall(refresh)
        if revision ~= expected then return end
        if not ok or not receipt then hide(); return end
        display(receipt, readiness, generation, refresh)
    end)
    if not waiting and revision == expected then hide() end
end
display = function(receipt, readiness, generation, refresh)
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
    if lastBytes == receipt and lastReady == readiness and lastGeneration == generation
        and lastScale == scale and lastRefresh == refresh then return true end
    -- A replacement owns the card and its input wait, even with identical
    -- pixels. Cancel the previous producer before installing the new one.
    hide()
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
    frame:RegisterEvent("LOADING_SCREEN_ENABLED")
    frame:RegisterEvent("PLAYER_REGEN_DISABLED")
    -- Visible receipts are point-in-time observations, not latched input
    -- permission. Invalidate on known focus/combat changes without polling.
    stopFocusWatch()
    local watch = { registry = EventRegistry }
    focusWatch = watch
    watch.registry:RegisterCallback(focusEvent, function()
        if focusWatch == watch then suspend(refresh, generation) end
    end, watch)
    lastBytes, lastReady, lastGeneration, lastScale = receipt, readiness, generation, scale
    lastRefresh = refresh
    frame:Show()
    return true
end

ns.ReceiptView = {
    Hide = hide,
    -- Explicit host dismissal after the displayed receipt's evidence has been
    -- archived. A request-scoped operation in flight owns the display, so
    -- uncertainty fails closed; clearing without a shown card is a no-op.
    Dismiss = function()
        if type(ns.ProbeQueue) == "table" and type(ns.ProbeQueue.Busy) == "function" then
            local busy, reason = ns.ProbeQueue.Busy()
            if busy == nil then return nil, reason end
            if busy then return nil, "receipt_busy" end
        end
        if type(ns.Reentry) == "table" and type(ns.Reentry.Busy) == "function" then
            local busy, reason = ns.Reentry.Busy()
            if busy == nil then return nil, reason end
            if busy then return nil, "receipt_busy" end
        end
        hide()
        return true
    end,
    Show = function(receipt, readiness, refresh)
        local session, failure = ns.Session.Current()
        if not session then hide(); return nil, failure end
        return display(receipt, readiness, session.generation, refresh)
    end,
    -- Identity markers bind no session. They share the exact display,
    -- invalidation and focus/combat rules; only the memo key differs.
    ShowIdentity = function(receipt, refresh)
        return display(receipt, nil, 0, refresh)
    end,
}
