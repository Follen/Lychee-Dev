local ADDON_NAME, ns = ...

local definitions = ns.ProbeDefinitions
-- Bounded by the loaded queue (at most 16 entries). Keep definitions for the
-- retained cleanup protocol, but acknowledged source no longer owns the UI
-- and cannot be loaded again in this runtime. Disk retirement stays host-owned.
local acknowledged = {}
ns.ProbeDefinitions = nil
local function restricted(value)
    return issecretvalue and issecretvalue(value)
end
local function plain(value)
    return not restricted(value) and type(value) == "table" and getmetatable(value) == nil
end
local function label(value)
    return not restricted(value) and type(value) == "string" and #value > 0
        and #value <= 128 and not string.find(value, "[%z\1-\31\127]")
end
local function hex(value, size)
    return not restricted(value) and type(value) == "string" and #value == size
        and string.match(value, "^[0-9a-f]+$") ~= nil
end
local fields = { release = true, sessionNonce = true, reloadNonce = true, character = true,
    realm = true, guid = true, product = true, build = true, code = true,
    codeSHA256 = true, codeAdler32 = true, codeBytes = true }
local function validate()
    if not plain(definitions) or restricted(definitions.schema) or definitions.schema ~= "lycheedev.queue.v1"
        or not plain(definitions.entries) then return nil, "queue_invalid_format" end
    for field in pairs(definitions) do
        if restricted(field) or (field ~= "schema" and field ~= "entries") then return nil, "queue_invalid_format" end
    end
    local count, bytes = 0, 0
    for id, entry in pairs(definitions.entries) do
        count = count + 1
        if count > 16 then return nil, "queue_limit" end
        if not label(id) or not string.match(id, "^[%w_%-]+$") or not plain(entry) then return nil, "queue_invalid_entry" end
        for field in pairs(entry) do
            if restricted(field) or not fields[field] then return nil, "queue_invalid_entry" end
        end
        for _, field in ipairs({ "release", "character", "realm", "guid", "product", "build" }) do
            if not label(entry[field]) then return nil, "queue_invalid_identity" end
        end
        if not hex(entry.sessionNonce, 32) or not hex(entry.reloadNonce, 32)
            or not hex(entry.codeSHA256, 64) or not hex(entry.codeAdler32, 8) then return nil, "queue_invalid_digest" end
        if restricted(entry.code) or type(entry.code) ~= "string" or #entry.code == 0 or #entry.code > 256 * 1024
            or string.byte(entry.code, 1) == 27 or string.find(entry.code, "%z")
            or restricted(entry.codeBytes) or entry.codeBytes ~= #entry.code
            or ns.CaptureWriter.DigestBytes(entry.code) ~= entry.codeAdler32 then return nil, "queue_code_mismatch" end
        bytes = bytes + #entry.code
        if bytes > 1024 * 1024 then return nil, "queue_limit" end
    end
    return true
end

local function selectEntry(requestId)
        local session, failure = ns.Session.Current()
        if not session then return nil, failure end
        if not label(requestId) then return nil, "queue_invalid_request" end
        local valid, reason = validate()
        if not valid then return nil, reason end
        local entry = definitions.entries[requestId]
        if not entry then return nil, "queue_request_missing" end
        local identity = ns.Startup.identity
        if entry.release ~= ns.Release or entry.product ~= identity.product or entry.build ~= identity.build
            or entry.character ~= session.character or entry.realm ~= session.realm or entry.guid ~= session.guid
            or entry.sessionNonce ~= session.sessionNonce then return nil, "queue_identity_mismatch" end
        return entry
end

