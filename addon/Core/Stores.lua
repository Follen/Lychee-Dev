local ADDON_NAME, ns = ...

-- Bounded persistence for the workbench: run history and in-game "save to
-- record" exports, stored under LycheeToolkitDB through Core/Persistence.
-- Record bodies are plain data with no live references. Session-only state
-- (pending/protected sets, byte counters) never persists.
local HISTORY_SCHEMA_VERSION = 1
local EXPORT_SCHEMA_VERSION = 1
local EXPORT_SCHEMA = "lycheedev.export.v1"

local MAX_HISTORY_BYTES = 16 * 1024 * 1024
local MIN_HISTORY_ENTRY_BYTES = 16 * 1024
local MAX_CODE_BYTES = 12000
local MAX_RESULT_BYTES = 48000
local MAX_EXPORT_BYTES = 16 * 1024 * 1024
local MAX_EXPORT_RECORDS = 200
local METADATA_MAX_STRING_BYTES = 2048
local METADATA_MAX_DEPTH = 2
local METADATA_MAX_KEYS = 32

local currentRoot
local historyBytes = 0
local exportBytes = 0
-- Pending/protected are session-only; a SavedVariables load starts a fresh
-- session and therefore a fresh module state.
local pendingExports = {}
local pendingExportCount = 0
local protectedExports = {}
local protectedExportCount = 0
local historyWritesBlocked = false
local exportsWritesBlocked = false

local function Restricted(value)
    return issecretvalue and issecretvalue(value)
end

local function Plain(value)
    return not Restricted(value)
        and type(value) == "table" and getmetatable(value) == nil
end

local function TrimText(value, limit)
    value = type(value) == "string" and value or tostring(value or "")
    if #value <= limit then
        return value
    end
    return value:sub(1, limit) .. "\n... <truncated>"
end

local function GetStoredTreeBytes(value, seen)
    if type(value) == "string" then
        return #value
    elseif type(value) ~= "table" then
        return 8
    elseif seen[value] then
        return 0
    end

    seen[value] = true
    local bytes = 48
    for key, child in pairs(value) do
        bytes = bytes + GetStoredTreeBytes(key, seen) + GetStoredTreeBytes(child, seen)
        if bytes >= MAX_HISTORY_BYTES then
            break
        end
    end
    return bytes
end

local function GetHistoryEntryBytes(entry)
    local treeBytes = Plain(entry.tree) and GetStoredTreeBytes(entry.tree, {}) or 0
    local contentBytes = #(entry.code or "") + #(entry.result or "") + treeBytes + 128
    return math.max(MIN_HISTORY_ENTRY_BYTES, contentBytes)
end

local function GetExportEntryBytes(entry)
    local payload = Plain(entry) and entry.payload or nil
    return Plain(payload) and #(payload.content or "") or 0
end

-- Bounded metadata copy: strings of at most 2048 chars, nesting of at most 2
-- levels, at most 32 keys per table; anything else is dropped.
local function CopyExportMetadata(value, depth)
    if Restricted(value) then
        return nil
    end
    local valueType = type(value)
    if valueType == "string" then
        return value:sub(1, METADATA_MAX_STRING_BYTES)
    elseif valueType == "number" or valueType == "boolean" then
        return value
    elseif valueType ~= "table" or depth >= METADATA_MAX_DEPTH then
        return nil
    end

    local copied = {}
    local count = 0
    for key, child in pairs(value) do
        if count >= METADATA_MAX_KEYS then
            break
        end
        local keyType = type(key)
        if (keyType == "string" or keyType == "number") and not Restricted(key) then
            local safeChild = CopyExportMetadata(child, depth + 1)
            if safeChild ~= nil then
                copied[key] = safeChild
                count = count + 1
            end
        end
    end
    return copied
end

local function ReadClientEnvironment()
    local version, build, _, interfaceVersion
    if GetBuildInfo then
        local succeeded, v, b, _, i = pcall(GetBuildInfo)
        if succeeded then
            version, build, interfaceVersion = v, b, i
        end
    end
    local locale
    if GetLocale then
        local succeeded, value = pcall(GetLocale)
        if succeeded and not Restricted(value) then
            locale = value
        end
    end
    return {
        addonName = ADDON_NAME,
        clientId = ns.PlatformProfile and ns.PlatformProfile.product or nil,
        version = not Restricted(version) and version or nil,
        build = not Restricted(build) and build or nil,
        interface = not Restricted(interfaceVersion) and interfaceVersion or nil,
        locale = locale,
    }
end

