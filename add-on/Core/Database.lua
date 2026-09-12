local ADDON_NAME, ns = ...

local SCHEMA_VERSION = 8
local MAX_HISTORY_BYTES = 16 * 1024 * 1024
local MAX_EXPORT_BYTES = 16 * 1024 * 1024
local MAX_EXPORT_RECORDS = 200
local MIN_HISTORY_ENTRY_BYTES = 16 * 1024
local MAX_CODE_BYTES = 12000
local MAX_RESULT_BYTES = 48000
local EXPORT_SCHEMA_VERSION = 2
local EVIDENCE_SCHEMA = "lychee.evidence.v1"
local AUTOMATION_INDEX_VERSION = 1
local MAX_AUTOMATION_RECORDS = 100
local MAX_AUTOMATION_INDEX_BYTES = 256 * 1024
local MAX_AUTOMATION_REPORT_BYTES = 1024 * 1024
local AUTOMATION_FAILURE_RESERVE_BYTES = 64 * 1024
local AUTOMATION_ACTIVE_STATUS = {
    running = true,
    finalizing = true,
}
local AUTOMATION_TERMINAL_STATUS = {
    succeeded = true,
    failed = true,
    cancelled = true,
    interrupted = true,
}

local db
local historyBytes = 0
local exportBytes = 0
local pendingExports = {}
local pendingExportCount = 0
-- Reload protection is session-only: it must never survive a SavedVariables load.
local protectedExports = {}
local protectedExportCount = 0
local automationWritesBlocked = false
local ticketOwners = {}

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
    local payload = type(entry) == "table" and entry.payload or nil
    return type(payload) == "table" and #(payload.content or "") or 0
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
    if protectedExports[ticket] then
        protectedExports[ticket] = nil
        protectedExportCount = math.max(0, protectedExportCount - 1)
    end
    local executionId = ticketOwners[ticket]
    if executionId then
        ticketOwners[ticket] = nil
        if db.automation and type(db.automation.records) == "table" then
            local record = db.automation.records[executionId]
            if record and record.ticket == ticket then
                record.resultAvailable = false
            end
        end
    end
end

local function FindPrunableIndex()
    for index = #db.exports.order, 1, -1 do
        if not protectedExports[db.exports.order[index]] then
            return index
        end
    end
    return nil
end

