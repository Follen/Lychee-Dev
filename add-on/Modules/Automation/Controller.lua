local ADDON_NAME, ns = ...
local L = ns.L

-- Automation controller: command dispatch, task identity, execution context,
-- terminal state freezing, result commit and completion notice triggering.

local Controller = {}
ns.Automation = Controller

local TASK_SCHEMA_VERSION = 1
local NOTICE_PROTOCOL_VERSION = 1
local MAX_NOTICE_BYTES = 512
local MAX_TASK_ID_BYTES = 64
local MAX_REQUEST_ID_BYTES = 128
local DEFAULT_OUTPUT_LIMIT = 384 * 1024
local MAX_OUTPUT_LIMIT = 512 * 1024
local RESULT_SCHEMA = "lychee.automation.result.v1"
local BUG_TASK_ID = "bug"

local TASK_ID_PATTERN = "^([A-Za-z0-9_%-]+)$"
local REQUEST_ID_PATTERN = "^([A-Za-z0-9%.%-]+)$"
local TICKET_PATTERN = "^LYCHEE%-([A-Za-z0-9%-]+)$"

-- How many finished results may sit unacknowledged before new requests are
-- refused. One is enough to guarantee no result is silently overwritten; a
-- small backlog keeps a missed acknowledgement from blocking the whole session.
local MAX_UNACKNOWLEDGED_RESULTS = 3

local activeContext
local awaitingNotice
local unacknowledgedResults = 0
local executedRequests = {}

local Finalize

local function Print(message)
    print("|cffd83b4eLychee Dev:|r " .. tostring(message))
end

local function Pack(...)
    return { n = select("#", ...), ... }
end

local function IsValidTaskId(taskId)
    return type(taskId) == "string" and #taskId >= 1 and #taskId <= MAX_TASK_ID_BYTES
        and taskId:match(TASK_ID_PATTERN) == taskId
end

local function IsValidRequestId(requestId)
    return type(requestId) == "string" and #requestId >= 1 and #requestId <= MAX_REQUEST_ID_BYTES
        and requestId:match(REQUEST_ID_PATTERN) == requestId
end

local function IsValidTicket(ticket)
    return type(ticket) == "string" and ticket:match(TICKET_PATTERN) ~= nil
end

local function GenerateLocalExecutionId()
    local sequence = ns.NextAutomationLocalRequestId() or 0
    return string.format("bug-%s-%04d", date("%Y%m%d-%H%M%S"), sequence)
end

local function EnsureReady()
    if ns.IsCombatBlocked() then
        ns.PrintCombatBlocked()
        return false
    end
    ns.InitializeDatabase()
    return true
end

local function ValidateTaskDefinition(taskId, definition)
    if type(definition) ~= "table" then
        return false, "not_table"
    end
    if definition.schema ~= TASK_SCHEMA_VERSION then
        return false, "schema"
    end
    if definition.kind ~= "lua" then
        return false, "kind"
    end
    if not IsValidRequestId(definition.requestId) then
        return false, "request_id"
    end
    if type(definition.revision) ~= "string" or #definition.revision == 0
        or #definition.revision > MAX_REQUEST_ID_BYTES then
        return false, "revision"
    end
    if type(definition.source) ~= "string" or #definition.source == 0 then
        return false, "source"
    end
    if type(definition.createdAt) ~= "number" then
        return false, "created_at"
    end
    if definition.sourceBytes ~= #definition.source then
        return false, "source_bytes"
    end
    if definition.sourceChecksum ~= ns.AutomationReport.Adler32(definition.source) then
        return false, "source_checksum"
    end
    if definition.outputLimit ~= nil
        and (type(definition.outputLimit) ~= "number" or definition.outputLimit < 1
            or definition.outputLimit > MAX_OUTPUT_LIMIT) then
        return false, "output_limit"
    end
    if definition.expectedClient ~= nil then
        if type(definition.expectedClient) ~= "table" then
            return false, "expected_client"
        end
        local expectedInterface = tonumber(definition.expectedClient.interface)
        if expectedInterface and ns.Client and ns.Client.interface ~= expectedInterface then
            return false, "client_mismatch"
        end
    end
    return true
