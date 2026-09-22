local ADDON_NAME, ns = ...

local requests, owner, count, bytes
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
    return ns.CaptureWriter.Encode({
        schema = "lycheedev.signal.v1", release = ns.Release, kind = "loaded",
        sessionNonce = identity.sessionNonce, requestId = requestId, reloadNonce = reloadNonce,
        character = identity.character, realm = identity.realm, sequence = identity.sequence,
        product = ns.Startup.identity.product, build = ns.Startup.identity.build,
        inputReady = ready, codeBytes = #code, codeAdler32 = ns.CaptureWriter.DigestBytes(code),
    }, 4096)
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
        requests[requestId] = { code = code, executable = executable, state = "loaded", reloadNonce = reloadNonce }
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
        local ok, result = pcall(executable)
        local active = ns.Session.Current()
        if not active or active.generation ~= session.generation then
            request.state = "unresolved"
            return nil, "probe_session_changed"
        end
        local body
        if ok then
            body = { probeStatus = "completed", result = result }
        else
            -- Never stringify an arbitrary error object or restricted value.
            local message = "probe_runtime_error"
            if not restricted(result) and type(result) == "string" and #result <= 4096 then message = result end
            body = { probeStatus = "failed", error = message }
        end
        local report, reportFailure = ns.ReportStore.Commit(requestId, request.code, body)
        request.state = report and "reported" or "unresolved"
        request.report = report
        if report then
            local _, storedBody = ns.ReportStore.Read(requestId)
            request.reportBody = storedBody
        end
        -- An executed probe whose result could not be stored must not run again.
        return report, reportFailure
    end,
}
