local ADDON_NAME, ns = ...

local requests, owner, count, bytes
local MAX_ASYNC_SECONDS,MAX_LOG_BYTES,MAX_LOG_ENTRIES,MAX_CLEANUPS=120,32768,100,16
local function restricted(value)
    return issecretvalue and issecretvalue(value)
end
local function context()
    local session, failure = ns.Session.Current()
    if not session then
        requests, owner, count, bytes = nil, nil, nil, nil
        return nil, failure
    end
    if owner ~= session.generation then
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
local function cleanup(request)
    if request.timer and type(request.timer.Cancel)=="function" then pcall(request.timer.Cancel,request.timer) end
    request.timer=nil
    local callbacks=request.cleanups or {}
    request.cleanups={}
    for index=#callbacks,1,-1 do pcall(callbacks[index]) end
end
local function publish(request,receipt)
    local function refresh()
        if request.state~="reported" then return nil end
        local current=ns.Session.ReadyReceipt()
        if current then return receipt,current end
    end
    local visible=ns.ReceiptView.Show(receipt,nil,refresh)
    if not visible then return end
    ns.Session.WhenInputReady(function(ready)
        if not ready or request.state~="reported" then return end
        local current=ns.Session.ReadyReceipt()
        if current then ns.ReceiptView.Show(receipt,current,refresh) end
    end)
end
local function complete(request,status,value,display)
    if request.state~="running" then return nil,"probe_not_running" end
    local active=ns.Session.Current()
    if not active or active.generation~=request.generation then
        request.state="unresolved";cleanup(request);return nil,"probe_session_changed"
    end
    local body={probeStatus=status,result=status=="completed" and value or nil,
        error=status=="failed" and safeMessage(value,"probe_runtime_error") or nil,
        logs=#request.logs>0 and request.logs or nil,logsTruncated=request.logsTruncated or nil}
    local report,failure=ns.ReportStore.Commit(request.requestId,request.code,body)
    request.state=report and "reported" or "unresolved"
    request.report=report
    if report then
        local _,storedBody=ns.ReportStore.Read(request.requestId)
        request.reportBody=storedBody
    end
    cleanup(request)
    if report and display then publish(request,report) end
    return report,failure
end
local function probeAPI(request)
    local api={}
    function api.Async(_,seconds)
        if request.state~="running" or request.async then return nil,"probe_async_state" end
        if restricted(seconds) or type(seconds)~="number" or seconds%1~=0 or seconds<1 or seconds>MAX_ASYNC_SECONDS
            or type(C_Timer)~="table" or type(C_Timer.NewTimer)~="function" then return nil,"probe_async_unavailable" end
        request.async=true
        request.timer=C_Timer.NewTimer(seconds,function()
            if request.state=="running" then complete(request,"failed","probe_timeout",true) end
        end)
        if not request.timer then request.async=false;return nil,"probe_async_unavailable" end
        return true
    end
    function api.Finish(_,value) return complete(request,"completed",value,true) end
    function api.Fail(_,message) return complete(request,"failed",message,true) end
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
    Load = function(requestId, code, reloadNonce)
        local session, failure = context()
        if not session then return nil, failure end
        if not key(requestId) then return nil, "probe_invalid_request" end
        if restricted(reloadNonce) or (reloadNonce ~= nil and (type(reloadNonce) ~= "string"
            or #reloadNonce ~= 32 or not string.match(reloadNonce, "^[0-9a-f]+$"))) then
            return nil, "probe_invalid_reload_nonce"
        end
        if restricted(code) or type(code) ~= "string" or #code == 0 or #code > 256 * 1024
            or string.byte(code, 1) == 27 then return nil, "probe_invalid_code" end
        if requests[requestId] then return nil, "probe_request_exists" end
        local receipt, reason = ns.ReportStore.Read(requestId)
        if receipt then return nil, "probe_report_exists" end
        if reason ~= "report_unavailable" then return nil, reason end
        if count >= 16 or bytes + #code > 1024 * 1024 then return nil, "probe_queue_limit" end
        -- Compile text only. Loading definitions never invokes their code.
        local executable = loadstring(code, "=LycheeProbe:" .. requestId)
        if not executable then return nil, "probe_syntax_error" end
        local signal, encodeFailure = loadedSignal(requestId, code, reloadNonce, false)
        if not signal then return nil, encodeFailure end
        requests[requestId] = { requestId=requestId,code = code, executable = executable, state = "loaded", reloadNonce = reloadNonce }
        count, bytes = count + 1, bytes + #code
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
        local receipt, reason = ns.ReportStore.Read(requestId)
        if receipt then return nil, "probe_report_exists" end
        if reason ~= "report_unavailable" then return nil, reason end
        -- Consume before calling user code, including recursive dispatch. pcall
        -- contains Lua errors; it is not a sandbox or a synchronous time limit.
        request.state = "running"
        local executable = request.executable
        request.executable = nil
        request.generation=session.generation
        request.logs,request.logBytes,request.cleanups={},0,{}
        local ok, result = pcall(executable,probeAPI(request))
        local active = ns.Session.Current()
        if not active or active.generation ~= session.generation then
            request.state = "unresolved";cleanup(request)
            return nil, "probe_session_changed"
        end
        if not ok then return complete(request,"failed",result,false) end
        if request.async then
            if request.state=="reported" then return request.report end
            if request.state~="running" then return nil,"probe_async_unresolved" end
            return loadedSignal(requestId,request.code,request.reloadNonce,false)
        end
        return complete(request,"completed",result,false)
    end,
}
