local ADDON_NAME, ns = ...

local requests, owner, count, bytes
local MAX_ASYNC_SECONDS,MAX_LOG_BYTES,MAX_LOG_ENTRIES,MAX_CLEANUPS=120,32768,100,16
local function notify()
    if ns.AutomationView and ns.AutomationView.Changed then ns.AutomationView.Changed() end
end
local function restricted(value)
    return issecretvalue and issecretvalue(value)
end
local function busy(except)
    for id, request in pairs(requests or {}) do
        if id ~= except and (request.state == "running" or request.state == "settling"
            or request.state == "quarantined" or request.cleanupFailed) then return true end
    end
    return false
end
local function context()
    local session, failure = ns.Session.Current()
    if not session then
        return nil, failure
    end
    if owner ~= session.generation then
        -- Input sessions are replaceable; live work is not. Only a verified
        -- new Lua runtime can discard an unresolved old executor.
        if busy() then return nil, "probe_runtime_busy" end
        requests, owner, count, bytes = {}, session.generation, 0, 0
    end
    return session
end
local function key(value)
    return not restricted(value) and type(value) == "string" and #value > 0
        and #value <= 128 and string.match(value, "^[%w_%-]+$") ~= nil
end
local function loadedSignal(requestId, code, reloadNonce, ready)
    local identity, failure = ns.Session.NextIdentity()
    if not identity then return nil, failure end
    return ns.CaptureWriter.EncodeSignal({
        schema = "lycheedev.signal.v1", release = ns.Release, kind = "loaded",
        sessionNonce = identity.sessionNonce, requestId = requestId, reloadNonce = reloadNonce,
        character = identity.character, realm = identity.realm, sequence = identity.sequence,
        product = ns.Startup.identity.product, build = ns.Startup.identity.build,
        inputReady = ready, codeBytes = #code, codeAdler32 = ns.CaptureWriter.DigestBytes(code),
    }, 4096)
end
local function safeMessage(value,fallback)
    if restricted(value) or type(value)~="string" or #value==0 or #value>4096 then return fallback end
    return value
end
local REPORT_ERROR_CODES = { report_byte_limit=true, report_secret_value=true,
    report_invalid_utf8=true, report_depth_limit=true, report_entry_limit=true,
    report_cycle=true, report_metatable=true, report_nonfinite_number=true,
    report_unsupported_value=true, report_nonstring_key=true,
    report_count_limit=true, report_store_limit=true,
    report_invalid_store=true, report_invalid_document=true }
local function minimalFailure(request, reason)
    local identity = ns.Session.NextIdentity()
    if not identity then return nil end
    local code = not restricted(reason) and REPORT_ERROR_CODES[reason] and reason or "report_encoding_failed"
    return ns.CaptureWriter.EncodeSignal({
        schema="lycheedev.signal.v1", release=ns.Release, kind="report_error",
        sessionNonce=identity.sessionNonce, requestId=request.requestId,
        character=identity.character, realm=identity.realm, guid=identity.guid,
        product=ns.Startup.identity.product, build=ns.Startup.identity.build,
        sequence=identity.sequence, inputReady=false, errorCode=code,
    }, 2048)
