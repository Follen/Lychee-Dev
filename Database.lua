local ADDON_NAME, ns = ...

local SCHEMA_VERSION = 3
local MAX_HISTORY_BYTES = 16 * 1024 * 1024
local MIN_HISTORY_ENTRY_BYTES = 16 * 1024
local MAX_CODE_BYTES = 12000
local MAX_RESULT_BYTES = 48000

local db
local historyBytes = 0

local function TrimText(value, limit)
    value = type(value) == "string" and value or tostring(value or "")
    if #value <= limit then
        return value
    end
    return value:sub(1, limit) .. "\n... <truncated>"
end

local function GetHistoryEntryBytes(entry)
    local contentBytes = #(entry.code or "") + #(entry.result or "") + 128
    return math.max(MIN_HISTORY_ENTRY_BYTES, contentBytes)
end

local function PruneHistoryToBudget()
    while historyBytes > MAX_HISTORY_BYTES and #db.history > 0 do
        local entry = db.history[#db.history]
        historyBytes = math.max(0, historyBytes - GetHistoryEntryBytes(entry))
        db.history[#db.history] = nil
    end
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

    if type(LycheeDevDB.schemaVersion) ~= "number" or LycheeDevDB.schemaVersion < SCHEMA_VERSION then
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
            historyBytes = historyBytes + GetHistoryEntryBytes(entry)
        end
    end

    PruneHistoryToBudget()

    ns.db = db
end

function ns.GetHistory()
    return db and db.history or nil
end

function ns.AddHistory(code, result, succeeded)
    if not db then
        return nil
    end

    local entry = {
        code = TrimText(code, MAX_CODE_BYTES),
        result = TrimText(result, MAX_RESULT_BYTES),
        succeeded = succeeded and true or false,
        timestamp = time(),
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
