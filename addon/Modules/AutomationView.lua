local ADDON_NAME, ns = ...

-- View-model for the workbench Automation page. It is a bounded, session-only
-- observation list over the SAME game-side probe machinery the CLI drives
-- (ns.ProbeQueue / ns.ProbeRunner / ns.ReportStore). There is no second
-- executor and no task registry here: every status is derived from the bridge
-- records, and Execute routes through ProbeQueue.Load + ProbeRunner.Dispatch,
-- the exact functions the /dev bridge verbs call.
--
-- Real status vocabulary (derived from the bridge surface):
--   queued        lycheedev.queue.v1 entry registered, nothing loaded yet
--   loaded        ProbeRunner retains the request (its internal loaded /
--                 running / unresolved states are not externally separable)
--   reported      ReportStore retains the report (awaiting acknowledgement)
--   acknowledged  report acknowledged and removed (acknowledge lifecycle done)
--   cleared       retired: no queue entry, no runner request, no report
--   unavailable   derivation failed; errorCode carries the bridge error code
local MAX_RECORDS = 200
local REPORT_DISPLAY_BYTES = 48 * 1024
local KIND_LUA = "lua"

-- Captured before Bridge/ProbeQueue.lua takes ownership of the table (see the
-- TOC order note). Strictly read-only: used to list queued request ids and
-- their code digests. When nil (different load order or bridge), the list can
-- only show report-backed history and says so through HasQueueRegistry().
local queueDefinitions = ns.ProbeDefinitions

local records = {}
local recordIndex = {}
local observedSequence = 0

local function Restricted(value)
    return issecretvalue and issecretvalue(value)
end

local function RequestId(value)
    return not Restricted(value) and type(value) == "string"
        and #value > 0 and #value <= 128
        and string.match(value, "^[%w_%-]+$") ~= nil
end

local function BoundedText(value, limit)
    if Restricted(value) or type(value) ~= "string" then
        return nil
    end
    if #value > limit then
        return value:sub(1, limit)
    end
    return value
end

-- Display-only bounded pattern reads over stored report/receipt text. Nothing
-- is evaluated or rewritten; the stored bodies stay byte-identical.
local function ReadSignalField(receipt, pattern)
    if Restricted(receipt) or type(receipt) ~= "string" then
        return nil
    end
    return string.match(receipt, pattern)
end

local function IsPending(record)
    return record.pending == true or record.hasReport == true
end

local function IsProtected(record)
    return record.protected == true
end

local function PruneRecordSlot()
    if #records < MAX_RECORDS then
        return true
    end
    for index = 1, #records do
        local record = records[index]
        if not IsProtected(record) and not IsPending(record) then
            table.remove(records, index)
            recordIndex[record.requestId] = nil
            return true
        end
    end
    return false
end

local function ReportRead(requestId)
    if not ns.ReportStore or type(ns.ReportStore.Read) ~= "function" then
        return nil, "bridge_unavailable"
    end
    return ns.ReportStore.Read(requestId)
end

-- Side-effect-free status derivation from the bridge records only.
local function DeriveStatus(requestId)
    local receipt, bodyOrReason = ReportRead(requestId)
    if receipt then
        return "reported", nil, receipt, bodyOrReason
    end
    if bodyOrReason ~= "report_unavailable" then
        return "unavailable", bodyOrReason
    end

    if ns.ReportStore and type(ns.ReportStore.Acknowledged) == "function" then
        local acknowledged = ns.ReportStore.Acknowledged(requestId)
        if acknowledged then
            return "acknowledged", nil, acknowledged
        end
    end

    if not ns.ProbeRunner or type(ns.ProbeRunner.Reported) ~= "function"
        or type(ns.ProbeRunner.VerifyAbsent) ~= "function" then
        return "unavailable", "bridge_unavailable"
    end
    local reportedReceipt, reportedReason = ns.ProbeRunner.Reported(requestId)
    if reportedReceipt then
        return "reported", nil, reportedReceipt
    end
    if reportedReason == "report_unavailable" then
        -- Runner reported once and the report was acknowledged away.
        return "acknowledged"
    elseif reportedReason ~= "probe_not_reported" then
        return "unavailable", reportedReason
    end

    local absent, probeReason = ns.ProbeRunner.VerifyAbsent(requestId)
    if absent == true then
        if not ns.ProbeQueue or type(ns.ProbeQueue.ReloadScope) ~= "function" then
            return "unavailable", "bridge_unavailable"
        end
        local scope, scopeReason = ns.ProbeQueue.ReloadScope(requestId)
        if scope then
            return "queued"
        elseif scopeReason == "queue_request_missing" then
            return "cleared"
        end
        return "unavailable", scopeReason
    elseif probeReason == "probe_still_retained" then
        return "loaded"
    end
    return "unavailable", probeReason