end
local function cleanup(request)
    local failures = {}
    local function attempt(callback, ...)
        local ok, reason = pcall(callback, ...)
        if not ok then failures[#failures+1] = safeMessage(reason,"probe_cleanup_error") end
    end
    if request.timer and type(request.timer.Cancel)=="function" then attempt(request.timer.Cancel,request.timer) end
    request.timer=nil
    local callbacks=request.cleanups or {}
    request.cleanups={}
    for index=#callbacks,1,-1 do attempt(callbacks[index]) end
    request.cleanupFailed = #failures > 0
    return request.cleanupFailed and {state="pending",errors=failures} or nil
end
local function publish(request,receipt)
    local function refresh()
        local session=ns.Session.Current()
        if not session or session.generation~=request.generation then return nil end
        if request.state=="unresolved" and request.failureReceipt==receipt then
            -- A terminal encoding failure has no full SavedVariables report.
            -- Refresh the exact failure marker without dispatching again.
            return receipt
        end
        if request.state~="reported" or request.report~=receipt then return nil end
        local current,_,signal=ns.Session.ReadyReceipt()
        if current then return receipt,current,signal end
    end
    local visible=ns.ReceiptView.Show(receipt,nil,refresh)
    if not visible then return end
    if request.state~="reported" then return end
    ns.Session.WhenInputReady(function(ready)
        if not ready or request.state~="reported" then return end
        local current,_,signal=ns.Session.ReadyReceipt()
        if current then ns.ReceiptView.Show(receipt,current,refresh,signal) end
    end)
end
local function seal(request,status,value,display)
    if request.state~="running" then return nil,"probe_not_running" end
    local active=ns.Session.Current()
    if not active or active.generation~=request.generation then
        request.state="quarantined";cleanup(request);return nil,"probe_session_changed"
    end
    -- Close callback admission before invoking cleanup. Neither a recursive
    -- Finish nor a cleanup exception can change the chosen execution result.
    request.state="settling"
    local resources=cleanup(request)
    local body={probeStatus=status,acceptedBudgetSeconds=request.budgetSeconds,
        result=status=="completed" and value or nil,
        error=status=="failed" and safeMessage(value,"probe_runtime_error") or nil,
        logs=#request.logs>0 and request.logs or nil,logsTruncated=request.logsTruncated or nil,
        resources=resources}
    local report,failure=ns.ReportStore.Commit(request.requestId,request.code,body)
    if not report and request.goal=="finished" and REPORT_ERROR_CODES[failure] then
        -- Encoding user values may fail (including secret values). Preserve the
        -- known execution outcome as a small ordinary report without echoing it.
        report=ns.ReportStore.Commit(request.requestId,request.code,{
            probeStatus="failed",acceptedBudgetSeconds=request.budgetSeconds,
            error=failure,resources=resources and {state="pending",errors={"probe_cleanup_error"}} or nil})
    end
    request.state=report and "reported" or "unresolved"
    request.report=report
    if report then
        local _,storedBody=ns.ReportStore.Read(request.requestId)
        request.reportBody=storedBody
    end
    if not report then
        report = minimalFailure(request, failure)
        request.failureReceipt = report
    end
    if request.goal=="finished" then
        local retained,retainFailure=ns.Investigation.Mark(request.requestId,request.report and "reported" or "report_error",report)
        if not retained then return nil,retainFailure end
    end
    if report and display then publish(request,report) end
    if request.goal ~= "finished" and ns.ActivityView then ns.ActivityView.Finish(request.requestId) end
    notify()
    return report,failure
end
local function complete(request,status,value,display)
    if request.state~="running" then return nil,"probe_not_running" end
    if request.pending then return nil,"probe_completion_requested" end
    if (request.depth or 0)>0 then
        request.pending={status=status,value=value,display=display}
        return true
    end
    return seal(request,status,value,display)
end
local function settle(request,ok,result,display)
    if not ok then request.pending={status="failed",value=result,display=display} end
    local pending=request.pending
    if pending and request.depth==0 then
        request.pending=nil
        return seal(request,pending.status,pending.value,pending.display)
    end
end
local function probeAPI(request)
    local api={}
    function api.Async(_,seconds)
        if request.state~="running" or request.async then return nil,"probe_async_state" end
        if restricted(seconds) or type(seconds)~="number" or seconds%1~=0 or seconds<1 or seconds>request.budgetSeconds
            or type(C_Timer)~="table" or type(C_Timer.NewTimer)~="function" then return nil,"probe_async_unavailable" end
        local now=ns.Compat and ns.Compat.MonotonicSeconds()
        if not now or not request.startedAt then return nil,"probe_clock_unavailable" end
        local remaining=seconds-(now-request.startedAt)
        if remaining<=0 then return nil,"probe_budget_exhausted" end
        request.async=true
        request.timer=C_Timer.NewTimer(remaining,function()
            if request.state=="running" then complete(request,"failed","probe_timeout",true) end
        end)
        if not request.timer then request.async=false;return nil,"probe_async_unavailable" end
        return true
    end
    function api.Finish(_,value) return complete(request,"completed",value,true) end
    function api.Fail(_,message) return complete(request,"failed",message,true) end
    function api.Callback(_,callback)
        if request.state~="running" or type(callback)~="function" or request.callbackCount>=16 then
            return nil,"probe_callback_limit"
        end
        request.callbackCount=request.callbackCount+1
        return function(...)
            local active=ns.Session.Current()
            if request.state~="running" or request.pending or not active
                or active.generation~=request.generation then return nil,"probe_callback_inactive" end
            request.depth=request.depth+1
            local ok,result=pcall(callback,...)
            request.depth=request.depth-1
            local receipt,failure=settle(request,ok,result,true)
            if receipt or failure then return receipt,failure end
            return ok and true or nil,result
        end
    end
    function api.OnCleanup(_,callback)
        if request.state~="running" or type(callback)~="function" or #request.cleanups>=MAX_CLEANUPS then
            return nil,"probe_cleanup_limit"
        end
        request.cleanups[#request.cleanups+1]=callback
        return true
    end
    function api.Log(_,...)
        if request.state~="running" then return nil,"probe_not_running" end
        if #request.logs>=MAX_LOG_ENTRIES or request.logBytes>=MAX_LOG_BYTES then request.logsTruncated=true;return nil,"probe_log_limit" end
        local values={n=select("#",...),...}
        local parts={}
        for index=1,values.n do
            local value=values[index]
            if restricted(value) then parts[index]="<secret>"
            elseif type(value)=="string" or type(value)=="number" or type(value)=="boolean" then parts[index]=tostring(value)
            else parts[index]="<unsupported>" end
        end
        local line=table.concat(parts,"  ")
        local remaining=MAX_LOG_BYTES-request.logBytes
        if #line>remaining then line=line:sub(1,remaining);request.logsTruncated=true end
        request.logs[#request.logs+1]=line;request.logBytes=request.logBytes+#line
        return not request.logsTruncated
    end
    function api.IsCancelled() local active=ns.Session.Current();return request.state~="running" or not active or active.generation~=request.generation end
    return api
end

ns.ProbeRunner = {
    State = function(requestId)
        local session,failure=context()
        if not session then return nil,failure end
        if not key(requestId) then return nil,"probe_invalid_request" end
        return requests[requestId] and requests[requestId].state or nil
    end,
    Busy = function() return busy() end,
    ResourcesReleased = function(requestId)
        local request=requests and requests[requestId]
        if request and (request.cleanupFailed or request.state=="running"
            or request.state=="settling" or request.state=="quarantined") then
            return nil,"probe_resources_pending"
        end
        return true
    end,
    Reported = function(requestId)
        local session, failure = context()
        if not session then return nil, failure end
        if not key(requestId) then return nil, "probe_invalid_request" end
        local request = requests[requestId]
        if not request or request.state ~= "reported" then return nil, "probe_not_reported" end
        local receipt, body = ns.ReportStore.Read(requestId)
        if not receipt then return nil, body end
        if receipt ~= request.report or body ~= request.reportBody then return nil, "probe_report_changed" end
        return receipt, body, request.code
    end,
    VerifyAbsent = function(requestId)
        local session, failure = context()
        if not session then return nil, failure end
        if not key(requestId) then return nil, "probe_invalid_request" end
        if requests[requestId] ~= nil then return nil, "probe_still_retained" end
        return true
    end,
    Load = function(requestId, code, reloadNonce, budgetSeconds, goal)
        local session, failure = context()
        if not session then return nil, failure end
        if not key(requestId) then return nil, "probe_invalid_request" end
        if restricted(reloadNonce) or (reloadNonce ~= nil and (type(reloadNonce) ~= "string"
            or #reloadNonce ~= 32 or not string.match(reloadNonce, "^[0-9a-f]+$"))) then
            return nil, "probe_invalid_reload_nonce"
        end
        if restricted(code) or type(code) ~= "string" or #code == 0 or #code > 256 * 1024
            or string.byte(code, 1) == 27 then return nil, "probe_invalid_code" end
        if budgetSeconds == nil then budgetSeconds = MAX_ASYNC_SECONDS end
        if restricted(budgetSeconds) or type(budgetSeconds) ~= "number" or
            budgetSeconds % 1 ~= 0 or budgetSeconds < 1 or budgetSeconds > MAX_ASYNC_SECONDS then
            return nil, "probe_invalid_budget"
        end
        if requests[requestId] then return nil, "probe_request_exists" end
        local receipt, reason = ns.ReportStore.Read(requestId)
        if receipt then return nil, "probe_report_exists" end
        if reason ~= "report_unavailable" then return nil, reason end
        if count >= 16 or bytes + #code > 1024 * 1024 then return nil, "probe_queue_limit" end
        -- Compile text only. Loading definitions never invokes their code.
        local executable = loadstring(code, "=LycheeProbe:" .. requestId)
        if not executable then
            if goal~="finished" then return nil, "probe_syntax_error" end
            executable=function() error("probe_syntax_error",0) end
        end
        local signal, encodeFailure = loadedSignal(requestId, code, reloadNonce, false)
        if not signal then return nil, encodeFailure end
        requests[requestId] = { requestId=requestId,code = code, executable = executable,
            state = "loaded", reloadNonce = reloadNonce, budgetSeconds = budgetSeconds, goal=goal }
        count, bytes = count + 1, bytes + #code
        notify()
        return signal
    end,
    RefreshLoaded = function(requestId)
        local session, failure = context()
        if not session then return nil, failure end
        if not key(requestId) then return nil, "probe_invalid_request" end
        local request = requests[requestId]
        if not request then return nil, "probe_not_loaded" end
        if request.state ~= "loaded" then return nil, "probe_already_dispatched" end
        local ready, reason = ns.Platform.ObserveInputState()
        if ready ~= true then return nil, reason end
        return loadedSignal(requestId, request.code, request.reloadNonce, true)
    end,
    Dispatch = function(requestId)
        local session, failure = context()
        if not session then return nil, failure end
        if not key(requestId) then return nil, "probe_invalid_request" end
        local request = requests[requestId]
        if not request then return nil, "probe_not_loaded" end
        if request.state ~= "loaded" then return nil, "probe_already_dispatched" end
        if busy(requestId) then return nil,"probe_runtime_busy" end
        local receipt, reason = ns.ReportStore.Read(requestId)
        if receipt then return nil, "probe_report_exists" end
        if reason ~= "report_unavailable" then return nil, reason end
        if request.goal=="finished" then
            local started,startFailure=ns.Investigation.Mark(requestId,"running")
            if not started then return nil,startFailure end
        end
        -- Consume before calling user code, including recursive dispatch. pcall
        -- contains Lua errors; it is not a sandbox or a synchronous time limit.
        request.state = "running"
        if ns.ActivityView then ns.ActivityView.Begin(requestId) end
        notify()
        local executable = request.executable
        request.executable = nil
        request.generation=session.generation
        request.startedAt=ns.Compat and ns.Compat.MonotonicSeconds()
        request.logs,request.logBytes,request.cleanups={},0,{}
        request.depth,request.callbackCount=1,0
        local ok, result = pcall(executable,probeAPI(request))
        request.depth=0
        local active = ns.Session.Current()
        if not active or active.generation ~= session.generation then
            request.state = "quarantined";cleanup(request)
            return nil, "probe_session_changed"
        end
        if not ok or request.pending then return settle(request,ok,result,false) end
        if request.async then
            if request.state=="reported" then return request.report end
            if request.state~="running" then return nil,"probe_async_unresolved" end
            -- Accepted asynchronous work has no terminal receipt yet. Keep
            -- the scene clear until seal publishes its actual result.
            return true
        end
        return complete(request,"completed",result,false)
    end,
}