local function CreateExportRecord(id, kind, title, content, createdAt, metadata, options)
    metadata = CopyExportMetadata(metadata, 0) or {}
    options = Plain(options) and options or {}
    return {
        schema = EXPORT_SCHEMA,
        id = id,
        createdAt = tonumber(createdAt) or 0,
        source = {
            kind = type(kind) == "string" and kind or "unknown",
            title = type(title) == "string" and title or "",
            path = type(metadata.path) == "string" and metadata.path or nil,
        },
        payload = {
            mediaType = type(options.mediaType) == "string" and options.mediaType or "text/plain",
            encoding = "utf-8",
            content = content,
            byteCount = #content,
        },
        environment = ReadClientEnvironment(),
        metadata = metadata,
    }
end

-- Idempotent load-time normalization. Unknown fields on records are preserved
-- verbatim; nothing is read from legacy databases and nothing is imported.
local function NormalizeExportRecord(id, entry)
    if not Plain(entry) or type(id) ~= "string" then
        return nil
    end

    local payload = entry.payload
    if Plain(payload) and type(payload.content) == "string" then
        entry.id = id
        entry.schema = type(entry.schema) == "string" and entry.schema or EXPORT_SCHEMA
        entry.createdAt = tonumber(entry.createdAt) or 0
        entry.source = Plain(entry.source) and entry.source or {}
        entry.source.kind = type(entry.source.kind) == "string" and entry.source.kind or "unknown"
        entry.source.title = type(entry.source.title) == "string" and entry.source.title or ""
        entry.source.path = type(entry.source.path) == "string" and entry.source.path or nil
        entry.metadata = CopyExportMetadata(entry.metadata, 0) or {}
        if type(entry.source.path) ~= "string" then
            entry.source.path = type(entry.metadata.path) == "string" and entry.metadata.path or nil
        end
        payload.mediaType = type(payload.mediaType) == "string" and payload.mediaType or "text/plain"
        payload.encoding = "utf-8"
        payload.byteCount = #payload.content
        entry.environment = Plain(entry.environment) and entry.environment or {}
        entry.environment.addonName = type(entry.environment.addonName) == "string"
            and entry.environment.addonName or ADDON_NAME
        return entry
    end
    return nil
end

local function NormalizeHistoryEntry(entry)
    if not Plain(entry) then
        return nil
    end
    entry.code = TrimText(entry.code, MAX_CODE_BYTES)
    entry.result = TrimText(entry.result, MAX_RESULT_BYTES)
    entry.succeeded = entry.succeeded and true or false
    if type(entry.timestamp) ~= "number" then
        entry.timestamp = nil
    end
    if not Plain(entry.tree) then
        entry.tree = nil
    end
    return entry
end

