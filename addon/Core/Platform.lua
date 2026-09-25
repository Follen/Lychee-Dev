local ADDON_NAME, ns = ...

-- The profile comes from Core\ClientGate.lua's runtime build observation.
-- An unsupported build has no profile; ObserveBuild reports that failure and
-- Runtime surfaces it to the user instead of loading with a guessed identity.
local profile = ns.PlatformProfile
local function restricted(value)
    return issecretvalue and issecretvalue(value)
end

ns.Platform = {
    -- A point-in-time observation, not permission to send input later. The
    -- caller must also own a current session and observe focus after slash
    -- dispatch unwinds. Missing or restricted APIs fail closed.
    ObserveInputState = function()
        if type(IsLoggedIn) ~= "function" or type(InCombatLockdown) ~= "function"
            or type(GetCurrentKeyBoardFocus) ~= "function" then
            return nil, "input_observation_unavailable"
        end
        local ok, loggedIn = pcall(IsLoggedIn)
        if not ok or restricted(loggedIn) or type(loggedIn) ~= "boolean" then
            return nil, "input_login_unavailable"
        end
        if not loggedIn then return false, "input_not_logged_in" end
        local combatOK, combat = pcall(InCombatLockdown)
        if not combatOK or restricted(combat) or type(combat) ~= "boolean" then
            return nil, "input_combat_unavailable"
        end
        if combat then return false, "input_combat_lockdown" end
        local focusOK, focus = pcall(GetCurrentKeyBoardFocus)
        if not focusOK or restricted(focus) then return nil, "input_focus_unavailable" end
        if focus ~= nil then return false, "input_keyboard_focus" end
        return true
    end,
    ObserveActor = function()
        local character, realm = UnitFullName("player")
        local guid = UnitGUID("player")
        for _, value in pairs({ character = character, realm = realm, guid = guid }) do
            if restricted(value) then return nil, "actor_restricted_identity" end
            if type(value) ~= "string" or #value == 0 or #value > 128
                or string.find(value, "[%z\1-\31\127]") then return nil, "actor_invalid_identity" end
        end
        if character == nil or realm == nil or guid == nil then return nil, "actor_unavailable" end
        return { character = character, realm = realm, guid = guid }
    end,
    ObserveBuild = function()
        if not profile then return nil, "platform_unsupported_build" end
        local version, build, _, interface = GetBuildInfo()
        if restricted(version) or restricted(build) or restricted(interface) then
            return nil, "platform_restricted_identity"
        end
        if type(version) ~= "string" or type(build) ~= "string"
            or not string.match(build, "^%d+$") or type(interface) ~= "number" then
            return nil, "platform_invalid_identity"
        end
        if version ~= profile.version or interface ~= profile.interface then
            return nil, "platform_unsupported_build"
        end
        return { product = profile.product, build = version .. "." .. build,
            interface = interface }
    end,
}
