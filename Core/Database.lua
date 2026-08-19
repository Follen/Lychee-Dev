local ADDON_NAME, ns = ...

local SCHEMA_VERSION = 6
local MAX_HISTORY_BYTES = 16 * 1024 * 1024
local MAX_EXPORT_BYTES = 16 * 1024 * 1024
local MAX_EXPORT_RECORDS = 200
local MIN_HISTORY_ENTRY_BYTES = 16 * 1024
local MAX_CODE_BYTES = 12000
local MAX_RESULT_BYTES = 48000
local EXPORT_SCHEMA_VERSION = 1

local db
local historyBytes = 0
local exportBytes = 0
local pendingExports = {}
local pendingExportCount = 0

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
    local treeBytes = entry.tree and GetStoredTreeBytes(entry.tree, {}) or 0
    local contentBytes = #(entry.code or "") + #(entry.result or "") + treeBytes + 128
    return math.max(MIN_HISTORY_ENTRY_BYTES, contentBytes)
end

local function PruneHistoryToBudget()
    while historyBytes > MAX_HISTORY_BYTES and #db.history > 0 do
        local entry = db.history[#db.history]
        historyBytes = math.max(0, historyBytes - GetHistoryEntryBytes(entry))
        db.history[#db.history] = nil
    end
end

local function GetExportEntryBytes(entry)
    return #(entry.content or "")
end

local function RemoveExport(ticket)
    local entry = db.exports.records[ticket]
    if entry then
        exportBytes = math.max(0, exportBytes - GetExportEntryBytes(entry))
        db.exports.records[ticket] = nil
    end
    if pendingExports[ticket] then
        pendingExports[ticket] = nil
        pendingExportCount = math.max(0, pendingExportCount - 1)
    end
end

local function PruneExportsToBudget()
    while (exportBytes > MAX_EXPORT_BYTES or #db.exports.order > MAX_EXPORT_RECORDS)
        and #db.exports.order > 0 do
        local index = #db.exports.order
        local ticket = db.exports.order[index]
        db.exports.order[index] = nil
        RemoveExport(ticket)
    end
    db.exports.totalBytes = exportBytes
end

local function CopyExportMetadata(value, depth)
    local valueType = type(value)
    if valueType == "string" then
        return value:sub(1, 2048)
    elseif valueType == "number" or valueType == "boolean" then
        return value
    elseif valueType ~= "table" or depth >= 2 then
        return nil
    end

    local copied = {}
    local count = 0
    for key, child in pairs(value) do
        if count >= 32 then
            break
        end
        local keyType = type(key)
        if keyType == "string" or keyType == "number" then
            local safeChild = CopyExportMetadata(child, depth + 1)
            if safeChild ~= nil then
                copied[key] = safeChild
                count = count + 1
            end
        end
    end
    return copied
end

local function InitializeExports()
    if type(db.exports) ~= "table" then
        db.exports = {}
    end
    local exports = db.exports
    exports.version = type(exports.version) == "number" and exports.version or EXPORT_SCHEMA_VERSION
    exports.nextId = math.max(0, math.floor(tonumber(exports.nextId) or 0))
    exports.records = type(exports.records) == "table" and exports.records or {}
    exports.order = type(exports.order) == "table" and exports.order or {}

    local validRecords = {}
    exportBytes = 0
    for ticket, entry in pairs(exports.records) do
        if type(ticket) == "string" and type(entry) == "table" and type(entry.content) == "string" then
            entry.ticket = ticket
            entry.kind = type(entry.kind) == "string" and entry.kind or "unknown"
            entry.title = type(entry.title) == "string" and entry.title or ""
            entry.createdAt = tonumber(entry.createdAt) or 0
            entry.byteCount = #entry.content
            entry.metadata = CopyExportMetadata(entry.metadata, 0) or {}
            validRecords[ticket] = entry
            exportBytes = exportBytes + GetExportEntryBytes(entry)
        end
    end
    exports.records = validRecords

    local ordered = {}
    local included = {}
    for index = 1, #exports.order do
        local ticket = exports.order[index]
        if validRecords[ticket] and not included[ticket] then
            ordered[#ordered + 1] = ticket
            included[ticket] = true
        end
    end
    for ticket in pairs(validRecords) do
        if not included[ticket] then
            ordered[#ordered + 1] = ticket
        end
    end
    table.sort(ordered, function(left, right)
        return validRecords[left].createdAt > validRecords[right].createdAt
    end)
    exports.order = ordered
    PruneExportsToBudget()
end

function ns.InitializeDatabase()
    if db then
        return
    end

    if type(LycheeDevDB) ~= "table" then
        if type(DumperDB) == "table" then
            LycheeDevDB = DumperDB
        else
            LycheeDevDB = {}
        end
    end
    DumperDB = nil

    if type(LycheeDevDB.history) ~= "table" then
        LycheeDevDB.history = {}
    end

    local storedTreeMigrationRequired = type(LycheeDevDB.schemaVersion) ~= "number"
        or LycheeDevDB.schemaVersion < SCHEMA_VERSION
    if storedTreeMigrationRequired then
        LycheeDevDB.schemaVersion = SCHEMA_VERSION
    end
    db = LycheeDevDB
    historyBytes = 0

    for index = #db.history, 1, -1 do
        local entry = db.history[index]
        if type(entry) ~= "table" then
            table.remove(db.history, index)
        else
            entry.code = TrimText(entry.code, MAX_CODE_BYTES)
            entry.result = TrimText(entry.result, MAX_RESULT_BYTES)
            entry.succeeded = entry.succeeded and true or false
            if type(entry.timestamp) ~= "number" then
                entry.timestamp = nil
            end
            if type(entry.tree) ~= "table" then
                entry.tree = nil
                if storedTreeMigrationRequired and entry.succeeded and ns.CreateStoredTreeFromSerialized then
                    entry.tree = ns.CreateStoredTreeFromSerialized(entry.result)
                end
            end
            historyBytes = historyBytes + GetHistoryEntryBytes(entry)
        end
    end

    PruneHistoryToBudget()
    InitializeExports()

    ns.db = db
end

function ns.GetHistory()
    return db and db.history or nil
end

function ns.AddHistory(code, result, succeeded, tree)
    if not db then
        return nil
    end

    local entry = {
        code = TrimText(code, MAX_CODE_BYTES),
        result = TrimText(result, MAX_RESULT_BYTES),
        succeeded = succeeded and true or false,
        timestamp = time(),
        tree = type(tree) == "table" and tree or nil,
    }
    table.insert(db.history, 1, entry)

    historyBytes = historyBytes + GetHistoryEntryBytes(entry)
    PruneHistoryToBudget()
    return entry
end

function ns.ClearHistory()
    if db then
        wipe(db.history)
        historyBytes = 0
    end
end

function ns.AddExport(kind, title, content, metadata)
    if not db then
        return nil, ns.L.EXPORT_DATABASE_UNAVAILABLE
    end
    content = type(content) == "string" and content or tostring(content or "")
    if content == "" then
        return nil, ns.L.EXPORT_EMPTY
    end

    local timestamp = time()
    local ticket
    repeat
        db.exports.nextId = db.exports.nextId + 1
        ticket = string.format("LYCHEE-%s-%04d", date("%Y%m%d-%H%M%S", timestamp), db.exports.nextId)
    until not db.exports.records[ticket]

    local version, build, buildDate, interfaceVersion
    if GetBuildInfo then
        version, build, buildDate, interfaceVersion = GetBuildInfo()
    end
    local entry = {
        ticket = ticket,
        kind = type(kind) == "string" and kind or "unknown",
        title = type(title) == "string" and title or "",
        content = content,
        byteCount = #content,
        createdAt = timestamp,
        metadata = CopyExportMetadata(metadata, 0) or {},
        client = {
            version = version,
            build = build,
            buildDate = buildDate,
            interface = interfaceVersion,
            locale = GetLocale and GetLocale() or nil,
        },
    }
    local entryBytes = GetExportEntryBytes(entry)
    if entryBytes > MAX_EXPORT_BYTES then
        return nil, ns.L.EXPORT_TOO_LARGE
    end

    db.exports.records[ticket] = entry
    table.insert(db.exports.order, 1, ticket)
    exportBytes = exportBytes + entryBytes
    pendingExports[ticket] = true
    pendingExportCount = pendingExportCount + 1
    PruneExportsToBudget()
    return ticket, entry
end

function ns.GetExport(ticket)
    return db and db.exports and db.exports.records[ticket] or nil
end

function ns.GetExports()
    return db and db.exports or nil
end

function ns.GetExportStats()
    if not db or not db.exports then
        return 0, 0, MAX_EXPORT_BYTES
    end
    return #db.exports.order, exportBytes, MAX_EXPORT_BYTES
end

function ns.IsExportPending(ticket)
    return type(ticket) == "string" and pendingExports[ticket] == true
end

function ns.GetPendingExportCount()
    return pendingExportCount
end

function ns.DeleteExport(ticket)
    if not db or not db.exports or type(ticket) ~= "string"
        or not db.exports.records[ticket] then
        return false
    end

    RemoveExport(ticket)
    for index = 1, #db.exports.order do
        if db.exports.order[index] == ticket then
            table.remove(db.exports.order, index)
            break
        end
    end
    db.exports.totalBytes = exportBytes
    return true
end

function ns.ClearExports()
    if not db or not db.exports then
        return 0
    end
    local removed = #db.exports.order
    wipe(db.exports.order)
    wipe(db.exports.records)
    wipe(pendingExports)
    exportBytes = 0
    pendingExportCount = 0
    db.exports.totalBytes = 0
    return removed
end