end

local function ApplyDerived(record)
    record.status, record.errorCode = nil, nil
    record.receipt, record.reportBody, record.hasReport = nil, nil, nil
    record.probeStatus, record.probeError = nil, nil

    local status, errorCode, receipt, body = DeriveStatus(record.requestId)
    record.status = status
    record.errorCode = errorCode
    if receipt then
        record.receipt = receipt
        record.hasReport = status == "reported" and true or nil
        record.signalKind = ReadSignalField(receipt, '"kind":"(%a+)"')
        local sequence = ReadSignalField(receipt, '"sequence":(%d+)')
        record.sequence = sequence and tonumber(sequence) or nil
        if record.codeBytes == nil then
            local codeBytes = ReadSignalField(receipt, '"codeBytes":(%d+)')
            record.codeBytes = codeBytes and tonumber(codeBytes) or nil
        end
        if record.codeAdler32 == nil then
            record.codeAdler32 = ReadSignalField(receipt, '"codeAdler32":"([0-9a-f]+)"')
        end
    end
    if status == "reported" and type(body) == "string" and not Restricted(body) then
        record.reportBody = body
        record.probeStatus = string.match(body, '"probeStatus":"(%a+)"')
        record.probeError = BoundedText(string.match(body, '"error":"([^"]*)"'), 128)
    end
    return record
end

