local ADDON_NAME, ns = ...

-- Combat trust boundary. A secret InCombatLockdown() result counts as blocked
-- (fail closed). The PLAYER_REGEN_DISABLED driver frame is created only when
-- the first shutdown callback registers, so an addon session that never opens
-- the workbench never creates it.
local shutdownCallbacks = {}
local driver

local function IsCombatBlocked()
    if not InCombatLockdown then
        return false
    end
    local succeeded, blocked = pcall(InCombatLockdown)
    if not succeeded then
        return true
    end
    if issecretvalue and issecretvalue(blocked) then
        return true
    end
    return blocked and true or false
end

local function RunCombatShutdown()
    for index = 1, #shutdownCallbacks do
        pcall(shutdownCallbacks[index])
    end
end

local function RegisterCombatShutdown(callback)
    if type(callback) ~= "function" then
        return
    end
    shutdownCallbacks[#shutdownCallbacks + 1] = callback
    if driver then
        return
    end
    driver = CreateFrame("Frame")
    driver:RegisterEvent("PLAYER_REGEN_DISABLED")
    driver:SetScript("OnEvent", RunCombatShutdown)
end

local function PrintBlocked()
    print("|cffd83b4eLychee Dev:|r " .. ns.L.COMBAT_BLOCKED)
end

ns.Safety = {
    IsCombatBlocked = IsCombatBlocked,
    RegisterCombatShutdown = RegisterCombatShutdown,
    RunCombatShutdown = RunCombatShutdown,
    PrintBlocked = PrintBlocked,
}

-- Named entry points kept at the namespace root for feature modules.
ns.IsCombatBlocked = IsCombatBlocked
ns.RegisterCombatShutdown = RegisterCombatShutdown