local function PruneHistoryToBudget(root)
    local entries = root.history.entries
    while historyBytes > MAX_HISTORY_BYTES and #entries > 0 do
        local entry = entries[#entries]
        historyBytes = math.max(0, historyBytes - GetHistoryEntryBytes(entry))
        entries[#entries] = nil
    end
end

local function RemoveExport(root, id)
    local entry = root.exports.records[id]
    if entry then
        exportBytes = math.max(0, exportBytes - GetExportEntryBytes(entry))
        root.exports.records[id] = nil
    end
    if pendingExports[id] then
        pendingExports[id] = nil
        pendingExportCount = math.max(0, pendingExportCount - 1)
    end
    if protectedExports[id] then
        protectedExports[id] = nil
        protectedExportCount = math.max(0, protectedExportCount - 1)
    end
end

local function FindPrunableIndex(root)
    local order = root.exports.order
    for index = #order, 1, -1 do
        if not protectedExports[order[index]] then
            return index
        end
    end
    return nil
end

local function PruneExportsToBudget(root)
    local exports = root.exports
    while (exportBytes > MAX_EXPORT_BYTES or #exports.order > MAX_EXPORT_RECORDS)
        and #exports.order > 0 do
        local index = FindPrunableIndex(root)
        if not index then
            break
        end
        local id = exports.order[index]
        table.remove(exports.order, index)
        RemoveExport(root, id)
    end
end

local function SectionVersion(section, defaultVersion)
    local stored = section.version
    if stored == nil then
        return defaultVersion
    end
    return tonumber(stored)
end

local function InitializeSections(root)
    historyWritesBlocked = false
    exportsWritesBlocked = false

    if not Plain(root.history) then
        root.history = {}
    end
    local history = root.history
    local historyVersion = SectionVersion(history, HISTORY_SCHEMA_VERSION)
    if historyVersion == nil or historyVersion > HISTORY_SCHEMA_VERSION then
        historyWritesBlocked = true
    end
    if type(history.entries) ~= "table" then
        history.entries = {}
    end
    historyBytes = 0
    for index = #history.entries, 1, -1 do
        local entry = NormalizeHistoryEntry(history.entries[index])
        if not entry then
            table.remove(history.entries, index)
        else
            historyBytes = historyBytes + GetHistoryEntryBytes(entry)
        end
    end
    PruneHistoryToBudget(root)

    if not Plain(root.exports) then
        root.exports = {}
    end
    local exports = root.exports
    local exportsVersion = SectionVersion(exports, EXPORT_SCHEMA_VERSION)
    if exportsVersion == nil or exportsVersion > EXPORT_SCHEMA_VERSION then
        exportsWritesBlocked = true
    end
    exports.nextId = math.max(0, math.floor(tonumber(exports.nextId) or 0))
    exports.records = Plain(exports.records) and exports.records or {}
    exports.order = type(exports.order) == "table" and exports.order or {}

    local validRecords = {}
    exportBytes = 0
    for id, entry in pairs(exports.records) do
        local normalized = type(id) == "string" and NormalizeExportRecord(id, entry) or nil
        if normalized then
            validRecords[id] = normalized
            exportBytes = exportBytes + GetExportEntryBytes(normalized)
        end
    end
    exports.records = validRecords

    local ordered = {}
    local included = {}
    for index = 1, #exports.order do
        local id = exports.order[index]
        if validRecords[id] and not included[id] then
            ordered[#ordered + 1] = id
            included[id] = true
        end
    end
    for id in pairs(validRecords) do
        if not included[id] then
            ordered[#ordered + 1] = id
        end
    end
    table.sort(ordered, function(left, right)
        return validRecords[left].createdAt > validRecords[right].createdAt
    end)
    exports.order = ordered
    PruneExportsToBudget(root)
end

-- Fetches the current database root at use time and (re)initializes the store
-- sections whenever the root is replaced (for example by a re-root).
local function EnsureState()
    local root, reason = ns.Persistence.Current()
    if not root then
        return nil, reason or "state_not_loaded"
    end
    if root ~= currentRoot then
        currentRoot = root
        pendingExports = {}
        pendingExportCount = 0
        protectedExports = {}
        protectedExportCount = 0
        InitializeSections(root)
    end
    return root
end

local function AllocateTicket(root, timestamp)
    local exports = root.exports
    local id
    repeat
        -- The counter only ever grows and is never reset or reused, so tickets
        -- stay unique across clears and sessions.
        exports.nextId = exports.nextId + 1
        id = string.format("LYCHEE-%s-%04d", date("%Y%m%d-%H%M%S", timestamp), exports.nextId)
    until not exports.records[id]
    return id
end

local History = {}

function History.Add(code, result, succeeded, tree)
    local root, reason = EnsureState()
    if not root then
        return nil, reason
    end
    if historyWritesBlocked then
        return nil, "state_unsupported_version"
    end

    local entry = {
        code = TrimText(code, MAX_CODE_BYTES),
        result = TrimText(result, MAX_RESULT_BYTES),
        succeeded = succeeded and true or false,
        timestamp = time(),
        tree = Plain(tree) and tree or nil,
    }
    table.insert(root.history.entries, 1, entry)

    historyBytes = historyBytes + GetHistoryEntryBytes(entry)
    PruneHistoryToBudget(root)
    return entry
end

function History.Get()
    local root = EnsureState()
    return root and root.history.entries or nil
end

function History.GetStats()
    local root = EnsureState()
    local count = root and #root.history.entries or 0
    return count, historyBytes, MAX_HISTORY_BYTES
end

function History.Clear()
    local root = EnsureState()
    if not root then
        return false
    end
    wipe(root.history.entries)
    historyBytes = 0
    return true
end

local Exports = {
    SCHEMA = EXPORT_SCHEMA,
    KINDS = {
        run_result = true,
        object_snapshot = true,
        object_node = true,
        event_log = true,
        function_trace = true,
        error_log = true,
        automation_result = true,
    },
}

function Exports.Add(kind, title, content, metadata, options)
    local root, reason = EnsureState()
    if not root then
        return nil, reason
    end
    if exportsWritesBlocked then
        return nil, ns.L.EXPORT_DATABASE_NEWER
    end
    if Restricted(content) then
        return nil, ns.L.EXPORT_FAILED
    end
    content = type(content) == "string" and content or tostring(content or "")
    if content == "" then
        return nil, ns.L.EXPORT_EMPTY
    end

    local timestamp = time()
    local id = AllocateTicket(root, timestamp)
    local entry = CreateExportRecord(id, kind, title, content, timestamp, metadata, options)
    local entryBytes = GetExportEntryBytes(entry)
    if entryBytes > MAX_EXPORT_BYTES then
        return nil, ns.L.EXPORT_TOO_LARGE
    end

    root.exports.records[id] = entry
    table.insert(root.exports.order, 1, id)
    exportBytes = exportBytes + entryBytes
    pendingExports[id] = true
    pendingExportCount = pendingExportCount + 1
    if Plain(options) and options.protect then
        protectedExports[id] = true
        protectedExportCount = protectedExportCount + 1
    end
    PruneExportsToBudget(root)
    return id, entry
end

function Exports.Get(id)
    local root = EnsureState()
    return root and root.exports.records[id] or nil
end

function Exports.GetOrder()
    local root = EnsureState()
    return root and root.exports.order or nil
end

function Exports.GetStats()
    local root = EnsureState()
    local count = root and #root.exports.order or 0
    return count, exportBytes, MAX_EXPORT_BYTES
end

function Exports.IsPending(id)
    return type(id) == "string" and pendingExports[id] == true
end

function Exports.GetPendingCount()
    return pendingExportCount
end

function Exports.IsProtected(id)
    return type(id) == "string" and protectedExports[id] == true
end

function Exports.GetProtectedCount()
    return protectedExportCount
end

function Exports.Protect(id)
    local root = EnsureState()
    if not root or type(id) ~= "string" or not root.exports.records[id] then
        return false
    end
    if not protectedExports[id] then
        protectedExports[id] = true
        protectedExportCount = protectedExportCount + 1
    end
    return true
end

-- Records the flush of one record to disk: it leaves the pending and protected
-- sets and becomes ordinary saved data.
function Exports.MarkSaved(id)
    local root = EnsureState()
    if not root or type(id) ~= "string" or not root.exports.records[id] then
        return false
    end
    if pendingExports[id] then
        pendingExports[id] = nil
        pendingExportCount = math.max(0, pendingExportCount - 1)
    end
    if protectedExports[id] then
        protectedExports[id] = nil
        protectedExportCount = math.max(0, protectedExportCount - 1)
    end
    return true
end

-- "pending" while the record still waits for its SavedVariables flush this
-- session, "saved" once it is ordinary on-disk data.
function Exports.GetState(id)
    local root = EnsureState()
    if not root or type(id) ~= "string" or not root.exports.records[id] then
        return "none"
    end
    return pendingExports[id] and "pending" or "saved"
end

function Exports.Delete(id)
    local root = EnsureState()
    if not root or type(id) ~= "string" or not root.exports.records[id] then
        return false
    end
    -- Results awaiting their reload disk write cannot be deleted.
    if protectedExports[id] then
        return false
    end

    RemoveExport(root, id)
    local order = root.exports.order
    for index = 1, #order do
        if order[index] == id then
            table.remove(order, index)
            break
        end
    end
    return true
end

function Exports.Clear()
    local root = EnsureState()
    if not root then
        return 0
    end
    local removed = 0
    local order = root.exports.order
    for index = #order, 1, -1 do
        local id = order[index]
        if not protectedExports[id] then
            table.remove(order, index)
            RemoveExport(root, id)
            removed = removed + 1
        end
    end
    return removed
end

-- Precheck that a new record fits after pruning protected records aside;
-- reserved bytes keep headroom for failure reports.
function Exports.CanFit(byteCount, reserve)
    local root = EnsureState()
    if not root then
        return false
    end
    byteCount = tonumber(byteCount) or 0
    local budget = MAX_EXPORT_BYTES - math.max(0, tonumber(reserve) or 0)
    if byteCount > budget then
        return false
    end
    local projectedBytes = exportBytes + byteCount
    local projectedCount = #root.exports.order + 1
    if projectedBytes <= budget and projectedCount <= MAX_EXPORT_RECORDS then
        return true
    end
    local order = root.exports.order
    for index = #order, 1, -1 do
        local id = order[index]
        if not protectedExports[id] then
            local entry = root.exports.records[id]
            if entry then
                projectedBytes = projectedBytes - GetExportEntryBytes(entry)
                projectedCount = projectedCount - 1
                if projectedBytes <= budget and projectedCount <= MAX_EXPORT_RECORDS then
                    return true
                end
            end
        end
    end
    return false
end

ns.Stores = {
    Initialize = EnsureState,
    History = History,
    Exports = Exports,
}