end

-- Notice payloads are validated ASCII subsets, so plain embedding is safe.
local function BuildNoticeJson(notice)
    local json = string.format('{"v":%d,"ticket":"%s","task":"%s","run":"%s","ts":%d}',
        NOTICE_PROTOCOL_VERSION, notice.ticket, notice.taskId, notice.executionId,
        math.floor(tonumber(notice.createdAt) or 0))
    if #json > MAX_NOTICE_BYTES then
        return nil
    end
    return json
end

local function ShowNotice(notice)
    if not notice or not notice.ticket then
        return false
    end
    local json = BuildNoticeJson(notice)
    if not json then
        Print(L.AUTO_NOTICE_FAILED)
        ns.AutomationOverlay.ShowError()
        return false
    end
    ns.AutomationOverlay.ShowNotice(json)
    return true
end

local function BuildEnvironmentInfo()
    local version, build, buildDate, interfaceVersion
    if GetBuildInfo then
        version, build, buildDate, interfaceVersion = GetBuildInfo()
    end
    local character, realm
    local succeeded, name = pcall(UnitName, "player")
    if succeeded and type(name) == "string" and not (issecretvalue and issecretvalue(name)) then
        character = name
    end
    succeeded, name = pcall(GetRealmName)
    if succeeded and type(name) == "string" and not (issecretvalue and issecretvalue(name)) then
        realm = name
    end
    return {
        clientId = ns.Client and ns.Client.id or nil,
        version = version,
        build = build,
        buildDate = buildDate,
        interface = interfaceVersion,
        locale = GetLocale and GetLocale() or nil,
        character = character,
        realm = realm,
    }
end

local function CommitContext(context, terminal)
    local spec = {
        taskId = context.taskId,
        executionId = context.executionId,
        revision = context.revision,
        kind = context.kind,
        status = terminal.status,
        startedAt = context.startedAt,
        finishedAt = time(),
        requestType = context.requestType,
        params = context.params,
        source = context.source,
        stdout = #context.outputParts > 0 and table.concat(context.outputParts) or nil,
        outputTruncated = context.outputTruncated,
        returns = terminal.returns,
        error = terminal.error,
        lifecycleLog = context.log,
        bugSnapshot = context.bugSnapshot,
        environment = BuildEnvironmentInfo(),
        complete = not context.outputTruncated,
        incompleteReasons = context.outputTruncated and { "output_truncated" } or nil,
    }

    local content, metadata, errorCode = ns.AutomationReportBuild(spec)
    if not content then
        -- The report exceeded its budget; commit a small explicit failure report
        -- instead of a silently truncated success.
        terminal.status = "failed"
        if not terminal.error then
            terminal.error = { message = "report_build_failed: " .. tostring(errorCode), stage = "report" }
        end
        terminal.errorCode = terminal.errorCode or "report_build_failed"
        spec.status = "failed"
        spec.complete = false
        spec.incompleteReasons = { "output_limit" }
        spec.stdout = nil
        spec.returns = nil
        spec.bugSnapshot = nil
        spec.source = nil
        spec.params = nil
        spec.error = terminal.error
        content, metadata, errorCode = ns.AutomationReportBuild(spec)
    end

    if not content then
        ns.FailAutomationExecution(context.executionId, "failed", "report_unavailable")
        ns.AutomationOverlay.ShowError()
        Print(L.AUTO_COMMIT_FAILED:format(tostring(errorCode or "unknown")))
        return nil
    end

    local ticket, entryOrError = ns.CommitAutomationResult(context.executionId, content, {
        status = terminal.status,
        complete = metadata.complete,
        resultSchema = RESULT_SCHEMA,
        checksumAlgorithm = "adler32",
        contentChecksum = metadata.contentChecksum,
        title = context.title,
        evidenceKind = context.evidenceKind,
        errorCode = terminal.errorCode or errorCode,
    })
    if not ticket then
        -- A failed commit must still reach a terminal index state: the request
        -- id is already spent, so an active record would strand the index until
        -- the next reload (spec 5.4 step 3).
        ns.FailAutomationExecution(context.executionId, "failed", "commit_failed")
        ns.AutomationOverlay.ShowError()
        Print(L.AUTO_COMMIT_FAILED:format(tostring(entryOrError or errorCode or "unknown")))
        return nil
    end

    awaitingNotice = {
        ticket = ticket,
        taskId = context.taskId,
        executionId = context.executionId,
        createdAt = entryOrError.createdAt,
    }
    unacknowledgedResults = unacknowledgedResults + 1
    ns.AutomationOverlay.HideStatus()
    if ShowNotice(awaitingNotice) then
        Print(L.AUTO_RESULT_READY:format(ticket))
    end
    return ticket
