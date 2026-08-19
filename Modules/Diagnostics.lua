local ADDON_NAME, ns = ...

local diagnostics = {}
local MAX_REPORT_BYTES = 48000

local function IsSecret(value)
    return issecretvalue and issecretvalue(value)
end

local function SafeText(value)
    if value == nil then return "" end
    if IsSecret(value) then return "<secret>" end
    local text = type(value) == "string" and value or tostring(value)
    if #text > MAX_REPORT_BYTES then
        return text:sub(1, MAX_REPORT_BYTES) .. "\n... <truncated>"
    end
    return text
end

local function GetGrabber()
    local grabber = _G.BugGrabber
    if type(grabber) ~= "table" or type(grabber.GetDB) ~= "function" then
        return nil
    end
    return grabber
end

local function GetSessionId(grabber)
    if type(grabber.GetSessionId) ~= "function" then return -1 end
    local succeeded, session = pcall(grabber.GetSessionId, grabber)
    if not succeeded or IsSecret(session) then return -1 end
    return tonumber(session) or -1
end

local function MatchesQuery(entry, query)
    if query == "" then return true end
    local searchable = table.concat({
        SafeText(entry.message),
        SafeText(entry.stack),
        SafeText(entry.source),
    }, "\n"):lower()
    return searchable:find(query, 1, true) ~= nil
end

local function CollectErrors(scope, query)
    local grabber = GetGrabber()
    if not grabber then return {}, -1, false end
    local succeeded, database = pcall(grabber.GetDB, grabber)
    if not succeeded or type(database) ~= "table" then return {}, -1, false end

    local session = GetSessionId(grabber)
    query = SafeText(query):match("^%s*(.-)%s*$"):lower()
    local errors = {}
    for index = #database, 1, -1 do
        local entry = database[index]
        if type(entry) == "table"
            and (scope == "all" or tonumber(entry.session) == session)
            and MatchesQuery(entry, query) then
            errors[#errors + 1] = entry
        end
    end
    return errors, session, true
end

local function DetectSource(entry)
    local source = SafeText(entry.source)
    if source ~= "" then return source end
    local combined = (SafeText(entry.message) .. "\n" .. SafeText(entry.stack)):gsub("\\", "/")
    return combined:match("[Aa]dd[Oo]ns/([^/%s]+)") or ns.L.UNKNOWN_SOURCE
end

function diagnostics.GetErrors(scope, query)
    if ns.IsCombatBlocked() then return false, nil, ns.L.COMBAT_BLOCKED end
    local errors, session, available = CollectErrors(scope, query)
    if not available then return false, nil, ns.L.BUGGRABBER_UNAVAILABLE end
    return true, { errors = errors, session = session }
end

function diagnostics.FormatAgentReport(entry)
    if type(entry) ~= "table" then return ns.L.SELECT_ERROR_DETAIL end
    local version, build, buildDate = GetBuildInfo()
    local timestamp = tonumber(entry.time) and date("%Y-%m-%d %H:%M:%S", entry.time) or ns.L.UNKNOWN_TIME
    local sections = {
        ns.L.AGENT_REPORT_TITLE,
        "",
        ns.L.REPORT_CONTEXT,
        string.format(ns.L.REPORT_TIME, timestamp),
        string.format(ns.L.REPORT_SOURCE, DetectSource(entry)),
        string.format(ns.L.REPORT_COUNT, tonumber(entry.counter) or 1),
        string.format(ns.L.REPORT_SESSION, tonumber(entry.session) or -1),
        "",
        ns.L.REPORT_ENVIRONMENT,
        string.format(ns.L.REPORT_CLIENT, SafeText(version)),
        string.format(ns.L.REPORT_BUILD, SafeText(build), SafeText(buildDate)),
        string.format(ns.L.REPORT_LOCALE, SafeText(GetLocale())),
        "",
        ns.L.REPORT_ERROR,
        SafeText(entry.message),
        "",
        ns.L.REPORT_STACK,
        SafeText(entry.stack) ~= "" and SafeText(entry.stack) or ns.L.REPORT_NOT_AVAILABLE,
    }
    local locals = SafeText(entry.locals)
    if locals ~= "" then
        sections[#sections + 1] = ""
        sections[#sections + 1] = ns.L.REPORT_LOCALS
        sections[#sections + 1] = locals
    end
    return table.concat(sections, "\n")
end

function diagnostics.FormatError(entry)
    return diagnostics.FormatAgentReport(entry)
end

function diagnostics.ResetErrors()
    if ns.IsCombatBlocked() then return false, ns.L.COMBAT_BLOCKED end
    local grabber = GetGrabber()
    if not grabber or type(grabber.Reset) ~= "function" then
        return false, ns.L.BUGGRABBER_UNAVAILABLE
    end
    local succeeded = pcall(grabber.Reset, grabber)
    return succeeded, succeeded and ns.L.ERRORS_CLEARED or ns.L.ERRORS_CLEAR_FAILED
end

ns.Diagnostics = diagnostics
