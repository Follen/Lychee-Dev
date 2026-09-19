local ADDON_NAME, ns = ...

-- One bootstrap listener is required to read a pending request after SV load.
-- Without a request it unregisters immediately; no overlay or timer is built.
local Reload = {}
ns.AutomationReload = Reload
local driver = CreateFrame("Frame")
local pending, expiry, markerShown
local entered, loadingDone = false, false
local requestedThisSession = false
local TTL = 120

local function ValidNonce(value)
    return type(value) == "string" and #value > 0 and #value <= 64
        and value:match("^[A-Za-z0-9%.%-]+$") ~= nil
end

local function PlainText(value)
    return not (issecretvalue and issecretvalue(value)) and type(value) == "string" and value ~= ""
end

function Reload.Clear(nonce)
    if nonce and (not pending or pending.nonce ~= nonce) then return false end
    if expiry then expiry:Cancel(); expiry = nil end
    driver:UnregisterAllEvents()
    if markerShown then ns.AutomationOverlay.HideIdentity() end
    markerShown, pending = nil, nil
    return true
end

local function TryReady()
    if not pending or not entered or not loadingDone or ns.IsCombatBlocked() then return end
    if not SlashCmdList.LYCHEEDEV or not ns.Automation or not ns.AutomationOverlay then return end
    local character, realm = UnitName("player"), GetRealmName()
    if not PlainText(character) or not PlainText(realm)
        or character ~= pending.character or realm ~= pending.realm then Reload.Clear(); return end
    local version, build = GetBuildInfo()
    local json = string.format(
        '{"v":1,"kind":"reload","run":"%s","status":"ready","ts":%d,"client":"%s","build":"%s.%s"}',
        pending.nonce, time(), ns.Client.id, version, build)
    markerShown = ns.AutomationOverlay.ShowIdentity(json) and true or false
    driver:UnregisterEvent("PLAYER_ENTERING_WORLD")
    driver:UnregisterEvent("LOADING_SCREEN_DISABLED")
end

function Reload.Request(nonce)
    if not ValidNonce(nonce) or ns.IsCombatBlocked() or requestedThisSession then return false end
    ns.InitializeDatabase()
    -- Do not overwrite an unknown/newer request format.
    if LycheeDevDB.reloadHandshake ~= nil then return false end
    local character, realm = UnitName("player"), GetRealmName()
    if not PlainText(character) or not PlainText(realm) then return false end
    Reload.Clear()
    ns.EnsureSafety()
    local request = {schema=1, nonce=nonce, requestedAt=time(),
        character=character, realm=realm}
    LycheeDevDB.reloadHandshake = request
    requestedThisSession = true
    local ok = pcall(ReloadUI)
    if not ok then
        if LycheeDevDB.reloadHandshake == request then LycheeDevDB.reloadHandshake = nil end
        return false
    end
    -- Only the next Lua session can publish ready, never this call stack.
    return true
end

driver:RegisterEvent("ADDON_LOADED")
driver:SetScript("OnEvent", function(_, event, name, isReloadingUi)
    if event == "ADDON_LOADED" then
        if name ~= ADDON_NAME then return end
        driver:UnregisterEvent("ADDON_LOADED")
        local request = type(LycheeDevDB) == "table" and LycheeDevDB.reloadHandshake
        if type(request) ~= "table" or request.schema ~= 1 then return end
        LycheeDevDB.reloadHandshake = nil
        if not ValidNonce(request.nonce) or type(request.requestedAt) ~= "number"
            or time() < request.requestedAt or time() - request.requestedAt > TTL
            or not PlainText(request.character) or not PlainText(request.realm) then return end
        pending = {nonce=request.nonce,character=request.character,realm=request.realm}
        ns.EnsureSafety()
        driver:RegisterEvent("PLAYER_ENTERING_WORLD")
        driver:RegisterEvent("LOADING_SCREEN_DISABLED")
        driver:RegisterEvent("LOADING_SCREEN_ENABLED")
        driver:RegisterEvent("PLAYER_LEAVING_WORLD")
        expiry = C_Timer.NewTimer(TTL, function() Reload.Clear() end)
    elseif event == "PLAYER_ENTERING_WORLD" then
        if not isReloadingUi then Reload.Clear(); return end
        entered = true
        TryReady()
    elseif event == "LOADING_SCREEN_DISABLED" then
        loadingDone = true
        TryReady()
    elseif event == "LOADING_SCREEN_ENABLED" then
        loadingDone = false
        if markerShown then Reload.Clear() end
    elseif event == "PLAYER_LEAVING_WORLD" then
        Reload.Clear()
    end
end)

ns.RegisterCombatShutdown(function() Reload.Clear() end)
