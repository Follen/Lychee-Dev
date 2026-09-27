local ADDON, ns = ...
local P = ns.Protocol
local frame, edit, label, card, strips, timer, active, frozen, nonce
local counter, commits, sequence = 0, 0, 0
local boot, build, state = "00000000", "unknown", "starting"
local WAKE, SUBMIT, CANCEL = "ALT-CTRL-]", "ALT-CTRL-SHIFT-]", "ALT-CTRL-["
local buttons = {}
local function secret(v) return issecretvalue and issecretvalue(v) end
local function log(event, value)
    local db = LycheeInputLabDB
    if not db then return end
    if #db.events >= 64 then table.remove(db.events, 1); db.truncated = true end
    db.events[#db.events + 1] = { event = event, value = value or "", at = GetTime(),
        alt = not not IsAltKeyDown(), ctrl = not not IsControlKeyDown(), shift = not not IsShiftKeyDown() }
end
local function showQR(text)
    local matrix = assert(ns.MatrixSymbol.Encode(text))
    if not card then
        card = CreateFrame("Frame", nil, UIParent)
        card:SetFrameStrata("TOOLTIP"); card:EnableMouse(false)
        -- The production receipt owns (16,-16). The independent lab must not
        -- cover it when another investigation reloads the client.
        card:SetPoint("TOPLEFT", UIParent, "TOPLEFT", 16, -512)
        local bg = card:CreateTexture(nil, "BACKGROUND"); bg:SetAllPoints(); bg:SetColorTexture(1, 1, 1, 1)
        strips = {}
    end
    local size, used, module = #matrix, 0, 3
    card:SetSize((size + 8) * module, (size + 8) * module)
    for y = 1, size do
        local x = 1
        while x <= size do
            if matrix[x][y] > 0 then
                local first = x
                repeat x = x + 1 until x > size or matrix[x][y] <= 0
                used = used + 1
                local t = strips[used]
                if not t then t = card:CreateTexture(nil, "ARTWORK"); strips[used] = t; t:SetColorTexture(0, 0, 0, 1) end
                t:ClearAllPoints(); t:SetPoint("TOPLEFT", card, "TOPLEFT", (first + 3) * module, -(y + 3) * module)
                t:SetSize((x - first) * module, module); t:Show()
            else x = x + 1 end
        end
    end
    for i = used + 1, #strips do strips[i]:Hide() end
    card:Show()