end

Finalize = function(context, terminal)
    if context.terminal then
        return
    end
    context.terminal = terminal
    if activeContext == context then
        activeContext = nil
    end
    context.record.status = "finalizing"
    for index = 1, #context.cleanups do
        local succeeded, errorText = pcall(context.cleanups[index])
        if not succeeded then
            context.log[#context.log + 1] = { at = time(), message = "cleanup_failed: " .. tostring(errorText) }
        end
    end
    context.finishedAt = time()
    CommitContext(context, terminal)
end

local function AddContextOutput(context, text)
    if context.outputTruncated then
        return
    end
    text = tostring(text or "")
    local prefix = #context.outputParts > 0 and "\n" or ""
    local remaining = context.outputLimit - context.outputBytes - #prefix
    if #text > remaining then
        context.outputParts[#context.outputParts + 1] = prefix
            .. text:sub(1, math.max(0, remaining))
            .. "\n... <output truncated>"
        context.outputTruncated = true
        return
    end
    context.outputParts[#context.outputParts + 1] = prefix .. text
    context.outputBytes = context.outputBytes + #prefix + #text
end

local function AddLifecycleLog(context, message)
    message = tostring(message or "")
    if #message > 512 then
        message = message:sub(1, 512) .. "... <truncated>"
    end
    context.log[#context.log + 1] = { at = time(), message = message }
end

local function BuildContextEnvironment(context)
    local environment = {}

    environment.print = function(...)
        local values = Pack(...)
        local parts = {}
        for index = 1, values.n do
            if issecretvalue and issecretvalue(values[index]) then
                parts[index] = "<secret>"
            else
                parts[index] = tostring(values[index])
            end
        end
        AddContextOutput(context, table.concat(parts, "  "))
    end

    environment.Finish = function(...)
        local count = select("#", ...)
        local returns
        if count > 0 then
            returns = {}
            for index = 1, count do
                returns[index] = select(index, ...)
            end
        end
        Finalize(context, { status = "succeeded", returns = returns })
    end

    environment.Fail = function(message, errorCode)
        Finalize(context, {
            status = "failed",
            error = { message = tostring(message or "failed"), stage = "task" },
            errorCode = type(errorCode) == "string" and errorCode or "task_failed",
        })
    end

    environment.IsCancelled = function()
        return context.cancelRequested
    end

    environment.OnCleanup = function(handler)
        if type(handler) == "function" then
            context.cleanups[#context.cleanups + 1] = handler
        end
    end

    environment.Log = function(message)
        AddLifecycleLog(context, message)
    end

    environment.SetAsync = function()
        context.asyncRequested = true
    end

    setmetatable(environment, { __index = _G, __newindex = _G })
    return environment
end

local function ExecuteTask(taskId, definition)
    local record, beginError = ns.BeginAutomationExecution(definition.requestId, {
        taskId = taskId,
        revision = definition.revision,
        kind = "lua",
    })
    if not record then
        Print(beginError or L.AUTO_REQUEST_REPEATED)
        return
    end

    local context = {
        taskId = taskId,
        title = taskId,
        executionId = definition.requestId,
        revision = definition.revision,
        kind = "lua",
        requestType = "task",
        evidenceKind = "automation_result",
        record = record,
        outputLimit = math.floor(math.min(tonumber(definition.outputLimit) or DEFAULT_OUTPUT_LIMIT,
            MAX_OUTPUT_LIMIT)),
        outputParts = {},
        outputBytes = 0,
        outputTruncated = false,
        log = {},
        cleanups = {},
        terminal = nil,
        asyncRequested = false,
        cancelRequested = false,
        startedAt = time(),
        source = {
            sourceBytes = definition.sourceBytes,
            sourceChecksum = definition.sourceChecksum,
            requestId = definition.requestId,
        },
        params = nil,
        bugSnapshot = nil,
    }
    context.environment = BuildContextEnvironment(context)
    activeContext = context
    executedRequests[definition.requestId] = true
    ns.AutomationOverlay.ShowRunning()

    local chunk, compileError = loadstring(definition.source, "LycheeDevAutomation:" .. taskId)
    if not chunk then
        Finalize(context, {
            status = "failed",
            error = { message = tostring(compileError), stage = "compile" },
            errorCode = "compile_error",
        })
        return
    end
    setfenv(chunk, context.environment)

    local packed = Pack(pcall(chunk))
    if not packed[1] then
        Finalize(context, {
            status = "failed",
            error = { message = tostring(packed[2]), stage = "runtime" },
            errorCode = "runtime_error",
        })
    elseif not context.terminal and not context.asyncRequested then
        -- Sync task: returning completes it with the returned values.
        local returns
        if packed.n > 1 then
            returns = {}
            for index = 2, packed.n do
                returns[index - 1] = packed[index]
            end
        end
        Finalize(context, { status = "succeeded", returns = returns })
    end
    -- Async tasks stay active until their callbacks finish or fail explicitly.
end

function Controller.Run(taskId)
    if not EnsureReady() then
        return
    end
    if not IsValidTaskId(taskId) then
        Print(L.AUTO_TASK_INVALID:format(tostring(taskId), "task_id"))
        return
    end
    local definitions = ns.AutomationTaskDefinitions
    local definition = type(definitions) == "table" and definitions[taskId] or nil
    if not definition then
        Print(L.AUTO_TASK_NOT_LOADED:format(taskId))
        return
    end
    local valid, reason = ValidateTaskDefinition(taskId, definition)
    if not valid then
        Print(L.AUTO_TASK_INVALID:format(taskId, tostring(reason)))
        return
    end
    local expiresAt = tonumber(definition.expiresAt) or 0
    if expiresAt > 0 and expiresAt <= time() then
        Print(L.AUTO_TASK_EXPIRED:format(taskId))
        return
    end
    if not ns.AutomationWritesEnabled() then
        Print(L.AUTO_DATABASE_UNSUPPORTED)
        return
    end
    if executedRequests[definition.requestId] then
        -- Same request may not run twice in one session; re-show its notice.
        local record = ns.GetAutomationRecord(definition.requestId)
        local entry = record and record.ticket and ns.GetExport(record.ticket) or nil
        if entry and not activeContext then
            ShowNotice({
                ticket = record.ticket,
                taskId = record.taskId,
                executionId = record.executionId,
                createdAt = entry.createdAt,
            })
        else
            Print(L.AUTO_REQUEST_REPEATED)
        end
        return
    end
    if activeContext or unacknowledgedResults >= MAX_UNACKNOWLEDGED_RESULTS then
        Print(L.AUTO_BUSY)
        return
    end
    ExecuteTask(taskId, definition)
end

function Controller.Bug(countText, requestIdText)
    if not EnsureReady() then
        return
    end
    local count = tonumber(countText)
    if not count or count ~= math.floor(count) or count < 1 or count > 100 then
        Print(L.AUTO_INVALID_COUNT)
        return
    end
    requestIdText = requestIdText or ""
    if requestIdText ~= "" and not IsValidRequestId(requestIdText) then
        Print(L.AUTO_TASK_INVALID:format(requestIdText, "request_id"))
        return
    end
    if not ns.AutomationWritesEnabled() then
        Print(L.AUTO_DATABASE_UNSUPPORTED)
        return
    end
    local executionId = requestIdText ~= "" and requestIdText or GenerateLocalExecutionId()
    if executedRequests[executionId] then
        Print(L.AUTO_REQUEST_REPEATED)
        return
    end
    if activeContext or unacknowledgedResults >= MAX_UNACKNOWLEDGED_RESULTS then
        Print(L.AUTO_BUSY)
        return
    end
    local snapshot, snapshotError = ns.Diagnostics.SnapshotRecentErrors(count, "all")
    if not snapshot then
        Print(L.AUTO_PROVIDER_FAILED:format(tostring(snapshotError)))
        return
    end

    local record, beginError = ns.BeginAutomationExecution(executionId, {
        taskId = BUG_TASK_ID,
        kind = "bug_snapshot",
    })
    if not record then
        Print(beginError or L.AUTO_REQUEST_REPEATED)
        return
    end
    executedRequests[executionId] = true

    local context = {
        taskId = BUG_TASK_ID,
        title = L.AUTO_BUG_TITLE:format(count),
        executionId = executionId,
        kind = "bug_snapshot",
        requestType = "bug",
        evidenceKind = "error_log",
        record = record,
        outputLimit = DEFAULT_OUTPUT_LIMIT,
        outputParts = {},
        outputBytes = 0,
        outputTruncated = false,
        log = {},
        cleanups = {},
        terminal = nil,
        asyncRequested = false,
        cancelRequested = false,
        startedAt = time(),
        source = nil,
        params = { count = count, scope = "provider_storage" },
        bugSnapshot = snapshot,
    }
    activeContext = context
    ns.AutomationOverlay.ShowRunning()
    Finalize(context, { status = "succeeded" })
end

function Controller.Status(taskId)
    if not EnsureReady() then
        return
    end
    if not IsValidTaskId(taskId) then
        Print(L.AUTO_TASK_INVALID:format(tostring(taskId), "task_id"))
        return
    end
    local automation = ns.GetAutomationIndex()
    local latest
    if automation then
        for index = 1, #automation.order do
            local record = automation.records[automation.order[index]]
            if record and record.taskId == taskId then
                latest = record
                break
            end
        end
    end
    if not latest then
        Print(L.AUTO_TASK_NOT_LOADED:format(taskId))
        return
    end
    local resultText
    if latest.ticket and ns.GetExport(latest.ticket) then
        resultText = latest.ticket
    elseif latest.ticket then
        resultText = L.AUTO_RESULT_EXPIRED
    else
        resultText = "-"
    end
    Print(L.AUTO_STATUS_LINE:format(taskId, latest.executionId, latest.status, resultText))
    if latest.ticket and resultText == latest.ticket and not activeContext then
        ShowNotice({
            ticket = latest.ticket,
            taskId = latest.taskId,
            executionId = latest.executionId,
            createdAt = ns.GetExport(latest.ticket).createdAt,
        })
    end
end

function Controller.Show(ticket)
    if not EnsureReady() then
        return
    end
    if not IsValidTicket(ticket) then
        Print(L.AUTO_INVALID_TICKET)
        return
    end
    local entry = ns.GetExport(ticket)
    local record = entry and ns.FindAutomationByTicket(ticket) or nil
    if not entry or not record then
        Print(L.AUTO_RESULT_MISSING:format(ticket))
        return
    end
    ShowNotice({
        ticket = ticket,
        taskId = record.taskId,
        executionId = record.executionId,
        createdAt = entry.createdAt,
    })
end

function Controller.Cancel(taskId)
    if not EnsureReady() then
        return
    end
    if not IsValidTaskId(taskId) then
        Print(L.AUTO_TASK_INVALID:format(tostring(taskId), "task_id"))
        return
    end
    if not activeContext or activeContext.taskId ~= taskId then
        Print(L.AUTO_NO_ACTIVE:format(taskId))
        return
    end
    activeContext.cancelRequested = true
    Finalize(activeContext, {
        status = "cancelled",
        errorCode = "cancelled_by_request",
    })
end

function Controller.Stop()
    ns.AutomationOverlay.HideNotice()
    ns.AutomationOverlay.HideStatus()
    ns.AutomationOverlay.HideIdentity()
    Print(L.AUTO_STOPPED)
end

function Controller.StopIdentify()
    ns.AutomationOverlay.HideIdentity()
    Print(L.AUTO_IDENTITY_HIDDEN)
end

-- Identity marker for host-side window discovery.
--
-- The host can capture several game windows but cannot tell which character is
-- behind each one; nothing observable from outside the game says so. This shows
-- a tiny QR code carrying the character identity and build, so a host can map
-- every running window to a name and let the user choose. It appears only on
-- explicit request because it shares the top-left corner with the completion
-- notice and would otherwise sit on screen permanently.
function Controller.Identify()
    if not EnsureReady() then
        return
    end

    local character, realm
    local succeeded, name = pcall(UnitName, "player")
    if succeeded and type(name) == "string" and name ~= ""
        and not (issecretvalue and issecretvalue(name)) then
        character = name
    end
    succeeded, name = pcall(GetRealmName)
    if succeeded and type(name) == "string" and name ~= ""
        and not (issecretvalue and issecretvalue(name)) then
        realm = name
    end

    if not character then
        Print(L.AUTO_IDENTITY_UNAVAILABLE)
        return
    end

    -- The marker and the completion notice share one corner. Replacing a notice
    -- would take away the only on-screen reference to its Ticket, so refuse
    -- instead of hiding evidence the user may still need.
    if awaitingNotice then
        Print(L.AUTO_IDENTITY_NOTICE_SHOWN)
        return
    end

    local version = GetBuildInfo and select(1, GetBuildInfo()) or nil
    if issecretvalue and issecretvalue(version) then
        version = nil
    end

    -- Character names and realm names are plain text, so escape them before
    -- embedding rather than assuming they are JSON-safe.
    local function EscapeJson(value)
        value = tostring(value or "")
        value = value:gsub("\\", "\\\\"):gsub('"', '\\"'):gsub("[%c]", " ")
        return value
    end

    local json = string.format('{"v":1,"id":"%s","realm":"%s","client":"%s","build":"%s"}',
        EscapeJson(character),
        EscapeJson(realm or ""),
        EscapeJson(ns.Client and ns.Client.id or ""),
        EscapeJson(version or ""))

    if #json > MAX_NOTICE_BYTES then
        Print(L.AUTO_IDENTITY_UNAVAILABLE)
        return
    end

    if ns.AutomationOverlay.ShowIdentity(json) then
        Print(L.AUTO_IDENTITY_SHOWN:format(character, realm or "-"))
    else
        Print(L.AUTO_IDENTITY_UNAVAILABLE)
    end
end

-- The plugin cannot observe whether the host read a result, so the host reports
-- it back as a chat command: `/dev auto ack <ticket> <received|failed>`. That
-- clears the pending-flush block immediately (no reload needed) and records the
-- outcome so the page can show it and a later session keeps it.
function Controller.Ack(ticket, outcome, nonce)
    if not EnsureReady() then
        return
    end
    if not IsValidTicket(ticket) then
        Print(L.AUTO_INVALID_TICKET)
        return
    end
    if outcome ~= "received" and outcome ~= "failed" then
        Print(L.AUTO_ACK_STATUS_INVALID)
        return
    end
    local record = ns.FindAutomationByTicket(ticket)
    if not record then
        Print(L.AUTO_ACK_TICKET_UNKNOWN:format(ticket))
        return
    end
    if nonce then
        if not IsValidRequestId(nonce) or #ticket > 64 then
            Print(L.AUTO_USAGE)
            return
        end
        if awaitingNotice and awaitingNotice.ticket ~= ticket then
            Print(L.AUTO_IDENTITY_NOTICE_SHOWN)
            return
        end
    end
    record.receivedAt = time()
    record.receivedStatus = outcome
    -- Only the first acknowledgement of a result frees a backlog slot; a repeat
    -- ack must not shrink the count twice.
    if not record.acknowledged then
        record.acknowledged = true
        if unacknowledgedResults > 0 then
            unacknowledgedResults = unacknowledgedResults - 1
        end
    end
    if awaitingNotice and awaitingNotice.ticket == ticket then
        awaitingNotice = nil
        ns.AutomationOverlay.HideNotice()
    end
    if nonce then
        -- Echo only validated identifiers. A fresh nonce separates this receipt
        -- from an older acknowledgement still visible on screen.
        local receipt = string.format(
            '{"v":1,"ticket":"%s","task":"ack","run":"%s","ts":%d,"status":"%s"}',
            ticket, nonce, record.receivedAt, outcome)
        ns.AutomationOverlay.ShowIdentity(receipt)
    end
    Print(L.AUTO_ACK_DONE:format(ticket, outcome))
end

function Controller.HandleCommand(text)
    local action, rest = tostring(text or ""):match("^(%S+)%s*(.-)%s*$")
    action = action and action:lower() or ""
    if action == "run" then
        local taskId = rest:match("^(%S+)$")
        if not taskId then
            Print(L.AUTO_USAGE)
            return
        end
        Controller.Run(taskId)
    elseif action == "bug" then
        local count, requestId = rest:match("^(%S+)%s+(%S+)$")
        if not count then
            count = rest:match("^(%S+)$")
        end
        if not count then
            Print(L.AUTO_USAGE)
            return
        end
        Controller.Bug(count, requestId)
    elseif action == "status" then
        local taskId = rest:match("^(%S+)$")
        if not taskId then
            Print(L.AUTO_USAGE)
            return
        end
        Controller.Status(taskId)
    elseif action == "show" then
        local ticket = rest:match("^(%S+)$")
        if not ticket then
            Print(L.AUTO_USAGE)
            return
        end
        Controller.Show(ticket)
    elseif action == "cancel" then
        local taskId = rest:match("^(%S+)$")
        if not taskId then
            Print(L.AUTO_USAGE)
            return
        end
        Controller.Cancel(taskId)
    elseif action == "stop" then
        if rest ~= "" then
            Print(L.AUTO_USAGE)
            return
        end
        Controller.Stop()
    elseif action == "identify" then
        if rest ~= "" then
            Print(L.AUTO_USAGE)
            return
        end
        Controller.Identify()
    elseif action == "unidentify" then
        if rest ~= "" then
            Print(L.AUTO_USAGE)
            return
        end
        Controller.StopIdentify()
    elseif action == "ack" then
        local ticket, outcome, nonce = rest:match("^(%S+)%s+(%S+)%s+(%S+)$")
        if not ticket then
            ticket, outcome = rest:match("^(%S+)%s+(%S+)$")
        end
        if not ticket or not outcome then
            Print(L.AUTO_USAGE)
            return
        end
        Controller.Ack(ticket, outcome:lower(), nonce)
    else
        Print(L.AUTO_USAGE)
    end
end

function Controller.IsBusy()
    -- Busy means "a new request would be refused", not "a notice is on screen":
    -- a few unacknowledged results may queue before the cap blocks new runs.
    return activeContext ~= nil
        or unacknowledgedResults >= MAX_UNACKNOWLEDGED_RESULTS
end

function Controller.GetUnacknowledgedCount()
    return unacknowledgedResults
end

function Controller.MaxUnacknowledgedResults()
    return MAX_UNACKNOWLEDGED_RESULTS
end

function Controller.GetAwaitingNotice()
    return awaitingNotice
end

-- The identity marker is own UI and must not survive a combat lockout, matching
-- the notice and status blocks. The callback is constant time while the overlay
-- has never been built, so registering it costs nothing for users who never use
-- the automation page.
ns.RegisterCombatShutdown(function()
    if ns.AutomationOverlay then
        ns.AutomationOverlay.HideIdentity()
    end
end)
