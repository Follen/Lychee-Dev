local ADDON_NAME, ns = ...

local shutdownCallbacks = {}
local driver

function ns.IsCombatBlocked()
    if not InCombatLockdown then
        return false
    end
    local blocked = InCombatLockdown()
    if issecretvalue and issecretvalue(blocked) then
        return true
    end
    return blocked and true or false
end

function ns.RegisterCombatShutdown(callback)
    if type(callback) == "function" then
        shutdownCallbacks[#shutdownCallbacks + 1] = callback
    end
end

local function ShutdownForCombat()
    for index = 1, #shutdownCallbacks do
        pcall(shutdownCallbacks[index])
    end
end

function ns.EnsureSafety()
    if driver then
        return
    end
    driver = CreateFrame("Frame")
    driver:RegisterEvent("PLAYER_REGEN_DISABLED")
    driver:SetScript("OnEvent", ShutdownForCombat)
end

function ns.PrintCombatBlocked()
    print("|cffd83b4eLychee Dev:|r " .. ns.L.COMBAT_BLOCKED)
end

ns.ShutdownForCombat = ShutdownForCombat
