local ADDON_NAME, ns = ...

-- Error diagnostics over the !BugGrabber error provider. The integration is
-- SOFT: no TOC dependency, the provider is read under pcall at use time, and a
-- missing provider is an explicit state, never fabricated errors.
local diagnostics = {}
local MAX_REPORT_BYTES = 48000
local SNAPSHOT_MAX_ERRORS = 100
local SNAPSHOT_TEXT_FIELDS = { "message", "stack", "locals", "source" }

-- Stable machine codes for snapshot failures; the caller reports them as data.
local SNAPSHOT_ERROR_COMBAT = "combat_blocked"
local SNAPSHOT_ERROR_COUNT = "invalid_count"
local SNAPSHOT_ERROR_PROVIDER = "provider_unavailable"
local SNAPSHOT_ERROR_PROVIDER_CALL = "provider_error"

diagnostics.SNAPSHOT_ERROR_COMBAT = SNAPSHOT_ERROR_COMBAT
diagnostics.SNAPSHOT_ERROR_COUNT = SNAPSHOT_ERROR_COUNT
diagnostics.SNAPSHOT_ERROR_PROVIDER = SNAPSHOT_ERROR_PROVIDER
diagnostics.SNAPSHOT_ERROR_PROVIDER_CALL = SNAPSHOT_ERROR_PROVIDER_CALL

local function IsSecret(value)
    return issecretvalue and issecretvalue(value)
end

-- Numeric provider fields are converted only after a secrecy check.
local function SafeNumber(value)
    if value == nil or IsSecret(value) then
        return nil
    end
    return tonumber(value)
end

-- Every report field passes here: secrets render as <secret>, everything else
-- is capped at 48 KB with a visible truncation marker.
local function SafeText(value)
    if value == nil then
        return ""
    end
    if IsSecret(value) then
        return "<secret>"
    end
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
    if type(grabber.GetSessionId) ~= "function" then
        return -1
    end
    local succeeded, session = pcall(grabber.GetSessionId, grabber)
    if not succeeded or IsSecret(session) then
        return -1
    end
    return tonumber(session) or -1
end

local function MatchesQuery(entry, query)
    if query == "" then
        return true
    end
    local searchable = table.concat({
        SafeText(entry.message),
        SafeText(entry.stack),
        SafeText(entry.source),
    }, "\n"):lower()
    return searchable:find(query, 1, true) ~= nil
end

