-- Isolated Lab: one physical key loads one preinstalled slot; no polling.
local prefix, limit = "LycheeInboxLab", 4
local state = { next = 1, receipts = {}, attempts = 0 }
_G.LycheeInboxLabState = state
local button = CreateFrame("Button", "LycheeInboxLabTrigger", UIParent)
button:RegisterForClicks("AnyUp")
local function status()
    local names = {}
    for i=1,limit do
        local name = prefix .. string.format("%02d", i)
        local known = C_AddOns.GetAddOnInfo(name)
        names[#names+1] = tostring(i) .. ":" .. (known and "known" or "missing")
    end
    print("MEMLOD next=" .. state.next .. " attempts=" .. state.attempts .. " " .. table.concat(names, ","))
end
button:SetScript("OnClick", function()
    if InCombatLockdown() then print("MEMLOD combat refused"); return end
    if state.next > limit then print("MEMLOD exhausted"); return end
    state.attempts = state.attempts + 1
    local index = state.next
    local name = prefix .. string.format("%02d", index)
    _G.LycheeInboxLabPayload = nil
    local ok, reason = C_AddOns.LoadAddOn(name)
    if not ok then print("MEMLOD load failed " .. tostring(reason)); return end
    state.next = index + 1 -- loaded slots are consumed even with invalid payload
    local p = _G.LycheeInboxLabPayload
    if type(p) ~= "table" or p.sequence ~= index or type(p.nonce) ~= "string" then
        print("MEMLOD invalid slot=" .. index); return
    end
    state.receipts[#state.receipts+1] = p
    print("MEMLOD accepted slot=" .. index .. " nonce=" .. p.nonce)
end)
SLASH_LYCHEEINBOXLAB1 = "/memlod"
SlashCmdList.LYCHEEINBOXLAB = function(input)
    if input == "stop" then ClearOverrideBindings(button); print("MEMLOD stopped")
    elseif input == "replay" then _G.LycheeInboxLabPayload=nil; local ok,reason=C_AddOns.LoadAddOn(prefix .. "01"); print("MEMLOD replay loaded=" .. tostring(ok) .. " payload=" .. tostring(_G.LycheeInboxLabPayload) .. " reason=" .. tostring(reason)) else status() end
end
local events=CreateFrame("Frame")
events:RegisterEvent("PLAYER_LOGIN")
events:SetScript("OnEvent", function(self)
    self:UnregisterAllEvents()
    if GetBindingAction("ALT-CTRL-F9") ~= "" then print("MEMLOD key occupied"); return end
    if InCombatLockdown() then print("MEMLOD combat refused"); return end
    SetOverrideBindingClick(button, false, "ALT-CTRL-F9", "LycheeInboxLabTrigger", "LeftButton")
    status()
end)
