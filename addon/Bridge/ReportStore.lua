local ADDON_NAME, ns = ...

local MAX_RECORDS, MAX_BYTES = 100, 16 * 1024 * 1024
local lastAcknowledgement
local function restricted(value)
    return issecretvalue and issecretvalue(value)
end
local function plain(value)
    return not restricted(value) and type(value) == "table" and getmetatable(value) == nil
end
local function label(value, limit)
    return not restricted(value) and type(value) == "string" and #value > 0
        and #value <= limit and not string.find(value, "[%z\1-\31\127]")
end
local function requestKey(value)
    return label(value, 128) and string.match(value, "^[%w_%-]+$") ~= nil
end

ns.ReportStore = {
    Acknowledged = function(requestId)
        if not requestKey(requestId) then return nil, "report_invalid_request" end
        local state = ns.Persistence.Current()
        local session = ns.Session.Current()
        local ack = lastAcknowledgement
        if not ack or not session or ack.root ~= state or ack.requestId ~= requestId
            or ack.sessionNonce ~= session.sessionNonce or ack.guid ~= session.guid
            or ack.generation ~= session.generation then
            return nil, "report_acknowledgement_unavailable"
        end
        local receipt, reason = ns.ReportStore.Read(requestId)
        if receipt or reason ~= "report_unavailable" then return nil, "report_still_retained" end
        return ack.receipt
    end,
    Commit = function(requestId, code, value)
        local state, failure = ns.Persistence.Current()
        if not state then return nil, failure end
        if not ns.Startup.ready or not state.options or state.options.bridgeEnabled ~= true then
            return nil, "bridge_disabled"
        end
        if not requestKey(requestId) then return nil, "report_invalid_request" end
        local session, sessionFailure = ns.Session.Current()
        if not session then return nil, sessionFailure end
        if restricted(code) or type(code) ~= "string" or #code > 256 * 1024 then
            return nil, "report_invalid_code"
        end
        if not plain(state.reports) then return nil, "report_invalid_store" end
        local previous = state.reports[requestId]
        if restricted(previous) or previous ~= nil then return nil, "report_request_exists" end
        local body, reason = ns.CaptureWriter.Encode(value)
        if not body then return nil, reason end
        local identity, identityFailure = ns.Session.NextIdentity()
        if not identity then return nil, identityFailure end
        local signal = {
            schema = "lycheedev.signal.v1", release = ns.Release, kind = "reported",
            sessionNonce = identity.sessionNonce, requestId = requestId,
            character = identity.character, realm = identity.realm, sequence = identity.sequence,
            product = ns.Startup.identity.product, build = ns.Startup.identity.build,
            inputReady = false, reportBytes = #body,
            reportAdler32 = ns.CaptureWriter.DigestBytes(body),
        }
        if #code > 0 then
            signal.codeBytes = #code
            signal.codeAdler32 = ns.CaptureWriter.DigestBytes(code)
        end
        local receipt, encodeFailure = ns.CaptureWriter.EncodeSignal(signal, 4096)
        if not receipt then return nil, encodeFailure end
        local count, bytes = 0, #receipt + #body
        for key, record in pairs(state.reports) do
            count = count + 1
            if count >= MAX_RECORDS then return nil, "report_count_limit" end
            if not requestKey(key) or not plain(record) or restricted(record.receipt)
                or restricted(record.body) or type(record.receipt) ~= "string"
                or type(record.body) ~= "string" then return nil, "report_invalid_store" end
            if #record.receipt == 0 or #record.receipt > 4096 or #record.body == 0
                or #record.body > 512 * 1024 then return nil, "report_invalid_store" end
            for field in pairs(record) do
                if restricted(field) or (field ~= "receipt" and field ~= "body") then
                    return nil, "report_invalid_store"
                end
            end
            bytes = bytes + #record.receipt + #record.body
            if bytes > MAX_BYTES then return nil, "report_store_limit" end
        end
        if bytes > MAX_BYTES then return nil, "report_store_limit" end
        -- No pruning or replacement of unacknowledged reports. One final write
        -- makes failures leave the previous store intact. Disk flush is separate.
        state.reports[requestId] = { receipt = receipt, body = body }
        return receipt
    end,
    Read = function(requestId)
        if not requestKey(requestId) then return nil, "report_invalid_request" end
        local state, failure = ns.Persistence.Current()
        if not state then return nil, failure end
        if not plain(state.reports) then return nil, "report_invalid_store" end
        local record = state.reports[requestId]
        if not restricted(record) and record == nil then return nil, "report_unavailable" end
        if not plain(record) or restricted(record.receipt) or restricted(record.body)
            or type(record.receipt) ~= "string" or type(record.body) ~= "string" then
            return nil, "report_invalid_store"
        end
        return record.receipt, record.body
    end,
    Acknowledge = function(reported)
        local state, failure = ns.Persistence.Current()
        if not state then return nil, failure end
        if not ns.Startup.ready or not state.options or state.options.bridgeEnabled ~= true then
            return nil, "bridge_disabled"
        end
        if not plain(reported) then return nil, "report_invalid_acknowledgement" end
        local requestId, bodyBytes, bodyAdler32 = reported.requestId, reported.reportBytes, reported.reportAdler32
        if not requestKey(requestId) or reported.schema ~= "lycheedev.signal.v1" or reported.kind ~= "reported" then
            return nil, "report_invalid_acknowledgement"
        end
        -- Encoding runs before the numeric shape checks: it rejects restricted
        -- values, metatables and cycles, so a secret field reports that it is
        -- secret instead of failing a type comparison it can never satisfy.
        local receipt, encodeFailure = ns.CaptureWriter.EncodeSignal(reported, 4096)
        if not receipt then return nil, encodeFailure end
        if type(reported.sequence) ~= "number" or reported.sequence < 1 or reported.sequence > 9007199254740991
            or reported.sequence % 1 ~= 0 or type(bodyBytes) ~= "number" or bodyBytes < 1 or bodyBytes > 512 * 1024
            or bodyBytes % 1 ~= 0 or type(bodyAdler32) ~= "string" or #bodyAdler32 ~= 8
            or not string.match(bodyAdler32, "^[0-9a-f]+$") then
            return nil, "report_invalid_acknowledgement"
        end
        local session, sessionFailure = ns.Session.Current()
        if not session then return nil, sessionFailure end
        -- The live session is authoritative for the actor and the nonce. The
        -- compact wire form still carries both while a caller can supply them,
        -- so an acknowledgement that contradicts the current session is refused
        -- rather than canonicalized into agreement. The host echoes the fields it
        -- archived, and the archived receipt is the compact form, so a caller
        -- that omits them is consistent and still passes.
        if reported.sessionNonce ~= nil and reported.sessionNonce ~= session.sessionNonce
            or reported.character ~= nil and reported.character ~= session.character
            or reported.realm ~= nil and reported.realm ~= session.realm
            or reported.release ~= nil and reported.release ~= ns.Release
            or reported.product ~= nil and reported.product ~= ns.Startup.identity.product
            or reported.build ~= nil and reported.build ~= ns.Startup.identity.build then
            return nil, "report_acknowledgement_identity"
        end
        -- The stored receipt is the canonical wire form. A caller echoes the
        -- fields the host archived, so both sides are projected the same way and
        -- the session nonce, which the wire omits, never has to be restated.
        local receipt, encodeFailure = ns.CaptureWriter.EncodeSignal(reported, 4096)
        if not receipt then return nil, encodeFailure end
        local storedReceipt, body = ns.ReportStore.Read(requestId)
        if not storedReceipt then return nil, body end
        if storedReceipt ~= receipt or #body ~= bodyBytes
            or ns.CaptureWriter.DigestBytes(body) ~= bodyAdler32 then
            return nil, "report_acknowledgement_mismatch"
        end
        -- After a reload the private counter starts again. A validated original
        -- receipt provides a floor, never an arbitrary caller-supplied sequence.
        local identity, identityFailure = ns.Session.NextIdentity(reported.sequence)
        if not identity then return nil, identityFailure end
        local signal = {}
        for key, value in pairs(reported) do signal[key] = value end
        signal.kind, signal.sequence, signal.inputReady = "acknowledged", identity.sequence, false
        local acknowledgement, signalFailure = ns.CaptureWriter.EncodeSignal(signal, 2048)
        if not acknowledgement then return nil, signalFailure end
        -- Prepare the bounded optical receipt before deletion. The executor must
        -- have archived and verified the full original report before this call.
        -- This proves only in-memory cleanup, not a SavedVariables disk flush.
        state.reports[requestId] = nil
        -- One bounded runtime-only handoff, never inherited across reload/login.
        lastAcknowledgement = { root=state, requestId=requestId, receipt=acknowledgement,
            sessionNonce=session.sessionNonce, guid=session.guid, generation=session.generation }
        return acknowledgement
    end,
}