local function Observe(incoming)
    if type(incoming) ~= "table" or not RequestId(incoming.requestId) then
        return nil, "auto_invalid_request"
    end
    local requestId = incoming.requestId
    local record = recordIndex[requestId]
    if not record then
        if not PruneRecordSlot() then
            return nil, "auto_record_limit"
        end
        observedSequence = observedSequence + 1
        record = { requestId = requestId, observedSeq = observedSequence }
        records[#records + 1] = record
        recordIndex[requestId] = record
    end
    for key, value in pairs(incoming) do
        if key ~= "requestId" and key ~= "observedSeq" then
            record[key] = value
        end
    end
    if type(record.observedAt) ~= "number" then
        record.observedAt = (time and time()) or 0
    end
    if type(record.kind) ~= "string" then
        record.kind = KIND_LUA
    end
    return record
end

local function Collect()
    if not ns.ProbeQueue and not ns.ReportStore and not ns.Persistence then
        return 0
    end

    local discovered, seen = {}, {}
    local entries
    if type(queueDefinitions) == "table" and not Restricted(queueDefinitions) then
        entries = queueDefinitions.entries
        if type(entries) == "table" and not Restricted(entries) then
            for id in pairs(entries) do
                if RequestId(id) and not seen[id] then
                    seen[id] = true
                    discovered[#discovered + 1] = id
                end
            end
        end
    end

    local state
    if ns.Persistence and type(ns.Persistence.Current) == "function" then
        state = ns.Persistence.Current()
    end
    if type(state) == "table" and not Restricted(state) then
        local reports = state.reports
        if type(reports) == "table" and not Restricted(reports) then
            for id in pairs(reports) do
                if RequestId(id) and not seen[id] then
                    seen[id] = true
                    discovered[#discovered + 1] = id
                end
            end
        end
        local ticket = state.reentry
        if type(ticket) == "table" and not Restricted(ticket) and RequestId(ticket.requestId)
            and not seen[ticket.requestId] then
            seen[ticket.requestId] = true
            discovered[#discovered + 1] = ticket.requestId
        end
    end

    -- Deterministic within one pass; across passes later-observed ids are
    -- newer (they get the higher observation sequence).
    table.sort(discovered)
    for index = 1, #discovered do
        local requestId = discovered[index]
        local record = recordIndex[requestId]
        if not record then
            record = Observe({ requestId = requestId })
        end
        if record then
            local entry = entries and entries[requestId] or nil
            if type(entry) == "table" and not Restricted(entry) then
                record.codeBytes = entry.codeBytes
                record.codeSHA256 = entry.codeSHA256
                record.codeAdler32 = entry.codeAdler32
            end
            ApplyDerived(record)
        end
    end
    return #discovered
end

local function GetOrder()
    local order = {}
    for index = #records, 1, -1 do
        order[#order + 1] = records[index].requestId
    end
    return order
end

local function GetRecord(target)
    if type(target) == "table" then
        return target
    end
    return RequestId(target) and recordIndex[target] or nil
end

local function RefreshRecord(target)
    local record = GetRecord(target)
    if not record then
        return nil, "auto_execution_unknown"
    end
    ApplyDerived(record)
    return record
end

-- Manual execute: the same two calls the /dev bridge load/run verbs make, in
-- the same order. When the bridge or session context is absent the honest
-- bridge error code is returned instead of inventing one.
local function Execute(target)
    local record = GetRecord(target)
    if not record then
        return nil, "auto_execution_unknown"
    end
    if not ns.ProbeQueue or type(ns.ProbeQueue.Load) ~= "function"
        or not ns.ProbeRunner or type(ns.ProbeRunner.Dispatch) ~= "function" then
        return nil, "bridge_unavailable"
    end
    local loadReceipt, loadReason = ns.ProbeQueue.Load(record.requestId)
    if not loadReceipt and loadReason ~= "probe_request_exists" then
        return nil, loadReason
    end
    return ns.ProbeRunner.Dispatch(record.requestId)
end

local function ShowNotice(target)
    local record = GetRecord(target)
    if not record then
        return nil, "auto_execution_unknown"
    end
    local receipt = record.receipt
    if not receipt then
        receipt = ReportRead(record.requestId)
    end
    if Restricted(receipt) or type(receipt) ~= "string" or #receipt == 0 then
        return nil, "auto_notice_unavailable"
    end
    if not ns.ReceiptView or type(ns.ReceiptView.Show) ~= "function" then
        return nil, "bridge_unavailable"
    end
    -- Read-only redisplay through the one overlay system.
    return ns.ReceiptView.Show(receipt)
end

local function HideNotice()
    if ns.ReceiptView and type(ns.ReceiptView.Hide) == "function" then
        ns.ReceiptView.Hide()
    end
    return true
end

-- Display text for the report view. The 48 KB display cap only bounds the
-- returned string; the stored body is never modified.
local function GetReportText(target)
    local record = GetRecord(target)
    if not record then
        return nil, "auto_execution_unknown"
    end
    local body = record.reportBody
    if body == nil then
        local receipt, stored = ReportRead(record.requestId)
        body = receipt and stored or nil
    end
    if Restricted(body) then
        return "<secret>"
    end
    if type(body) ~= "string" then
        return nil, ns.L.AUTO_NO_REPORT
    end
    if #body > REPORT_DISPLAY_BYTES then
        return body:sub(1, REPORT_DISPLAY_BYTES)
            .. "\n... " .. string.format(ns.L.AUTO_REPORT_DISPLAY_LIMIT, REPORT_DISPLAY_BYTES / 1024)
    end
    return body
end

-- Mirrors ns.Stores protection semantics and the ReportStore acknowledge
-- lifecycle: records that are protected, or pending (a report the store still
-- retains), survive the clear.
local function ClearRecords()
    local removed = 0
    for index = #records, 1, -1 do
        local record = records[index]
        if not IsProtected(record) and not IsPending(record) then
            table.remove(records, index)
            recordIndex[record.requestId] = nil
            removed = removed + 1
        end
    end
    return removed
end

ns.AutomationView = {
    MAX_RECORDS = MAX_RECORDS,
    REPORT_DISPLAY_BYTES = REPORT_DISPLAY_BYTES,
    Observe = Observe,
    Collect = Collect,
    GetOrder = GetOrder,
    GetRecord = GetRecord,
    GetCount = function()
        return #records
    end,
    DeriveStatus = DeriveStatus,
    RefreshRecord = RefreshRecord,
    Execute = Execute,
    ShowNotice = ShowNotice,
    HideNotice = HideNotice,
    GetReportText = GetReportText,
    ClearRecords = ClearRecords,
    HasQueueRegistry = function()
        return type(queueDefinitions) == "table" and type(queueDefinitions.entries) == "table"
    end,
}