end
local function publish(nextState, body)
    state = nextState; sequence = sequence + 1
    body = body or ""
    local payload = table.concat({"LDIL1", ns.Config.token, build, boot, tostring(sequence), state,
        nonce or "-", P.Digest(body), tostring(#body), tostring(commits),
        edit and edit:HasFocus() and "1" or "0"}, "|")
    LycheeInputLabDB.last = payload
    log(nextState, body)
    showQR(payload)
    if label then label:SetText("Lychee Input Lab: " .. nextState .. "\nEcho only; no Lua execution.\nCtrl+Alt+[ releases input.") end
end
local function release(nextState, body)
    if active and edit then
        local text = edit:GetText()
        if not secret(text) and type(text) == "string" then log("received_text", string.sub(text, 1, 129)) end
    end
    active = false; frozen = nil
    if timer then timer:Cancel(); timer = nil end
    if edit then edit:ClearFocus() end
    if frame then frame:Hide(); frame:UnregisterAllEvents() end
    if nextState then publish(nextState, body) end
end
local function cancel()
    release(nil)
    if card then card:Hide() end
    if LycheeInputLabDB then LycheeInputLabDB.hidden = true end
    state = "dismissed"; log("dismissed")
end
local function submit()
    if not active then return end
    if not frozen then release("rejected"); return end
    local body = frozen.body
    commits = commits + 1
    release("accepted", body)
end
local wake
local function key(_, keyName)
    if not active or secret(keyName) then return end
    log("key", keyName)
    if keyName == "[" then cancel()
    elseif keyName == "]" and frozen then submit()
    elseif keyName == "]" then wake() end
end
local function create()
    frame = CreateFrame("Frame", nil, UIParent)
    frame:SetSize(520, 160); frame:SetPoint("CENTER"); frame:SetFrameStrata("DIALOG")
    local bg = frame:CreateTexture(nil, "BACKGROUND"); bg:SetAllPoints(); bg:SetColorTexture(0.04, 0.04, 0.04, 0.97)
    label = frame:CreateFontString(nil, "OVERLAY", "GameFontNormal")
    label:SetPoint("TOP", 0, -14); label:SetWidth(490)
    edit = CreateFrame("EditBox", nil, frame, "InputBoxTemplate")
    edit:SetSize(470, 30); edit:SetPoint("CENTER", 0, -18); edit:SetAutoFocus(false); edit:SetMaxLetters(129)
    edit:SetScript("OnKeyDown", key)
    edit:SetScript("OnEnterPressed", function() log("ordinary_enter") end)
    edit:SetScript("OnEscapePressed", function() log("ordinary_escape") end)
    edit:SetScript("OnTabPressed", function() log("ordinary_tab") end)
    edit:SetScript("OnEditFocusLost", function() if active then release("focus_lost") end end)
    edit:SetScript("OnTextChanged", function(self)
        if not active then return end
        local text = self:GetText()
        if secret(text) then release("restricted"); return end
        -- The wake character may arrive after focus; it is never protocol data.
        if not frozen and text == "]" then self:SetText(""); return end
        if frozen then
            if text ~= frozen.frame then self:SetText(frozen.frame) end
            return
        end
        if #text > 128 then release("capacity"); return end
        local body = P.Parse(text, nonce)
        if body then
            frozen = { body = body, frame = text }
            publish("staged", body)
        end
    end)
    local close = CreateFrame("Button", nil, frame, "UIPanelButtonTemplate")
    close:SetSize(100, 24); close:SetPoint("BOTTOM", 0, 10); close:SetText("Release input"); close:SetScript("OnClick", cancel)
    frame:SetScript("OnEvent", function() if active then release("interrupted") end end)
end
-- Kept separate from its binding so the focused EditBox can invoke the same action.
wake = function(source)
    if active then log("wake_idempotent"); return end
    -- A completed transaction stays sealed until explicit dismissal. A delayed
    -- or repeated submit must not open a fresh receiver after focus is released.
    if state == "accepted" then log("completed_awaiting_dismiss"); return end
    if InCombatLockdown() then publish("combat"); return end
    if not frame then create() end
    counter = counter + 1
    LycheeInputLabDB.hidden = false
    nonce = string.format("%08x%08x", tonumber(boot, 16), counter)
    frozen = nil; edit:SetText(""); active = true
    frame:RegisterEvent("PLAYER_REGEN_DISABLED"); frame:RegisterEvent("PLAYER_LEAVING_WORLD")
    frame:Show(); edit:SetFocus()
    timer = C_Timer.NewTimer(20, function() if active then release("timeout") end end)
    publish("ready")
end
local startup = CreateFrame("Frame")
startup:RegisterEvent("ADDON_LOADED"); startup:RegisterEvent("PLAYER_LOGIN")
startup:SetScript("OnEvent", function(self, event, name)
    if event == "ADDON_LOADED" and name == ADDON then
        local hidden = type(LycheeInputLabDB) == "table" and LycheeInputLabDB.token == ns.Config.token and LycheeInputLabDB.hidden == true
        local previous = type(LycheeInputLabDB) == "table" and LycheeInputLabDB.boots or 0
        if type(previous) ~= "number" or previous < 0 or previous > 1000000 then previous = 0 end
        LycheeInputLabDB = { boots = previous + 1, events = {}, truncated = false, token = ns.Config.token, hidden = hidden }
        boot = string.format("%08x", math.floor(GetTime() * 1000) % 4294967296)
        local _, number = GetBuildInfo(); build = tostring(number)
        self:UnregisterEvent("ADDON_LOADED")
    elseif event == "PLAYER_LOGIN" then
        self:UnregisterEvent("PLAYER_LOGIN")
        if InCombatLockdown() then publish("combat"); return end
        for _, spec in ipairs({{WAKE, "Wake", function() wake("binding") end}, {SUBMIT, "Submit", function() submit() end}, {CANCEL, "Cancel", cancel}}) do
            local binding = GetBindingAction(spec[1], true)
            if binding and binding ~= "" and binding ~= "NONE" then publish("binding_conflict"); return end
        end
        for _, spec in ipairs({{WAKE, "Wake", function() wake("binding") end}, {SUBMIT, "Submit", function() submit() end}, {CANCEL, "Cancel", cancel}}) do
            local button = CreateFrame("Button", "LycheeInputLab" .. spec[2], UIParent)
            -- Global chords use binding dispatch; focused punctuation is receiver-local.
            button:RegisterForClicks("AnyDown"); button:SetScript("OnClick", spec[3]); buttons[#buttons + 1] = button
            SetOverrideBindingClick(startup, false, spec[1], button:GetName())
        end
        if not LycheeInputLabDB.hidden then publish("idle") end
    end
end)
SLASH_LYCHEEINPUTLAB1 = "/lycheeinputlab"
SlashCmdList.LYCHEEINPUTLAB = function(value)
    if value == "hide" then cancel() elseif value == "status" then publish(state) else wake() end
end
