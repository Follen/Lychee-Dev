local ADDON_NAME, ns = ...

local ready = false
local function restricted(value)
    return issecretvalue and issecretvalue(value)
end
local function plain(value)
    return not restricted(value)
        and type(value) == "table" and getmetatable(value) == nil
end

local function validate(value)
    if not plain(value) then return nil, "state_invalid_root" end
    if restricted(value.schema) then return nil, "state_invalid_schema" end
    if value.schema ~= 1 then return nil, "state_unsupported_schema" end
    if restricted(value.options) or (value.options ~= nil and not plain(value.options)) then return nil, "state_invalid_options" end
    if restricted(value.reports) or (value.reports ~= nil and not plain(value.reports)) then return nil, "state_invalid_reports" end
    -- Workbench stores (Core/Stores.lua) own these sections and their own
    -- schema versions; the root schema stays 1 for the bridge contract.
    if restricted(value.history) or (value.history ~= nil and not plain(value.history)) then return nil, "state_invalid_history" end
    if restricted(value.exports) or (value.exports ~= nil and not plain(value.exports)) then return nil, "state_invalid_exports" end
    if value.options then
        local enabled = value.options.bridgeEnabled
        if issecretvalue and issecretvalue(enabled) then return nil, "state_invalid_option" end
        if enabled ~= nil and type(enabled) ~= "boolean" then return nil, "state_invalid_option" end
    end
    if value.exports ~= nil then
        local nextId = value.exports.nextId
        if issecretvalue and issecretvalue(nextId) then return nil, "state_invalid_exports" end
        if nextId ~= nil and (type(nextId) ~= "number" or nextId < 0 or nextId % 1 ~= 0) then
            return nil, "state_invalid_exports"
        end
    end
    return value
end

local function bridgeState(value)
    if not plain(value) then return nil, "bridge_state_invalid_root" end
    if restricted(value.schema) or value.schema ~= 1 then return nil, "bridge_state_unsupported_schema" end
    if restricted(value.reports) or (value.reports ~= nil and not plain(value.reports)) then
        return nil, "bridge_state_invalid_reports"
    end
    return value
end

ns.Persistence = {
    Load = function()
        ready = false
        -- Called only by the owning ADDON_LOADED handler. No legacy globals
        -- are read, cleared, or imported, and newer schemas are left untouched.
        local value = LycheeToolkitDB
        if restricted(value) then return nil, "state_invalid_root" end
        if value == nil then value = { schema = 1 } end
        local valid, reason = validate(value)
        if not valid then ready = false; return nil, reason end
        -- New bridge state belongs only to this character. Older account-level
        -- reports/tickets stay untouched for host archival; never import them.
        local character = LycheeToolkitBridgeDB
        if character == nil then character = { schema = 1, reports = {} } end
        local validBridge, bridgeFailure = bridgeState(character)
        if not validBridge then return nil, bridgeFailure end
        if value.options == nil then value.options = {} end
        if character.reports == nil then character.reports = {} end
        LycheeToolkitDB = value
        LycheeToolkitBridgeDB = character
        ready = true
        return true
    end,
    Current = function()
        if not ready then return nil, "state_not_loaded" end
        -- Read the current root at use time, including after a profile re-root.
        return validate(LycheeToolkitDB)
    end,
    Bridge = function()
        if not ready then return nil, "state_not_loaded" end
        return bridgeState(LycheeToolkitBridgeDB)
    end,
    BridgeEnabled = function()
        local state = ns.Persistence.Current()
        return state and state.options and state.options.bridgeEnabled == true or false
    end,
}