ns.ProbeQueue = {
    -- Registered entries mean a request-scoped operation still owns this
    -- window's queue and display. Unreadable definitions fail closed.
    Busy = function()
        if definitions == nil then return false end
        local valid, reason = validate()
        if not valid then return nil, reason end
        if next(definitions.entries) == nil then return false end
        local actor, failure = ns.Platform.ObserveActor()
        if not actor then return nil, failure end
        -- The installation queue is shared by windows; another character
        -- cannot own this window merely because its definition is loaded here.
        for id, entry in pairs(definitions.entries) do
            if not acknowledged[id] and entry.character == actor.character
                and entry.realm == actor.realm and entry.guid == actor.guid then return true end
        end
        return false
    end,
    ReloadScope = function(requestId)
        local entry, reason = selectEntry(requestId)
        if not entry then return nil, reason end
        return { requestId = requestId, sessionNonce = entry.sessionNonce, reloadNonce = entry.reloadNonce,
            release = entry.release, product = entry.product, build = entry.build,
            character = entry.character, realm = entry.realm, guid = entry.guid,
            codeSHA256 = entry.codeSHA256, codeAdler32 = entry.codeAdler32, codeBytes = entry.codeBytes }
    end,
    VerifyRetired = function(requestId, nonce)
        local session, failure = ns.Session.Current()
        if not session then return nil, failure end
        if not label(requestId) or not string.match(requestId, "^[%w_%-]+$") then
            return nil, "queue_invalid_request"
        end
        if not hex(nonce, 32) then return nil, "queue_invalid_cleanup_nonce" end
        local valid, reason = validate()
        if not valid then return nil, reason end
        if definitions.entries[requestId] ~= nil then return nil, "queue_request_retained" end
        local receipt, reportFailure = ns.ReportStore.Read(requestId)
        if receipt ~= nil then return nil, "report_still_retained" end
        if reportFailure ~= "report_unavailable" then return nil, reportFailure end
        local absent, probeFailure = ns.ProbeRunner.VerifyAbsent(requestId)
        if not absent then return nil, probeFailure end
        local identity, identityFailure = ns.Session.NextIdentity()
        if not identity then return nil, identityFailure end
        -- This observes loaded memory, never disk or host ownership. No report,
        -- queue entry or executable is removed to manufacture a success receipt.
        return ns.CaptureWriter.Encode({
            schema = "lycheedev.signal.v1", release = ns.Release, kind = "cleared",
            sessionNonce = identity.sessionNonce, requestId = requestId, cleanupNonce = nonce,
            character = identity.character, realm = identity.realm, guid = identity.guid,
            product = ns.Startup.identity.product, build = ns.Startup.identity.build,
            sequence = identity.sequence, runtimeEpoch = identity.runtimeEpoch, inputReady = false,
        }, 4096)
    end,
    Load = function(requestId)
        local entry, reason = selectEntry(requestId)
        if not entry then return nil, reason end
        if acknowledged[requestId] then return nil, "queue_request_acknowledged" end
        -- SHA-256 is checked by the host. The game checks actual code bytes and
        -- Adler-32; neither a carried digest nor loading is execution permission.
        return ns.ProbeRunner.Load(requestId, entry.code, entry.reloadNonce)
    end,
    Acknowledge = function(requestId, sequence)
        local entry, reason = selectEntry(requestId)
        if not entry then return nil, reason end
        if restricted(sequence) or type(sequence) ~= "number" or sequence < 1
            or sequence > 9007199254740991 or sequence % 1 ~= 0 then return nil, "report_invalid_sequence" end
        local receipt, body = ns.ReportStore.Read(requestId)
        if not receipt then return nil, body end
        -- Reconstruct only the canonical receipt format generated by Commit.
        -- The store compares its exact bytes before deletion, so neither an
        -- arbitrary sequence nor changed code/body can authorize another report.
        local acknowledgement, failure = ns.ReportStore.Acknowledge({
            schema = "lycheedev.signal.v1", release = entry.release, kind = "reported",
            sessionNonce = entry.sessionNonce, requestId = requestId,
            character = entry.character, realm = entry.realm, sequence = sequence,
            product = entry.product, build = entry.build, inputReady = false,
            codeBytes = entry.codeBytes, codeAdler32 = entry.codeAdler32,
            reportBytes = #body, reportAdler32 = ns.CaptureWriter.DigestBytes(body),
        })
        if acknowledgement then acknowledged[requestId] = true end
        return acknowledgement, failure
    end,
    -- Host-side recovery for a stuck runtime whose queue blocks identity after
    -- its disk entry was retired (abandon) or its receipt was lost. Session
    -- free and actor scoped exactly like Busy: entries of other characters on
    -- a shared installation are never touched. A pending reentry ticket is
    -- stale residue here by definition; the caller verified no disk owner.
    Reset = function(nonce)
        if not hex(nonce, 32) then return nil, "reset_invalid_nonce" end
        local valid, reason = validate()
        if not valid then return nil, reason end
        local actor, actorFailure = ns.Platform.ObserveActor()
        if not actor then return nil, actorFailure or "no_actor" end
        if ns.Reentry and type(ns.Reentry.Cancel) == "function" then ns.Reentry.Cancel(true) end
        local reset = 0
        for id, entry in pairs(definitions.entries) do
            if not acknowledged[id] and entry.character == actor.character
                and entry.realm == actor.realm and entry.guid == actor.guid then
                acknowledged[id] = true
                reset = reset + 1
            end
        end
        local identity, identityFailure = ns.Platform.ObserveBuild()
        if not identity then return nil, identityFailure end
        return ns.CaptureWriter.Encode({
            schema = "lycheedev.signal.v1", release = ns.Release, kind = "reset",
            sessionNonce = "", probeNonce = nonce,
            requestId = "", character = actor.character, realm = actor.realm,
            guid = actor.guid, product = identity.product,
            build = identity.build, sequence = 0,
            inputReady = false,
        }, 4096)
    end,
}