-- Newest first (reverse provider-storage order). scope "current" keeps only
-- entries recorded in the active capture session.
local function CollectErrors(scope, query)
    local grabber = GetGrabber()
    if not grabber then
        return {}, -1, false
    end
    local succeeded, database = pcall(grabber.GetDB, grabber)
    if not succeeded or type(database) ~= "table" then
        return {}, -1, false
    end

    local session = GetSessionId(grabber)
    query = SafeText(query):match("^%s*(.-)%s*$"):lower()
    local errors = {}
    for index = #database, 1, -1 do
        local entry = database[index]
        if type(entry) == "table"
            and (scope == "all" or SafeNumber(entry.session) == session)
            and MatchesQuery(entry, query) then
            errors[#errors + 1] = entry
        end
    end
    return errors, session, true
end

local function DetectSource(entry)
    local source = SafeText(entry.source)
    if source ~= "" then
        return source
    end
    local combined = (SafeText(entry.message) .. "\n" .. SafeText(entry.stack)):gsub("\\", "/")
    return combined:match("[Aa]dd[Oo]ns/([^/%s]+)") or ns.L.UNKNOWN_SOURCE
end

diagnostics.DetectSource = DetectSource
diagnostics.SafeText = SafeText

function diagnostics.GetErrors(scope, query)
    if ns.Safety.IsCombatBlocked() then
        return false, nil, ns.L.COMBAT_BLOCKED
    end
    local errors, session, available = CollectErrors(scope, query)
    if not available then
        return false, nil, ns.L.BUGGRABBER_UNAVAILABLE
    end
    return true, { errors = errors, session = session }
end

-- Human-readable agent report: title, context, environment, error, stack (or
-- REPORT_NOT_AVAILABLE) and optional locals. Every field is SafeText-capped.
function diagnostics.FormatAgentReport(entry)
    if type(entry) ~= "table" then
        return ns.L.SELECT_ERROR_DETAIL
    end
    local version, build, buildDate = GetBuildInfo()
    local timestamp = SafeNumber(entry.time) and date("%Y-%m-%d %H:%M:%S", SafeNumber(entry.time)) or ns.L.UNKNOWN_TIME
    local sections = {
        ns.L.AGENT_REPORT_TITLE,
        "",
        ns.L.REPORT_CONTEXT,
        string.format(ns.L.REPORT_TIME, timestamp),
        string.format(ns.L.REPORT_SOURCE, DetectSource(entry)),
        string.format(ns.L.REPORT_COUNT, SafeNumber(entry.counter) or 1),
        string.format(ns.L.REPORT_SESSION, SafeNumber(entry.session) or -1),
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
    if ns.Safety.IsCombatBlocked() then
        return false, ns.L.COMBAT_BLOCKED
    end
    local grabber = GetGrabber()
    if not grabber or type(grabber.Reset) ~= "function" then
        return false, ns.L.BUGGRABBER_UNAVAILABLE
    end
    local succeeded = pcall(grabber.Reset, grabber)
    return succeeded, succeeded and ns.L.ERRORS_CLEARED or ns.L.ERRORS_CLEAR_FAILED
end

local function GetProviderVersion(grabber)
    if type(grabber.version) == "string" and grabber.version ~= "" and not IsSecret(grabber.version) then
        return grabber.version
    end
    if ns.Compat and ns.Compat.GetAddOnMetadata then
        return ns.Compat.GetAddOnMetadata("!BugGrabber", "Version")
    end
    return nil
end

-- Copy one error entry immediately so later BugGrabber updates cannot change
-- it. Absent, secret or non-string text fields land in missingFields instead of
-- being tostring-substituted; source is optional, the other text fields make
-- the copy incomplete.
local function CopyErrorEntry(entry)
    local missing = {}
    local copy = {}
    for index = 1, #SNAPSHOT_TEXT_FIELDS do
        local field = SNAPSHOT_TEXT_FIELDS[index]
        local value = entry[field]
        if value == nil then
            if field ~= "source" then
                missing[#missing + 1] = field
            end
        elseif IsSecret(value) then
            missing[#missing + 1] = field
        elseif type(value) ~= "string" then
            missing[#missing + 1] = field
        else
            copy[field] = value
        end
    end
    local timeValue = SafeNumber(entry.time)
    local sessionValue = SafeNumber(entry.session)
    local counterValue = SafeNumber(entry.counter)
    if not timeValue then
        missing[#missing + 1] = "time"
    end
    if not sessionValue then
        missing[#missing + 1] = "session"
    end
    if not counterValue then
        missing[#missing + 1] = "counter"
    end
    copy.time = timeValue
    copy.session = sessionValue
    copy.counter = counterValue
    copy.missingFields = missing
    return copy
end

diagnostics.CopyErrorEntry = CopyErrorEntry

-- Machine snapshot for the future host "live bugs" entry and automation
-- exports. Never calls FormatAgentReport and never truncates fields silently.
-- Returns nil plus a stable error code on failure: combat_blocked,
-- invalid_count, provider_unavailable, provider_error.
function diagnostics.SnapshotRecentErrors(count, scope)
    if ns.Safety.IsCombatBlocked() then
        return nil, SNAPSHOT_ERROR_COMBAT
    end
    if type(count) ~= "number" or count ~= math.floor(count)
        or count < 1 or count > SNAPSHOT_MAX_ERRORS then
        return nil, SNAPSHOT_ERROR_COUNT
    end
    local grabber = GetGrabber()
    if not grabber then
        return nil, SNAPSHOT_ERROR_PROVIDER
    end
    local succeeded, database = pcall(grabber.GetDB, grabber)
    if not succeeded or type(database) ~= "table" then
        return nil, SNAPSHOT_ERROR_PROVIDER_CALL
    end

    local session = GetSessionId(grabber)
    local allSessions = scope ~= "current_session"
    local availableCount = 0
    local errors = {}
    local incompleteReasons = {}
    for index = #database, 1, -1 do
        local entry = database[index]
        if type(entry) == "table" then
            if allSessions or SafeNumber(entry.session) == session then
                availableCount = availableCount + 1
                if #errors < count then
                    local copy = CopyErrorEntry(entry)
                    errors[#errors + 1] = copy
                    for fieldIndex = 1, #copy.missingFields do
                        incompleteReasons[#incompleteReasons + 1]
                            = "error[" .. #errors .. "]." .. copy.missingFields[fieldIndex]
                    end
                end
            end
        end
    end

    return {
        scope = allSessions and "provider_storage" or "current_session",
        requestedCount = count,
        returnedCount = #errors,
        availableCount = availableCount,
        ordering = "provider_storage_reverse",
        session = session,
        providerVersion = GetProviderVersion(grabber),
        capturedAt = time(),
        complete = #incompleteReasons == 0,
        incompleteReasons = incompleteReasons,
        errors = errors,
    }, nil
end

ns.Diagnostics = diagnostics