local function PruneExportsToBudget()
    while (exportBytes > MAX_EXPORT_BYTES or #db.exports.order > MAX_EXPORT_RECORDS)
        and #db.exports.order > 0 do
        local index = FindPrunableIndex()
        if not index then
            break
        end
        local ticket = db.exports.order[index]
        table.remove(db.exports.order, index)
        RemoveExport(ticket)
    end
    db.exports.totalBytes = exportBytes
end

-- Precheck that a new record fits after pruning; protected records cannot be freed.
local function CanFitExport(entryBytes, reserve)
    local budget = MAX_EXPORT_BYTES - math.max(0, reserve or 0)
    if entryBytes > budget then
        return false
    end
    local projectedBytes = exportBytes + entryBytes
    local projectedCount = #db.exports.order + 1
    if projectedBytes <= budget and projectedCount <= MAX_EXPORT_RECORDS then
        return true
    end
    for index = #db.exports.order, 1, -1 do
        local ticket = db.exports.order[index]
        if not protectedExports[ticket] then
            local entry = db.exports.records[ticket]
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

local function CreateEvidenceRecord(ticket, kind, title, content, createdAt, metadata, client)
    metadata = CopyExportMetadata(metadata, 0) or {}
    client = type(client) == "table" and client or {}
    return {
        schema = EVIDENCE_SCHEMA,
        ticket = ticket,
        createdAt = tonumber(createdAt) or 0,
        source = {
            kind = type(kind) == "string" and kind or "unknown",
            title = type(title) == "string" and title or "",
            path = type(metadata.path) == "string" and metadata.path or nil,
        },
        payload = {
            mediaType = "text/plain",
            encoding = "utf-8",
            content = content,
            byteCount = #content,
        },
        environment = {
            addonName = ADDON_NAME,
            clientId = client.clientId,
            version = client.version,
            build = client.build,
            buildDate = client.buildDate,
            interface = client.interface,
            locale = client.locale,
        },
        metadata = metadata,
    }
end

local function NormalizeEvidenceRecord(ticket, entry)
    if type(entry) ~= "table" then
        return nil
    end

    local payload = entry.payload
    if type(payload) == "table" and type(payload.content) == "string" then
        entry.ticket = ticket
        entry.schema = type(entry.schema) == "string" and entry.schema or EVIDENCE_SCHEMA
        entry.createdAt = tonumber(entry.createdAt) or 0
        entry.source = type(entry.source) == "table" and entry.source or {}
        entry.source.kind = type(entry.source.kind) == "string" and entry.source.kind or "unknown"
        entry.source.title = type(entry.source.title) == "string" and entry.source.title or ""
        entry.metadata = CopyExportMetadata(entry.metadata, 0) or {}
        if type(entry.source.path) ~= "string" then
            entry.source.path = type(entry.metadata.path) == "string" and entry.metadata.path or nil
        end
        payload.mediaType = type(payload.mediaType) == "string" and payload.mediaType or "text/plain"
        payload.encoding = type(payload.encoding) == "string" and payload.encoding or "utf-8"
        payload.byteCount = #payload.content
        entry.environment = type(entry.environment) == "table" and entry.environment or {}
        entry.environment.addonName = type(entry.environment.addonName) == "string"
            and entry.environment.addonName or ADDON_NAME
        return entry
    end

    if type(entry.content) ~= "string" then
        return nil
    end
    local client = type(entry.client) == "table" and entry.client or {}
    local migrated = CreateEvidenceRecord(ticket, entry.kind, entry.title, entry.content,
        entry.createdAt, entry.metadata, client)
    for key, value in pairs(entry) do
        if migrated[key] == nil and key ~= "content" and key ~= "kind"
            and key ~= "title" and key ~= "client" and key ~= "byteCount" then
            migrated[key] = value
        end
    end
    return migrated
end

local function InitializeExports()
    if type(db.exports) ~= "table" then
        db.exports = {}
    end
    local exports = db.exports
    exports.version = math.max(tonumber(exports.version) or 0, EXPORT_SCHEMA_VERSION)
    exports.nextId = math.max(0, math.floor(tonumber(exports.nextId) or 0))
    exports.records = type(exports.records) == "table" and exports.records or {}
    exports.order = type(exports.order) == "table" and exports.order or {}

    local validRecords = {}
    exportBytes = 0
    for ticket, entry in pairs(exports.records) do
        local normalized = type(ticket) == "string" and NormalizeEvidenceRecord(ticket, entry) or nil
        if normalized then
            validRecords[ticket] = normalized
            exportBytes = exportBytes + GetExportEntryBytes(normalized)
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

local function AllocateTicket(timestamp)
    local ticket
    repeat
        db.exports.nextId = db.exports.nextId + 1
        ticket = string.format("LYCHEE-%s-%04d", date("%Y%m%d-%H%M%S", timestamp), db.exports.nextId)
    until not db.exports.records[ticket]
    return ticket
end
local function GetAutomationIndexBytes(record)
    local bytes = 96
    local keys = { "taskId", "executionId", "revision", "kind", "status", "errorCode", "ticket" }
    for index = 1, #keys do
        local value = record[keys[index]]
        if type(value) == "string" then
            bytes = bytes + #value
        end
    end
    return bytes
end

local function PruneAutomationIndex()
    local automation = db.automation
    if not automation then
        return
    end
    local indexBytes = 0
    for _, record in pairs(automation.records) do
        indexBytes = indexBytes + GetAutomationIndexBytes(record)
    end
    local position = #automation.order
    while position >= 1
        and (#automation.order > MAX_AUTOMATION_RECORDS or indexBytes > MAX_AUTOMATION_INDEX_BYTES) do
        local executionId = automation.order[position]
        local record = automation.records[executionId]
        local isProtected = record and record.ticket and protectedExports[record.ticket]
        if record and AUTOMATION_TERMINAL_STATUS[record.status] and not isProtected then
            indexBytes = math.max(0, indexBytes - GetAutomationIndexBytes(record))
            automation.records[executionId] = nil
            if record.ticket and ticketOwners[record.ticket] == executionId then
                ticketOwners[record.ticket] = nil
            end
            table.remove(automation.order, position)
        end
        position = position - 1
    end
end

local function InitializeAutomation()
    if type(db.automation) ~= "table" then
        db.automation = { version = AUTOMATION_INDEX_VERSION, nextLocalRequestId = 1 }
    end
    local automation = db.automation
    automation.nextLocalRequestId = math.max(0, math.floor(tonumber(automation.nextLocalRequestId) or 0))
    automation.records = type(automation.records) == "table" and automation.records or {}
    automation.order = type(automation.order) == "table" and automation.order or {}
    automationWritesBlocked = (tonumber(automation.version) or 0) > AUTOMATION_INDEX_VERSION
    automation.version = math.max(AUTOMATION_INDEX_VERSION, tonumber(automation.version) or AUTOMATION_INDEX_VERSION)

    ticketOwners = {}
    local validRecords = {}
    for executionId, record in pairs(automation.records) do
        if type(record) == "table" and type(executionId) == "string" then
            record.executionId = executionId
            record.taskId = type(record.taskId) == "string" and record.taskId or executionId
            record.kind = type(record.kind) == "string" and record.kind or "lua"
            -- A non-terminal status loaded from disk belongs to a lost session.
            record.status = AUTOMATION_TERMINAL_STATUS[record.status] and record.status or "interrupted"
            record.startedAt = tonumber(record.startedAt) or 0
            record.finishedAt = tonumber(record.finishedAt) or record.startedAt
            record.revision = type(record.revision) == "string" and record.revision or nil
            record.errorCode = type(record.errorCode) == "string" and record.errorCode or nil
            record.ticket = type(record.ticket) == "string" and record.ticket or nil
            record.resultAvailable = false
            validRecords[executionId] = record
            if record.ticket then
                ticketOwners[record.ticket] = executionId
            end
        end
    end
    automation.records = validRecords

    local ordered = {}
    local included = {}
    for index = 1, #automation.order do
        local executionId = automation.order[index]
        if validRecords[executionId] and not included[executionId] then
            ordered[#ordered + 1] = executionId
            included[executionId] = true
        end
    end
    for executionId in pairs(validRecords) do
        if not included[executionId] then
            ordered[#ordered + 1] = executionId
        end
    end
    automation.order = ordered

    for _, executionId in ipairs(ordered) do
        local record = validRecords[executionId]
        if record.ticket and db.exports.records[record.ticket] then
            record.resultAvailable = true
        end
    end
    PruneAutomationIndex()
end

function ns.BeginAutomationExecution(executionId, info)
    if not db or automationWritesBlocked or type(db.automation) ~= "table" then
        return nil, ns.L.AUTO_DATABASE_UNSUPPORTED
    end
    if type(executionId) ~= "string" or executionId == ""
        or db.automation.records[executionId] then
        return nil, ns.L.AUTO_REQUEST_REPEATED
    end
    info = type(info) == "table" and info or {}
    local record = {
        taskId = type(info.taskId) == "string" and info.taskId or executionId,
        executionId = executionId,
        revision = type(info.revision) == "string" and info.revision or nil,
        kind = type(info.kind) == "string" and info.kind or "lua",
        status = "running",
        startedAt = time(),
        finishedAt = nil,
        ticket = nil,
        resultAvailable = false,
        errorCode = nil,
    }
    db.automation.records[executionId] = record
    table.insert(db.automation.order, 1, executionId)
    PruneAutomationIndex()
    return record
end

-- Atomic commit for one automation result: validates everything first, then
-- inserts the evidence record and updates the execution index in one step.
function ns.CommitAutomationResult(executionId, content, summary)
    if not db then
        return nil, ns.L.EXPORT_DATABASE_UNAVAILABLE
    end
    if automationWritesBlocked or type(db.automation) ~= "table" then
        return nil, ns.L.AUTO_DATABASE_UNSUPPORTED
    end
    local record = type(executionId) == "string" and db.automation.records[executionId] or nil
    if not record then
        return nil, ns.L.AUTO_EXECUTION_UNKNOWN
    end
    if not AUTOMATION_ACTIVE_STATUS[record.status] then
        return nil, ns.L.AUTO_REQUEST_REPEATED
    end
    summary = type(summary) == "table" and summary or {}
    content = type(content) == "string" and content or ""
    if content == "" then
        return nil, ns.L.EXPORT_EMPTY
    end
    local status = summary.status
    if not AUTOMATION_TERMINAL_STATUS[status] then
        return nil, ns.L.AUTO_EXECUTION_UNKNOWN
    end
    local contentBytes = #content
    if contentBytes > MAX_AUTOMATION_REPORT_BYTES then
        return nil, ns.L.AUTO_REPORT_TOO_LARGE
    end

    local reserve = status == "failed" and 0 or AUTOMATION_FAILURE_RESERVE_BYTES
    if not CanFitExport(contentBytes, reserve) then
        return nil, ns.L.EXPORT_TOO_LARGE
    end

    -- Remember the active state so a failed commit can restore it exactly.
    local previousStatus = record.status
    local previousErrorCode = record.errorCode
    local timestamp = time()
    local ticket = AllocateTicket(timestamp)
    local version, build, buildDate, interfaceVersion
    if GetBuildInfo then
        version, build, buildDate, interfaceVersion = GetBuildInfo()
    end
    local entry = CreateEvidenceRecord(ticket, summary.evidenceKind or "automation_result",
        summary.title or record.taskId or executionId, content, timestamp, {
            taskId = record.taskId,
            executionId = executionId,
            revision = record.revision,
            resultSchema = summary.resultSchema,
            status = status,
            complete = summary.complete and true or false,
            checksumAlgorithm = summary.checksumAlgorithm,
            contentChecksum = summary.contentChecksum,
        }, {
            clientId = ns.Client and ns.Client.id or nil,
            version = version,
            build = build,
            buildDate = buildDate,
            interface = interfaceVersion,
            locale = GetLocale and GetLocale() or nil,
        })
    entry.payload.mediaType = "application/json"

    db.exports.records[ticket] = entry
    table.insert(db.exports.order, 1, ticket)
    exportBytes = exportBytes + contentBytes
    pendingExports[ticket] = true
    pendingExportCount = pendingExportCount + 1
    protectedExports[ticket] = true
    protectedExportCount = protectedExportCount + 1
    ticketOwners[ticket] = executionId

    record.status = status
    record.finishedAt = timestamp
    record.ticket = ticket
    record.resultAvailable = true
    record.errorCode = type(summary.errorCode) == "string" and summary.errorCode or nil

    PruneExportsToBudget()
    PruneAutomationIndex()
    if not db.exports.records[ticket] or not db.automation.records[executionId] then
        -- Roll the partial commit back: a failed commit must not leave a ticket
        -- in the index without its evidence payload (spec 5.4 step 3). The
        -- index record returns to its active state so the caller can finalize
        -- it, and every mutation above is reversed exactly once.
        db.exports.records[ticket] = nil
        for index = #db.exports.order, 1, -1 do
            if db.exports.order[index] == ticket then
                table.remove(db.exports.order, index)
            end
        end
        exportBytes = math.max(0, exportBytes - contentBytes)
        db.exports.totalBytes = exportBytes
        if pendingExports[ticket] then
            pendingExports[ticket] = nil
            pendingExportCount = math.max(0, pendingExportCount - 1)
        end
        if protectedExports[ticket] then
            protectedExports[ticket] = nil
            protectedExportCount = math.max(0, protectedExportCount - 1)
        end
        if ticketOwners[ticket] == executionId then
            ticketOwners[ticket] = nil
        end
        if db.automation.records[executionId] ~= record then
            db.automation.records[executionId] = record
            table.insert(db.automation.order, 1, executionId)
        end
        record.status = previousStatus
        record.finishedAt = nil
        record.ticket = nil
        record.resultAvailable = false
        record.errorCode = previousErrorCode
        return nil, ns.L.EXPORT_FAILED
    end
    return ticket, entry
end

-- Finalize an execution index record without a result ticket. Used only when
-- even a minimal failure report could not be committed; no notice may be shown.
function ns.FailAutomationExecution(executionId, status, errorCode)
    if not db or type(db.automation) ~= "table" then
        return false
    end
    local record = type(executionId) == "string" and db.automation.records[executionId] or nil
    if not record or not AUTOMATION_ACTIVE_STATUS[record.status] then
        return false
    end
    record.status = AUTOMATION_TERMINAL_STATUS[status] and status or "failed"
    record.finishedAt = time()
    record.ticket = nil
    record.resultAvailable = false
    record.errorCode = type(errorCode) == "string" and errorCode or nil
    PruneAutomationIndex()
    return true
end

-- Clear terminal, unprotected automation index summaries. Evidence records in
-- the shared export store are intentionally kept; protected executions stay.
function ns.ClearAutomationHistory()
    if not db or type(db.automation) ~= "table" then
        return 0
    end
    local removed = 0
    for index = #db.automation.order, 1, -1 do
        local executionId = db.automation.order[index]
        local record = db.automation.records[executionId]
        local isProtected = record and record.ticket and protectedExports[record.ticket]
        if record and AUTOMATION_TERMINAL_STATUS[record.status] and not isProtected then
            db.automation.records[executionId] = nil
            if record.ticket and ticketOwners[record.ticket] == executionId then
                ticketOwners[record.ticket] = nil
            end
            table.remove(db.automation.order, index)
            removed = removed + 1
        end
    end
    return removed
end

function ns.GetAutomationIndex()
    return db and db.automation or nil
end

function ns.GetAutomationRecord(executionId)
    return db and db.automation and db.automation.records[executionId] or nil
end

function ns.FindAutomationByTicket(ticket)
    if not db or not db.automation or type(ticket) ~= "string" then
        return nil
    end
    local executionId = ticketOwners[ticket]
    if executionId and db.automation.records[executionId] then
        return db.automation.records[executionId]
    end
    for _, candidateId in ipairs(db.automation.order) do
        local record = db.automation.records[candidateId]
        if record and record.ticket == ticket then
            return record
        end
    end
    return nil
end

function ns.AutomationWritesEnabled()
    return db ~= nil and not automationWritesBlocked and type(db.automation) == "table"
end

-- One place that answers "where is this result in its life cycle?" so the page,
-- the notices and any future caller cannot disagree. The host's own report wins
-- over the in-memory flush marker, because a result can be acknowledged while
-- still protected in the session that produced it:
--   pending  - committed, still waiting for the output-side reload
--   flushed  - on disk, no read reported yet
--   received - the host reported a successful read
--   failed   - the host reported a failed read
--   expired  - the evidence itself is gone; only the index summary remains
function ns.AutomationResultState(record)
    if type(record) ~= "table" or not record.ticket then
        return "none"
    end
    if not ns.GetExport(record.ticket) then
        return "expired"
    end
    if record.receivedStatus == "received" then
        return "received"
    end
    if record.receivedStatus == "failed" then
        return "failed"
    end
    if protectedExports[record.ticket] then
        return "pending"
    end
    return "flushed"
end

function ns.NextAutomationLocalRequestId()
    if not db or type(db.automation) ~= "table" or automationWritesBlocked then
        return nil
    end
    db.automation.nextLocalRequestId = db.automation.nextLocalRequestId + 1
    return db.automation.nextLocalRequestId
end

function ns.IsExportProtected(ticket)
    return type(ticket) == "string" and protectedExports[ticket] == true
end

function ns.GetProtectedExportCount()
    return protectedExportCount
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

    -- Reload protection never persists; a SavedVariables load starts a fresh session.
    protectedExports = {}
    protectedExportCount = 0
    ticketOwners = {}
    automationWritesBlocked = false

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
    InitializeAutomation()

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
    local ticket = AllocateTicket(timestamp)

    local version, build, buildDate, interfaceVersion
    if GetBuildInfo then
        version, build, buildDate, interfaceVersion = GetBuildInfo()
    end
    local entry = CreateEvidenceRecord(ticket, kind, title, content, timestamp, metadata, {
        clientId = ns.Client and ns.Client.id or nil,
        version = version,
        build = build,
        buildDate = buildDate,
        interface = interfaceVersion,
        locale = GetLocale and GetLocale() or nil,
    })
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
    -- Results awaiting their reload disk write cannot be deleted.
    if protectedExports[ticket] then
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
    local removed = 0
    for index = #db.exports.order, 1, -1 do
        local ticket = db.exports.order[index]
        if not protectedExports[ticket] then
            table.remove(db.exports.order, index)
            RemoveExport(ticket)
            removed = removed + 1
        end
    end
    db.exports.totalBytes = exportBytes
    return removed
end
